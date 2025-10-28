#![allow(clippy::too_many_arguments)]

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use log::debug;
use pared::sync::Parc;
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasm_shared::{
    get_memory,
    memory_reader::{read_string as read_memory_string, write_bytes},
};
use wasmi::{core::F64, Caller, Linker};

use crate::source::{
    model::{Filter, FilterType, Manga, MangaPageResult},
    wasm_store::{ObjectValue, Value, ValueMap, ValueRef, WasmStore},
};

enum ObjectType {
    Null = 0,
    Int = 1,
    Float = 2,
    String = 3,
    Bool = 4,
    Array = 5,
    Object = 6,
    Date = 7,
    Node = 8,
    #[allow(dead_code)]
    Unknown = 9,
}

trait FieldAsValue {
    fn field_as_value(&self, field: &str) -> Option<Value>;
}

pub fn register_std_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "std", "copy", copy)?;
    register_wasm_function!(linker, "std", "destroy", destroy)?;
    register_wasm_function!(linker, "std", "create_null", create_null)?;
    register_wasm_function!(linker, "std", "create_int", create_int)?;
    register_wasm_function!(linker, "std", "create_float", create_float)?;
    register_wasm_function!(linker, "std", "create_string", create_string)?;
    register_wasm_function!(linker, "std", "create_bool", create_bool)?;
    register_wasm_function!(linker, "std", "create_array", create_array)?;
    register_wasm_function!(linker, "std", "create_object", create_object)?;
    register_wasm_function!(linker, "std", "create_date", create_date)?;
    register_wasm_function!(linker, "std", "typeof", type_of)?;
    register_wasm_function!(linker, "std", "string_len", string_len)?;
    register_wasm_function!(linker, "std", "read_string", read_string)?;
    register_wasm_function!(linker, "std", "read_int", read_int)?;
    register_wasm_function!(linker, "std", "read_float", read_float)?;
    register_wasm_function!(linker, "std", "read_bool", read_bool)?;
    register_wasm_function!(linker, "std", "read_date", read_date)?;
    register_wasm_function!(linker, "std", "read_date_string", read_date_string)?;
    register_wasm_function!(linker, "std", "object_len", object_len)?;
    register_wasm_function!(linker, "std", "object_get", object_get)?;
    register_wasm_function!(linker, "std", "object_set", object_set)?;
    register_wasm_function!(linker, "std", "object_remove", object_remove)?;
    register_wasm_function!(linker, "std", "object_keys", object_keys)?;
    register_wasm_function!(linker, "std", "object_values", object_values)?;
    register_wasm_function!(linker, "std", "array_len", array_len)?;
    register_wasm_function!(linker, "std", "array_get", array_get)?;
    register_wasm_function!(linker, "std", "array_set", array_set)?;
    register_wasm_function!(linker, "std", "array_append", array_append)?;
    register_wasm_function!(linker, "std", "array_remove", array_remove)?;
    Ok(())
}

#[aidoku_wasm_function]
fn copy(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in copy")?;
    let wasm_store = caller.data_mut();
    let value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in copy")?;
    Ok(wasm_store.store_std_value(value, None) as i32)
}

#[aidoku_wasm_function]
fn destroy(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<()> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in destroy")?;
    caller.data_mut().remove_std_value(descriptor);
    Ok(())
}

#[aidoku_wasm_function]
fn create_null(caller: Caller<'_, WasmStore>) -> Result<i32> {
    Ok(create_value(caller, Value::Null))
}

#[aidoku_wasm_function]
fn create_int(caller: Caller<'_, WasmStore>, value: i64) -> Result<i32> {
    Ok(create_value(caller, value.into()))
}

#[aidoku_wasm_function]
fn create_float(caller: Caller<'_, WasmStore>, value: F64) -> Result<i32> {
    Ok(create_value(caller, value.to_float().into()))
}

