use anyhow::{bail, Context, Result};
use postcard::from_bytes;
use std::convert::TryInto;
use wasmi::{AsContext, Memory};

/// Read WASM result pointer and deserialize using `postcard`
/// T: type of Ok result
pub fn read_next<T: serde::de::DeserializeOwned>(
    memory: &Memory,
    store: &impl AsContext,
    ptr: i32,
) -> Result<T> {
    if ptr < 0 {
        eprintln!("pointer = {ptr}");

        // Handle fixed error codes
        match ptr {
            -2 => bail!("Unimplemented"),
            -3 => bail!("RequestError"),
            _ => bail!("Unknown error"),
        }
    }

    // Read the first 8 bytes: len and capacity
    let mut header = [0u8; 8];
    memory
        .read(store, ptr as usize, &mut header)
        .context("Failed to read memory header")?;

    let len = i32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
    let _cap = i32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;

    // Read the full buffer
    let mut buffer = vec![0u8; len];
    memory
        .read(store, ptr as usize, &mut buffer)
        .context("Failed to read full buffer")?;

    // Check if this is an AidokuError::Message
    if buffer[0..4] == (-1i32).to_le_bytes() {
        let err_string =
            String::from_utf8(buffer[12..].to_vec()).unwrap_or_else(|_| "<invalid utf8>".into());
        bail!(err_string);
    }

    // Otherwise, deserialize the Ok(T) payload using postcard
    from_bytes::<T>(&buffer[8..])
        .map_err(|err| {
            eprintln!("Error = {err}");
            eprintln!("capture = {:?}", String::from_utf8(buffer));

            err
        })
        .context("Deserialize failed")
}
