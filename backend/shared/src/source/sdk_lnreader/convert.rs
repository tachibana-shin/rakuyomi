//! `JsValue` <-> Rust conversions between the LNReader plugin contract
//! (`NovelItem`/`SourceNovel`/`ChapterItem`, see `docs/docs.md` in
//! `lnreader-plugins`) and Rakuyomi's own `Manga`/`Chapter`/`Page` (see
//! `source::model`), following the mapping in the v2 feasibility report §3.

use std::collections::HashMap;

use anyhow::{Context as _, Result};
use boa_engine::{js_string, property::PropertyKey, Context, JsValue};

use crate::source::model::{Chapter, Manga, Page, PublishingStatus};

pub(super) fn get_prop(object: &JsValue, key: &str, context: &mut Context) -> Result<JsValue> {
    let obj = object
        .as_object()
        .with_context(|| format!("expected an object while reading `{key}`"))?;
    let prop_key: PropertyKey = js_string!(key).into();
    obj.get(prop_key, context)
        .map_err(|e| anyhow::anyhow!("failed to read `{key}`: {e}"))
}

fn opt_string(object: &JsValue, key: &str, context: &mut Context) -> Result<Option<String>> {
    let value = get_prop(object, key, context)?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(
        value
            .to_string(context)
            .map_err(|e| anyhow::anyhow!("`{key}` is not a string: {e}"))?
            .to_std_string_escaped(),
    ))
}

fn req_string(object: &JsValue, key: &str, context: &mut Context) -> Result<String> {
    opt_string(object, key, context)?.with_context(|| format!("required field `{key}` is missing"))
}

fn opt_f32(object: &JsValue, key: &str, context: &mut Context) -> Result<Option<f32>> {
    let value = get_prop(object, key, context)?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(
        value
            .to_number(context)
            .map_err(|e| anyhow::anyhow!("`{key}` is not a number: {e}"))? as f32,
    ))
}

/// Reads an array-like `JsValue` (a real `Array`, or anything with a numeric
/// `.length`) into a `Vec<JsValue>`.
pub(super) fn js_array_to_vec(value: &JsValue, context: &mut Context) -> Result<Vec<JsValue>> {
    if value.is_undefined() || value.is_null() {
        return Ok(Vec::new());
    }
    let length = get_prop(value, "length", context)?
        .to_number(context)
        .map_err(|e| anyhow::anyhow!("array has no numeric `length`: {e}"))?
        as usize;

    let obj = value.as_object().context("expected an array-like object")?;
    // Not `Vec::with_capacity(length)`: `length` is plugin-controlled and
    // read off the JS value before any element is, so a plugin claiming an
    // absurd length (deliberately or not) would trigger an immediate,
    // unbounded allocation before a single real element is read. Growing
    // naturally bounds the allocation by how many elements actually exist.
    let mut items = Vec::new();
    for i in 0..length {
        let key: PropertyKey = i.into();
        let item = obj
            .get(key, context)
            .map_err(|e| anyhow::anyhow!("failed to read array element {i}: {e}"))?;
        items.push(item);
    }
    Ok(items)
}

/// Best-effort mapping of LNReader's `NovelStatus` (`@libs/novelStatus`,
/// itself just string constants — see `docs.md`'s `SourceNovel::status`) to
/// Rakuyomi's `PublishingStatus`. Plugins may also set an arbitrary raw
/// string, so unrecognized values fall back to `Unknown` rather than erroring.
fn status_from_str(value: &str) -> PublishingStatus {
    match value {
        "Ongoing" => PublishingStatus::Ongoing,
        "Completed" | "Publishing Finished" => PublishingStatus::Completed,
        "Cancelled" => PublishingStatus::Cancelled,
        "On Hiatus" => PublishingStatus::Hiatus,
        // "Licensed": the source stopped distributing it (licensing), closest
        // fit in Rakuyomi's own status set.
        "Licensed" => PublishingStatus::NotPublished,
        _ => PublishingStatus::Unknown,
    }
}

/// Converts one `Plugin.NovelItem` (`{path, name, cover?}`, see `docs.md`)
/// into a `Manga`, as returned by `popularNovels`/`searchNovels`.
pub(super) fn manga_from_novel_item(
    item: &JsValue,
    source_id: &str,
    context: &mut Context,
) -> Result<Manga> {
    let id = req_string(item, "path", context)?;
    let title = opt_string(item, "name", context)?;
    let cover_url = opt_string(item, "cover", context)?.and_then(|u| url::Url::parse(&u).ok());

    Ok(Manga {
        source_id: source_id.to_string(),
        id,
        title,
        cover_url,
        ..Default::default()
    })
}

/// Converts one `Plugin.SourceNovel` (see `docs.md`) into a `Manga`, as
/// returned by `parseNovel`. `manga_id` is the id we asked for (== the
/// `path` we called `parseNovel` with) — used as-is rather than re-read from
/// `SourceNovel::path`, since the plugin contract requires them to match
/// anyway (`docs.md`: "SourceNovel::path should be the same value as
/// NovelItem::path provided as parameter").
pub(super) fn manga_from_source_novel(
    novel: &JsValue,
    source_id: &str,
    manga_id: &str,
    context: &mut Context,
) -> Result<Manga> {
    let title = opt_string(novel, "name", context)?;
    let cover_url = opt_string(novel, "cover", context)?.and_then(|u| url::Url::parse(&u).ok());
    let author = opt_string(novel, "author", context)?;
    let artist = opt_string(novel, "artist", context)?;
    let description =
        opt_string(novel, "summary", context)?.map(|summary| sanitize_summary(&summary));
    let tags = opt_string(novel, "genres", context)?
        .map(|genres| genres.split(',').map(|s| s.trim().to_string()).collect());
    let status = opt_string(novel, "status", context)?
        .map(|s| status_from_str(&s))
        .unwrap_or_default();

    Ok(Manga {
        source_id: source_id.to_string(),
        id: manga_id.to_string(),
        title,
        author,
        artist,
        description,
        tags,
        cover_url,
        status,
        ..Default::default()
    })
}

