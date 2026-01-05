#![cfg_attr(feature = "all", allow(unused_variables))]
#![cfg_attr(feature = "all", allow(unused_mut))]

use anyhow::Result;

use boa_engine::{JsString, Source};
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::source::wasm_store::{Value, WasmStore};

pub fn register_js_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "js", "context_create", context_create)?;
    register_wasm_function!(linker, "js", "context_eval", context_eval)?;
    register_wasm_function!(linker, "js", "context_get", context_get)?;

    register_wasm_function!(linker, "js", "webview_create", webview_create)?;
    register_wasm_function!(linker, "js", "webview_load", webview_load)?;
    register_wasm_function!(linker, "js", "webview_load_html", webview_load_html)?;
    register_wasm_function!(linker, "js", "webview_wait_for_load", webview_wait_for_load)?;
    register_wasm_function!(linker, "js", "webview_eval", webview_eval)?;
    Ok(())
}

type FFIResult = Result<i32>;

enum ResultContext {
    // Success,
    #[allow(clippy::enum_variant_names)]
    MissingResult,
    InvalidContext,
    InvalidString,
    // InvalidHandler,
    // InvalidRequest,
}

impl From<ResultContext> for i32 {
    fn from(result: ResultContext) -> Self {
        match result {
            // Result::Success => 0,
            ResultContext::MissingResult => -1,
            ResultContext::InvalidContext => -2,
            ResultContext::InvalidString => -3,
            // ResultContext::InvalidHandler => -4,
            // ResultContext::InvalidRequest => -5,
        }
    }
}

#[aidoku_wasm_function]
fn context_create(mut caller: Caller<'_, WasmStore>) -> FFIResult {
    let store = caller.data_mut();

    Ok(store.create_js_context() as i32)
}
#[aidoku_wasm_function]
fn context_eval(mut caller: Caller<'_, WasmStore>, ctx_id: i32, src: Option<String>) -> FFIResult {
    let store = caller.data_mut();
    let Some(context) = store.get_js_context(ctx_id as usize).map(|ctx| &mut ctx.0) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    let Some(src) = src else {
        return Ok(ResultContext::InvalidString.into());
    };

    let Ok(result) = context.eval(Source::from_bytes(&src)) else {
        return Ok(ResultContext::MissingResult.into());
    };
    let Some(result_string) = result
        .to_string(context)
        .ok()
        .and_then(|s| s.to_std_string().ok())
    else {
        return Ok(ResultContext::MissingResult.into());
    };

    Ok(store.store_std_value(Value::String(result_string).into(), None) as i32)
}

#[aidoku_wasm_function]
fn context_get(mut caller: Caller<'_, WasmStore>, ctx_id: i32, name: Option<String>) -> FFIResult {
    let store = caller.data_mut();
    let Some(context) = store.get_js_context(ctx_id as usize).map(|ctx| &mut ctx.0) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    let Some(name) = name else {
        return Ok(ResultContext::InvalidString.into());
    };

    let key: JsString = name.into();
    let Ok(result) = context.global_object().get(key, context) else {
        return Ok(ResultContext::MissingResult.into());
    };
    let Some(result_string) = result
        .to_string(context)
        .ok()
        .and_then(|s| s.to_std_string().ok())
    else {
        return Ok(ResultContext::MissingResult.into());
    };

    Ok(store.store_std_value(Value::String(result_string).into(), None) as i32)
}

#[aidoku_wasm_function]
fn webview_create(mut caller: Caller<'_, WasmStore>) -> FFIResult {
    #[cfg(not(feature = "all"))]
    {
        let store = caller.data_mut();

        Ok(store.create_webview() as i32)
    }

    #[cfg(feature = "all")]
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_load(
    mut caller: Caller<'_, WasmStore>,
    webview_ptr: i32,
    request_ptr: i32,
) -> FFIResult {
    #[cfg(not(feature = "all"))]
    {
        let store = caller.data_mut();

        store.load_webview(webview_ptr as usize, request_ptr as usize)?;

        Ok(0)
    }

    #[cfg(feature = "all")]
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_load_html(
    mut caller: Caller<'_, WasmStore>,
    webview_ptr: i32,
    html: Option<String>,
    url: Option<String>,
) -> FFIResult {
    #[cfg(not(feature = "all"))]
    {
        let store = caller.data_mut();

        let Some(webview) = store.get_webview(webview_ptr as usize) else {
            return Ok(ResultContext::InvalidContext as i32);
        };
        let Some(url) = url.and_then(|s| url::Url::parse(&s).ok()) else {
            return Ok(ResultContext::InvalidString as i32);
        };

        webview.load(html, &url)?;

        Ok(0)
    }

    #[cfg(feature = "all")]
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_wait_for_load(mut caller: Caller<'_, WasmStore>, webview_ptr: i32) -> FFIResult {
    #[cfg(not(feature = "all"))]
    {
        let store = caller.data_mut();

        let Some(webview) = store.get_webview(webview_ptr as usize) else {
            return Ok(ResultContext::InvalidContext as i32);
        };

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { webview.wait_for_load().await })
        })?;

        Ok(0)
    }

    #[cfg(feature = "all")]
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_eval(
    mut caller: Caller<'_, WasmStore>,
    webview_ptr: i32,
    code: Option<String>,
) -> FFIResult {
    #[cfg(not(feature = "all"))]
    {
        let store = caller.data_mut();

        let Some(webview) = store.get_webview(webview_ptr as usize) else {
            return Ok(ResultContext::InvalidContext as i32);
        };
        let Some(code) = code else {
            return Ok(ResultContext::InvalidString as i32);
        };

        let value = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { webview.eval(&code).await })
        })?;

        Ok(store.store_std_value(Value::String(value).into(), None) as i32)
    }

    #[cfg(feature = "all")]
    Ok(-1)
}
