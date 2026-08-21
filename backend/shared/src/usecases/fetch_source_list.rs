use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use url::Url;

/// Fetches a source list index and returns its JSON form.
///
/// JSON indexes (Aidoku `index.min.json`, LNReader `plugins.min.json`,
/// MangaYomi `index.json`) are parsed as-is. Keiyoushi publishes its index
/// as Mihon's protobuf `SourceRepo` (`index.pb`), usually gzipped; it is
/// decoded into the same JSON entries the old JSON index used, so callers
/// can treat every source list as JSON.
///
/// When an `index.pb` URL cannot be fetched or decoded, the `index.min.json`
/// sibling in the same directory is tried as a fallback (other Tachiyomi/
/// Mihon repos publish only the JSON index).
pub async fn fetch_source_list(client: &reqwest::Client, url: &Url) -> Result<Value> {
    match fetch_index(client, url).await {
        Ok(value) => Ok(value),
        Err(err) => {
            if let Some(fallback) = sibling_json_url(url) {
                if let Ok(value) = fetch_index(client, &fallback).await {
                    // An empty JSON index is a placeholder, not a working
                    // fallback.
                    let placeholder = match &value {
                        Value::Array(entries) => entries.is_empty(),
                        _ => false,
                    };
                    if !placeholder {
                        return Ok(value);
                    }
                }
            }
            Err(err)
        }
    }
}

/// Fetches one index at the given URL, accepting JSON, gzipped JSON and the
/// gzipped keiyoushi protobuf index.
async fn fetch_index(client: &reqwest::Client, url: &Url) -> Result<Value> {
    let response = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to fetch source list at {url}"))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read source list at {url}"))?;

    parse_index_bytes(&bytes).ok_or_else(|| anyhow::anyhow!("failed to parse source list at {url}"))
}

/// Parses a source list body as JSON, gzipped JSON or the gzipped keiyoushi
/// protobuf index.
///
/// The keiyoushi `index.pb` is served as `application/octet-stream` with
/// the gzip magic inside, so reqwest's transparent decompression (which
/// only triggers on a `content-encoding: gzip` header) does not apply.
fn parse_index_bytes(bytes: &[u8]) -> Option<Value> {
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        return Some(value);
    }

    let data = if bytes.starts_with(&[0x1f, 0x8b]) {
        gunzip(bytes).ok()?
    } else {
        bytes.to_vec()
    };
    if let Ok(value) = serde_json::from_slice::<Value>(&data) {
        return Some(value);
    }

    decode_repo_index(&data).ok()
}

/// The `index.min.json` URL next to an `index.pb` URL (same path, same
/// query), or `None` when the URL does not point at `index.pb`.
fn sibling_json_url(url: &Url) -> Option<Url> {
    let prefix = url.path().strip_suffix("index.pb")?;
    let mut fallback = url.clone();
    fallback.set_path(&format!("{prefix}index.min.json"));
    Some(fallback)
}

/// Decompresses a gzip stream.
fn gunzip(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;

    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut data = Vec::new();
    decoder
        .read_to_end(&mut data)
        .context("failed to read gzip stream")?;
    Ok(data)
}

/// Decodes the keiyoushi index (`index.proto` in `keiyoushi/extensions-source`)
/// into the JSON entries of the old keiyoushi JSON index: one entry per
/// `(extension, language)` pair.
///
/// ```proto
/// message Index {
///   string name = 1;
///   string badgeLabel = 2;
///   string signingKey = 3;
///   Contact contact = 4;          // { website = 1, discord = 2 }
///   oneof extensions {
///     ExtensionList extensionList = 101;
///     string extensionListUrl = 102;
///   }
/// }
/// message ExtensionList {
///   repeated Extension extensions = 1;
/// }
/// message Extension {
///   string name = 1;
///   string packageName = 2;
///   Resources resources = 3;      // { apkUrl = 1, iconUrl = 2, jarUrl = 501 }
///   string extensionLib = 4;
///   int64 versionCode = 5;
///   string versionName = 6;
///   ContentWarning contentWarning = 7; // enum, varint
///   repeated Source sources = 8;  // { id = 1, name = 2, language = 3, ... }
/// }
/// ```
fn decode_repo_index(data: &[u8]) -> Result<Value> {
    let mut reader = Reader::new(data);
    let mut entries = Vec::new();
    while let Some((field, wire)) = reader.tag()? {
        if field == 101 && wire == 2 {
            // `ExtensionList extensionList` (the `oneof` variant in use).
            let list = reader.bytes()?;
            let mut list_reader = Reader::new(list);
            while let Some((list_field, list_wire)) = list_reader.tag()? {
                if list_field == 1 && list_wire == 2 {
                    entries.extend(decode_extension(list_reader.bytes()?)?);
                } else {
                    list_reader.skip(list_wire)?;
                }
            }
        } else if field == 102 && wire == 2 {
            bail!("source list uses the remote `extensionListUrl` variant");
        } else {
            reader.skip(wire)?;
        }
    }
    Ok(Value::Array(entries))
}