/// Sanitizes an LNReader `SourceNovel::summary` into safe HTML suitable
/// for `Manga::description`.
///
/// LNReader plugins scrape descriptions as raw HTML fragments: embedded
/// `<style>`/`<script>`/`<iframe>` blocks, markup tags, HTML entities,
/// comments, stray whitespace, and — Ranobes-style — bare CSS rules
/// (`.selector { ... }`) outside any `<style>` tag. The shared frontend
/// renderer renders `Manga::description` as HTML, so the fragment is
/// filtered to a safe formatting subset rather than flattened to prose:
///
/// 1. drop HTML comments/declarations and embedded content blocks
///    (`<script>`, `<style>`, `<iframe>`, ... — tags *and* their content),
/// 2. drop bare CSS rules (`.selector { ... }`, single- or multi-line),
/// 3. keep only the formatting tags `p`, `br`, `hr`, `b`, `strong`, `i`,
///    `em`, `u`, `h1`-`h6`, `blockquote`, `ul`, `ol`, `li` (all attributes
///    stripped) and `a` restricted to a safe `href`
///    (`http`/`https`/`mailto` only); every other tag loses its markup
///    while its text survives,
/// 4. keep HTML entities and paragraph structure as-is — entities are NOT
///    decoded, since the output is HTML (`&amp;` must stay encoded to
///    render as `&`),
/// 5. collapse runs of *horizontal* whitespace inside a line to a single
///    space and trim line edges, normalizing at most one blank line
///    between paragraphs.
///
/// LNReader-only by design: Aidoku descriptions flow through their own
/// native HTML pipeline (`wasm_imports::html`) and are intentionally left
/// untouched here.
fn sanitize_summary(raw: &str) -> String {
    let without_comments = strip_comments_and_declarations(raw);
    let without_blocks = strip_embedded_blocks(&without_comments);
    let without_css = strip_bare_css(&without_blocks);
    let safe_markup = sanitize_markup(&without_css);
    normalize_paragraphs(&safe_markup)
}

/// Embedded content block tags whose *content* is dropped along with the
/// markup, not just the tags: scripts and styles carry active code, iframes
/// embed foreign documents, and the rest (`object`, `embed`, `svg`, `math`,
/// `form`, ...) contributes no readable prose. Void elements (`<meta>`,
/// `<link>`, `<base>`, `<img>`, ...) are intentionally absent — they have no
/// content to drop and `sanitize_markup` strips their tags anyway.
const EMBEDDED_BLOCK_TAGS: &[&str] = &[
    "script", "style", "iframe", "noscript", "template", "object", "embed", "svg", "math", "form",
    "textarea", "select", "option",
];

/// Removes embedded content blocks — opening tag, body, closing tag — for
/// every `EMBEDDED_BLOCK_TAGS` entry, matching tag names
/// case-insensitively and allowing attributes on the opening tag. An
/// unclosed block drops the remainder of the string.
fn strip_embedded_blocks(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let mut out = String::with_capacity(raw.len());
    let mut from = 0;
    loop {
        // Earliest opening of any embedded block tag at or after `from`.
        let mut earliest: Option<(usize, usize)> = None; // (position, tag index)
        for (idx, tag) in EMBEDDED_BLOCK_TAGS.iter().enumerate() {
            let open = format!("<{tag}");
            if let Some(pos) = find_tag_start(&lower, &open, from) {
                match earliest {
                    Some((ep, _)) if ep <= pos => {}
                    _ => earliest = Some((pos, idx)),
                }
            }
        }
        let Some((open, idx)) = earliest else {
            out.push_str(&raw[from..]);
            break;
        };
        out.push_str(&raw[from..open]);
        let closing = format!("</{}", EMBEDDED_BLOCK_TAGS[idx]);
        match find_tag_start(&lower, &closing, open) {
            Some(close) => {
                // Skip past the closing tag's `>` (drop the remainder if malformed).
                from = match lower[close..].find('>') {
                    Some(offset) => close + offset + 1,
                    None => raw.len(),
                };
            }
            None => break, // unterminated block: drop the rest
        }
    }
    out
}

/// Case-insensitive position of a tag-open prefix (`<style`, `</style`,
/// ...) at or after `from`, only when followed by a valid tag boundary
/// (whitespace, `>`, `/`, or end of string) so `<stylesheet>` is not
/// mistaken for `<style>`. `haystack` must be the ASCII-lowercased copy of
/// the same string `raw` slices come from (byte lengths match 1:1).
fn find_tag_start(haystack: &str, tag: &str, from: usize) -> Option<usize> {
    let mut search_from = from;
    while let Some(rel) = haystack[search_from..].find(tag) {
        let pos = search_from + rel;
        let boundary_ok = match haystack.as_bytes().get(pos + tag.len()) {
            None => true,
            Some(byte) => byte.is_ascii_whitespace() || *byte == b'>' || *byte == b'/',
        };
        if boundary_ok {
            return Some(pos);
        }
        search_from = pos + tag.len();
    }
    None
}

