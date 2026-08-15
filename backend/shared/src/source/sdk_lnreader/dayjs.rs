//! Native `require('dayjs')` binding: date parsing/formatting/arithmetic via
//! `chrono` (already a dependency), not a second date library reimplemented
//! in JS. Not exercised by any of the 5 local test fixtures (none of them
//! call dayjs at all) — implemented for fidelity with the wider ~133-source
//! corpus this SDK targets, prioritized by real usage frequency there:
//! `.format(token)` first (`LL`/`LLL`/`DD MMMM YYYY`-style tokens), then
//! `.subtract()`, then `.diff()`/`.add()`/`.fromNow()`.
//!
//! Everything runs in UTC — there's no browser/OS locale or timezone
//! context to inherit here, unlike real dayjs running in a device WebView.
//! `LL`/`LLL` are rendered using English token expansions (dayjs's
//! `localizedFormat` plugin defaults); a readable but not perfectly
//! localized result is an accepted tradeoff for this MVP.

use boa_engine::{
    js_string, native_function::NativeFunction, object::FunctionObjectBuilder, Context, JsArgs,
    JsResult, JsValue,
};
use chrono::{
    DateTime, Datelike, Duration, Months, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
};

use super::arg_string;

fn arg_f64(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<f64> {
    args.get_or_undefined(index).to_number(context)
}

fn now_ms() -> f64 {
    Utc::now().timestamp_millis() as f64
}

fn ms_to_datetime(ms: f64) -> Option<DateTime<Utc>> {
    if !ms.is_finite() {
        return None;
    }
    Utc.timestamp_millis_opt(ms as i64).single()
}

/// Tries, in order: RFC3339, a handful of common `NaiveDateTime`/`NaiveDate`
/// formats seen in scraped novel/chapter listings, then a bare integer
/// string (epoch milliseconds — dayjs treats a numeric argument this way,
/// and some plugin code passes that same value through as a string).
fn parse_flexible(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis());
    }
    const DATETIME_FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y/%m/%d %H:%M:%S",
        "%d-%m-%Y %H:%M:%S",
    ];
    for fmt in DATETIME_FORMATS {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&dt).timestamp_millis());
        }
    }
    const DATE_FORMATS: &[&str] = &[
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%d-%m-%Y",
        "%d/%m/%Y",
        "%m/%d/%Y",
        "%b %d, %Y",
        "%B %d, %Y",
        "%d %B %Y",
        "%d %b %Y",
    ];
    for fmt in DATE_FORMATS {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            if let Some(dt) = d.and_hms_opt(0, 0, 0) {
                return Some(Utc.from_utc_datetime(&dt).timestamp_millis());
            }
        }
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse::<i64>().ok();
    }
    None
}

