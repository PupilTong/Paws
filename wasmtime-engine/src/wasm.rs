use anyhow::{anyhow, Result};
use wasmtime::{Caller, Engine as WasmEngine, Linker};

use engine::{EngineRenderer, HostErrorCode, RuntimeState};

/// Resolves the WASM memory export **once** and passes the full linear-memory
/// `&[u8]` into `f`.
///
/// Handles both regular `Memory` exports (WAT tests) and `SharedMemory`
/// exports (modules compiled with `wasm32-wasip1-threads`). The export lookup
/// (`get_export("memory")`) and memory-type dispatch happen exactly once per
/// call, regardless of how much data the callback reads.
fn with_memory_data<R: EngineRenderer, T>(
    caller: &mut Caller<'_, RuntimeState<R>>,
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
pub fn read_cstr<R: EngineRenderer>(
    caller: &mut Caller<'_, RuntimeState<R>>,
    ptr: i32,
) -> Result<String> {
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
fn read_i32_slice<R: EngineRenderer>(
    caller: &mut Caller<'_, RuntimeState<R>>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u32>> {
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
fn write_to_memory<R: EngineRenderer>(
    caller: &mut Caller<'_, RuntimeState<R>>,
    ptr: i32,
    data: &[u8],
) -> Result<()> {
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
fn read_byte_vec<R: EngineRenderer>(
    caller: &mut Caller<'_, RuntimeState<R>>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>> {
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

pub fn build_linker<R: EngineRenderer>(engine: &WasmEngine) -> Linker<RuntimeState<R>> {
    let mut linker: Linker<RuntimeState<R>> = Linker::new(engine);
    linker
        .func_wrap(
            "env",
            "__create_element",
            |mut caller: Caller<'_, RuntimeState<R>>, name_ptr: i32| -> Result<i32> {
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
            "__create_element_ns",
            |mut caller: Caller<'_, RuntimeState<R>>, ns_ptr: i32, tag_ptr: i32| -> Result<i32> {
                let ns = match read_cstr(&mut caller, ns_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                let tag = match read_cstr(&mut caller, tag_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                caller.data_mut().clear_error();
                let id = caller.data_mut().create_element_ns(ns, tag);
                Ok(id as i32)
            },
        )
        .expect("link __create_element_ns");

    linker
        .func_wrap(
            "env",
            "__get_namespace_uri",
            |mut caller: Caller<'_, RuntimeState<R>>,
             id: i32,
             buf_ptr: i32,
             buf_len: i32|
             -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_namespace_uri(id as u32) {
                    Ok(Some(ns)) => {
                        let bytes = ns.as_bytes();
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
        .expect("link __get_namespace_uri");

    linker
        .func_wrap(
            "env",
            "__create_text_node",
            |mut caller: Caller<'_, RuntimeState<R>>, text_ptr: i32| -> Result<i32> {
                let text = match read_cstr(&mut caller, text_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                caller.data_mut().clear_error();
                let id = caller.data_mut().create_text_node(text);
                Ok(id as i32)
            },
        )
        .expect("link __create_text_node");

    linker
        .func_wrap(
            "env",
            "__set_inline_style",
            |mut caller: Caller<'_, RuntimeState<R>>,
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, parent: i32, child: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>,
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
            |mut caller: Caller<'_, RuntimeState<R>>, css_ptr: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>,
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
            |mut caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32, name_ptr: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>,
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
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32, name_ptr: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>, parent: i32, child: i32| -> Result<i32> {
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
            |mut caller: Caller<'_, RuntimeState<R>>,
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
            "env",
            "__insert_before",
            |mut caller: Caller<'_, RuntimeState<R>>,
             parent: i32,
             new_child: i32,
             ref_child: i32|
             -> Result<i32> {
                if parent < 0 || new_child < 0 || ref_child < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative element id");
                    return Ok(code);
                }
                match caller.data_mut().insert_before(
                    parent as u32,
                    new_child as u32,
                    ref_child as u32,
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
        .expect("link __insert_before");

    linker
        .func_wrap(
            "env",
            "__clone_node",
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32, deep: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data_mut().clone_node(id as u32, deep != 0) {
                    Ok(new_id) => {
                        caller.data_mut().clear_error();
                        Ok(new_id as i32)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __clone_node");

    linker
        .func_wrap(
            "env",
            "__set_node_value",
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32, value_ptr: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                let value = match read_cstr(&mut caller, value_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                match caller.data_mut().set_node_value(id as u32, value) {
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
        .expect("link __set_node_value");

    linker
        .func_wrap(
            "env",
            "__get_node_type",
            |mut caller: Caller<'_, RuntimeState<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative node id");
                    return Ok(code);
                }
                match caller.data().get_node_type(id as u32) {
                    Ok(type_code) => {
                        caller.data_mut().clear_error();
                        Ok(type_code as i32)
                    }
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __get_node_type");

    linker
        .func_wrap(
            "paws",
            "paws_add_parsed_stylesheet",
            |mut caller: Caller<'_, RuntimeState<R>>, ptr: i32, len: i32| -> Result<()> {
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

    // ── Event system host functions ─────────────────────────────────

    linker
        .func_wrap(
            "env",
            "__add_event_listener",
            |mut caller: Caller<'_, RuntimeState<R>>,
             target_id: i32,
             type_ptr: i32,
             callback_id: i32,
             options_flags: i32|
             -> Result<i32> {
                if target_id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidEventTarget, "negative target id");
                    return Ok(code);
                }
                let event_type = match read_cstr(&mut caller, type_ptr) {
                    Ok(v) => v,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                caller.data_mut().clear_error();
                let options = engine::events::ListenerOptions::from_bits(options_flags as u32);
                match caller.data_mut().add_event_listener(
                    target_id as u32,
                    stylo_atoms::Atom::from(event_type),
                    callback_id as u32,
                    options,
                ) {
                    Ok(()) => Ok(0),
                    Err(code) => {
                        caller.data_mut().set_error(code, code.message());
                        Ok(code.as_i32())
                    }
                }
            },
        )
        .expect("link __add_event_listener");

    linker
        .func_wrap(
            "env",
            "__remove_event_listener",
            |mut caller: Caller<'_, RuntimeState<R>>,
             target_id: i32,
             type_ptr: i32,
             callback_id: i32,
             options_flags: i32|
             -> Result<i32> {
                if target_id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidEventTarget, "negative target id");
                    return Ok(code);
                }
                let event_type = match read_cstr(&mut caller, type_ptr) {
                    Ok(v) => v,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                caller.data_mut().clear_error();
                let capture = (options_flags as u32) & 0b001 != 0;
                match caller.data_mut().remove_event_listener(
                    target_id as u32,
                    stylo_atoms::Atom::from(event_type),
                    callback_id as u32,
                    capture,
                ) {
                    Ok(()) => Ok(0),
                    Err(code) => {
                        caller.data_mut().set_error(code, code.message());
                        Ok(code.as_i32())
                    }
                }
            },
        )
        .expect("link __remove_event_listener");

    linker
        .func_wrap(
            "env",
            "__dispatch_event",
            |mut caller: Caller<'_, RuntimeState<R>>,
             target_id: i32,
             type_ptr: i32,
             bubbles: i32,
             cancelable: i32,
             composed: i32|
             -> Result<i32> {
                if target_id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidEventTarget, "negative target id");
                    return Ok(code);
                }

                let event_type = match read_cstr(&mut caller, type_ptr) {
                    Ok(v) => v,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };

                // Check for re-entrant dispatch
                if caller
                    .data()
                    .current_event
                    .as_ref()
                    .is_some_and(|e| e.dispatch_flag)
                {
                    let code = caller.data_mut().set_error(
                        HostErrorCode::EventAlreadyDispatching,
                        HostErrorCode::EventAlreadyDispatching.message(),
                    );
                    return Ok(code);
                }

                caller.data_mut().clear_error();

                dispatch_event_wasm(
                    &mut caller,
                    target_id as u32,
                    stylo_atoms::Atom::from(event_type),
                    bubbles != 0,
                    cancelable != 0,
                    composed != 0,
                )
            },
        )
        .expect("link __dispatch_event");

    // ── Event state accessors (mutation) ────────────────────────────

    linker
        .func_wrap(
            "env",
            "__event_stop_propagation",
            |mut caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data_mut().current_event.as_mut() {
                    Some(ev) => {
                        ev.stop_propagation_flag = true;
                        Ok(0)
                    }
                    None => {
                        let code = caller.data_mut().set_error(
                            HostErrorCode::NoActiveEvent,
                            HostErrorCode::NoActiveEvent.message(),
                        );
                        Ok(code)
                    }
                }
            },
        )
        .expect("link __event_stop_propagation");

    linker
        .func_wrap(
            "env",
            "__event_stop_immediate_propagation",
            |mut caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data_mut().current_event.as_mut() {
                    Some(ev) => {
                        ev.stop_propagation_flag = true;
                        ev.stop_immediate_propagation_flag = true;
                        Ok(0)
                    }
                    None => {
                        let code = caller.data_mut().set_error(
                            HostErrorCode::NoActiveEvent,
                            HostErrorCode::NoActiveEvent.message(),
                        );
                        Ok(code)
                    }
                }
            },
        )
        .expect("link __event_stop_immediate_propagation");

    linker
        .func_wrap(
            "env",
            "__event_prevent_default",
            |mut caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data_mut().current_event.as_mut() {
                    Some(ev) => {
                        if ev.cancelable && !ev.in_passive_listener {
                            ev.canceled_flag = true;
                        }
                        Ok(0)
                    }
                    None => {
                        let code = caller.data_mut().set_error(
                            HostErrorCode::NoActiveEvent,
                            HostErrorCode::NoActiveEvent.message(),
                        );
                        Ok(code)
                    }
                }
            },
        )
        .expect("link __event_prevent_default");

    // ── Event state accessors (read-only) ───────────────────────────

    linker
        .func_wrap(
            "env",
            "__event_target",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.target.map_or(-1, |id| u64::from(id) as i32)),
                    None => Ok(HostErrorCode::NoActiveEvent.as_i32()),
                }
            },
        )
        .expect("link __event_target");

    linker
        .func_wrap(
            "env",
            "__event_current_target",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.current_target.map_or(-1, |id| u64::from(id) as i32)),
                    None => Ok(HostErrorCode::NoActiveEvent.as_i32()),
                }
            },
        )
        .expect("link __event_current_target");

    linker
        .func_wrap(
            "env",
            "__event_phase",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.event_phase as i32),
                    None => Ok(HostErrorCode::NoActiveEvent.as_i32()),
                }
            },
        )
        .expect("link __event_phase");

    linker
        .func_wrap(
            "env",
            "__event_bubbles",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.bubbles as i32),
                    None => Ok(HostErrorCode::NoActiveEvent.as_i32()),
                }
            },
        )
        .expect("link __event_bubbles");

    linker
        .func_wrap(
            "env",
            "__event_cancelable",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.cancelable as i32),
                    None => Ok(HostErrorCode::NoActiveEvent.as_i32()),
                }
            },
        )
        .expect("link __event_cancelable");

    linker
        .func_wrap(
            "env",
            "__event_default_prevented",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.default_prevented() as i32),
                    None => Ok(HostErrorCode::NoActiveEvent.as_i32()),
                }
            },
        )
        .expect("link __event_default_prevented");

    linker
        .func_wrap(
            "env",
            "__event_composed",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<i32> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.composed as i32),
                    None => Ok(HostErrorCode::NoActiveEvent.as_i32()),
                }
            },
        )
        .expect("link __event_composed");

    linker
        .func_wrap(
            "env",
            "__event_timestamp",
            |caller: Caller<'_, RuntimeState<R>>| -> Result<f64> {
                match caller.data().current_event.as_ref() {
                    Some(ev) => Ok(ev.time_stamp),
                    None => Ok(-1.0),
                }
            },
        )
        .expect("link __event_timestamp");

    // ── Shadow DOM ──────────────────────────────────────────────────

    linker
        .func_wrap(
            "env",
            "__attach_shadow",
            |mut caller: Caller<'_, RuntimeState<R>>, host_id: i32, mode_ptr: i32| -> Result<i32> {
                if host_id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidParent, "negative host id");
                    return Ok(code);
                }
                let mode = match read_cstr(&mut caller, mode_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        let code = caller
                            .data_mut()
                            .set_error(HostErrorCode::MemoryError, err.to_string());
                        return Ok(code);
                    }
                };
                caller.data_mut().clear_error();
                match caller.data_mut().attach_shadow(host_id as u32, &mode) {
                    Ok(id) => Ok(id as i32),
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __attach_shadow");

    linker
        .func_wrap(
            "env",
            "__get_shadow_root",
            |mut caller: Caller<'_, RuntimeState<R>>, host_id: i32| -> Result<i32> {
                if host_id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative id");
                    return Ok(code);
                }
                caller.data_mut().clear_error();
                match caller.data().get_shadow_root(host_id as u32) {
                    Some(id) => Ok(id as i32),
                    None => Ok(-1),
                }
            },
        )
        .expect("link __get_shadow_root");

    linker
        .func_wrap(
            "env",
            "__add_shadow_stylesheet",
            |mut caller: Caller<'_, RuntimeState<R>>,
             shadow_root_id: i32,
             css_ptr: i32|
             -> Result<i32> {
                if shadow_root_id < 0 {
                    let code = caller
                        .data_mut()
                        .set_error(HostErrorCode::InvalidChild, "negative shadow root id");
                    return Ok(code);
                }
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
                match caller
                    .data_mut()
                    .add_shadow_stylesheet(shadow_root_id as u32, css)
                {
                    Ok(()) => Ok(0),
                    Err(code) => {
                        let err_code = caller.data_mut().set_error(code, code.message());
                        Ok(err_code)
                    }
                }
            },
        )
        .expect("link __add_shadow_stylesheet");

    linker
}