/// Removes HTML comments (`<!-- ... -->`), CDATA sections (`<![CDATA[
/// ...]]>`), doctype declarations (`<!DOCTYPE ...>`) and processing
/// instructions (`<?...?>`) — including their content. An unterminated
/// comment/CDATA drops the remainder of the string. Any other `<` is kept
/// verbatim.
fn strip_comments_and_declarations(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    loop {
        let Some(lt) = rest.find('<') else {
            out.push_str(rest);
            break;
        };
        let after = &rest[lt..];
        let (terminator, len) = if after.starts_with("<!--") {
            (Some("-->"), 3)
        } else if after.starts_with("<![CDATA[") {
            (Some("]]>"), 3)
        } else if after.starts_with("<!") || after.starts_with("<?") {
            (None, 1)
        } else {
            out.push_str(&rest[..lt + 1]);
            rest = &after[1..];
            continue;
        };
        out.push_str(&rest[..lt]);
        match terminator {
            Some(term) => match after.find(term) {
                Some(end) => rest = &after[end + len..],
                None => break, // unterminated: drop the rest
            },
            None => match after.find('>') {
                Some(end) => rest = &after[end + 1..],
                None => break,
            },
        }
    }
    out
}

/// Formatting tags kept by `sanitize_markup` (attribute-free except for
/// `a`, see `safe_href`).
const ALLOWED_TAGS: &[&str] = &[
    "p",
    "br",
    "hr",
    "b",
    "strong",
    "i",
    "em",
    "u",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "ul",
    "ol",
    "li",
    "a",
];

/// Void elements among `ALLOWED_TAGS`: emitted in bare form and never
/// tracked for closing tags.
const VOID_TAGS: &[&str] = &["br", "hr"];

/// Filters a fragment down to the safe formatting subset: every `ALLOWED_TAGS`
/// tag is kept (with attributes filtered), while disallowed tags lose their
/// markup but their text survives. Comments/declarations and bare CSS rules
/// that survived the earlier passes are removed here too.
fn sanitize_markup(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    // Names of opening tags whose markup was dropped; their matching closing
    // tags are dropped as well so disallowed markup never leaves orphaned
    // close tags behind.
    let mut suppressed: Vec<String> = Vec::new();

    loop {
        let Some(lt) = rest.find('<') else {
            out.push_str(&strip_bare_css(rest));
            break;
        };
        let after = &rest[lt..];

        // Comments/declarations: drop through the terminator.
        if after.starts_with("<!--") {
            out.push_str(&strip_bare_css(&rest[..lt]));
            match after.find("-->") {
                Some(end) => rest = &after[end + 3..],
                None => break,
            }
            continue;
        }
        if after.starts_with("<![CDATA[") {
            out.push_str(&strip_bare_css(&rest[..lt]));
            match after.find("]]>") {
                Some(end) => rest = &after[end + 3..],
                None => break,
            }
            continue;
        }
        if after.starts_with("<!") || after.starts_with("<?") {
            out.push_str(&strip_bare_css(&rest[..lt]));
            match after.find('>') {
                Some(end) => rest = &after[end + 1..],
                None => break,
            }
            continue;
        }

        // Only treat `<` as a tag start when followed by a plausible tag
        // character; "a < b" stays literal text.
        let Some(&next_byte) = after.as_bytes().get(1) else {
            out.push_str(&strip_bare_css(rest));
            break;
        };
        let is_tag_start =
            next_byte.is_ascii_alphabetic() || matches!(next_byte, b'/' | b'!' | b'?');
        if !is_tag_start {
            out.push_str(&strip_bare_css(&rest[..lt + 1]));
            rest = &after[1..];
            continue;
        }

        let Some(tag_end) = find_tag_end(after) else {
            // Dangling `<` with no closing `>`: keep the rest verbatim.
            out.push_str(&strip_bare_css(&rest[..lt]));
            out.push_str(&strip_bare_css(after));
            break;
        };
        out.push_str(&strip_bare_css(&rest[..lt]));
        let tag = &after[..=tag_end];
        let (name, closing, href) = parse_tag(tag);

        if closing {
            // Closing tag: drop it if it pairs a dropped opening tag, drop
            // void closing tags, otherwise emit it for allowed tags.
            if let Some(pos) = suppressed.iter().rposition(|n| n == &name) {
                suppressed.remove(pos);
            } else if ALLOWED_TAGS.contains(&name.as_str()) && !VOID_TAGS.contains(&name.as_str()) {
                out.push_str("</");
                out.push_str(&name);
                out.push('>');
            }
        } else if VOID_TAGS.contains(&name.as_str()) {
            out.push('<');
            out.push_str(&name);
            out.push('>');
        } else if ALLOWED_TAGS.contains(&name.as_str()) {
            if name == "a" {
                match href.and_then(|value| safe_href(&value)) {
                    Some(value) => {
                        out.push_str("<a href=\"");
                        out.push_str(&html_escape::encode_double_quoted_attribute(&value));
                        out.push_str("\">");
                    }
                    None => suppressed.push(name),
                }
            } else {
                out.push('<');
                out.push_str(&name);
                out.push('>');
            }
        } else {
            suppressed.push(name);
        }

        rest = &after[tag_end + 1..];
    }
    out
}

