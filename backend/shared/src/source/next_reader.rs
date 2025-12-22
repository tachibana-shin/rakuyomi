use anyhow::{bail, Context, Result};
use postcard::from_bytes;
use std::convert::TryInto;
use wasmi::{AsContext, Memory};

pub fn read_next_raw(memory: &Memory, store: &impl AsContext, ptr: i32) -> Result<Vec<u8>> {
    if ptr < 0 {
        eprintln!("pointer = {ptr}");

        // Handle fixed error codes
        match ptr {
            -2 => bail!("Unimplemented"),
            -3 => bail!("RequestError"),
            _ => bail!("Unknown error"),
        }
    }

    let mut tag_bytes = [0u8; 4];
    memory
        .read(store, ptr as usize, &mut tag_bytes)
        .context("Failed to read memory header")?;

    let tag = i32::from_le_bytes(tag_bytes);

    if tag == -1 {
        let mut hdr = [0u8; 8];
        memory.read(store, ptr as usize + 4, &mut hdr)?;

        let _cap = i32::from_le_bytes(hdr[0..4].try_into().unwrap()) as usize;
        let len = i32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;

        let mut buf = vec![0u8; 12 + len];
        memory.read(store, ptr as usize, &mut buf)?;

        let msg = String::from_utf8_lossy(&buf[12..]).to_string();
        bail!(msg);
    }

    let len = tag as usize;

    let mut cap_bytes = [0u8; 4];
    memory.read(store, ptr as usize + 4, &mut cap_bytes)?;
    let _cap = i32::from_le_bytes(cap_bytes) as usize;

    let mut buffer = vec![0u8; 8 + len];
    memory.read(store, ptr as usize, &mut buffer)?;

    Ok(buffer[8..].to_vec())
}
/// Read WASM result pointer and deserialize using `postcard`
/// T: type of Ok result
pub fn read_next<T: serde::de::DeserializeOwned>(
    memory: &Memory,
    store: &impl AsContext,
    ptr: i32,
) -> Result<T> {
    let buffer = read_next_raw(memory, store, ptr)?;
    from_bytes::<T>(&buffer)
        .map_err(|err| {
            eprintln!("Error = {err}");
            eprintln!("capture = {:?}", String::from_utf8(buffer));

            err
        })
        .context("Deserialize failed")
}