#[aidoku_wasm_function]
fn create_string(mut caller: Caller<'_, WasmStore>, offset: i32, length: i32) -> Result<i32> {
    let memory = get_memory(&mut caller).context("failed to get memory in create_string")?;
    let string = read_memory_string(&memory, &caller, offset as usize, length as usize)
        .context("failed to read string from memory in create_string")?;
    Ok(create_value(caller, string.into()))
}

#[aidoku_wasm_function]
fn create_bool(caller: Caller<'_, WasmStore>, value_i32: i32) -> Result<i32> {
    Ok(create_value(caller, Value::Bool(value_i32 != 0)))
}

#[aidoku_wasm_function]
fn create_array(caller: Caller<'_, WasmStore>) -> Result<i32> {
    Ok(create_value(caller, Value::Array(Vec::default())))
}

#[aidoku_wasm_function]
fn create_date(caller: Caller<'_, WasmStore>, seconds_since_1970: F64) -> Result<i32> {
    let seconds_since_1970 = seconds_since_1970.to_float();
    let full_seconds = seconds_since_1970.floor() as i64;
    let nanos_remainder = ((seconds_since_1970 - full_seconds as f64) * (10f64.powi(9))) as u32;
    let date_time: DateTime<chrono_tz::Tz> = chrono_tz::UTC
        .timestamp_opt(full_seconds, nanos_remainder)
        .single()
        .context("failed to create DateTime in create_date")?;
    Ok(create_value(caller, date_time.into()))
}

fn create_value(mut caller: Caller<'_, WasmStore>, value: Value) -> i32 {
    let wasm_store = caller.data_mut();
    wasm_store.store_std_value(value.into(), None) as i32
}

#[aidoku_wasm_function]
fn create_object(caller: Caller<'_, WasmStore>) -> Result<i32> {
    Ok(create_value(caller, ValueMap::default().into()))
}

#[aidoku_wasm_function]
fn type_of(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in type_of")?;
    let wasm_store = caller.data_mut();
    let value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in type_of")?;
    let object_type = match *value {
        Value::Null => ObjectType::Null,
        Value::Int(_) => ObjectType::Int,
        Value::Float(_) => ObjectType::Float,
        Value::String(_) => ObjectType::String,
        Value::Bool(_) => ObjectType::Bool,
        Value::Array(_) => ObjectType::Array,
        Value::Object(_) => ObjectType::Object,
        Value::Date(_) => ObjectType::Date,
        Value::HTMLElements(_) => ObjectType::Node,
    };
    Ok(object_type as i32)
}

#[aidoku_wasm_function]
fn string_len(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in string_len")?;
    let wasm_store = caller.data_mut();
    let value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in string_len")?;
    match value.as_ref() {
        Value::String(s) => Ok(s.len() as i32),
        _ => Err(anyhow::anyhow!("expected String value in string_len")),
    }
}

#[aidoku_wasm_function]
fn read_string(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    buffer_i32: i32,
    size_i32: i32,
) {
    || -> Option<()> {
        let descriptor: usize = descriptor_i32.try_into().ok()?;
        let buffer: usize = buffer_i32.try_into().ok()?;
        let size: usize = size_i32.try_into().ok()?;

        let wasm_store = caller.data();
        let value_ref = wasm_store.get_std_value(descriptor)?;
        let string = value_ref.try_unwrap_string_ref().ok()?;

        let memory = get_memory(&mut caller)?;
        if size <= string.len() {
            let string_slice = &string.as_str()[..size];
            write_bytes(&memory, &mut caller, string_slice.as_bytes(), buffer)?;
        };

        Some(())
    }();
}

#[aidoku_wasm_function]
fn read_int(caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i64> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in read_int")?;
    let wasm_store = caller.data();
    let value: ValueRef = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in read_int")?;
    let result = match value.as_ref() {
        Value::Bool(b) => {
            if *b {
                1i64
            } else {
                0i64
            }
        }
        Value::Int(i) => *i,
        Value::Float(f) => f.trunc() as i64,
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    };
    Ok(result)
}