/// Position of the `>` closing a tag that starts at the beginning of
/// `after` (`<...`), skipping over quoted attribute values so `href="a>b"`
/// is not cut short. `None` when the tag is unterminated.
fn find_tag_end(after: &str) -> Option<usize> {
    let bytes = after.as_bytes();
    let mut in_quote: Option<u8> = None;
    for (i, &byte) in bytes.iter().enumerate().skip(1) {
        match in_quote {
            Some(quote) => {
                if byte == quote {
                    in_quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => in_quote = Some(byte),
                b'>' => return Some(i),
                _ => {}
            },
        }
    }
    None
}

/// Parses one tag (with surrounding `<`/`>`) into its lowercased name,
/// whether it is a closing tag, and — for opening tags — the value of the
/// first `href` attribute if any. Every other attribute is discarded: only
/// `a`'s `href` can survive sanitization.
fn parse_tag(tag: &str) -> (String, bool, Option<String>) {
    let inner = tag
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or("");
    let bytes = inner.as_bytes();
    let mut i = 0;
    let mut closing = false;
    if bytes.first() == Some(&b'/') {
        closing = true;
        i = 1;
    }
    while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    let name_start = i;
    while i < bytes.len() && !(bytes[i] as char).is_ascii_whitespace() && bytes[i] != b'/' {
        i += 1;
    }
    let name = String::from_utf8_lossy(&bytes[name_start..i]).to_ascii_lowercase();
    if closing {
        return (name, true, None);
    }

    let mut href: Option<String> = None;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'/' {
            break;
        }
        let attr_start = i;
        while i < bytes.len() && !(bytes[i] as char).is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let attr_name = String::from_utf8_lossy(&bytes[attr_start..i]).to_ascii_lowercase();
        let mut j = i;
        while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'=' {
            j += 1;
            while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            let value = if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                let quote = bytes[j];
                j += 1;
                let value_start = j;
                while j < bytes.len() && bytes[j] != quote {
                    j += 1;
                }
                let value = String::from_utf8_lossy(&bytes[value_start..j]).into_owned();
                if j < bytes.len() {
                    j += 1;
                }
                value
            } else {
                let value_start = j;
                while j < bytes.len()
                    && !(bytes[j] as char).is_ascii_whitespace()
                    && bytes[j] != b'>'
                {
                    j += 1;
                }
                String::from_utf8_lossy(&bytes[value_start..j]).into_owned()
            };
            if attr_name == "href" && href.is_none() {
                href = Some(value);
            }
            i = j;
        } else {
            // Boolean attribute (no value).
            i = j;
        }
    }
    (name, false, href)
}

/// Accepts an `href` whose scheme is `http`, `https`, or `mailto`
/// (case-insensitive); everything else (`javascript:`, `data:`, bare
/// relative paths, ...) is rejected. The value is entity-decoded before
/// validation so entity-obfuscated schemes (`jav&#x61;script:`) cannot slip
/// through, and the *decoded* value is returned — the caller re-encodes it
/// for emission, keeping the round-trip single-encoded. Values containing
/// whitespace or control characters are rejected too, so scheme obfuscation
/// (`java\nscript:`) cannot slip through.
fn safe_href(value: &str) -> Option<String> {
    let decoded = html_escape::decode_html_entities(value.trim());
    if decoded.is_empty() || decoded.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let lower = decoded.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        Some(decoded.into_owned())
    } else {
        None
    }
}

/// Removes bare CSS rules that Ranobes-style summaries carry outside any
/// `<style>` tag: recognizable `.selector { ... }` / `#id { ... }` rules,
/// possibly spanning multiple lines. A rule starts on a line whose first
/// non-whitespace char is `.` or `#` and whose selector prefix is
/// selector-legal up to a `{` on that same line; everything from there
/// through the balancing `}` is dropped, using brace depth so nested
/// blocks close correctly. An unterminated rule drops through to the end
/// of the string. Lines that merely contain braces (ordinary prose) are
/// left alone.
fn strip_bare_css(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut lines = raw.split_inclusive('\n');
    while let Some(line) = lines.next() {
        if !css_rule_starts(line) {
            out.push_str(line);
            continue;
        }
        let mut depth = brace_delta(line);
        while depth > 0 {
            match lines.next() {
                Some(next) => depth += brace_delta(next),
                None => break,
            }
        }
    }
    out
}

/// True when `line` starts a bare CSS rule: after leading whitespace, a
/// `.class` / `#id` selector whose first character is a valid identifier
/// start and whose remaining selector characters are all selector-legal,
/// terminated by a `{` on the same line.
fn css_rule_starts(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    match chars.next() {
        Some('.') | Some('#') => {}
        _ => return false,
    }
    // A bare identifier can't begin with a digit, so requiring a letter
    // start also keeps prose like ".5 miles { ... }" out of the filter.
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '-' || c == '_')
    {
        return false;
    }
    let Some(open) = trimmed.find('{') else {
        return false;
    };
    trimmed[..open].chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '-' | '_'
                    | '.'
                    | '#'
                    | ':'
                    | '['
                    | ']'
                    | '('
                    | ')'
                    | ','
                    | '>'
                    | '+'
                    | '~'
                    | '*'
                    | '='
                    | '"'
                    | '\''
                    | '&'
                    | '%'
                    | '!'
                    | '/'
                    | ' '
            )
    })
}

/// Net brace depth of a line (`{` opens, `}` closes) — used to find the
/// end of a multi-line CSS rule.
fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |depth, c| match c {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
}

