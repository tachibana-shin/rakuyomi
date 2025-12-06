#![allow(clippy::too_many_arguments)]

use std::usize;

use anyhow::{Context, Result};
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasm_shared::{get_memory, memory_reader::write_bytes};
use wasmi::{core::F64, Caller, Linker};

use crate::source::wasm_store::{self, Value, WasmStore};

pub fn register_std_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "std", "read_buffer", read_buffer)?; // OK. fixed
    register_wasm_function!(linker, "std", "buffer_len", buffer_len)?; // OK
    register_wasm_function!(linker, "std", "destroy", destroy)?; // OK
    register_wasm_function!(linker, "std", "current_date", current_date)?; // OK
    register_wasm_function!(linker, "std", "utc_offset", utc_offset)?; // OK
    register_wasm_function!(linker, "std", "parse_date", parse_date)?; // OK
    Ok(())
}

enum ResultContext {
    // Success,
    InvalidDescriptor,
    // InvalidBufferSize,
    // FailedMemoryWrite,
    // InvalidString,
    InvalidDateString,
}

impl From<ResultContext> for i32 {
    fn from(result: ResultContext) -> i32 {
        match result {
            // ResultContext::Success => 0,
            ResultContext::InvalidDescriptor => -1,
            // Result::InvalidBufferSize => -2,
            // ResultContext::FailedMemoryWrite => -3,
            // ResultContext::InvalidString => -4,
            ResultContext::InvalidDateString => -5,
        }
    }
}

macro_rules! serialize_variant {
    ($value:expr, $unwrap_fn:ident) => {{
        postcard::to_allocvec(
            &$value
                .into_iter()
                .map(|v| v.$unwrap_fn().unwrap())
                .collect::<Vec<_>>(),
        )
    }};
}
macro_rules! serialize_null {
    ($value:expr) => {{
        postcard::to_allocvec(&$value.into_iter().map(|_| None::<()>).collect::<Vec<_>>())
    }};
}

fn read_buffer_data(caller: &mut Caller<'_, WasmStore>, pointer: usize) -> Result<Vec<u8>> {
    let wasm_store = caller.data_mut();
    let value_ref = wasm_store.get_std_value(pointer.clone()).context(-1)?;
    let data = <wasm_store::Value as Clone>::clone(&value_ref);

    Ok(match data {
        Value::String(string) => {
            if wasm_store.is_str_encode(pointer) {
                postcard::to_allocvec(&string).unwrap()
            } else {
                string.to_string().into_bytes()
            }
        }
        Value::Int(value) => postcard::to_allocvec(&value).unwrap(),
        Value::Float(value) => postcard::to_allocvec(&value).unwrap(),
        Value::Bool(value) => postcard::to_allocvec(&value).unwrap(),
        Value::Date(value) => postcard::to_allocvec(&value).unwrap(),
        Value::Vec(value) => postcard::to_allocvec(&value).unwrap(),
        Value::Array(value) => {
            if value.len() == 0 {
                return Ok(postcard::to_allocvec::<Vec<String>>(&vec![]).unwrap());
            } else {
                let first = value.first().unwrap();
                let bytes = match first {
                    Value::String(_) => serialize_variant!(value, try_unwrap_string),
                    Value::Int(_) => serialize_variant!(value, try_unwrap_int),
                    Value::Float(_) => serialize_variant!(value, try_unwrap_float),
                    Value::Bool(_) => serialize_variant!(value, try_unwrap_bool),
                    Value::Date(_) => serialize_variant!(value, try_unwrap_date),
                    Value::Vec(_) => serialize_variant!(value, try_unwrap_vec),

                    Value::Array(_) => anyhow::bail!("Can't serialize Array"),
                    Value::Object(_) => anyhow::bail!("Can't serialize Object"),
                    Value::HTMLElements(_) => anyhow::bail!("Can't serialize HTMLElements"),

                    Value::Null => serialize_null!(value),

                    Value::NextFilters(_) => serialize_variant!(value, try_unwrap_next_filters),
                    Value::NextManga(_) => serialize_variant!(value, try_unwrap_next_manga),
                    Value::NextChapter(_) => serialize_variant!(value, try_unwrap_next_chapter),
                    Value::NextPageContext(_) => {
                        serialize_variant!(value, try_unwrap_next_page_context)
                    }
                    Value::NextImageResponse(_) => {
                        serialize_variant!(value, try_unwrap_next_image_response)
                    }
                }?;

                return Ok(bytes);
            }
        }
        Value::Object(_) => anyhow::bail!("Can't serialize Object"),
        Value::HTMLElements(_) => anyhow::bail!("Can't serialize HTMLElements"),
        Value::Null => anyhow::bail!("Can't serialize Null"),
        Value::NextFilters(array) => postcard::to_allocvec(&array).unwrap(),
        Value::NextManga(manga) => postcard::to_allocvec(&manga).unwrap(),
        Value::NextChapter(chapter) => postcard::to_allocvec(&chapter).unwrap(),
        Value::NextPageContext(ctx) => postcard::to_allocvec(&ctx).unwrap(),
        Value::NextImageResponse(res) => postcard::to_allocvec(&res).unwrap(),
    })
}