#[aidoku_wasm_function]
fn read_float(caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<F64> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in read_float")?;
    let wasm_store = caller.data();
    let value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in read_float")?;
    let result = match value.as_ref() {
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        Value::String(s) => s.parse().unwrap_or(-1f64),
        _ => -1f64,
    };
    Ok(result.into())
}

#[aidoku_wasm_function]
fn read_bool(caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in read_bool")?;
    let wasm_store = caller.data();
    let value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in read_bool")?;
    let result = match value.as_ref() {
        Value::Bool(b) => {
            if *b {
                1i32
            } else {
                0i32
            }
        }
        Value::Int(i) => {
            if *i != 0 {
                1i32
            } else {
                0i32
            }
        }
        _ => 0,
    };
    Ok(result)
}

#[aidoku_wasm_function]
fn read_date(caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<F64> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in read_date")?;
    let wasm_store = caller.data();
    let value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in read_date")?;
    let result = match value.as_ref() {
        Value::Date(date) => {
            date.timestamp() as f64 + (date.timestamp_subsec_nanos() as f64) / (10f64.powi(9))
        }
        _ => 0f64,
    };
    Ok(result.into())
}

fn parse_flexible_datetime(
    string: &str,
    format_string: &str,
    timezone: chrono_tz::Tz,
) -> Result<chrono::DateTime<chrono_tz::Tz>> {
    // Try parse as full datetime first
    if let Ok(dt) = NaiveDateTime::parse_from_str(string, format_string) {
        return dt
            .and_local_timezone(timezone)
            .single()
            .context("failed to apply timezone to parsed datetime");
    }

    // Fallback – try parse only date and add 00:00:00
    if let Ok(date) = NaiveDate::parse_from_str(string, format_string) {
        let dt = NaiveDateTime::new(date, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        return dt
            .and_local_timezone(timezone)
            .single()
            .context("failed to apply timezone to fallback date");
    }

    // If both failed
    anyhow::bail!(
        "failed to parse date string in read_date_string: {}",
        string
    );
}

#[aidoku_wasm_function]
fn read_date_string(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    format_i32: i32,
    format_len_i32: i32,
    locale_i32: i32,
    locale_len_i32: i32,
    timezone_i32: i32,
    timezone_len_i32: i32,
) -> Result<F64> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in read_date_string")?;
    let format: usize = format_i32
        .try_into()
        .context("failed to convert format_i32 in read_date_string")?;
    let format_len: usize = format_len_i32
        .try_into()
        .ok()
        .filter(|&len| len > 0)
        .context("invalid format_len in read_date_string")?;
    let locale: Option<usize> = locale_i32.try_into().ok();
    let locale_len: Option<usize> = locale_len_i32.try_into().ok().filter(|&len| len > 0);
    let timezone: Option<usize> = timezone_i32.try_into().ok();
    let timezone_len: Option<usize> = timezone_len_i32.try_into().ok().filter(|&len| len > 0);
    let wasm_store = caller.data();
    let value_ref = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in read_date_string")?;
    let string = match value_ref.as_ref() {
        Value::String(s) => Some(s),
        _ => None,
    }
    .context("expected String value in read_date_string")?;
    let memory = get_memory(&mut caller).context("failed to get memory in read_date_string")?;
    let format_string = read_memory_string(&memory, &caller, format, format_len)
        .context("failed to read format string in read_date_string")?;
    let _locale_string = match (locale, locale_len) {
        (Some(locale), Some(locale_len)) => Some(
            read_memory_string(&memory, &caller, locale, locale_len)
                .context("failed to read locale string in read_date_string")?,
        ),
        _ => None,
    };
    let timezone_string = match (timezone, timezone_len) {
        (Some(timezone), Some(timezone_len)) => Some(
            read_memory_string(&memory, &caller, timezone, timezone_len)
                .context("failed to read timezone string in read_date_string")?,
        ),
        _ => None,
    };
    let timezone: chrono_tz::Tz = timezone_string
        .as_deref()
        .and_then(|tz_str| tz_str.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let format_string = swift_dateformat_to_strptime(&format_string);

    let date_time = parse_flexible_datetime(string, &format_string, timezone)
        .context("failed to parse date string in read_date_string")?;
    // chrono::NaiveDateTime::parse_from_str(string, &format_string)
    //     .ok()
    //     .and_then(|dt| dt.and_local_timezone(timezone).single())
    //     .context("failed to parse date string in read_date_string")?;

    let timestamp = date_time.timestamp() as f64
        + (date_time.timestamp_subsec_nanos() as f64) / (10f64.powi(9));
    Ok(timestamp.into())
}

/// Converts a Swift dateFormat string to a strptime-compatible format string
///
/// This function handles the most common Swift dateFormat patterns and converts them
/// to their equivalent strptime format specifiers.
pub fn swift_dateformat_to_strptime(swift_format: &str) -> String {
    let mut result = String::new();
    let mut chars = swift_format.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Year patterns
            'y' => {
                let count = count_consecutive_chars(&mut chars, 'y') + 1;
                match count {
                    1 | 2 => result.push_str("%y"), // 2-digit year
                    _ => result.push_str("%Y"),     // 4-digit year
                }
            }

            // Month patterns
            'M' => {
                let count = count_consecutive_chars(&mut chars, 'M') + 1;
                match count {
                    1 | 2 => result.push_str("%m"), // Numeric month (1-12)
                    3 => result.push_str("%b"),     // Abbreviated month name
                    _ => result.push_str("%B"),     // Full month name
                }
            }

            // Day patterns
            'd' => {
                let count = count_consecutive_chars(&mut chars, 'd') + 1;
                match count {
                    1 | 2 => result.push_str("%d"), // Day of month (01-31)
                    _ => result.push_str("%d"),     // Same for multiple d's
                }
            }

            // Weekday patterns
            'E' => {
                let count = count_consecutive_chars(&mut chars, 'E') + 1;
                match count {
                    1..=3 => result.push_str("%a"), // Abbreviated weekday
                    _ => result.push_str("%A"),     // Full weekday name
                }
            }

            // Hour patterns (24-hour)
            'H' => {
                let count = count_consecutive_chars(&mut chars, 'H') + 1;
                match count {
                    1 | 2 => result.push_str("%H"), // Hour 00-23
                    _ => result.push_str("%H"),
                }
            }

            // Hour patterns (12-hour)
            'h' => {
                let count = count_consecutive_chars(&mut chars, 'h') + 1;
                match count {
                    1 | 2 => result.push_str("%I"), // Hour 01-12
                    _ => result.push_str("%I"),
                }
            }

            // Minute patterns
            'm' => {
                let count = count_consecutive_chars(&mut chars, 'm') + 1;
                match count {
                    1 | 2 => result.push_str("%M"), // Minutes 00-59
                    _ => result.push_str("%M"),
                }
            }

            // Second patterns
            's' => {
                let count = count_consecutive_chars(&mut chars, 's') + 1;
                match count {
                    1 | 2 => result.push_str("%S"), // Seconds 00-59
                    _ => result.push_str("%S"),
                }
            }

            // AM/PM patterns
            'a' => {
                let _count = count_consecutive_chars(&mut chars, 'a');
                result.push_str("%p"); // AM/PM
            }

            // Fractional seconds
            'S' => {
                let count = count_consecutive_chars(&mut chars, 'S') + 1;
                // strptime doesn't have direct support for fractional seconds
                // This is a limitation - you might need to handle this separately
                match count {
                    1..=6 => result.push_str("%f"), // Microseconds (not standard strptime)
                    _ => result.push_str("%f"),
                }
            }

            // Time zone patterns
            'z' => {
                let count = count_consecutive_chars(&mut chars, 'z') + 1;
                match count {
                    1..=3 => result.push_str("%z"), // +HHMM offset
                    _ => result.push_str("%Z"),     // Time zone name
                }
            }

            'Z' => {
                let _count = count_consecutive_chars(&mut chars, 'Z');
                result.push_str("%Z"); // Time zone name
            }

            // Handle quoted literals
            '\'' => {
                // Skip the opening quote and collect characters until closing quote
                while let Some(quoted_char) = chars.next() {
                    if quoted_char == '\'' {
                        break; // Found closing quote
                    }
                    result.push(quoted_char);
                }
            }

            // Literal characters - pass through unchanged
            _ => result.push(ch),
        }
    }

    result
}