/// Preserves paragraph structure while cleaning line-level whitespace.
/// Runs of horizontal whitespace (spaces, tabs, `\r`, ...) inside a line
/// collapse to a single space and each line's edges are trimmed; runs of
/// blank lines between paragraphs are capped at one empty line, and
/// leading/trailing blank lines are dropped. Newlines that separate
/// paragraphs survive.
fn normalize_paragraphs(raw: &str) -> String {
    // Pass 1: collapse horizontal whitespace per line.
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut pending_space = false;
    for ch in raw.chars() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut current));
            pending_space = false;
        } else if ch.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !current.is_empty() {
                current.push(' ');
            }
            pending_space = false;
            current.push(ch);
        }
    }
    lines.push(current);

    // Pass 2: join, capping blank-line runs at one empty line between
    // paragraphs and dropping blank lines at the edges.
    let mut out = String::with_capacity(raw.len());
    let mut pending_blank = false; // a blank line seen since last content
    let mut started = false;
    for line in lines {
        if line.is_empty() {
            if started {
                pending_blank = true;
            }
            continue;
        }
        if started {
            if pending_blank {
                out.push_str("\n\n");
            } else {
                out.push('\n');
            }
        }
        pending_blank = false;
        started = true;
        out.push_str(&line);
    }
    out
}

/// Converts raw `Plugin.ChapterItem[]` JsValues (see `docs.md`) into
/// `Chapter`s, one-to-one — not the `vi.hakovn` "one Aidoku chapter = one
/// volume" pattern (report v2 §2). `source_order_base` is the `source_order`
/// of the first item, so a caller paginating via `parsePage` can convert
/// each page with a running offset and keep `source_order` continuous across
/// the whole concatenated list.
///
/// Deliberately does NOT reverse: the caller owns the final ordering. The
/// single reversal happens exactly once, on the fully concatenated list
/// (`worker.rs::parse_and_convert_novel`) — never per page — so the global
/// newest-first order survives; see that function's doc comment for the
/// rationale.
pub(super) fn chapters_from_chapter_items(
    items: &[JsValue],
    source_id: &str,
    manga_id: &str,
    source_order_base: usize,
    context: &mut Context,
) -> Result<Vec<Chapter>> {
    let mut chapters = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        chapters.push(chapter_from_chapter_item(
            item,
            source_id,
            manga_id,
            source_order_base + index,
            context,
        )?);
    }
    Ok(chapters)
}

/// Reads `SourceNovel.totalPages` — how many pages the plugin's chapter list
/// spans. `parseNovel()` returns page 1's chapters inside
/// `SourceNovel.chapters`; pages 2..=totalPages are fetched via
/// `parsePage(novelId, page)` (see `worker.rs::parse_and_convert_novel`).
/// A missing/absent `totalPages` (or one that doesn't read as a number)
/// means a single-page list: `1`, so single-page sources keep the exact
/// pre-pagination behavior with zero `parsePage` calls.
///
/// Capped at [`MAX_TOTAL_PAGES`] regardless of what the plugin declares --
/// `parse_and_convert_novel` loops `2..=totalPages`, each iteration a real
/// `parsePage` call, so an unbounded value here is unbounded plugin-directed
/// work (real novel listings top out at a few hundred pages at most).
pub(super) fn source_novel_total_pages(novel: &JsValue, context: &mut Context) -> Result<usize> {
    const MAX_TOTAL_PAGES: usize = 1000;

    let value = get_prop(novel, "totalPages", context)?;
    if value.is_undefined() || value.is_null() {
        return Ok(1);
    }
    let pages = value
        .to_number(context)
        .map_err(|e| anyhow::anyhow!("`totalPages` is not a number: {e}"))?
        as usize;
    Ok(pages.clamp(1, MAX_TOTAL_PAGES))
}

fn chapter_from_chapter_item(
    item: &JsValue,
    source_id: &str,
    manga_id: &str,
    source_order: usize,
    context: &mut Context,
) -> Result<Chapter> {
    let id = req_string(item, "path", context)?;
    let title = opt_string(item, "name", context)?;
    let chapter_num = opt_f32(item, "chapterNumber", context)?;
    let date_uploaded =
        opt_string(item, "releaseTime", context)?.and_then(|s| parse_release_time(&s));

    Ok(Chapter {
        source_id: source_id.to_string(),
        id,
        manga_id: manga_id.to_string(),
        title,
        chapter_num,
        date_uploaded,
        source_order,
        ..Default::default()
    })
}

/// `ChapterItem::releaseTime` is documented as `YYYY-MM-DD` but plugins vary
/// in practice. Two shapes are parsed here, in the order real sources emit
/// them:
///
/// 1. a full RFC3339 timestamp (`2021-06-27T02:06:47.000Z`) — what a plugin
///    produces when it converts a scraped date string through
///    `Date#toISOString()` (Ranobes-style, via the `Date` shim in
///    `js_runtime.rs`'s runtime prelude), and
/// 2. the documented bare `YYYY-MM-DD` date (parsed as midnight UTC).
///
/// The raw space-separated shape (`2021-06-27 02:06:47`) deliberately does
/// NOT parse here: that string only reaches a `releaseTime` after a plugin
/// has run it through `Date`, at which point it is already RFC3339 (see the
/// `Date` shim). No dependency on the `dayjs` shim, which isn't wired up for
/// native use yet — see the plan's "Suite" section.
fn parse_release_time(value: &str) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(dt.with_timezone(&chrono_tz::UTC));
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().with_timezone(&chrono_tz::UTC))
}

