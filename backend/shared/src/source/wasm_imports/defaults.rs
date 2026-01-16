use anyhow::{Context, Result};
use pared::sync::Parc;
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::source::wasm_store::{Value, WasmStore};

pub fn register_defaults_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "defaults", "get", get)?;
    register_wasm_function!(linker, "defaults", "set", set)?;

    Ok(())
}

#[aidoku_wasm_function]
fn get(mut caller: Caller<'_, WasmStore>, key: Option<String>) -> Result<i32> {
    let key = key.context("key is required for get")?;
    let wasm_store = caller.data_mut();

    #[cfg(not(feature = "all"))]
    {
        let Some(value) = anyhow::Context::context(DEFAULTS_GET.get(), "Please set DEFAULTS_GET")?(
            &wasm_store.id,
            &key,
        )?
        else {
            return Ok(ResultContext::InvalidValue.into());
        };

        let pointer = wasm_store.store_std_value(Parc::from(Value::from(value)), None);

        return Ok(pointer as i32);
    }

    // FIXME actually implement a defaults system
    if key == "languages" {
        return Ok(wasm_store.store_std_value(
            Value::from(wasm_store.settings.languages.clone()).into(),
            None,
        ) as i32);
    }

    let value = wasm_store
        .source_settings
        .get(&key)
        .context("key not found in source settings")?;

    Ok(wasm_store.store_std_value(Parc::from(Value::from(value)), None) as i32)
}

#[aidoku_wasm_function]
fn set(_caller: Caller<'_, WasmStore>, key: Option<String>, value: i32) -> Result<()> {
    let key = key.context("key is required for set")?;
    println!("defaults.set: {key:?} -> {value}");
    Ok(())
}
