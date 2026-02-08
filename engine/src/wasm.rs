use anyhow::{anyhow, Result};
use wasmtime::{Caller, Engine as WasmEngine, Linker};

use crate::runtime::{HostErrorCode, RuntimeState};

pub fn read_cstr(caller: &mut Caller<'_, RuntimeState>, ptr: i32) -> Result<String> {
    let memory = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| anyhow!("missing memory export"))?;
    let data = memory.data(caller);
    let start = ptr as usize;
    if start >= data.len() {
        return Err(anyhow!("pointer out of bounds"));
    }
    let mut end = start;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }
    if end == data.len() {
        return Err(anyhow!("unterminated string"));
    }
    let bytes = &data[start..end];
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .map_err(|_| anyhow!("invalid utf-8 string"))
}

fn read_i32_slice(caller: &mut Caller<'_, RuntimeState>, ptr: i32, len: i32) -> Result<Vec<u32>> {
    if ptr < 0 || len < 0 {
        return Err(anyhow!("pointer or length out of bounds"));
    }
    let memory = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| anyhow!("missing memory export"))?;
    let data = memory.data(caller);
    let start = ptr as usize;
    let byte_len = (len as usize)
        .checked_mul(std::mem::size_of::<i32>())
        .ok_or_else(|| anyhow!("length overflow"))?;
    let end = start
        .checked_add(byte_len)
        .ok_or_else(|| anyhow!("length overflow"))?;
    if end > data.len() {
        return Err(anyhow!("pointer out of bounds"));
    }

    let mut values = Vec::with_capacity(len as usize);
    for index in 0..len as usize {
        let offset = start + index * 4;
        let bytes = [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ];
        let value = i32::from_le_bytes(bytes);
        if value < 0 {
            return Err(anyhow!("negative child id"));
        }
        values.push(value as u32);
    }

    Ok(values)
}

pub fn build_linker(engine: &WasmEngine) -> Linker<RuntimeState> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            "env",
            "__CreateElement",
            |mut caller: Caller<'_, RuntimeState>, name_ptr: i32| -> Result<i32> {
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                caller.data_mut().clear_error();
                let id = caller.data_mut().create_element(name);
                Ok(id as i32)
            },
        )
        .expect("link __CreateElement");

    linker
        .func_wrap(
            "env",
            "__SetInlineStyle",
            |mut caller: Caller<'_, RuntimeState>,
             id: i32,
             name_ptr: i32,
             value_ptr: i32|
             -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                let value = match read_cstr(&mut caller, value_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };

                match caller.data_mut().set_inline_style(id as u32, name, value) {
                    Ok(()) => {
                        caller.data_mut().clear_error();
                        Ok(0)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __SetInlineStyle");

    linker
        .func_wrap(
            "env",
            "__DestroyElement",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data_mut().destroy_element(id as u32) {
                    Ok(()) => {
                        caller.data_mut().clear_error();
                        Ok(0)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __DestroyElement");

    linker
        .func_wrap(
            "env",
            "__AppendElement",
            |mut caller: Caller<'_, RuntimeState>, parent: i32, child: i32| -> Result<i32> {
                if parent < 0 || child < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative element id");
                    return Ok(code);
                }
                match caller
                    .data_mut()
                    .append_element(parent as u32, child as u32)
                {
                    Ok(()) => {
                        caller.data_mut().clear_error();
                        Ok(0)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __AppendElement");

    linker
        .func_wrap(
            "env",
            "__AppendElements",
            |mut caller: Caller<'_, RuntimeState>,
             parent: i32,
             ptr: i32,
             len: i32|
             -> Result<i32> {
                // The array-like interface uses a contiguous i32 slice in WASM linear memory.
                // This minimizes host calls and allows bulk validation in a single pass,
                // which is faster than repeated per-element FFI transitions.
                if parent < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidParent, "negative parent id");
                    return Ok(code);
                }
                let children = match read_i32_slice(&mut caller, ptr, len) {
                    Ok(values) => values,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                match caller.data_mut().append_elements(parent as u32, &children) {
                    Ok(()) => {
                        caller.data_mut().clear_error();
                        Ok(0)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __AppendElements");

    linker
        .func_wrap(
            "env",
            "__AddStylesheet",
            |mut caller: Caller<'_, RuntimeState>, css_ptr: i32| -> Result<i32> {
                let css = match read_cstr(&mut caller, css_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                caller.data_mut().clear_error();
                caller.data_mut().add_stylesheet(css);
                Ok(0)
            },
        )
        .expect("link __AddStylesheet");

    linker
}