/// Reads a plain `{key: value}` JS object (e.g. `PluginBase.imageRequestInit.headers`)
/// into a `HashMap<String, String>`. Non-string keys (symbols) are skipped;
/// non-string values are coerced via `.to_string()`.
pub(super) fn js_object_to_string_map(
    value: &JsValue,
    context: &mut Context,
) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    if value.is_undefined() || value.is_null() {
        return Ok(map);
    }
    let obj = value.as_object().context("expected an object")?;
    for key in obj
        .own_property_keys(context)
        .map_err(|e| anyhow::anyhow!("failed to enumerate object keys: {e}"))?
    {
        let PropertyKey::String(key_str) = &key else {
            continue;
        };
        let value = obj.get(key.clone(), context).map_err(|e| {
            anyhow::anyhow!("failed to read `{}`: {e}", key_str.to_std_string_escaped())
        })?;
        let value_str = value
            .to_string(context)
            .map_err(|e| anyhow::anyhow!("failed to stringify value: {e}"))?
            .to_std_string_escaped();
        map.insert(key_str.to_std_string_escaped(), value_str);
    }
    Ok(map)
}

/// Builds the single `Page` for a chapter's HTML content, per the "1 chapter
/// = 1 Page" strategy (report v2 §2) — `chapter_downloader.rs` treats a page
/// with `.text` set as a novel chapter and assembles it into the EPUB
/// automatically, no changes needed there.
///
/// `html` is always real, already-scraped markup from `parseChapter()` — the
/// plugin's own DOM read of the chapter page — never markdown/plain prose.
/// `chapter_downloader.rs`'s `into_html()` treats `.text` as markdown by
/// default and only skips that conversion for text prefixed with a literal
/// `<!-- html -->` marker (its own escape hatch for sources whose `.text` is
/// real HTML); LNReader content needs that marker unconditionally, every
/// time, or its own tags (`<p>`, `<div>`, ...) get fed into the markdown
/// parser as literal text instead of being rendered.
pub(super) fn page_from_chapter_html(
    html: String,
    source_id: &str,
    chapter_id: &str,
    title: Option<String>,
) -> Page {
    Page {
        source_id: source_id.to_string(),
        chapter_id: chapter_id.to_string(),
        index: 0,
        image_url: None,
        base64: title,
        text: Some(format!("<!-- html -->{html}")),
        ctx: None,
    }
}

#[cfg(test)]
mod page_from_chapter_html_tests {
    use super::*;

    #[test]
    fn prepends_the_html_marker_so_downstream_skips_markdown_conversion() {
        let page = page_from_chapter_html(
            "<p>First paragraph.</p><p>Second paragraph.</p>".to_string(),
            "novelbuddy",
            "some-chapter-id",
            Some("Chapter 1".to_string()),
        );

        let text = page.text.expect("page_from_chapter_html always sets .text");
        assert!(text.starts_with("<!-- html -->"));
        assert!(text.contains("<p>First paragraph.</p>"));
        assert!(text.contains("<p>Second paragraph.</p>"));
    }

    #[test]
    fn marker_makes_crate_util_into_html_treat_real_chapter_markup_as_html_passthrough() {
        // Regression test for the bug this marker fixes: without it,
        // `into_html` (chapter_downloader.rs's markdown/HTML dispatch) fed
        // real scraped HTML into the markdown parser, which doesn't
        // recognize compact, single-line HTML as an HTML block and left the
        // tags visible as literal text with no paragraph breaks.
        let page = page_from_chapter_html(
            "<p>First paragraph.</p><p>Second paragraph.</p>".to_string(),
            "novelbuddy",
            "some-chapter-id",
            None,
        );

        let rendered = crate::util::into_html(&page.text.unwrap());
        assert_eq!(rendered, "<p>First paragraph.</p><p>Second paragraph.</p>");
    }
}

#[cfg(test)]
mod chapters_from_chapter_items_tests {
    use super::*;

    /// `chapters_from_chapter_items` must keep the raw list order and number
    /// `source_order` continuously from the caller's base — the reversal
    /// that used to live in `chapters_from_source_novel` is now done exactly
    /// once over the *concatenated* paginated list in
    /// `worker.rs::parse_and_convert_novel`, and this helper is the page-
    /// order-preserving half of that (see its doc comment).
    #[test]
    fn preserves_list_order_with_continuous_source_order() {
        let mut context = Context::default();
        let items = super::super::js_runtime::eval(
            &mut context,
            r#"([
                { path: 'c1', name: 'Chapter 1' },
                { path: 'c2', name: 'Chapter 2' },
                { path: 'c3', name: 'Chapter 3' },
            ])"#,
            "test chapter items",
        )
        .expect("test snippet should evaluate");
        let items = js_array_to_vec(&items, &mut context).expect("items should convert");

        // Page 2 of a paginated list: source_order continues from page 1's
        // count (here 3) instead of restarting at 0.
        let chapters =
            chapters_from_chapter_items(&items, "test-source", "test-manga", 3, &mut context)
                .expect("chapters_from_chapter_items should succeed");

        let ids: Vec<&str> = chapters.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "c2", "c3"]);

        let orders: Vec<usize> = chapters.iter().map(|c| c.source_order).collect();
        assert_eq!(orders, vec![3, 4, 5]);
    }

    #[test]
    fn source_novel_total_pages_defaults_to_one_when_missing() {
        let mut context = Context::default();
        let novel =
            super::super::js_runtime::eval(&mut context, r#"({ chapters: [] })"#, "test novel")
                .expect("test snippet should evaluate");

        assert_eq!(
            source_novel_total_pages(&novel, &mut context).expect("total pages should read"),
            1
        );
    }

    #[test]
    fn source_novel_total_pages_reads_the_declared_page_count() {
        let mut context = Context::default();
        let novel = super::super::js_runtime::eval(
            &mut context,
            r#"({ chapters: [], totalPages: 25 })"#,
            "test novel",
        )
        .expect("test snippet should evaluate");

        assert_eq!(
            source_novel_total_pages(&novel, &mut context).expect("total pages should read"),
            25
        );
    }

    /// `releaseTime` values in both supported shapes (full RFC3339
    /// timestamp — the Ranobes `Date#toISOString()` pipeline — and the
    /// documented bare `YYYY-MM-DD`) must land in `Chapter.date_uploaded`.
    #[test]
    fn parses_release_time_rfc3339_and_bare_date_values() {
        let mut context = Context::default();
        let items = super::super::js_runtime::eval(
            &mut context,
            r#"([
                { path: 'c1', name: 'Chapter 1', releaseTime: '2021-06-27T02:06:47.000Z' },
                { path: 'c2', name: 'Chapter 2', releaseTime: '2021-06-27' },
            ])"#,
            "test chapter items",
        )
        .expect("test snippet should evaluate");
        let items = js_array_to_vec(&items, &mut context).expect("items should convert");

        let chapters =
            chapters_from_chapter_items(&items, "test-source", "test-manga", 0, &mut context)
                .expect("chapters_from_chapter_items should succeed");

        assert_eq!(
            chapters[0].date_uploaded.map(|d| d.to_rfc3339()),
            Some("2021-06-27T02:06:47+00:00".to_string())
        );
        assert_eq!(
            chapters[1].date_uploaded.map(|d| d.to_rfc3339()),
            Some("2021-06-27T00:00:00+00:00".to_string())
        );
    }
}

