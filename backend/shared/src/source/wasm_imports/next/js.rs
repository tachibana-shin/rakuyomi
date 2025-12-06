use anyhow::Result;

use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::source::wasm_store::WasmStore;

pub fn register_js_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "js", "context_create", context_create)?;
    register_wasm_function!(linker, "js", "context_eval", context_eval)?;
    register_wasm_function!(linker, "js", "webview_create", webview_create)?;
    register_wasm_function!(linker, "js", "webview_load", webview_load)?;
    register_wasm_function!(linker, "js", "webview_load_html", webview_load_html)?;
    register_wasm_function!(linker, "js", "webview_wait_for_load", webview_wait_for_load)?;
    register_wasm_function!(linker, "js", "webview_eval", webview_eval)?;
    Ok(())
}

type FFIResult = Result<i32>;
#[aidoku_wasm_function]
fn context_create(_caller: Caller<'_, WasmStore>) -> FFIResult {
    Ok(-1)
}
#[aidoku_wasm_function]
fn context_eval(_caller: Caller<'_, WasmStore>, _id: i32, _code: Option<String>) -> FFIResult {
    Ok(-1)
}

#[aidoku_wasm_function]
fn webview_create(_caller: Caller<'_, WasmStore>) -> FFIResult {
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_load(_caller: Caller<'_, WasmStore>, _webview: i32, _request: i32) -> FFIResult {
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_load_html(
    _caller: Caller<'_, WasmStore>,
    _webview: i32,
    _html: Option<String>,
    _url: Option<String>,
) -> FFIResult {
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_wait_for_load(_caller: Caller<'_, WasmStore>, _webview: i32) -> FFIResult {
    Ok(-1)
}
#[aidoku_wasm_function]
fn webview_eval(_caller: Caller<'_, WasmStore>, _webview: i32, _url: Option<String>) -> FFIResult {
    Ok(-1)
}