/// Expands keiyoushi entry ids in place: the index publishes one entry per
/// bundled source language, all sharing the extension package name, and a
/// loaded multi-source APK registers its sources as `<pkg>:<lang>`. Entries
/// of a package published with several languages therefore get
/// `<pkg>:<lang>` ids (matching the sources an install produces), while
/// single-language packages keep the plain package id.
pub fn expand_keiyoushi_ids(entries: &mut [(String, Option<String>)]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (id, _) in entries.iter() {
        *counts.entry(id.clone()).or_default() += 1;
    }
    for (id, lang) in entries.iter_mut() {
        if counts.get(id.as_str()).copied().unwrap_or(0) > 1 {
            if let Some(lang) = lang.clone() {
                *id = format!("{id}:{lang}");
            }
        }
    }
}

/// Decodes one `Extension` message into the JSON entries of the keiyoushi
/// JSON index, one per published language. Returns an empty list when the
/// entry carries no installable package.
fn decode_extension(src: &[u8]) -> Result<Vec<Value>> {
    let mut reader = Reader::new(src);
    let mut name = None;
    let mut pkg = None;
    let mut apk = None;
    let mut code = None;
    let mut version = None;
    let mut langs = Vec::new();

    while let Some((field, wire)) = reader.tag()? {
        match (field, wire) {
            (1, 2) => name = Some(reader.string()?),
            (2, 2) => pkg = Some(reader.string()?),
            (3, 2) => {
                let resources = reader.bytes()?;
                let mut resources_reader = Reader::new(resources);
                while let Some((resources_field, resources_wire)) = resources_reader.tag()? {
                    if resources_field == 1 && resources_wire == 2 {
                        apk = Some(resources_reader.string()?);
                    } else {
                        resources_reader.skip(resources_wire)?;
                    }
                }
            }
            (4, 2) => {
                reader.string()?;
            }
            (5, 0) => code = Some(reader.varint()?),
            (6, 2) => version = Some(reader.string()?),
            (7, 0) => {
                reader.varint()?;
            }
            (8, 2) => {
                let source = reader.bytes()?;
                let mut source_reader = Reader::new(source);
                while let Some((source_field, source_wire)) = source_reader.tag()? {
                    if source_field == 3 && source_wire == 2 {
                        langs.push(source_reader.string()?);
                    } else {
                        source_reader.skip(source_wire)?;
                    }
                }
            }
            _ => reader.skip(wire)?,
        }
    }

    let (Some(name), Some(pkg), Some(apk)) = (name, pkg, apk) else {
        return Ok(Vec::new());
    };
    if langs.is_empty() {
        langs.push("all".to_string());
    }

    Ok(langs
        .into_iter()
        .map(|lang| {
            let mut entry = serde_json::Map::new();
            entry.insert("name".to_string(), Value::String(name.clone()));
            entry.insert("pkg".to_string(), Value::String(pkg.clone()));
            entry.insert("apk".to_string(), Value::String(apk.clone()));
            entry.insert("lang".to_string(), Value::String(lang));
            if let Some(code) = code {
                entry.insert(
                    "code".to_string(),
                    Value::Number(serde_json::Number::from(code)),
                );
            }
            entry.insert(
                "version".to_string(),
                Value::String(version.clone().unwrap_or_default()),
            );
            Value::Object(entry)
        })
        .collect())
}