#[cfg(test)]
mod parse_release_time_tests {
    use super::*;

    #[test]
    fn parses_rfc3339_timestamps_and_bare_dates() {
        assert_eq!(
            parse_release_time("2021-06-27T02:06:47.000Z").map(|d| d.to_rfc3339()),
            Some("2021-06-27T02:06:47+00:00".to_string())
        );
        assert_eq!(
            parse_release_time("2021-06-27").map(|d| d.to_rfc3339()),
            Some("2021-06-27T00:00:00+00:00".to_string())
        );
    }

    #[test]
    fn space_separated_datetimes_are_normalized_by_the_js_date_shim_not_here() {
        // The space-separated shape (Ranobes raw `date`) is normalized to
        // `T` by the `Date` shim in `js_runtime.rs` before a plugin's
        // `new Date(...).toISOString()` runs; a plugin passing the RAW string
        // straight into `releaseTime` (never through `Date`) still yields no
        // timestamp here. This documents the boundary, not a missing format.
        assert_eq!(parse_release_time("2021-06-27 02:06:47"), None);
    }
}

#[cfg(test)]
mod sanitize_summary_tests {
    use super::*;

    #[test]
    fn strips_style_blocks_including_their_content() {
        assert_eq!(
            sanitize_summary("A <style>.hidden { display: none; }</style> summary"),
            "A summary"
        );
    }

    #[test]
    fn strips_script_blocks_including_their_content() {
        assert_eq!(
            sanitize_summary("Intro<script>void 0</script> outro"),
            "Intro outro"
        );
    }

    #[test]
    fn matches_style_and_script_case_insensitively() {
        assert_eq!(
            sanitize_summary("Before<STYLE>p{}</STYLE><Script>x</Script>After"),
            "BeforeAfter"
        );
    }

    #[test]
    fn strips_iframe_blocks_including_their_content() {
        assert_eq!(
            sanitize_summary(
                "<iframe src=\"https://evil.example\">fallback text</iframe><p>after</p>"
            ),
            "<p>after</p>"
        );
    }

    #[test]
    fn keeps_paragraph_tags_while_normalizing_the_gaps() {
        assert_eq!(
            sanitize_summary("<p>Hello</p>\n\t<p>World</p>"),
            "<p>Hello</p>\n<p>World</p>"
        );
    }

    #[test]
    fn strips_bare_css_rules_ranobes_style() {
        let summary = "Blurb\n.chapter-content { color: #333; font-size: 14px; }\nMore blurb";
        assert_eq!(sanitize_summary(summary), "Blurb\nMore blurb");
    }

    #[test]
    fn strips_multiline_bare_css_rules() {
        let summary = "Start\n.chapter-body {\n  font-family: serif;\n  line-height: 1.6;\n}\nEnd";
        assert_eq!(sanitize_summary(summary), "Start\nEnd");
    }

    #[test]
    fn strips_css_inside_tags_but_keeps_the_paragraphs() {
        assert_eq!(
            sanitize_summary("<p>Intro</p>\n<p>.hidden { display: none }</p>\n<p>Outro</p>"),
            "<p>Intro</p>\n<p></p>\n<p>Outro</p>"
        );
    }

    #[test]
    fn leaves_prose_containing_braces_alone() {
        let prose = "He wrote {a draft} and stopped.";
        assert_eq!(sanitize_summary(prose), prose);
    }

    #[test]
    fn preserves_paragraph_breaks_and_caps_blank_line_runs() {
        assert_eq!(
            sanitize_summary("<p>First</p>\n<p>Second</p>"),
            "<p>First</p>\n<p>Second</p>"
        );
        assert_eq!(
            sanitize_summary("<p>First</p>\n\n\n\n<p>Second</p>"),
            "<p>First</p>\n\n<p>Second</p>"
        );
        assert_eq!(
            sanitize_summary("\n\nFirst\n\n\nSecond\n\n"),
            "First\n\nSecond"
        );
    }

    #[test]
    fn collapses_horizontal_whitespace_within_lines_only() {
        let summary = "Line one\t   with  extra  spacing\n   indented line  ";
        assert_eq!(
            sanitize_summary(summary),
            "Line one with extra spacing\nindented line"
        );
    }

