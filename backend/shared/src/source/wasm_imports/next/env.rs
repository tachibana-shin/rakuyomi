use anyhow::Result;
use log::{error, info};
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{errors::HostError, Caller, Linker};

use crate::source::wasm_store::WasmStore;

pub fn register_env_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "env", "print", print)?; // OK
    register_wasm_function!(linker, "env", "sleep", sleep)?; // OK
    linker.func_wrap("env", "abort", abort)?;
    register_wasm_function!(linker, "env", "send_partial_result", send_partial_result)?; // OK

    Ok(())
}

#[aidoku_wasm_function]
fn print(caller: Caller<'_, WasmStore>, string: Option<String>) -> Result<()> {
    let string = string.unwrap_or_default();
    let wasm_store = caller.data();

    info!("{}: env.print: {string}", wasm_store.id);
    Ok(())
}
#[aidoku_wasm_function]
pub fn sleep(_caller: Caller<'_, WasmStore>, seconds: i32) {
    std::thread::sleep(std::time::Duration::from_secs(seconds as u64));
}
#[aidoku_wasm_function]
fn send_partial_result(_caller: Caller<'_, WasmStore>, _i: i32) -> Result<()> {
    Ok(())
}

#[derive(thiserror::Error, Debug)]
#[error("source aborted")]
struct AbortError {}

impl HostError for AbortError {}

fn abort(caller: Caller<'_, WasmStore>) -> core::result::Result<(), wasmi::Error> {
    let wasm_store = caller.data();

    error!("{}: env.abort called", &wasm_store.id);

    Err(wasmi::Error::host(AbortError {}))
}
