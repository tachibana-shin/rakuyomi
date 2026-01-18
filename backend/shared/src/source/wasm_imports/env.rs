use anyhow::Result;
use log::{error, info};
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasm_shared::{
    get_memory,
    memory_reader::{read_bytes, read_string},
};
use wasmi::{errors::HostError, Caller, Linker};

use crate::source::wasm_store::WasmStore;

pub fn register_env_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "env", "print", print)?;
    linker.func_wrap("env", "abort", abort)?;

    Ok(())
}

#[aidoku_wasm_function]
fn print(caller: Caller<'_, WasmStore>, string: Option<String>) -> Result<()> {
    let string = string.unwrap_or_default();
    let wasm_store = caller.data();

    info!("{}: env.print: {string}", wasm_store.id);
    #[cfg(not(feature = "all"))]
    (anyhow::Context::context(
        crate::source::wasm_imports::next::env::LOG_PRINT.get(),
        "Please set LOG_PRINT",
    )?)(&caller, &string)?;

    Ok(())
}

#[derive(thiserror::Error, Debug)]
#[error("source aborted")]
struct AbortError {
    message: String,
    file_name: String,
    line: i32,
    column: i32,
}

impl HostError for AbortError {}

fn abort(
    mut caller: Caller<'_, WasmStore>,
    msg_offset: i32,
    file_name_offset: i32,
    line: i32,
    column: i32,
) -> core::result::Result<(), wasmi::Error> {
    // For some stupid reason, unlike _all_ of the Aidoku WASM function exports, this
    // specifically receives the offsets of the beginning of the stream, and the length comes
    // before the offset (?)
    let memory = get_memory(&mut caller).unwrap();
    let msg_length = read_bytes(&memory, &caller, (msg_offset - 4) as usize, 1)
        .and_then(|bytes| bytes.first().cloned())
        .unwrap_or(0) as usize;

    let file_name_length = read_bytes(&memory, &caller, (file_name_offset - 4) as usize, 1)
        .and_then(|bytes| bytes.first().cloned())
        .unwrap_or(0) as usize;

    let message = read_string(&memory, &caller, msg_offset as usize, msg_length);
    let file = read_string(
        &memory,
        &caller,
        file_name_offset as usize,
        file_name_length,
    );

    let wasm_store = caller.data();

    error!(
        "{}: env.abort called with {:?} (file: {:?}, {line}:{column})",
        &wasm_store.id, &message, &file
    );

    let error = AbortError {
        message: message.unwrap_or_default(),
        file_name: file.unwrap_or_default(),
        line,
        column,
    };

    Err(wasmi::Error::host(error))
}