    #[test]
    fn preserves_html_entities_so_they_render_correctly() {
        // The output is HTML: entities must stay encoded (`&amp;` renders as
        // `&`, and `&lt;i&gt;` must stay literal text, not become italic).
        assert_eq!(
            sanitize_summary("Tom &amp; Jerry &mdash; &quot;quoted&quot; &lt;i&gt;"),
            "Tom &amp; Jerry &mdash; &quot;quoted&quot; &lt;i&gt;"
        );
    }

    #[test]
    fn keeps_bold_italic_and_underline_markup() {
        let summary =
            "<p><b>bold</b> <strong>strong</strong> <i>italic</i> <em>em</em> <u>under</u></p>";
        assert_eq!(sanitize_summary(summary), summary);
    }

    #[test]
    fn normalizes_tag_case() {
        assert_eq!(
            sanitize_summary("<B>bold</B> <STRONG>x</STRONG>"),
            "<b>bold</b> <strong>x</strong>"
        );
    }

    #[test]
    fn keeps_headings_blockquote_and_horizontal_rules() {
        let summary = "<h1>Title</h1>\n<h2>Sub</h2>\n<blockquote>Quote</blockquote>\na<br>b<hr>c";
        assert_eq!(sanitize_summary(summary), summary);
    }

    #[test]
    fn keeps_ordered_and_unordered_lists() {
        let summary = "<ul><li>One</li><li>Two</li></ul>\n<ol><li>a</li></ol>";
        assert_eq!(sanitize_summary(summary), summary);
    }

    #[test]
    fn keeps_safe_links_and_strips_every_other_attribute() {
        assert_eq!(
            sanitize_summary(
                "<a href=\"https://example.com/page?q=1&amp;r=2\" target=\"_blank\" style=\"color:red\" onclick=\"evil()\">Link</a>"
            ),
            "<a href=\"https://example.com/page?q=1&amp;r=2\">Link</a>"
        );
    }

    #[test]
    fn keeps_http_https_and_mailto_links() {
        let summary = "<a href=\"http://a.example\">h</a> <a href=\"https://b.example\">s</a> <a href=\"mailto:x@y.example\">m</a>";
        assert_eq!(sanitize_summary(summary), summary);
    }

    #[test]
    fn strips_javascript_uri_links_but_keeps_their_text() {
        assert_eq!(
            sanitize_summary("<a href=\"javascript:alert(1)\">click</a>"),
            "click"
        );
    }

    #[test]
    fn strips_entity_obfuscated_javascript_links() {
        assert_eq!(
            sanitize_summary("<a href=\"jav&#x61;script:alert(1)\">x</a>"),
            "x"
        );
    }

    #[test]
    fn strips_inline_event_and_style_attributes() {
        assert_eq!(
            sanitize_summary("<p onclick=\"evil()\" style=\"color:red\">text</p>"),
            "<p>text</p>"
        );
    }

    #[test]
    fn drops_unknown_tags_but_keeps_their_text() {
        assert_eq!(
            sanitize_summary("<div><span>kept</span></div> <font color=\"red\">text</font> <img src=\"x\" onerror=\"evil()\">"),
            "kept text"
        );
    }

    #[test]
    fn strips_comments_and_doctype_declarations() {
        assert_eq!(
            sanitize_summary("before<!-- comment with > inside -->after <!DOCTYPE html><p>ok</p>"),
            "beforeafter <p>ok</p>"
        );
    }

    #[test]
    fn passes_clean_prose_through_unchanged() {
        let prose = "A perfectly clean description with normal spacing.";
        assert_eq!(sanitize_summary(prose), prose);
    }

    #[test]
    fn trims_leading_and_trailing_whitespace() {
        assert_eq!(sanitize_summary("  \n\t padded \n "), "padded");
    }

    #[test]
    fn integrates_into_manga_from_source_novel() {
        let mut context = Context::default();
        let novel = super::super::js_runtime::eval(
            &mut context,
            r#"({
                name: 'Test Novel',
                summary: '<style>p{color:red}</style><p>Synopsis &amp; more</p>\n<script>void 0</script>',
            })"#,
            "test novel",
        )
        .expect("test snippet should evaluate");

        let manga = manga_from_source_novel(&novel, "test-source", "test-manga", &mut context)
            .expect("manga_from_source_novel should succeed");

        assert_eq!(
            manga.description.as_deref(),
            Some("<p>Synopsis &amp; more</p>")
        );
    }

    #[test]
    fn manga_integration_preserves_paragraphs_and_filters_css() {
        let mut context = Context::default();
        let novel = super::super::js_runtime::eval(
            &mut context,
            r#"({
                name: 'Test Novel',
                summary: '<p>First &amp; second para.</p>\n.chapter-meta { color: gray; }\n<p>Third para.</p>',
            })"#,
            "test novel",
        )
        .expect("test snippet should evaluate");

        let manga = manga_from_source_novel(&novel, "test-source", "test-manga", &mut context)
            .expect("manga_from_source_novel should succeed");

        assert_eq!(
            manga.description.as_deref(),
            Some("<p>First &amp; second para.</p>\n<p>Third para.</p>")
        );
    }

    #[test]
    fn leaves_description_none_when_summary_is_missing() {
        let mut context = Context::default();
        let novel = super::super::js_runtime::eval(
            &mut context,
            r#"({ name: 'Test Novel' })"#,
            "test novel",
        )
        .expect("test snippet should evaluate");

        let manga = manga_from_source_novel(&novel, "test-source", "test-manga", &mut context)
            .expect("manga_from_source_novel should succeed");

        assert_eq!(manga.description, None);
    }
}