/// Implements the three-phase W3C event dispatch algorithm using wasmtime's
/// `Caller` for re-entrant WASM calls.
///
/// This function handles the borrow juggling required by wasmtime: each
/// `Caller::data()` / `Caller::data_mut()` borrow must be released before
/// calling into WASM (which re-enters host functions).
fn dispatch_event_wasm<R: EngineRenderer>(
    caller: &mut Caller<'_, RuntimeState<R>>,
    target_id: u32,
    event_type: stylo_atoms::Atom,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
) -> Result<i32> {
    use engine::events::dispatch::build_event_path;
    use engine::events::event::EventPhase;
    use engine::events::Event;

    let target_nid = taffy::NodeId::from(target_id as u64);

    // 1. Build event path (borrow doc, then release)
    let path = match build_event_path(&caller.data().doc, target_nid) {
        Some(p) => p,
        None => {
            let code = caller.data_mut().set_error(
                HostErrorCode::InvalidEventTarget,
                "target not found in tree",
            );
            return Ok(code);
        }
    };

    let target_index = path.len() - 1;

    // 2. Initialize event and store in RuntimeState
    let mut event = Event::new(event_type.clone(), bubbles, cancelable, composed);
    event.target = Some(target_nid);
    event.dispatch_flag = true;
    caller.data_mut().current_event = Some(event);

    // 3. Get the WASM export for listener invocation
    let invoke_fn = caller
        .get_export("__paws_invoke_listener")
        .and_then(|e| e.into_func());

    // 4. Capture phase: path[0..target_index]
    for &node_id in &path[..target_index] {
        {
            let ev = caller.data().current_event.as_ref().unwrap();
            if ev.stop_propagation_flag {
                break;
            }
        }

        {
            let ev = caller.data_mut().current_event.as_mut().unwrap();
            ev.event_phase = EventPhase::Capturing;
            ev.current_target = Some(node_id);
        }

        dispatch_listeners_on_node(
            caller,
            node_id,
            &event_type,
            EventPhase::Capturing,
            invoke_fn.as_ref(),
        )?;
    }

    // 5. At-target phase
    {
        let ev = caller.data().current_event.as_ref().unwrap();
        if !ev.stop_propagation_flag {
            {
                let ev = caller.data_mut().current_event.as_mut().unwrap();
                ev.event_phase = EventPhase::AtTarget;
                ev.current_target = Some(target_nid);
            }
            dispatch_listeners_on_node(
                caller,
                target_nid,
                &event_type,
                EventPhase::AtTarget,
                invoke_fn.as_ref(),
            )?;
        }
    }

    // 6. Bubble phase (only if bubbles)
    if bubbles {
        for i in (0..target_index).rev() {
            {
                let ev = caller.data().current_event.as_ref().unwrap();
                if ev.stop_propagation_flag {
                    break;
                }
            }

            {
                let ev = caller.data_mut().current_event.as_mut().unwrap();
                ev.event_phase = EventPhase::Bubbling;
                ev.current_target = Some(path[i]);
            }

            dispatch_listeners_on_node(
                caller,
                path[i],
                &event_type,
                EventPhase::Bubbling,
                invoke_fn.as_ref(),
            )?;
        }
    }

    // 7. Finalize
    let canceled = {
        let ev = caller.data_mut().current_event.as_mut().unwrap();
        ev.dispatch_flag = false;
        ev.event_phase = EventPhase::None;
        ev.current_target = None;
        ev.default_prevented()
    };
    caller.data_mut().current_event = None;

    // 8. Clean up removed listeners
    for &node_id in &path {
        if let Some(node) = caller.data_mut().doc.get_node_mut(node_id) {
            node.event_listeners.retain(|l| !l.removed);
        }
    }

    // Return 1 if NOT canceled, 0 if canceled (matches W3C dispatchEvent return)
    Ok(if canceled { 0 } else { 1 })
}

