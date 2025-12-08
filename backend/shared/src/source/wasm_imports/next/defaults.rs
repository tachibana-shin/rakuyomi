use anyhow::Result;
use pared::sync::Parc;
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::source::wasm_store::{Value, WasmStore};

pub fn register_defaults_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "defaults", "get", get)?;
    register_wasm_function!(linker, "defaults", "set", set)?;

    Ok(())
}

#[allow(dead_code)]
enum ResultContext {
    Success,
    InvalidKey,
    InvalidValue,
    FailedEncoding,
    FailedDecoding,
}
impl From<ResultContext> for i32 {
    fn from(result: ResultContext) -> Self {
        match result {
            ResultContext::Success => 0,
            ResultContext::InvalidKey => -1,
            ResultContext::InvalidValue => -2,
            ResultContext::FailedEncoding => -3,
            ResultContext::FailedDecoding => -4,
        }
    }
}

#[aidoku_wasm_function]
fn get(mut caller: Caller<'_, WasmStore>, key: Option<String>) -> Result<i32> {
    let Some(key) = key else {
        return Ok(ResultContext::InvalidKey.into());
    };

    let wasm_store = caller.data_mut();

    // FIXME actually implement a defaults system
    if key == "languages" {
        return Ok(wasm_store.store_std_value(
            Value::from(wasm_store.settings.languages.clone()).into(),
            None,
        ) as i32);
    }

    let Some(value) = wasm_store.source_settings.get(&key).cloned() else {
        return Ok(ResultContext::InvalidValue.into());
    };

    let pointer = wasm_store.store_std_value(Parc::from(Value::from(value)), None);
    wasm_store.mark_str_encode(pointer);
    Ok(pointer as i32)
}

#[aidoku_wasm_function]
fn set(_caller: Caller<'_, WasmStore>, key: Option<String>, _kind: i32, value: i32) -> Result<i32> {
    let Some(key) = key else {
        return Ok(ResultContext::InvalidKey.into());
    };
    println!("defaults.set: {key:?} -> {value}");
    Ok(0)
}
