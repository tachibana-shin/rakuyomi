use anyhow::Result;
use pared::sync::Parc;
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::{
    settings::SourceSettingValue,
    source::{
        next_reader::{read_next, read_next_raw},
        wasm_store::{Value, WasmStore},
    },
};

#[cfg(not(feature = "all"))]
pub static DEFAULTS_SET: std::sync::OnceLock<
    fn(source_id: &str, key: &str, value: &SourceSettingValue) -> Result<()>,
> = std::sync::OnceLock::new();

#[cfg(not(feature = "all"))]
pub static DEFAULTS_GET: std::sync::OnceLock<
    fn(source_id: &str, key: &str) -> Result<Option<SourceSettingValue>>,
> = std::sync::OnceLock::new();
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
        wasm_store.mark_str_encode(pointer);

        return Ok(pointer as i32);
    }
    // FIXME actually implement a defaults system
    if key == "languages" {
        return Ok(wasm_store.store_std_value(
            Value::from(wasm_store.settings.languages.clone()).into(),
            None,
        ) as i32);
    }

    let Some(value) = wasm_store.source_settings.get(&key) else {
        return Ok(ResultContext::InvalidValue.into());
    };

    let pointer = wasm_store.store_std_value(Parc::from(Value::from(value)), None);
    wasm_store.mark_str_encode(pointer);
    Ok(pointer as i32)
}

#[aidoku_wasm_function]
fn set(
    mut caller: Caller<'_, WasmStore>,
    key: Option<String>,
    kind: i32,
    value_ptr: i32,
) -> Result<i32> {
    let Some(key) = key else {
        return Ok(ResultContext::InvalidKey.into());
    };

    let memory = {
        let Some(memory) = wasm_shared::get_memory(&mut caller) else {
            anyhow::bail!("get_memory failed");
        };
        memory
    };
    let decoded = match kind {
        0 => read_next_raw(&memory, &caller, value_ptr).map(SourceSettingValue::Data),
        1 => read_next::<bool>(&memory, &caller, value_ptr).map(SourceSettingValue::Bool),
        2 => read_next::<i64>(&memory, &caller, value_ptr).map(SourceSettingValue::Int),
        3 => read_next::<f64>(&memory, &caller, value_ptr).map(SourceSettingValue::Float),
        4 => read_next::<String>(&memory, &caller, value_ptr).map(SourceSettingValue::String),
        5 => read_next::<Vec<String>>(&memory, &caller, value_ptr).map(SourceSettingValue::Vec),
        6 => Ok(SourceSettingValue::Null),
        _ => return Ok(ResultContext::FailedDecoding.into()),
    };

    let value = match decoded {
        Ok(v) => v,
        Err(_) => return Ok(ResultContext::FailedDecoding.into()),
    };

    #[cfg(feature = "all")]
    {
        let wasm_store = caller.data_mut();

        wasm_store
            .source_settings
            .save(&key.clone(), value.clone())?;
    }
    #[cfg(not(feature = "all"))]
    (anyhow::Context::context(DEFAULTS_SET.get(), "Please set DEFAULTS_SET")?)(
        &caller.data().id,
        &key,
        &value,
    )?;

    println!("defaults.set: {:?} -> {:?}", key, value);
    Ok(0)
}