/// Helper function to count consecutive occurrences of a character
fn count_consecutive_chars(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    target: char,
) -> usize {
    let mut count = 0;
    while let Some(&ch) = chars.peek() {
        if ch == target {
            chars.next();
            count += 1;
        } else {
            break;
        }
    }
    count
}

#[aidoku_wasm_function]
fn object_len(caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in object_len")?;
    let wasm_store = caller.data();
    if let Value::Object(ObjectValue::ValueMap(hm)) = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in object_len")?
        .as_ref()
    {
        Ok(hm.len() as i32)
    } else {
        Err(anyhow::anyhow!("expected ValueMap in object_len"))
    }
}

#[aidoku_wasm_function]
fn object_get(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    key_i32: i32,
    key_len_i32: i32,
) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in object_get")?;
    let key_offset: usize = key_i32
        .try_into()
        .context("failed to convert key_i32 in object_get")?;
    let key_len: usize = key_len_i32
        .try_into()
        .ok()
        .filter(|&key_len| key_len > 0)
        .context("invalid key_len in object_get")?;
    let memory = get_memory(&mut caller).context("failed to get memory in object_get")?;
    let key = read_memory_string(&memory, &caller, key_offset, key_len)
        .context("failed to read key string in object_get")?;
    let wasm_store = caller.data_mut();
    let object_ref = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in object_get")?
        .try_project(|value| match value {
            Value::Object(obj) => Ok(obj),
            _ => Err(anyhow!("expected Object value in object_get")),
        })?;
    let value = match object_ref.as_ref() {
        ObjectValue::ValueMap(_) => object_ref.try_project(|object| {
            if let ObjectValue::ValueMap(map) = object {
                map.get(&key).context("key not found in ValueMap")
            } else {
                bail!("expected ValueMap in object_get")
            }
        })?,
        ObjectValue::Manga(m) => m
            .field_as_value(&key)
            .context("key not found in Manga in object_get")?
            .into(),
        ObjectValue::MangaPageResult(mpr) => mpr
            .field_as_value(&key)
            .context("key not found in MangaPageResult in object_get")?
            .into(),
        ObjectValue::Filter(f) => f
            .field_as_value(&key)
            .context("key not found in Filter in object_get")?
            .into(),
        _ => return Err(anyhow::anyhow!("missing implementation in object_get")),
    };
    Ok(wasm_store.store_std_value(value, Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn object_set(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    key_i32: i32,
    key_len_i32: i32,
    value_i32: i32,
) -> Result<()> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in object_set")?;
    let value_descriptor: usize = value_i32
        .try_into()
        .context("failed to convert value_i32 in object_set")?;
    let key_offset: usize = key_i32
        .try_into()
        .context("failed to convert key_i32 in object_set")?;
    let key_len: usize = key_len_i32
        .try_into()
        .ok()
        .filter(|&key_len| key_len > 0)
        .context("invalid key_len in object_set")?;
    let memory = get_memory(&mut caller).context("failed to get memory in object_set")?;
    let key = read_memory_string(&memory, &caller, key_offset, key_len)
        .context("failed to read key string in object_set")?;
    let wasm_store = caller.data_mut();
    let value = wasm_store
        .get_std_value(value_descriptor)
        .context("failed to get value in object_set")?
        .as_ref()
        .clone();
    let mut hm_value = Parc::unwrap_or_clone(
        wasm_store
            .take_std_value(descriptor)
            .context("failed to take value in object_set")?,
    );
    let object_value = hm_value
        .try_unwrap_object_mut()
        .map_err(|_| anyhow::anyhow!("expected object in object_set"))?;
    let hashmap = object_value
        .try_unwrap_value_map_mut()
        .map_err(|_| anyhow::anyhow!("expected ValueMap in object_set"))?;
    hashmap.insert(key, value);
    wasm_store.set_std_value(descriptor, hm_value.into());
    Ok(())
}

#[aidoku_wasm_function]
fn object_remove(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    key_i32: i32,
    key_len_i32: i32,
) -> Result<()> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in object_remove")?;
    let key_offset: usize = key_i32
        .try_into()
        .context("failed to convert key_i32 in object_remove")?;
    let key_len: usize = key_len_i32
        .try_into()
        .ok()
        .filter(|&key_len| key_len > 0)
        .context("invalid key_len in object_remove")?;
    let memory = get_memory(&mut caller).context("failed to get memory in object_remove")?;
    let key = read_memory_string(&memory, &caller, key_offset, key_len)
        .context("failed to read key string in object_remove")?;
    let wasm_store = caller.data_mut();
    let mut value = Parc::unwrap_or_clone(
        wasm_store
            .take_std_value(descriptor)
            .context("failed to take value in object_remove")?,
    );
    let object_value = value
        .try_unwrap_object_mut()
        .map_err(|_| anyhow::anyhow!("expected object in object_remove"))?;
    let hashmap = object_value
        .try_unwrap_value_map_mut()
        .map_err(|_| anyhow::anyhow!("expected ValueMap in object_remove"))?;
    hashmap.remove(&key);
    wasm_store.set_std_value(descriptor, value.into());
    Ok(())
}

