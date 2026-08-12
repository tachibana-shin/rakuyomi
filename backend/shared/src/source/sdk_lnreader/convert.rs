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
    let mut items = Vec::with_capacity(length);
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

/// Sanitizes an LNReader `SourceNovel::summary` into plain text suitable
/// for `Manga::description`.
///
/// LNReader plugins scrape descriptions as raw HTML fragments: embedded
/// `<style>`/`<script>` blocks, markup tags, HTML entities, and stray
/// whitespace. Ranobes-style summaries additionally carry bare CSS rules
/// (`.selector { ... }`) outside any `<style>` tag. Rakuyomi renders
/// `Manga::description` as plain text, so the fragment is reduced to its
/// prose — but paragraph structure is preserved, not flattened:
///
/// 1. drop `<style>`/`<script>` blocks (tags *and* their content),
/// 2. strip the remaining tags,
/// 3. drop bare CSS rules (`.selector { ... }`, single- or multi-line),
/// 4. decode HTML entities,
/// 5. collapse runs of *horizontal* whitespace inside a line to a single
///    space and trim line edges, normalizing at most one blank line
///    between paragraphs.
///
/// LNReader-only by design: Aidoku descriptions flow through their own
/// native HTML pipeline (`wasm_imports::html`) and are intentionally left
/// untouched here.
fn sanitize_summary(raw: &str) -> String {
    let without_blocks = strip_embedded_blocks(raw);
    let without_tags = strip_tags(&without_blocks);
    let without_css = strip_bare_css(&without_tags);
    let decoded = html_escape::decode_html_entities(&without_css);
    normalize_paragraphs(&decoded)
}

/// Removes `<style>`/`<script>` blocks — opening tag, body, closing tag —
/// matching tag names case-insensitively and allowing attributes on the
/// opening tag. An unclosed block drops the remainder of the string.
fn strip_embedded_blocks(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let mut out = String::with_capacity(raw.len());
    let mut from = 0;
    loop {
        let style = find_tag_start(&lower, "<style", from);
        let script = find_tag_start(&lower, "<script", from);
        let open = match (style, script) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => {
                out.push_str(&raw[from..]);
                break;
            }
        };
        out.push_str(&raw[from..open]);
        let closing = if style == Some(open) {
            "</style"
        } else {
            "</script"
        };
        match find_tag_start(&lower, closing, open) {
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

/// Removes every remaining HTML tag (`<...>`, including attributes and
/// self-closing/comment forms) from a fragment. A dangling `<` with no
/// closing `>` is kept verbatim.
fn strip_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open..];
        match after_open.find('>') {
            Some(close) => rest = &after_open[close + 1..],
            None => break,
        }
    }
    out.push_str(rest);
    out
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
    line.chars()
        .fold(0, |depth, c| match c {
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

/// Reads `SourceNovel.chapters` (`Plugin.ChapterItem[]`, see `docs.md`) into
/// `Chapter`s, one-to-one — not the `vi.hakovn` "one Aidoku chapter = one
/// volume" pattern (report v2 §2).
///
/// Reversed before returning: Rakuyomi's chapter-navigation fallback
/// (`isBeforeChapter.lua`/`findNextChapter.lua`, shared with Aidoku,
/// unmodified) assumes a source's raw chapter order runs newest -> oldest
/// whenever `chapter_num` is unavailable to compare by directly — the same
/// assumption Aidoku sources are already expected to satisfy. Confirmed
/// live against a real install (`novelbuddy`'s own database) that its
/// `parseNovel()` chapters arrive oldest-first, the opposite of that
/// assumption; reversing here makes LNReader conform to the same
/// convention Aidoku already relies on, without touching the shared Lua
/// logic itself. Not yet verified across the rest of the real LNReader
/// corpus (some plugins, e.g. `royalroad`, set `chapterNumber` directly and
/// never hit this fallback at all) — a deliberate, accepted simplification
/// for this pass; revisit with broader real-corpus testing later.
pub(super) fn chapters_from_source_novel(
    novel: &JsValue,
    source_id: &str,
    manga_id: &str,
    context: &mut Context,
) -> Result<Vec<Chapter>> {
    let chapters_value = get_prop(novel, "chapters", context)?;
    let items = js_array_to_vec(&chapters_value, context)?;

    let mut chapters = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        chapters.push(chapter_from_chapter_item(
            item, source_id, manga_id, index, context,
        )?);
    }
    chapters.reverse();
    Ok(chapters)
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
/// in practice; only the documented format is parsed here (no dependency on
/// the `dayjs` shim, which isn't wired up for native use yet — see the plan's
/// "Suite" section).
fn parse_release_time(value: &str) -> Option<chrono::DateTime<chrono_tz::Tz>> {
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
mod chapters_from_source_novel_ordering_tests {
    use super::*;

    #[test]
    fn reverses_the_plugins_own_chapter_order() {
        let mut context = Context::default();
        let novel = super::super::js_runtime::eval(
            &mut context,
            r#"({
                chapters: [
                    { path: 'c1', name: 'Chapter 1' },
                    { path: 'c2', name: 'Chapter 2' },
                    { path: 'c3', name: 'Chapter 3' },
                ],
            })"#,
            "test novel",
        )
        .expect("test snippet should evaluate");

        let chapters =
            chapters_from_source_novel(&novel, "test-source", "test-manga", &mut context)
                .expect("chapters_from_source_novel should succeed");

        let ids: Vec<&str> = chapters.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["c3", "c2", "c1"]);
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
    fn strips_tags_but_preserves_paragraph_breaks() {
        assert_eq!(
            sanitize_summary("<p>Hello</p>\n\t<p>World</p>"),
            "Hello\nWorld"
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
    fn strips_css_wrapped_in_tags_when_it_forms_its_own_line() {
        assert_eq!(
            sanitize_summary("<p>Intro</p>\n<p>.hidden { display: none }</p>\n<p>Outro</p>"),
            "Intro\nOutro"
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
            "First\nSecond"
        );
        assert_eq!(
            sanitize_summary("<p>First</p>\n\n\n\n<p>Second</p>"),
            "First\n\nSecond"
        );
        assert_eq!(sanitize_summary("\n\nFirst\n\n\nSecond\n\n"), "First\n\nSecond");
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
    fn decodes_html_entities() {
        assert_eq!(
            sanitize_summary("Tom &amp; Jerry &mdash; &quot;quoted&quot; &lt;i&gt;"),
            "Tom & Jerry — \"quoted\" <i>"
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

        assert_eq!(manga.description.as_deref(), Some("Synopsis & more"));
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
            Some("First & second para.\nThird para.")
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