/// Expands dayjs's `localizedFormat` plugin macros to their underlying
/// English token patterns — real plugin code calls `.format('LL')` etc.
/// expecting that plugin to already be loaded; we skip the plugin
/// indirection and go straight to the token pattern it would produce.
///
/// Scans `token` for a macro name wherever it appears (real plugin code
/// often composes one with literal text/other tokens, e.g.
/// `.format('LL, [at] LT')`, not just a bare macro alone), skipping
/// bracket-literal (`[...]`) sections the same way `render()` does below --
/// consistent with `render()`'s own bracket-aware, longest-match-first
/// token scan, since this expansion's output is fed straight into it.
fn expand_locale_macros(token: &str) -> String {
    const MACROS: &[(&str, &str)] = &[
        ("LLLL", "dddd, MMMM D, YYYY h:mm A"),
        ("LLL", "MMMM D, YYYY h:mm A"),
        ("LTS", "h:mm:ss A"),
        ("LL", "MMMM D, YYYY"),
        ("LT", "h:mm A"),
        ("L", "MM/DD/YYYY"),
    ];

    let chars: Vec<char> = token.chars().collect();
    let mut out = String::with_capacity(token.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == ']') {
                // Keep the delimiters: `render()` is the pass that actually
                // strips them, and it must still see this section as a
                // literal rather than plain text open to its own token scan.
                out.extend(&chars[i..i + 1 + end + 1]);
                i += end + 2;
                continue;
            }
        }
        let rest: String = chars[i..].iter().collect();
        if let Some((matched, expansion)) =
            MACROS.iter().find(|(pattern, _)| rest.starts_with(pattern))
        {
            out.push_str(expansion);
            i += matched.chars().count();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Ordinal suffix for the `Do` token (1st, 2nd, 3rd, 4th, 11th, ...).
fn ordinal_suffix(day: u32) -> &'static str {
    match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

type TokenRenderer = fn(&DateTime<Utc>) -> String;

/// dayjs format tokens this renders, longest-match-first so `YYYY` is tried
/// before `YY`, `MMMM` before `MM`, `Do`/`DD` before `D`, etc.
const TOKENS: &[(&str, TokenRenderer)] = &[
    ("YYYY", |d| format!("{:04}", d.year())),
    ("YY", |d| format!("{:02}", d.year().rem_euclid(100))),
    ("MMMM", |d| d.format("%B").to_string()),
    ("MMM", |d| d.format("%b").to_string()),
    ("MM", |d| format!("{:02}", d.month())),
    ("Do", |d| format!("{}{}", d.day(), ordinal_suffix(d.day()))),
    ("DD", |d| format!("{:02}", d.day())),
    ("dddd", |d| d.format("%A").to_string()),
    ("ddd", |d| d.format("%a").to_string()),
    ("HH", |d| format!("{:02}", d.hour())),
    ("hh", |d| format!("{:02}", d.hour12().1)),
    ("mm", |d| format!("{:02}", d.minute())),
    ("ss", |d| format!("{:02}", d.second())),
    ("A", |d| if d.hour12().0 { "PM" } else { "AM" }.to_string()),
    ("a", |d| if d.hour12().0 { "pm" } else { "am" }.to_string()),
    ("M", |d| d.month().to_string()),
    ("D", |d| d.day().to_string()),
    ("H", |d| d.hour().to_string()),
    ("h", |d| d.hour12().1.to_string()),
    ("m", |d| d.minute().to_string()),
    ("s", |d| d.second().to_string()),
    ("Z", |_| "+00:00".to_string()),
];

/// Renders one dayjs format string against `dt`, token by token, honoring
/// `[literal]` escapes the same way real dayjs does.
fn render(dt: &DateTime<Utc>, token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    let mut out = String::with_capacity(token.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == ']') {
                out.extend(&chars[i + 1..i + 1 + end]);
                i += end + 2;
                continue;
            }
        }
        let rest: String = chars[i..].iter().collect();
        if let Some((matched, render_fn)) =
            TOKENS.iter().find(|(pattern, _)| rest.starts_with(pattern))
        {
            out.push_str(&render_fn(dt));
            i += matched.chars().count();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn format_dayjs(ms: f64, token: &str) -> String {
    let Some(dt) = ms_to_datetime(ms) else {
        return "Invalid Date".to_string();
    };
    let expanded = expand_locale_macros(token);
    render(&dt, &expanded)
}

/// Normalizes a dayjs unit string to one of `year/month/week/day/hour/
/// minute/second/millisecond`. Single-letter units are checked with their
/// original case first — real dayjs is case-sensitive there (`M` = month,
/// `m` = minute), which plain lowercasing would collide.
fn normalize_unit(unit: &str) -> &'static str {
    match unit {
        "y" => return "year",
        "M" => return "month",
        "w" => return "week",
        "d" => return "day",
        "h" => return "hour",
        "m" => return "minute",
        "s" => return "second",
        "ms" => return "millisecond",
        _ => {}
    }
    let lower = unit.to_ascii_lowercase();
    if lower == "millisecond" || lower == "milliseconds" {
        return "millisecond";
    }
    match lower.trim_end_matches('s') {
        "year" => "year",
        "month" | "mon" => "month",
        "week" => "week",
        "day" => "day",
        "hour" => "hour",
        "minute" => "minute",
        "second" | "sec" => "second",
        _ => "millisecond",
    }
}

fn add_months(dt: DateTime<Utc>, months: i64) -> Option<DateTime<Utc>> {
    if months >= 0 {
        dt.checked_add_months(Months::new(months as u32))
    } else {
        dt.checked_sub_months(Months::new((-months) as u32))
    }
}

/// `.add(amount, unit)` / `.subtract(amount, unit)` (the latter is just
/// `.add(-amount, unit)` on the JS side, see `RUNTIME_PRELUDE`). Calendar
/// units (`month`/`year`) use real calendar arithmetic
/// (`checked_add_months`), not a fixed-length approximation.
fn add_unit(ms: f64, amount: f64, unit: &str) -> f64 {
    let Some(dt) = ms_to_datetime(ms) else {
        return f64::NAN;
    };
    // `amount` is plugin-controlled and can be an arbitrary JS number (e.g.
    // `1e18`): the infallible `Duration::weeks`/`days`/etc. constructors
    // panic on overflow after the `as i64` cast, which would take the whole
    // worker down. The `try_*` constructors and `checked_add_signed` turn
    // that into `f64::NAN` instead, same as any other out-of-range result
    // here.
    let result = match normalize_unit(unit) {
        "year" => add_months(dt, (amount * 12.0).round() as i64),
        "month" => add_months(dt, amount.round() as i64),
        "week" => Duration::try_weeks(amount as i64).and_then(|d| dt.checked_add_signed(d)),
        "day" => Duration::try_days(amount as i64).and_then(|d| dt.checked_add_signed(d)),
        "hour" => Duration::try_hours(amount as i64).and_then(|d| dt.checked_add_signed(d)),
        "minute" => Duration::try_minutes(amount as i64).and_then(|d| dt.checked_add_signed(d)),
        "second" => Duration::try_seconds(amount as i64).and_then(|d| dt.checked_add_signed(d)),
        _ => Duration::try_milliseconds(amount as i64).and_then(|d| dt.checked_add_signed(d)),
    };
    result
        .map(|d| d.timestamp_millis() as f64)
        .unwrap_or(f64::NAN)
}

/// `.diff(other, unit)`. Month/year use the average calendar length
/// (30.436875/365.25 days) rather than the exact calendar difference real
/// dayjs computes — an accepted approximation for this MVP (see module doc
/// comment), truncated toward zero like real dayjs.
fn diff(ms1: f64, ms2: f64, unit: &str) -> f64 {
    let delta_ms = ms1 - ms2;
    let scaled = match normalize_unit(unit) {
        "year" => delta_ms / (365.25 * 86_400_000.0),
        "month" => delta_ms / (30.436_875 * 86_400_000.0),
        "week" => delta_ms / (7.0 * 86_400_000.0),
        "day" => delta_ms / 86_400_000.0,
        "hour" => delta_ms / 3_600_000.0,
        "minute" => delta_ms / 60_000.0,
        "second" => delta_ms / 1_000.0,
        _ => delta_ms,
    };
    scaled.trunc()
}

/// `.fromNow()`. Thresholds mirror real dayjs's default English
/// `relativeTime` wording (45s/90s/45min/90min/22h/36h/26d/45d/320d/548d).
fn from_now(ms: f64) -> String {
    let Some(dt) = ms_to_datetime(ms) else {
        return "Invalid Date".to_string();
    };
    let delta = Utc::now().signed_duration_since(dt);
    let future = delta.num_milliseconds() < 0;
    let secs = delta.num_seconds().abs();

    let phrase = if secs < 45 {
        "a few seconds".to_string()
    } else if secs < 90 {
        "a minute".to_string()
    } else if secs < 45 * 60 {
        format!("{} minutes", (secs as f64 / 60.0).round() as i64)
    } else if secs < 90 * 60 {
        "an hour".to_string()
    } else if secs < 22 * 3600 {
        format!("{} hours", (secs as f64 / 3600.0).round() as i64)
    } else if secs < 36 * 3600 {
        "a day".to_string()
    } else if secs < 26 * 86400 {
        format!("{} days", (secs as f64 / 86400.0).round() as i64)
    } else if secs < 45 * 86400 {
        "a month".to_string()
    } else if secs < 320 * 86400 {
        format!(
            "{} months",
            (secs as f64 / (30.436_875 * 86400.0)).round() as i64
        )
    } else if secs < 548 * 86400 {
        "a year".to_string()
    } else {
        format!(
            "{} years",
            (secs as f64 / (365.25 * 86400.0)).round() as i64
        )
    };

    if future {
        format!("in {phrase}")
    } else {
        format!("{phrase} ago")
    }
}

fn native_dayjs_now(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    Ok(JsValue::from(now_ms()))
}

fn native_dayjs_parse(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let s = arg_string(args, 0, context)?;
    Ok(match parse_flexible(&s) {
        Some(ms) => JsValue::from(ms as f64),
        None => JsValue::null(),
    })
}

fn native_dayjs_format(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ms = arg_f64(args, 0, context)?;
    let token = arg_string(args, 1, context)?;
    Ok(JsValue::from(js_string!(format_dayjs(ms, &token).as_str())))
}

fn native_dayjs_add(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ms = arg_f64(args, 0, context)?;
    let amount = arg_f64(args, 1, context)?;
    let unit = arg_string(args, 2, context)?;
    Ok(JsValue::from(add_unit(ms, amount, &unit)))
}

fn native_dayjs_diff(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ms1 = arg_f64(args, 0, context)?;
    let ms2 = arg_f64(args, 1, context)?;
    let unit = arg_string(args, 2, context)?;
    Ok(JsValue::from(diff(ms1, ms2, &unit)))
}

fn native_dayjs_from_now(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ms = arg_f64(args, 0, context)?;
    Ok(JsValue::from(js_string!(from_now(ms).as_str())))
}

fn register_fn(
    context: &mut Context,
    name: &str,
    length: usize,
    f: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
) {
    let native = NativeFunction::from_fn_ptr(f);
    let func = FunctionObjectBuilder::new(context.realm(), native)
        .name(name)
        .length(length)
        .build();
    context
        .global_object()
        .set(js_string!(name), func, false, context)
        .unwrap_or_else(|_| panic!("registering {name} should not fail"));
}

/// Registers every `__native_dayjs_*` primitive as a global function.
/// [`super::js_runtime::RUNTIME_PRELUDE`] wraps these into the `Dayjs`
/// class/`dayjs()` factory `require('dayjs')` resolves to.
pub(super) fn register(context: &mut Context) {
    register_fn(context, "__native_dayjs_now", 0, native_dayjs_now);
    register_fn(context, "__native_dayjs_parse", 1, native_dayjs_parse);
    register_fn(context, "__native_dayjs_format", 2, native_dayjs_format);
    register_fn(context, "__native_dayjs_add", 3, native_dayjs_add);
    register_fn(context, "__native_dayjs_diff", 3, native_dayjs_diff);
    register_fn(context, "__native_dayjs_from_now", 1, native_dayjs_from_now);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ms() -> f64 {
        // 2024-01-05 09:07:03 UTC
        Utc.with_ymd_and_hms(2024, 1, 5, 9, 7, 3)
            .unwrap()
            .timestamp_millis() as f64
    }

    #[test]
    fn formats_dd_mmmm_yyyy() {
        assert_eq!(format_dayjs(sample_ms(), "DD MMMM YYYY"), "05 January 2024");
    }

    #[test]
    fn formats_ll_macro() {
        assert_eq!(format_dayjs(sample_ms(), "LL"), "January 5, 2024");
    }

    #[test]
    fn formats_lll_macro() {
        assert_eq!(format_dayjs(sample_ms(), "LLL"), "January 5, 2024 9:07 AM");
    }

    #[test]
    fn formats_ordinal_day() {
        assert_eq!(format_dayjs(sample_ms(), "Do MMM"), "5th Jan");
    }

    #[test]
    fn honors_literal_brackets() {
        assert_eq!(format_dayjs(sample_ms(), "YYYY[-W]"), "2024-W");
    }

    #[test]
    fn literal_bracket_content_is_not_reinterpreted_as_a_token() {
        // A token name inside a bracket literal must stay literal, not get
        // expanded a second time once `expand_locale_macros` hands its
        // output to `render`.
        assert_eq!(format_dayjs(sample_ms(), "[YYYY] YYYY"), "YYYY 2024");
    }

    #[test]
    fn invalid_ms_formats_as_invalid_date() {
        assert_eq!(format_dayjs(f64::NAN, "YYYY"), "Invalid Date");
    }

    #[test]
    fn subtract_days_matches_add_negative() {
        let ms = sample_ms();
        let subtracted = add_unit(ms, -3.0, "day");
        assert_eq!(format_dayjs(subtracted, "YYYY-MM-DD"), "2024-01-02");
    }

    #[test]
    fn add_respects_calendar_months() {
        let ms = sample_ms();
        let added = add_unit(ms, 1.0, "month");
        assert_eq!(format_dayjs(added, "YYYY-MM-DD"), "2024-02-05");
    }

    #[test]
    fn normalize_unit_distinguishes_month_and_minute_case() {
        assert_eq!(normalize_unit("M"), "month");
        assert_eq!(normalize_unit("m"), "minute");
        assert_eq!(normalize_unit("months"), "month");
        assert_eq!(normalize_unit("minutes"), "minute");
        assert_eq!(normalize_unit("ms"), "millisecond");
    }

    #[test]
    fn diff_in_days_truncates_toward_zero() {
        let start = sample_ms();
        let end = start + 2.5 * 86_400_000.0;
        assert_eq!(diff(end, start, "day"), 2.0);
    }

    #[test]
    fn parses_common_scraped_date_formats() {
        assert!(parse_flexible("2024-01-05").is_some());
        assert!(parse_flexible("Jan 5, 2024").is_some());
        assert!(parse_flexible("05 January 2024").is_some());
        assert_eq!(parse_flexible("not a date"), None);
    }
}
