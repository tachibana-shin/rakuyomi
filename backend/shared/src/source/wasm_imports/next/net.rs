use crate::{
    source::{
        wasm_imports::net::{get_building_request, DEFAULT_USER_AGENT},
        wasm_store::ResponseData,
    },
    util::has_internet_connection,
};
use anyhow::{Context, Result};
use futures::executor;
use log::warn;
use reqwest::{Method, Request};

use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasm_shared::{get_memory, memory_reader::read_values};
use wasmi::{Caller, Linker};

use crate::source::wasm_store::{RequestState, WasmStore};

#[repr(C)]
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Head,
    Delete,
    Patch,
    Options,
    Connect,
    Trace,
}

pub fn register_net_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "net", "init", init)?; // OK
    register_wasm_function!(linker, "net", "send", send)?; // OK
    register_wasm_function!(linker, "net", "send_all", send_all)?; // OK
    register_wasm_function!(linker, "net", "set_url", set_url)?; // OK
    register_wasm_function!(linker, "net", "set_header", set_header)?; // OK
    register_wasm_function!(linker, "net", "set_body", set_body)?; // OK
    register_wasm_function!(linker, "net", "data_len", data_len)?; // OK
    register_wasm_function!(linker, "net", "read_data", read_data)?; // OK
    register_wasm_function!(linker, "net", "get_image", get_image)?; // OK
    register_wasm_function!(linker, "net", "get_status_code", get_status_code)?; // OK
    register_wasm_function!(linker, "net", "get_header", get_header)?; // OK
    register_wasm_function!(linker, "net", "html", html)?; // OK
    register_wasm_function!(linker, "net", "set_rate_limit", set_rate_limit)?; // OK

    Ok(())
}

#[allow(dead_code)]
enum ResultContext {
    Success,
    InvalidDescriptor,
    InvalidString,
    InvalidMethod,
    InvalidUrl,
    // InvalidHtml,
    // InvalidBufferSize,
    MissingData,
    MissingResponse,
    // MissingUrl,
    RequestError,
    FailedMemoryWrite,
    NotAnImage,
}

impl From<ResultContext> for Result<i32> {
    fn from(result: ResultContext) -> Self {
        match result {
            ResultContext::Success => Ok(0),
            ResultContext::InvalidDescriptor => Ok(-1),
            ResultContext::InvalidString => Ok(-2),
            ResultContext::InvalidMethod => Ok(-3),
            ResultContext::InvalidUrl => Ok(-4),
            // Result::InvalidHtml => -5,
            // Result::InvalidBufferSize => Ok(-6),
            ResultContext::MissingData => Ok(-7),
            ResultContext::MissingResponse => Ok(-8),
            // Result::MissingUrl => Ok(-9),
            ResultContext::RequestError => Ok(-10),
            ResultContext::FailedMemoryWrite => Ok(-11),
            ResultContext::NotAnImage => Ok(-12),
        }
    }
}

impl From<ResultContext> for i32 {
    fn from(result: ResultContext) -> Self {
        match result {
            ResultContext::Success => 0,
            ResultContext::InvalidDescriptor => -1,
            ResultContext::InvalidString => -2,
            ResultContext::InvalidMethod => -3,
            ResultContext::InvalidUrl => -4,
            // Result::InvalidHtml => -5,
            // Result::InvalidBufferSize => Ok(-6),
            ResultContext::MissingData => -7,
            ResultContext::MissingResponse => -8,
            // Result::MissingUrl => Ok(-9),
            ResultContext::RequestError => -10,
            ResultContext::FailedMemoryWrite => -11,
            ResultContext::NotAnImage => -12,
        }
    }
}
type FFIResult = Result<i32>;

fn to_method(value: HttpMethod) -> Method {
    match value {
        HttpMethod::Get => Method::GET,
        HttpMethod::Post => Method::POST,
        HttpMethod::Put => Method::PUT,
        HttpMethod::Head => Method::HEAD,
        HttpMethod::Delete => Method::DELETE,
        HttpMethod::Patch => Method::PATCH,
        HttpMethod::Options => Method::OPTIONS,
        HttpMethod::Connect => Method::CONNECT,
        HttpMethod::Trace => Method::TRACE,
    }
}
#[aidoku_wasm_function]
fn init(mut caller: Caller<'_, WasmStore>, method: i32) -> FFIResult {
    let method = match method {
        0 => HttpMethod::Get,
        1 => HttpMethod::Post,
        2 => HttpMethod::Put,
        3 => HttpMethod::Head,
        4 => HttpMethod::Delete,
        5 => HttpMethod::Patch,
        6 => HttpMethod::Options,
        7 => HttpMethod::Connect,
        8 => HttpMethod::Trace,
        _ => return ResultContext::InvalidMethod.into(),
    };
    let wasm_store = caller.data_mut();

    // TODO maybe also return a mut reference in create_request to building state?
    // should help with type safety down below. or maybe not idk ig its fine
    let request_descriptor = wasm_store.create_request();
    let Some(request) = get_building_request(wasm_store, request_descriptor).ok() else {
        return ResultContext::FailedMemoryWrite.into();
    };
    request.method = Some(to_method(method));

    request
        .headers
        .insert("User-Agent".into(), DEFAULT_USER_AGENT.into());

    Ok(request_descriptor as i32)
}