#[aidoku_wasm_function]
fn read_buffer(
    mut caller: Caller<'_, WasmStore>,
    ptr: i32,
    buffer_i32: i32,
    size_i32: i32,
) -> Result<i32> {
    let Some(ptr) = usize::try_from(ptr).ok() else {
        eprintln!("invalid offset");
        return Ok(-1);
    };
    let buffer = read_buffer_data(&mut caller, ptr);

    match buffer {
        Ok(buffer) => {
            let offset: usize = match usize::try_from(buffer_i32) {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("invalid offset");
                    return Ok(-1);
                }
            };

            let size: usize = match usize::try_from(size_i32) {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("invalid size");
                    return Ok(-1);
                }
            };

            let Some(memory) = get_memory(&mut caller) else {
                eprintln!("Memory error");
                return Ok(-1);
            };

            if size <= buffer.len() {
                write_bytes(&memory, &mut caller, &buffer, offset).expect("REASON");
            };
        }
        Err(error) => {
            eprintln!("Error: {error}");

            return Ok(-1);
        }
    };

    Ok(0)
}

#[aidoku_wasm_function]
fn buffer_len(mut caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    let Some(ptr) = usize::try_from(ptr).ok() else {
        eprintln!("invalid offset");
        return Ok(-1);
    };

    let buffer = read_buffer_data(&mut caller, ptr);

    Ok(match buffer {
        Ok(buffer) => buffer.len().try_into().unwrap(),
        Err(error) => {
            eprintln!("Get buffer error: {error}");

            return Ok(-1);
        }
    })
}

#[aidoku_wasm_function]
fn destroy(mut caller: Caller<'_, WasmStore>, ptr: i32) -> Result<()> {
    let wasm_store = caller.data_mut();

    wasm_store.remove_std_value(ptr as usize);

    Ok(())
}

#[aidoku_wasm_function]
fn current_date(_caller: Caller<'_, WasmStore>) -> Result<F64> {
    use chrono::Utc;
    Ok((Utc::now().timestamp() as f64).into())
}

#[aidoku_wasm_function]
fn utc_offset(_caller: Caller<'_, WasmStore>) -> Result<i64> {
    use chrono::Local;
    Ok(Local::now().offset().utc_minus_local() as i64)
}

#[aidoku_wasm_function]
fn parse_date(
    _caller: Caller<'_, WasmStore>,
    string: Option<String>,
    format: Option<String>,
    locale: Option<String>,
    timezone: Option<String>,
) -> Result<F64> {
    let Some(string) = string else {
        return Ok((Into::<i32>::into(ResultContext::InvalidDescriptor) as f64).into());
    };
    let Some(format) = format else {
        return Ok((Into::<i32>::into(ResultContext::InvalidDescriptor) as f64).into());
    };
    let Some(_locale) = locale else {
        return Ok((Into::<i32>::into(ResultContext::InvalidDescriptor) as f64).into());
    };
    let Some(timezone) = timezone else {
        return Ok((Into::<i32>::into(ResultContext::InvalidDescriptor) as f64).into());
    };

    let timezone: chrono_tz::Tz = timezone.parse().ok().unwrap_or(chrono_tz::UTC);
    let format_string = crate::source::wasm_imports::std::swift_dateformat_to_strptime(&format);

    let Some(date_time) = crate::source::wasm_imports::std::parse_flexible_datetime(
        &string,
        &format_string,
        timezone,
    )
    .ok() else {
        return Ok((Into::<i32>::into(ResultContext::InvalidDateString) as f64).into());
    };
    // chrono::NaiveDateTime::parse_from_str(string, &format_string)
    //     .ok()
    //     .and_then(|dt| dt.and_local_timezone(timezone).single())
    //     .context("failed to parse date string in read_date_string")?;

    let timestamp = date_time.timestamp() as f64
        + (date_time.timestamp_subsec_nanos() as f64) / (10f64.powi(9));
    Ok(timestamp.into())
}