/// Minimal protobuf wire-format reader for the fixed `SourceRepo` schema
/// above. Only proto2 optional/repeated fields are expected, so unknown
/// fields are skipped by wire type and every other wire type is an error.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Reads the next field tag, or `None` at the end of the message.
    fn tag(&mut self) -> Result<Option<(u64, u64)>> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        let tag = self.varint()?;
        let field = tag >> 3;
        let wire = tag & 0b111;
        if field == 0 {
            bail!("invalid protobuf tag {tag}");
        }
        Ok(Some((field, wire)))
    }

    fn varint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = *self
                .buf
                .get(self.pos)
                .context("protobuf varint out of bounds")?;
            self.pos += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        bail!("protobuf varint is too long")
    }

    fn bytes(&mut self) -> Result<&'a [u8]> {
        let len = self.varint()? as usize;
        let end = self
            .pos
            .checked_add(len)
            .context("protobuf length overflow")?;
        let data = self
            .buf
            .get(self.pos..end)
            .context("protobuf field out of bounds")?;
        self.pos = end;
        Ok(data)
    }

    fn string(&mut self) -> Result<String> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }

    /// Skips a field of the given wire type.
    fn skip(&mut self, wire: u64) -> Result<()> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => {
                self.pos += 8;
            }
            2 => {
                self.bytes()?;
            }
            5 => {
                self.pos += 4;
            }
            _ => bail!("unsupported protobuf wire type {wire}"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn field_bytes(field: u64, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, (field << 3) | 2);
        push_varint(&mut out, data.len() as u64);
        out.extend_from_slice(data);
        out
    }

    fn field_string(field: u64, value: &str) -> Vec<u8> {
        field_bytes(field, value.as_bytes())
    }

    fn field_varint(field: u64, value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, field << 3);
        push_varint(&mut out, value);
        out
    }

    fn source(lang: &str) -> Vec<u8> {
        let mut out = field_varint(1, 1234);
        out.extend(field_string(2, "Placeholder"));
        out.extend(field_string(3, lang));
        out.extend(field_string(4, "https://example.com"));
        field_bytes(8, &out)
    }

    fn extension(
        name: &str,
        pkg: &str,
        apk: &str,
        version: &str,
        code: u64,
        langs: &[&str],
    ) -> Vec<u8> {
        let mut out = field_string(1, name);
        out.extend(field_string(2, pkg));
        let mut resources = field_string(1, apk);
        resources.extend(field_string(2, "https://example.com/icon.png"));
        resources.extend(field_string(501, "https://example.com/ext.jar"));
        out.extend(field_bytes(3, &resources));
        out.extend(field_string(4, "1.6"));
        out.extend(field_varint(5, code));
        out.extend(field_string(6, version));
        out.extend(field_varint(7, 3));
        for lang in langs {
            out.extend(source(lang));
        }
        out
    }

    /// Wraps extensions in the `Index.extensionList` oneof (field 101
    /// containing an `ExtensionList` with repeated field 1).
    fn index(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut out = field_string(1, "Keiyoushi");
        out.extend(field_string(2, "KEI"));
        out.extend(field_string(3, "9add655a"));
        let mut contact = field_string(1, "https://keiyoushi.github.io");
        contact.extend(field_string(2, "https://discord.gg/3FbCpdKbdY"));
        out.extend(field_bytes(4, &contact));
        let mut list = Vec::new();
        for entry in entries {
            list.extend(field_bytes(1, entry));
        }
        out.extend(field_bytes(101, &list));
        out
    }

    fn gunzip_for_test(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn sibling_json_url_rewrites_pb_to_min_json() {
        let url =
            Url::parse("https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.pb")
                .unwrap();
        assert_eq!(
            sibling_json_url(&url).unwrap().as_str(),
            "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json"
        );

        let url = Url::parse("https://example.com/index.min.json").unwrap();
        assert_eq!(sibling_json_url(&url), None);
    }

    #[test]
    fn parses_plain_json_index() {
        // The plain JSON body is what a user-entered `index.min.json` URL
        // yields.
        let body = br#"[
            {"id": "en.aquamanga", "name": "Aqua Manga", "lang": "en", "version": 1},
            {"id": "royalroad", "name": "Royal Road", "lang": "English"}
        ]"#;
        let value = parse_index_bytes(body).expect("plain JSON must parse");
        assert_eq!(value.as_array().unwrap().len(), 2);
        assert_eq!(value[0]["id"], "en.aquamanga");
    }

    #[test]
    fn parses_gzipped_json_index() {
        let body = b"[{\"id\":\"en.aquamanga\",\"name\":\"Aqua Manga\"}]";
        let gz = gunzip_for_test(body);
        let value = parse_index_bytes(&gz).expect("gzipped JSON must parse");
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["name"], "Aqua Manga");
    }

    #[test]
    fn rejects_unparseable_bodies() {
        assert_eq!(parse_index_bytes(b"<!DOCTYPE html>"), None);
        assert_eq!(parse_index_bytes(b"\x00\x01\x02\x03 garbage"), None);
    }

    #[test]
    fn decodes_repo_index_entries() {
        let pb = index(&[
            extension(
                "MangaPill",
                "eu.kanade.tachiyomi.en.mangapill",
                "https://github.com/keiyoushi/extensions/releases/download/v1.4.x/tachiyomi-en.mangapill-v1.4.x.apk",
                "1.4.199",
                199,
                &["en"],
            ),
            extension(
                "Akuma",
                "eu.kanade.tachiyomi.extension.all.akuma",
                "https://github.com/keiyoushi/extensions/releases/download/88e1412-0/tachiyomi-all.akuma-v1.4.10.apk",
                "1.4.10",
                10,
                &["en", "id"],
            ),
        ]);

        let value = decode_repo_index(&pb).unwrap();
        let entries = value.as_array().unwrap();
        assert_eq!(entries.len(), 3, "one entry per (extension, language)");

        let first = &entries[0];
        assert_eq!(first["name"], "MangaPill");
        assert_eq!(first["pkg"], "eu.kanade.tachiyomi.en.mangapill");
        assert_eq!(first["apk"], "https://github.com/keiyoushi/extensions/releases/download/v1.4.x/tachiyomi-en.mangapill-v1.4.x.apk");
        assert_eq!(first["lang"], "en");
        assert_eq!(first["code"], 199);
        assert_eq!(first["version"], "1.4.199");

        let akuma: Vec<_> = entries
            .iter()
            .filter(|entry| entry["pkg"] == "eu.kanade.tachiyomi.extension.all.akuma")
            .map(|entry| entry["lang"].as_str().unwrap())
            .collect();
        assert_eq!(akuma, vec!["en", "id"]);
    }

    #[test]
    fn gunzips_and_decodes_repo_index() {
        let pb = index(&[extension(
            "AHottie",
            "eu.kanade.tachiyomi.extension.all.ahottie",
            "https://example.com/tachiyomi-all.ahottie-v1.6.4.apk",
            "1.6.4",
            4,
            &["all"],
        )]);
        let gz = gunzip_for_test(&pb);

        let raw = gunzip(&gz).unwrap();
        assert_eq!(raw, pb);
        let value = decode_repo_index(&raw).unwrap();
        let entries = value.as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]["pkg"],
            "eu.kanade.tachiyomi.extension.all.ahottie"
        );
    }

    #[test]
    fn skips_entries_without_installable_package() {
        let pb = index(&[
            extension(
                "MangaPill",
                "eu.kanade.tachiyomi.en.mangapill",
                "https://example.com/mangapill.apk",
                "1.4.199",
                199,
                &["en"],
            ),
            field_string(2, "just a badge label, not an extension"),
        ]);

        let value = decode_repo_index(&pb).unwrap();
        let entries = value.as_array().unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn missing_languages_default_to_all() {
        let pb = index(&[extension(
            "Mini",
            "eu.kanade.tachiyomi.all.mini",
            "https://example.com/mini.apk",
            "1.0.1",
            1,
            &[],
        )]);

        let value = decode_repo_index(&pb).unwrap();
        let entries = value.as_array().unwrap();
        assert_eq!(entries[0]["lang"], "all");
        assert_eq!(entries[0]["version"], "1.0.1");
    }

    #[test]
    fn test_expand_keiyoushi_ids_multi_language_package() {
        let mut entries = vec![
            (
                "eu.kanade.tachiyomi.extension.all.hentai3".to_string(),
                Some("all".to_string()),
            ),
            (
                "eu.kanade.tachiyomi.extension.all.hentai3".to_string(),
                Some("en".to_string()),
            ),
            (
                "eu.kanade.tachiyomi.extension.all.hentai3".to_string(),
                Some("ja".to_string()),
            ),
            (
                "eu.kanade.tachiyomi.extension.en.mangapill".to_string(),
                Some("en".to_string()),
            ),
        ];
        expand_keiyoushi_ids(&mut entries);
        assert_eq!(
            entries[0].0,
            "eu.kanade.tachiyomi.extension.all.hentai3:all"
        );
        assert_eq!(entries[1].0, "eu.kanade.tachiyomi.extension.all.hentai3:en");
        assert_eq!(entries[2].0, "eu.kanade.tachiyomi.extension.all.hentai3:ja");
        // Single-language packages keep the plain package id.
        assert_eq!(entries[3].0, "eu.kanade.tachiyomi.extension.en.mangapill");
    }

    #[test]
    fn test_expand_keiyoushi_ids_keeps_entry_without_lang() {
        let mut entries = vec![
            ("some.pkg".to_string(), None),
            ("some.pkg".to_string(), Some("en".to_string())),
        ];
        expand_keiyoushi_ids(&mut entries);
        // No language to append: the id stays untouched.
        assert_eq!(entries[0].0, "some.pkg");
        assert_eq!(entries[1].0, "some.pkg:en");
    }
}