#[aidoku_wasm_function]
fn send(caller: Caller<'_, WasmStore>, request_ptr: i32) -> FFIResult {
    crate::source::wasm_imports::net::send(caller, request_ptr)?;
    ResultContext::Success.into()
}
#[aidoku_wasm_function]
fn send_all(mut caller: Caller<'_, WasmStore>, rd: i32, len: i32) -> FFIResult {
    let Some(memory) = get_memory(&mut caller) else {
        return ResultContext::FailedMemoryWrite.into();
    };

    let ids = {
        let Some(v) = read_values::<i32>(&memory, &caller, rd as usize, len as usize) else {
            return ResultContext::MissingData.into();
        };
        v
    };
    println!("Send all {:?}", ids);

    let store = caller.data_mut();
    let cancellation_token = store.context.cancellation_token.clone();

    let has_internet_connection =
        executor::block_on(cancellation_token.run_until_cancelled(has_internet_connection()))
            .context("failed to check internet connection")?;
    if !has_internet_connection {
        anyhow::bail!("no internet connection available");
    }

    for request_descriptor_i32 in ids {
        let Some(request_descriptor_i32) = usize::try_from(request_descriptor_i32).ok() else {
            return ResultContext::InvalidDescriptor.into();
        };
        let request_builder = get_building_request(store, request_descriptor_i32)?;
        let client = reqwest::Client::new();
        let request = Request::try_from(&*request_builder).context("failed to build request")?;

        let warn_cancellation = || {
            warn!(
                "request to {:?} was cancelled mid-flight!",
                &request_builder.url
            );
        };

        let response = match executor::block_on(
            cancellation_token.run_until_cancelled(client.execute(request)),
        ) {
            Some(response) => response
                .map_err(|err| {
                    println!("request failed: {err}");
                    err
                })
                .context("failed to execute request")?,
            _ => {
                warn_cancellation();
                anyhow::bail!("request was cancelled mid-flight");
            }
        };

        let response_data = ResponseData {
            url: response.url().clone(),
            headers: response.headers().clone(),
            status_code: response.status(),
            body: match executor::block_on(cancellation_token.run_until_cancelled(response.bytes()))
            {
                Some(bytes) => bytes
                    .context("failed to read response bytes")
                    .map(|bytes| bytes.to_vec())
                    .ok(),
                _ => {
                    warn_cancellation();
                    anyhow::bail!("request was cancelled mid-flight while reading body");
                }
            },
            bytes_read: 0,
        };

        *store
            .get_mut_request(request_descriptor_i32)
            .context("failed to get request state")? = RequestState::Sent(response_data);
    }

    ResultContext::Success.into()
}

#[aidoku_wasm_function]
fn set_url(caller: Caller<'_, WasmStore>, request_ptr: i32, url: Option<String>) -> FFIResult {
    crate::source::wasm_imports::net::set_url(caller, request_ptr, url)?;
    ResultContext::Success.into()
}
#[aidoku_wasm_function]
fn set_header(
    caller: Caller<'_, WasmStore>,
    request_ptr: i32,
    name: Option<String>,
    value: Option<String>,
) -> Result<i32> {
    crate::source::wasm_imports::net::set_header(caller, request_ptr, name, value)?;

    ResultContext::Success.into()
}

#[aidoku_wasm_function]
fn set_body(caller: Caller<'_, WasmStore>, request_ptr: i32, bytes: Option<Vec<u8>>) -> FFIResult {
    crate::source::wasm_imports::net::set_body(caller, request_ptr, bytes)?;
    ResultContext::Success.into()
}

#[aidoku_wasm_function]
fn data_len(caller: Caller<'_, WasmStore>, request_ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::net::get_data_size(caller, request_ptr)
}

#[aidoku_wasm_function]
fn read_data(caller: Caller<'_, WasmStore>, request_ptr: i32, buffer: i32, size: i32) -> FFIResult {
    crate::source::wasm_imports::net::get_data(caller, request_ptr, buffer, size)?;
    ResultContext::Success.into()
}

#[aidoku_wasm_function]
fn get_image(mut caller: Caller<'_, WasmStore>, request_ptr: i32) -> FFIResult {
    let wasm_store = caller.data_mut();
    let Some(request_descriptor): Option<usize> = request_ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let bytes_to_create_image = {
        let Some(request) = wasm_store.get_mut_request(request_descriptor) else {
            return ResultContext::InvalidDescriptor.into();
        };

        let response = match request {
            RequestState::Sent(response) => Some(response),
            _ => None,
        };
        let Some(response) = response else {
            return ResultContext::RequestError.into();
        };

        response
            .body
            .as_ref()
            .context("response body not found")?
            .clone()
    };

    Ok(wasm_store
        .create_image(&bytes_to_create_image)
        .map(|v| v as i32)
        .unwrap_or(ResultContext::NotAnImage.into()))
}
#[aidoku_wasm_function]
fn get_status_code(caller: Caller<'_, WasmStore>, request_ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::net::get_status_code(caller, request_ptr)
}
#[aidoku_wasm_function]
fn get_header(
    caller: Caller<'_, WasmStore>,
    request_ptr: i32,
    name: Option<String>,
) -> Result<i32> {
    crate::source::wasm_imports::net::get_header(caller, request_ptr, name)
}

#[aidoku_wasm_function]
fn html(caller: Caller<'_, WasmStore>, request_ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::net::html(caller, request_ptr)
}
#[aidoku_wasm_function]
fn set_rate_limit(
    _caller: Caller<'_, WasmStore>,
    _permits: i32,
    _period: i32,
    _unit: i32,
) -> Result<()> {
    // leaving this function unimplemented for now
    Ok(())
}