#[aidoku_wasm_function]
fn object_keys(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in object_keys")?;
    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in object_keys")?;
    let hashmap_object = std_value
        .try_unwrap_object_ref()
        .ok()
        .context("expected object in object_keys")?
        .try_unwrap_value_map_ref()
        .ok()
        .context("expected ValueMap in object_keys")?;
    let keys: Vec<Value> = hashmap_object.keys().cloned().map(Value::String).collect();
    Ok(wasm_store.store_std_value(Value::Array(keys).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn object_values(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in object_values")?;
    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in object_values")?;
    let hashmap_object = std_value
        .try_unwrap_object_ref()
        .ok()
        .context("expected object in object_values")?
        .try_unwrap_value_map_ref()
        .ok()
        .context("expected ValueMap in object_values")?;
    let values: Vec<Value> = hashmap_object.values().cloned().collect();
    Ok(wasm_store.store_std_value(Value::Array(values).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn array_len(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in array_len")?;
    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in array_len")?;
    let array = std_value
        .try_unwrap_array_ref()
        .ok()
        .context("expected array in array_len")?;
    Ok(array.len() as i32)
}

#[aidoku_wasm_function]
fn array_get(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    index_i32: i32,
) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in array_get")?;
    let index: usize = index_i32
        .try_into()
        .context("failed to convert index_i32 in array_get")?;
    let wasm_store = caller.data_mut();
    let value_ref = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in array_get")?
        .try_project(|maybe_array| match maybe_array {
            Value::Array(arr) => arr.get(index).ok_or(()),
            _ => Err(()),
        })
        .map_err(|_| anyhow::anyhow!("expected array and valid index in array_get"))?;
    Ok(wasm_store.store_std_value(value_ref, Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn array_set(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    index_i32: i32,
    value_i32: i32,
) -> Result<()> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in array_set")?;
    let value_descriptor: usize = value_i32
        .try_into()
        .context("failed to convert value_i32 in array_set")?;
    let wasm_store = caller.data_mut();
    let value_ref = wasm_store
        .get_std_value(value_descriptor)
        .context("failed to get value in array_set")?;
    let mut array_value = Parc::unwrap_or_clone(
        wasm_store
            .take_std_value(descriptor)
            .context("failed to take value in array_set")?,
    );
    let array = array_value
        .try_unwrap_array_mut()
        .map_err(|_| anyhow::anyhow!("expected array in array_set"))?;
    let index: usize = index_i32
        .try_into()
        .ok()
        .filter(|&index| index < array.len())
        .context("invalid index in array_set")?;
    array[index] = value_ref.as_ref().clone();
    wasm_store.set_std_value(descriptor, array_value.into());
    Ok(())
}

#[aidoku_wasm_function]
fn array_append(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    value_i32: i32,
) -> Result<()> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in array_append")?;
    let value_descriptor: usize = value_i32
        .try_into()
        .context("failed to convert value_i32 in array_append")?;
    let wasm_store = caller.data_mut();
    let value_ref = wasm_store
        .get_std_value(value_descriptor)
        .context("failed to get value in array_append")?;
    let array_value_ref = wasm_store
        .take_std_value(descriptor)
        .context("failed to take value in array_append")?;
    if Parc::strong_count(&array_value_ref) > 1 {
        debug!(
            "attempting to add to array with more than 1 reference (got {}), slow!",
            Parc::strong_count(&array_value_ref)
        );
    }
    let mut array_value_ref = Parc::unwrap_or_clone(array_value_ref);
    let array_value = array_value_ref
        .try_unwrap_array_mut()
        .map_err(|_| anyhow::anyhow!("expected array in array_append"))?;
    array_value.push(value_ref.as_ref().clone());
    wasm_store.set_std_value(descriptor, array_value_ref.into());
    Ok(())
}

#[aidoku_wasm_function]
fn array_remove(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    index_i32: i32,
) -> Result<()> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("failed to convert descriptor_i32 in array_remove")?;
    let wasm_store = caller.data_mut();
    let mut array = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value in array_remove")?
        .as_ref()
    {
        Value::Array(arr) => Some(arr.clone()),
        _ => None,
    }
    .context("expected array in array_remove")?;
    let index: usize = index_i32
        .try_into()
        .ok()
        .filter(|&index| index < array.len())
        .context("invalid index in array_remove")?;
    array.remove(index);
    wasm_store.set_std_value(descriptor, Value::Array(array).into());
    Ok(())
}

// TODO maybe write a macro for this
impl FieldAsValue for Manga {
    fn field_as_value(&self, field: &str) -> Option<Value> {
        match field {
            "source_id" => Some(Value::String(self.source_id.clone())),
            "id" => Some(Value::String(self.id.clone())),
            "title" => self.title.clone().map(Value::String).or(Some(Value::Null)),
            "author" => self.author.clone().map(Value::String).or(Some(Value::Null)),
            "artist" => self.artist.clone().map(Value::String).or(Some(Value::Null)),
            "description" => self
                .description
                .clone()
                .map(Value::String)
                .or(Some(Value::Null)),
            "tags" => self
                .tags
                .clone()
                .map(|tags| {
                    Value::Array(tags.iter().map(|tag| Value::String(tag.clone())).collect())
                })
                .or(Some(Value::Null)),
            "cover_url" => self
                .cover_url
                .clone()
                .map(|url| Value::String(url.to_string()))
                .or(Some(Value::Null)),
            "url" => self
                .url
                .clone()
                .map(|url| Value::String(url.to_string()))
                .or(Some(Value::Null)),
            "status" => Some(Value::Int(self.status.clone() as i64)),
            "nsfw" => Some(Value::Int(self.nsfw.clone() as i64)),
            "viewer" => Some(Value::Int(self.viewer.clone() as i64)),
            "last_updated" => self.last_updated.map(Value::Date).or(Some(Value::Null)),
            "last_opened" => self.last_opened.map(Value::Date).or(Some(Value::Null)),
            "last_read" => self.last_read.map(Value::Date).or(Some(Value::Null)),
            "date_added" => self.date_added.map(Value::Date).or(Some(Value::Null)),
            _ => None,
        }
    }
}

impl FieldAsValue for MangaPageResult {
    fn field_as_value(&self, field: &str) -> Option<Value> {
        match field {
            "manga" => {
                let value_array = self
                    .manga
                    .iter()
                    .map(|m| Value::Object(ObjectValue::Manga(m.clone())))
                    .collect();

                Some(Value::Array(value_array))
            }
            "has_next_page" => Some(Value::Bool(self.has_next_page)),
            _ => None,
        }
    }
}

impl FieldAsValue for Filter {
    fn field_as_value(&self, field: &str) -> Option<Value> {
        match field {
            "type" => Some(Value::Int(FilterType::from(self) as i64)),
            "name" => Some(Value::String(self.name())),
            // FIXME i dont think this should be here but
            "value" => match &self {
                Filter::Title(title) => Some(Value::String(title.clone())),
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, Timelike};

    use super::*;

    #[test]
    fn test_basic_conversions() {
        assert_eq!(swift_dateformat_to_strptime("yyyy-MM-dd"), "%Y-%m-%d");
        assert_eq!(swift_dateformat_to_strptime("yy/MM/dd"), "%y/%m/%d");
        assert_eq!(swift_dateformat_to_strptime("MMM dd, yyyy"), "%b %d, %Y");
        assert_eq!(swift_dateformat_to_strptime("MMMM dd, yyyy"), "%B %d, %Y");
    }

    #[test]
    fn test_time_conversions() {
        assert_eq!(swift_dateformat_to_strptime("HH:mm:ss"), "%H:%M:%S");
        assert_eq!(swift_dateformat_to_strptime("h:mm:ss a"), "%I:%M:%S %p");
        assert_eq!(swift_dateformat_to_strptime("HH:mm"), "%H:%M");
    }

    #[test]
    fn test_weekday_conversions() {
        assert_eq!(swift_dateformat_to_strptime("EEE"), "%a");
        assert_eq!(swift_dateformat_to_strptime("EEEE"), "%A");
    }

    #[test]
    fn test_complex_format() {
        assert_eq!(
            swift_dateformat_to_strptime("EEEE, MMMM dd, yyyy 'at' h:mm:ss a"),
            "%A, %B %d, %Y at %I:%M:%S %p"
        );
    }

    #[test]
    fn test_timezone_conversions() {
        assert_eq!(
            swift_dateformat_to_strptime("yyyy-MM-dd'T'HH:mm:ssZ"),
            "%Y-%m-%dT%H:%M:%S%Z"
        );
        assert_eq!(
            swift_dateformat_to_strptime("yyyy-MM-dd HH:mm:ss z"),
            "%Y-%m-%d %H:%M:%S %z"
        );
    }

    #[test]
    fn test_weebcentral_format() {
        let swift_format = "yyyy-MM-dd'T'HH:mm:ss.SSS'Z'";
        let expected_strptime = "%Y-%m-%dT%H:%M:%S.%fZ";

        assert_eq!(
            swift_dateformat_to_strptime(swift_format),
            expected_strptime
        );

        let date_string = "2024-09-07T17:04:15.717Z";
        let date_time = chrono::DateTime::parse_from_rfc3339(date_string)
            .expect("Failed to parse date string")
            .with_timezone(&chrono_tz::UTC);

        assert_eq!(date_time.day(), 7);
        assert_eq!(date_time.month(), 9);
        assert_eq!(date_time.year(), 2024);
        assert_eq!(date_time.hour(), 17);
        assert_eq!(date_time.minute(), 4);
        assert_eq!(date_time.second(), 15);
        assert_eq!(date_time.nanosecond(), 717_000_000);
    }
}
