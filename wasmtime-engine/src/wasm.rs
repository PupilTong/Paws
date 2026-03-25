use anyhow::{anyhow, Result};
use wasmtime::{Caller, Engine as WasmEngine, Linker};

use engine::{HostErrorCode, RuntimeState};

/// Resolves the WASM memory export **once** and passes the full linear-memory
/// `&[u8]` into `f`.
///
/// Handles both regular `Memory` exports (WAT tests) and `SharedMemory`
/// exports (modules compiled with `wasm32-wasip1-threads`). The export lookup
/// (`get_export("memory")`) and memory-type dispatch happen exactly once per
/// call, regardless of how much data the callback reads.
fn with_memory_data<T>(
    caller: &mut Caller<'_, RuntimeState>,
    f: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<T> {
    let export = caller
        .get_export("memory")
        .ok_or_else(|| anyhow!("missing memory export"))?;

    // Try regular Memory first (WAT / non-threaded modules).
    if let Some(memory) = export.clone().into_memory() {
        let data = memory.data(&*caller);
        return f(data);
    }

    // Fall back to SharedMemory (wasm32-wasip1-threads modules).
    if let Some(shared) = export.into_shared_memory() {
        let raw = shared.data();
        // SAFETY: Shared memory may be concurrently modified, but in our
        // single-threaded WASM execution model no concurrent writes occur
        // during host function calls. We read a snapshot of the data.
        let data = unsafe { std::slice::from_raw_parts(raw.as_ptr() as *const u8, raw.len()) };
        return f(data);
    }

    Err(anyhow!("memory export is neither Memory nor SharedMemory"))
}

/// Reads a null-terminated C string starting at `ptr` in WASM linear memory.
///
/// The memory export is resolved once. The full `&[u8]` slice is scanned
/// in-place for the null terminator — no intermediate `Vec` allocations and
/// no chunked reads.
pub fn read_cstr(caller: &mut Caller<'_, RuntimeState>, ptr: i32) -> Result<String> {
    let start = ptr as usize;

    with_memory_data(caller, |data| {
        if start >= data.len() {
            return Err(anyhow!("pointer out of bounds"));
        }
        match data[start..].iter().position(|&b| b == 0) {
            Some(null_pos) => std::str::from_utf8(&data[start..start + null_pos])
                .map(|s| s.to_string())
                .map_err(|_| anyhow!("invalid utf-8 string")),
            None => Err(anyhow!("unterminated string")),
        }
    })
}

/// Reads a contiguous `i32` array from WASM linear memory and returns the
/// values as `u32`s (after validating they are non-negative).
///
/// The memory export is resolved once; elements are read directly from the
/// backing `&[u8]` slice without an intermediate copy.
fn read_i32_slice(caller: &mut Caller<'_, RuntimeState>, ptr: i32, len: i32) -> Result<Vec<u32>> {
    if ptr < 0 || len < 0 {
        return Err(anyhow!("pointer or length out of bounds"));
    }
    let start = ptr as usize;
    let count = len as usize;
    let byte_len = count
        .checked_mul(std::mem::size_of::<i32>())
        .ok_or_else(|| anyhow!("length overflow"))?;
    let end = start
        .checked_add(byte_len)
        .ok_or_else(|| anyhow!("length overflow"))?;

    with_memory_data(caller, |data| {
        if end > data.len() {
            return Err(anyhow!("pointer out of bounds"));
        }
        let mut values = Vec::with_capacity(count);
        for i in 0..count {
            let off = start + i * 4;
            let bytes = [data[off], data[off + 1], data[off + 2], data[off + 3]];
            let value = i32::from_le_bytes(bytes);
            if value < 0 {
                return Err(anyhow!("negative child id"));
            }
            values.push(value as u32);
        }
        Ok(values)
    })
}

/// Writes bytes into WASM linear memory at the given offset.
///
/// Returns `Ok(())` if the write fits within the memory, or `Err` on
/// out-of-bounds access.
fn write_to_memory(caller: &mut Caller<'_, RuntimeState>, ptr: i32, data: &[u8]) -> Result<()> {
    let start = ptr as usize;
    let end = start
        .checked_add(data.len())
        .ok_or_else(|| anyhow!("length overflow"))?;

    let export = caller
        .get_export("memory")
        .ok_or_else(|| anyhow!("missing memory export"))?;

    if let Some(memory) = export.clone().into_memory() {
        let mem = memory.data_mut(&mut *caller);
        if end > mem.len() {
            return Err(anyhow!("pointer out of bounds"));
        }
        mem[start..end].copy_from_slice(data);
        return Ok(());
    }

    if let Some(shared) = export.into_shared_memory() {
        let raw = shared.data();
        if end > raw.len() {
            return Err(anyhow!("pointer out of bounds"));
        }
        // SAFETY: Single-threaded WASM execution — no concurrent writes during
        // host function calls.
        unsafe {
            let dst = raw.as_ptr() as *mut u8;
            std::ptr::copy_nonoverlapping(data.as_ptr(), dst.add(start), data.len());
        }
        return Ok(());
    }

    Err(anyhow!("memory export is neither Memory nor SharedMemory"))
}

/// Reads a raw byte region from WASM linear memory.
///
/// Unlike `read_cstr`, this function *does* allocate a `Vec<u8>` because the
/// caller needs owned bytes. However, the memory export is resolved only once.
fn read_byte_vec(caller: &mut Caller<'_, RuntimeState>, ptr: i32, len: i32) -> Result<Vec<u8>> {
    if ptr < 0 || len < 0 {
        return Err(anyhow!("pointer or length out of bounds"));
    }
    let start = ptr as usize;
    let byte_len = len as usize;
    let end = start
        .checked_add(byte_len)
        .ok_or_else(|| anyhow!("length overflow"))?;

    with_memory_data(caller, |data| {
        if end > data.len() {
            return Err(anyhow!("pointer out of bounds"));
        }
        Ok(data[start..end].to_vec())
    })
}

pub fn build_linker(engine: &WasmEngine) -> Linker<RuntimeState> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            "env",
            "__create_element",
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
        .expect("link __create_element");

    linker
        .func_wrap(
            "env",
            "__set_inline_style",
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
        .expect("link __set_inline_style");

    linker
        .func_wrap(
            "env",
            "__destroy_element",
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
        .expect("link __destroy_element");

    linker
        .func_wrap(
            "env",
            "__append_element",
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
        .expect("link __append_element");

    linker
        .func_wrap(
            "env",
            "__append_elements",
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
        .expect("link __append_elements");

    linker
        .func_wrap(
            "env",
            "__add_stylesheet",
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
        .expect("link __add_stylesheet");

    linker
        .func_wrap(
            "env",
            "__set_attribute",
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

                match caller.data_mut().set_attribute(id as u32, name, value) {
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
        .expect("link __set_attribute");

    linker
        .func_wrap(
            "env",
            "__commit",
            |mut caller: Caller<'_, RuntimeState>| -> Result<i32> {
                caller.data_mut().commit();
                caller.data_mut().clear_error();
                Ok(0)
            },
        )
        .expect("link __commit");

    // -----------------------------------------------------------------------
    // New DOM query / mutation host functions
    // -----------------------------------------------------------------------

    linker
        .func_wrap(
            "env",
            "__get_first_child",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_first_child(id as u32) {
                    Ok(Some(child_id)) => {
                        caller.data_mut().clear_error();
                        Ok(child_id as i32)
                    }
                    Ok(None) => {
                        caller.data_mut().clear_error();
                        Ok(-1)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_first_child");

    linker
        .func_wrap(
            "env",
            "__get_last_child",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_last_child(id as u32) {
                    Ok(Some(child_id)) => {
                        caller.data_mut().clear_error();
                        Ok(child_id as i32)
                    }
                    Ok(None) => {
                        caller.data_mut().clear_error();
                        Ok(-1)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_last_child");

    linker
        .func_wrap(
            "env",
            "__get_next_sibling",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_next_sibling(id as u32) {
                    Ok(Some(sibling_id)) => {
                        caller.data_mut().clear_error();
                        Ok(sibling_id as i32)
                    }
                    Ok(None) => {
                        caller.data_mut().clear_error();
                        Ok(-1)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_next_sibling");

    linker
        .func_wrap(
            "env",
            "__get_previous_sibling",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_previous_sibling(id as u32) {
                    Ok(Some(sibling_id)) => {
                        caller.data_mut().clear_error();
                        Ok(sibling_id as i32)
                    }
                    Ok(None) => {
                        caller.data_mut().clear_error();
                        Ok(-1)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_previous_sibling");

    linker
        .func_wrap(
            "env",
            "__get_parent_element",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_parent_element(id as u32) {
                    Ok(Some(parent_id)) => {
                        caller.data_mut().clear_error();
                        Ok(parent_id as i32)
                    }
                    Ok(None) => {
                        caller.data_mut().clear_error();
                        Ok(-1)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_parent_element");

    linker
        .func_wrap(
            "env",
            "__get_parent_node",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_parent_node(id as u32) {
                    Ok(Some(parent_id)) => {
                        caller.data_mut().clear_error();
                        Ok(parent_id as i32)
                    }
                    Ok(None) => {
                        caller.data_mut().clear_error();
                        Ok(-1)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_parent_node");

    linker
        .func_wrap(
            "env",
            "__is_connected",
            |mut caller: Caller<'_, RuntimeState>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().is_connected(id as u32) {
                    Ok(connected) => {
                        caller.data_mut().clear_error();
                        Ok(if connected { 1 } else { 0 })
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __is_connected");

    linker
        .func_wrap(
            "env",
            "__has_attribute",
            |mut caller: Caller<'_, RuntimeState>, id: i32, name_ptr: i32| -> Result<i32> {
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
                match caller.data().has_attribute(id as u32, &name) {
                    Ok(has) => {
                        caller.data_mut().clear_error();
                        Ok(if has { 1 } else { 0 })
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __has_attribute");

    linker
        .func_wrap(
            "env",
            "__get_attribute",
            |mut caller: Caller<'_, RuntimeState>,
             id: i32,
             name_ptr: i32,
             buf_ptr: i32,
             buf_len: i32|
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
                match caller.data().get_attribute(id as u32, &name) {
                    Ok(Some(value)) => {
                        let bytes = value.as_bytes();
                        let needed = bytes.len() as i32;
                        if buf_len >= needed {
                            if let Err(err) = write_to_memory(&mut caller, buf_ptr, bytes) {
                                let code = caller
                                    .data_mut()
                                    .set_error(HostErrorCode::MemoryError, err.to_string());
                                return Ok(code);
                            }
                        }
                        caller.data_mut().clear_error();
                        Ok(needed)
                    }
                    Ok(None) => {
                        caller.data_mut().clear_error();
                        Ok(-1)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_attribute");

    linker
        .func_wrap(
            "env",
            "__remove_attribute",
            |mut caller: Caller<'_, RuntimeState>, id: i32, name_ptr: i32| -> Result<i32> {
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
                match caller.data_mut().remove_attribute(id as u32, &name) {
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
        .expect("link __remove_attribute");

    linker
        .func_wrap(
            "env",
            "__remove_child",
            |mut caller: Caller<'_, RuntimeState>, parent: i32, child: i32| -> Result<i32> {
                if parent < 0 || child < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative element id");
                    return Ok(code);
                }
                match caller.data_mut().remove_child(parent as u32, child as u32) {
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
        .expect("link __remove_child");

    linker
        .func_wrap(
            "env",
            "__replace_child",
            |mut caller: Caller<'_, RuntimeState>,
             parent: i32,
             new_child: i32,
             old_child: i32|
             -> Result<i32> {
                if parent < 0 || new_child < 0 || old_child < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative element id");
                    return Ok(code);
                }
                match caller.data_mut().replace_child(
                    parent as u32,
                    new_child as u32,
                    old_child as u32,
                ) {
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
        .expect("link __replace_child");

    linker
        .func_wrap(
            "paws",
            "paws_add_parsed_stylesheet",
            |mut caller: Caller<'_, RuntimeState>, ptr: i32, len: i32| -> Result<()> {
                let bytes = match read_byte_vec(&mut caller, ptr, len) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        let _ = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(());
                    }
                };
                caller.data_mut().clear_error();
                caller.data_mut().add_parsed_stylesheet(&bytes);
                Ok(())
            },
        )
        .expect("link paws_add_parsed_stylesheet");

    linker
}