/// Invokes matching listeners on a single node during WASM dispatch.
fn dispatch_listeners_on_node<R: EngineRenderer>(
    caller: &mut Caller<'_, RuntimeState<R>>,
    node_id: taffy::NodeId,
    event_type: &stylo_atoms::Atom,
    phase: engine::events::event::EventPhase,
    invoke_fn: Option<&wasmtime::Func>,
) -> Result<()> {
    use engine::events::dispatch::collect_matching_listeners;

    // Snapshot listeners (borrow released after)
    let listeners = collect_matching_listeners(&caller.data().doc, node_id, event_type, phase);

    for snap in &listeners {
        // Re-check removed flag
        {
            let active = caller
                .data()
                .doc
                .get_node(node_id)
                .and_then(|n| n.event_listeners.get(snap.index))
                .is_some_and(|l| !l.removed);
            if !active {
                continue;
            }
        }

        // Mark once listeners for removal
        if snap.once {
            if let Some(node) = caller.data_mut().doc.get_node_mut(node_id) {
                if let Some(entry) = node.event_listeners.get_mut(snap.index) {
                    entry.removed = true;
                }
            }
        }

        // Set passive flag
        {
            let ev = caller.data_mut().current_event.as_mut().unwrap();
            ev.in_passive_listener = snap.passive;
        }

        // Invoke the WASM listener callback (if the export exists)
        if let Some(func) = invoke_fn {
            let mut results = [];
            func.call(
                &mut *caller,
                &[wasmtime::Val::I32(snap.callback_id as i32)],
                &mut results,
            )?;
        }

        // Clear passive flag
        {
            let ev = caller.data_mut().current_event.as_mut().unwrap();
            ev.in_passive_listener = false;
        }

        // Check stop immediate propagation
        {
            let ev = caller.data().current_event.as_ref().unwrap();
            if ev.stop_immediate_propagation_flag {
                break;
            }
        }
    }

    Ok(())
}
