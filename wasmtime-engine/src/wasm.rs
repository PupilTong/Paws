#![forbid(unsafe_code)]

use wasmtime::{format_err, Caller, Engine as WasmEngine, Linker, Result};

use engine::{EngineRenderer, HostErrorCode, RuntimeState};

use crate::shared_memory_access::{with_shared_bytes, with_shared_bytes_mut};
use crate::store_data::{MainThreadToken, StoreData};

/// Runs `f` on the main-thread [`RuntimeState`]. Returns
/// [`HostErrorCode::WrongThread`]'s `i32` code if called from a worker
/// thread that never acquired a [`MainThreadToken`].
fn with_state_i32<R: EngineRenderer>(
    caller: &mut Caller<'_, StoreData<R>>,
    f: impl FnOnce(&mut RuntimeState<R>) -> i32,
) -> i32 {
    match MainThreadToken::current() {
        Some(token) => caller.data().with_state(&token, f),
        None => HostErrorCode::WrongThread as i32,
    }
}

/// Generic version of [`with_state_i32`] for host functions that return
/// non-`i32` values. Returns [`None`] if called from a worker thread; the
/// caller chooses the fallback value.
fn with_state<R: EngineRenderer, T>(
    caller: &mut Caller<'_, StoreData<R>>,
    f: impl FnOnce(&mut RuntimeState<R>) -> T,
) -> Option<T> {
    let token = MainThreadToken::current()?;
    Some(caller.data().with_state(&token, f))
}

/// Resolves the WASM memory export **once** and passes the full linear-memory
/// `&[u8]` into `f`.
///
/// Handles both regular `Memory` exports (WAT tests) and `SharedMemory`
/// exports (modules compiled with `wasm32-wasip1-threads`). The export lookup
/// (`get_export("memory")`) and memory-type dispatch happen exactly once per
/// call, regardless of how much data the callback reads.
fn with_memory_data<R: EngineRenderer, T>(
    caller: &mut Caller<'_, StoreData<R>>,
    f: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<T> {
    let export = caller
        .get_export("memory")
        .ok_or_else(|| format_err!("missing memory export"))?;

    // Try regular Memory first (WAT / non-threaded modules).
    if let Some(memory) = export.clone().into_memory() {
        let data = memory.data(&*caller);
        return f(data);
    }

    // Fall back to SharedMemory (wasm32-wasip1-threads modules).
    if let Some(shared) = export.into_shared_memory() {
        return with_shared_bytes(&shared, f);
    }

    Err(format_err!(
        "memory export is neither Memory nor SharedMemory"
    ))
}

/// Like [`with_memory_data`] but provides `&mut [u8]` access.
///
/// Handles both regular [`wasmtime::Memory`] (WAT tests) and
/// [`wasmtime::SharedMemory`] (wasm32-wasip1-threads modules). WASI shims
/// that write back into guest memory use this helper.
fn with_memory_data_mut<R: EngineRenderer, T>(
    caller: &mut Caller<'_, StoreData<R>>,
    f: impl FnOnce(&mut [u8]) -> Result<T>,
) -> Result<T> {
    let export = caller
        .get_export("memory")
        .ok_or_else(|| format_err!("missing memory export"))?;

    if let Some(memory) = export.clone().into_memory() {
        let data = memory.data_mut(&mut *caller);
        return f(data);
    }

    if let Some(shared) = export.into_shared_memory() {
        return with_shared_bytes_mut(&shared, f);
    }

    Err(format_err!(
        "memory export is neither Memory nor SharedMemory"
    ))
}

/// Reads a null-terminated C string starting at `ptr` in WASM linear memory.
///
/// The memory export is resolved once. The full `&[u8]` slice is scanned
/// in-place for the null terminator — no intermediate `Vec` allocations and
/// no chunked reads.
pub fn read_cstr<R: EngineRenderer>(
    caller: &mut Caller<'_, StoreData<R>>,
    ptr: i32,
) -> Result<String> {
    let start = ptr as usize;

    with_memory_data(caller, |data| {
        if start >= data.len() {
            return Err(format_err!("pointer out of bounds"));
        }
        match data[start..].iter().position(|&b| b == 0) {
            Some(null_pos) => std::str::from_utf8(&data[start..start + null_pos])
                .map(|s| s.to_string())
                .map_err(|_| format_err!("invalid utf-8 string")),
            None => Err(format_err!("unterminated string")),
        }
    })
}

/// Reads a contiguous `i32` array from WASM linear memory and returns the
/// values as `u32`s (after validating they are non-negative).
///
/// The memory export is resolved once; elements are read directly from the
/// backing `&[u8]` slice without an intermediate copy.
fn read_i32_slice<R: EngineRenderer>(
    caller: &mut Caller<'_, StoreData<R>>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u32>> {
    if ptr < 0 || len < 0 {
        return Err(format_err!("pointer or length out of bounds"));
    }
    let start = ptr as usize;
    let count = len as usize;
    let byte_len = count
        .checked_mul(std::mem::size_of::<i32>())
        .ok_or_else(|| format_err!("length overflow"))?;
    let end = start
        .checked_add(byte_len)
        .ok_or_else(|| format_err!("length overflow"))?;

    with_memory_data(caller, |data| {
        if end > data.len() {
            return Err(format_err!("pointer out of bounds"));
        }
        let mut values = Vec::with_capacity(count);
        for i in 0..count {
            let off = start + i * 4;
            let bytes = [data[off], data[off + 1], data[off + 2], data[off + 3]];
            let value = i32::from_le_bytes(bytes);
            if value < 0 {
                return Err(format_err!("negative child id"));
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
    caller: &mut Caller<'_, StoreData<R>>,
    ptr: i32,
    data: &[u8],
) -> Result<()> {
    let start = ptr as usize;
    let end = start
        .checked_add(data.len())
        .ok_or_else(|| format_err!("length overflow"))?;

    with_memory_data_mut(caller, |mem| {
        if end > mem.len() {
            return Err(format_err!("pointer out of bounds"));
        }
        mem[start..end].copy_from_slice(data);
        Ok(())
    })
}

/// Reads a raw byte region from WASM linear memory.
///
/// Unlike `read_cstr`, this function *does* allocate a `Vec<u8>` because the
/// caller needs owned bytes. However, the memory export is resolved only once.
fn read_byte_vec<R: EngineRenderer>(
    caller: &mut Caller<'_, StoreData<R>>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>> {
    if ptr < 0 || len < 0 {
        return Err(format_err!("pointer or length out of bounds"));
    }
    let start = ptr as usize;
    let byte_len = len as usize;
    let end = start
        .checked_add(byte_len)
        .ok_or_else(|| format_err!("length overflow"))?;

    with_memory_data(caller, |data| {
        if end > data.len() {
            return Err(format_err!("pointer out of bounds"));
        }
        Ok(data[start..end].to_vec())
    })
}

pub fn build_linker<R: EngineRenderer>(engine: &WasmEngine) -> Linker<StoreData<R>> {
    let mut linker: Linker<StoreData<R>> = Linker::new(engine);
    linker
        .func_wrap(
            "env",
            "__create_element",
            |mut caller: Caller<'_, StoreData<R>>, name_ptr: i32| -> Result<i32> {
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    s.create_element(name) as i32
                }))
            },
        )
        .expect("link __create_element");

    linker
        .func_wrap(
            "env",
            "__create_element_ns",
            |mut caller: Caller<'_, StoreData<R>>, ns_ptr: i32, tag_ptr: i32| -> Result<i32> {
                // Reads a C-string from guest memory; on failure stores a
                // MemoryError in the runtime state and early-returns with the
                // stored error code. Kept as a closure-local macro so we don't
                // repeat this 9-line boilerplate for every pointer argument.
                macro_rules! try_read_cstr {
                    ($ptr:expr) => {
                        match read_cstr(&mut caller, $ptr) {
                            Ok(value) => value,
                            Err(err) => {
                                return Ok(with_state_i32(&mut caller, |s| {
                                    s.set_error(HostErrorCode::MemoryError, err.to_string())
                                }));
                            }
                        }
                    };
                }

                let ns = try_read_cstr!(ns_ptr);
                let tag = try_read_cstr!(tag_ptr);
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    s.create_element_ns(ns, tag) as i32
                }))
            },
        )
        .expect("link __create_element_ns");

    linker
        .func_wrap(
            "env",
            "__get_namespace_uri",
            |mut caller: Caller<'_, StoreData<R>>,
             id: i32,
             buf_ptr: i32,
             buf_len: i32|
             -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                let Some(query) = with_state(&mut caller, |s| {
                    s.get_namespace_uri(id as u32)
                        .map(|opt| opt.map(|ns| ns.as_bytes().to_vec()))
                }) else {
                    return Ok(HostErrorCode::WrongThread as i32);
                };
                match query {
                    Ok(Some(bytes)) => {
                        let needed = bytes.len() as i32;
                        if buf_len >= needed {
                            if let Err(err) = write_to_memory(&mut caller, buf_ptr, &bytes) {
                                return Ok(with_state_i32(&mut caller, |s| {
                                    s.set_error(HostErrorCode::MemoryError, err.to_string())
                                }));
                            }
                        }
                        with_state(&mut caller, |s| s.clear_error());
                        Ok(needed)
                    }
                    Ok(None) => {
                        with_state(&mut caller, |s| s.clear_error());
                        Ok(-1)
                    }
                    Err(code) => Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(code, code.message())
                    })),
                }
            },
        )
        .expect("link __get_namespace_uri");

    linker
        .func_wrap(
            "env",
            "__create_text_node",
            |mut caller: Caller<'_, StoreData<R>>, text_ptr: i32| -> Result<i32> {
                let text = match read_cstr(&mut caller, text_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    s.create_text_node(text) as i32
                }))
            },
        )
        .expect("link __create_text_node");

    linker
        .func_wrap(
            "env",
            "__set_inline_style",
            |mut caller: Caller<'_, StoreData<R>>,
             id: i32,
             name_ptr: i32,
             value_ptr: i32|
             -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                let value = match read_cstr(&mut caller, value_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };

                Ok(with_state_i32(&mut caller, |s| {
                    match s.set_inline_style(id as u32, name, value) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __set_inline_style");

    linker
        .func_wrap(
            "env",
            "__destroy_element",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.destroy_element(id as u32) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __destroy_element");

    linker
        .func_wrap(
            "env",
            "__append_element",
            |mut caller: Caller<'_, StoreData<R>>, parent: i32, child: i32| -> Result<i32> {
                if parent < 0 || child < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative element id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.append_element(parent as u32, child as u32) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __append_element");

    linker
        .func_wrap(
            "env",
            "__append_elements",
            |mut caller: Caller<'_, StoreData<R>>,
             parent: i32,
             ptr: i32,
             len: i32|
             -> Result<i32> {
                // The array-like interface uses a contiguous i32 slice in WASM linear memory.
                // This minimizes host calls and allows bulk validation in a single pass,
                // which is faster than repeated per-element FFI transitions.
                if parent < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidParent, "negative parent id")
                    }));
                }
                let children = match read_i32_slice(&mut caller, ptr, len) {
                    Ok(values) => values,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    match s.append_elements(parent as u32, &children) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __append_elements");

    linker
        .func_wrap(
            "env",
            "__add_stylesheet",
            |mut caller: Caller<'_, StoreData<R>>, css_ptr: i32| -> Result<i32> {
                let css = match read_cstr(&mut caller, css_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    s.add_stylesheet(css);
                    0
                }))
            },
        )
        .expect("link __add_stylesheet");

    linker
        .func_wrap(
            "env",
            "__set_attribute",
            |mut caller: Caller<'_, StoreData<R>>,
             id: i32,
             name_ptr: i32,
             value_ptr: i32|
             -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                let value = match read_cstr(&mut caller, value_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };

                Ok(with_state_i32(&mut caller, |s| {
                    match s.set_attribute(id as u32, name, value) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __set_attribute");

    linker
        .func_wrap(
            "env",
            "__commit",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| {
                    s.commit();
                    s.clear_error();
                    0
                }))
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
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.get_first_child(id as u32) {
                        Ok(Some(child_id)) => {
                            s.clear_error();
                            child_id as i32
                        }
                        Ok(None) => {
                            s.clear_error();
                            -1
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __get_first_child");

    linker
        .func_wrap(
            "env",
            "__get_last_child",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.get_last_child(id as u32) {
                        Ok(Some(child_id)) => {
                            s.clear_error();
                            child_id as i32
                        }
                        Ok(None) => {
                            s.clear_error();
                            -1
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __get_last_child");

    linker
        .func_wrap(
            "env",
            "__get_next_sibling",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.get_next_sibling(id as u32) {
                        Ok(Some(sibling_id)) => {
                            s.clear_error();
                            sibling_id as i32
                        }
                        Ok(None) => {
                            s.clear_error();
                            -1
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __get_next_sibling");

    linker
        .func_wrap(
            "env",
            "__get_previous_sibling",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.get_previous_sibling(id as u32) {
                        Ok(Some(sibling_id)) => {
                            s.clear_error();
                            sibling_id as i32
                        }
                        Ok(None) => {
                            s.clear_error();
                            -1
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __get_previous_sibling");

    linker
        .func_wrap(
            "env",
            "__get_parent_element",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.get_parent_element(id as u32) {
                        Ok(Some(parent_id)) => {
                            s.clear_error();
                            parent_id as i32
                        }
                        Ok(None) => {
                            s.clear_error();
                            -1
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __get_parent_element");

    linker
        .func_wrap(
            "env",
            "__get_parent_node",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.get_parent_node(id as u32) {
                        Ok(Some(parent_id)) => {
                            s.clear_error();
                            parent_id as i32
                        }
                        Ok(None) => {
                            s.clear_error();
                            -1
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __get_parent_node");

    linker
        .func_wrap(
            "env",
            "__is_connected",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.is_connected(id as u32) {
                        Ok(connected) => {
                            s.clear_error();
                            if connected { 1 } else { 0 }
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __is_connected");

    linker
        .func_wrap(
            "env",
            "__has_attribute",
            |mut caller: Caller<'_, StoreData<R>>, id: i32, name_ptr: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    match s.has_attribute(id as u32, &name) {
                        Ok(has) => {
                            s.clear_error();
                            if has { 1 } else { 0 }
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __has_attribute");

    linker
        .func_wrap(
            "env",
            "__get_attribute",
            |mut caller: Caller<'_, StoreData<R>>,
             id: i32,
             name_ptr: i32,
             buf_ptr: i32,
             buf_len: i32|
             -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                let Some(query) = with_state(&mut caller, |s| {
                    s.get_attribute(id as u32, &name)
                        .map(|opt| opt.map(|v| v.as_bytes().to_vec()))
                }) else {
                    return Ok(HostErrorCode::WrongThread as i32);
                };
                match query {
                    Ok(Some(bytes)) => {
                        let needed = bytes.len() as i32;
                        if buf_len >= needed {
                            if let Err(err) = write_to_memory(&mut caller, buf_ptr, &bytes) {
                                return Ok(with_state_i32(&mut caller, |s| {
                                    s.set_error(HostErrorCode::MemoryError, err.to_string())
                                }));
                            }
                        }
                        with_state(&mut caller, |s| s.clear_error());
                        Ok(needed)
                    }
                    Ok(None) => {
                        with_state(&mut caller, |s| s.clear_error());
                        Ok(-1)
                    }
                    Err(code) => Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(code, code.message())
                    })),
                }
            },
        )
        .expect("link __get_attribute");

    linker
        .func_wrap(
            "env",
            "__remove_attribute",
            |mut caller: Caller<'_, StoreData<R>>, id: i32, name_ptr: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                let name = match read_cstr(&mut caller, name_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    match s.remove_attribute(id as u32, &name) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __remove_attribute");

    linker
        .func_wrap(
            "env",
            "__remove_child",
            |mut caller: Caller<'_, StoreData<R>>, parent: i32, child: i32| -> Result<i32> {
                if parent < 0 || child < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative element id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.remove_child(parent as u32, child as u32) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __remove_child");

    linker
        .func_wrap(
            "env",
            "__replace_child",
            |mut caller: Caller<'_, StoreData<R>>,
             parent: i32,
             new_child: i32,
             old_child: i32|
             -> Result<i32> {
                if parent < 0 || new_child < 0 || old_child < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative element id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.replace_child(
                        parent as u32,
                        new_child as u32,
                        old_child as u32,
                    ) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __replace_child");

    linker
        .func_wrap(
            "env",
            "__insert_before",
            |mut caller: Caller<'_, StoreData<R>>,
             parent: i32,
             new_child: i32,
             ref_child: i32|
             -> Result<i32> {
                if parent < 0 || new_child < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative element id")
                    }));
                }
                // ref_child == -1 means "append at end" (no reference child).
                let ref_child_opt = if ref_child == -1 {
                    None
                } else if ref_child < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative ref_child id")
                    }));
                } else {
                    Some(ref_child as u32)
                };
                Ok(with_state_i32(&mut caller, |s| {
                    match s.insert_before(
                        parent as u32,
                        new_child as u32,
                        ref_child_opt,
                    ) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __insert_before");

    linker
        .func_wrap(
            "env",
            "__clone_node",
            |mut caller: Caller<'_, StoreData<R>>, id: i32, deep: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.clone_node(id as u32, deep != 0) {
                        Ok(new_id) => {
                            s.clear_error();
                            new_id as i32
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __clone_node");

    linker
        .func_wrap(
            "env",
            "__set_node_value",
            |mut caller: Caller<'_, StoreData<R>>, id: i32, value_ptr: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                let value = match read_cstr(&mut caller, value_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    match s.set_node_value(id as u32, value) {
                        Ok(()) => {
                            s.clear_error();
                            0
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __set_node_value");

    linker
        .func_wrap(
            "env",
            "__get_node_type",
            |mut caller: Caller<'_, StoreData<R>>, id: i32| -> Result<i32> {
                if id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative node id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    match s.get_node_type(id as u32) {
                        Ok(type_code) => {
                            s.clear_error();
                            type_code as i32
                        }
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __get_node_type");

    linker
        .func_wrap(
            "paws",
            "paws_add_parsed_stylesheet",
            |mut caller: Caller<'_, StoreData<R>>, ptr: i32, len: i32| -> Result<()> {
                let bytes = match read_byte_vec(&mut caller, ptr, len) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        let _ = with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        });
                        return Ok(());
                    }
                };
                let _ = with_state(&mut caller, |s| {
                    s.clear_error();
                    s.add_parsed_stylesheet(&bytes);
                });
                Ok(())
            },
        )
        .expect("link paws_add_parsed_stylesheet");

    // ── Event system host functions ─────────────────────────────────

    linker
        .func_wrap(
            "env",
            "__add_event_listener",
            |mut caller: Caller<'_, StoreData<R>>,
             target_id: i32,
             type_ptr: i32,
             callback_id: i32,
             options_flags: i32|
             -> Result<i32> {
                if target_id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidEventTarget, "negative target id")
                    }));
                }
                let event_type = match read_cstr(&mut caller, type_ptr) {
                    Ok(v) => v,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                let options = engine::events::ListenerOptions::from_bits(options_flags as u32);
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    match s.add_event_listener(
                        target_id as u32,
                        stylo_atoms::Atom::from(event_type),
                        callback_id as u32,
                        options,
                    ) {
                        Ok(()) => 0,
                        Err(code) => {
                            s.set_error(code, code.message());
                            code.as_i32()
                        }
                    }
                }))
            },
        )
        .expect("link __add_event_listener");

    linker
        .func_wrap(
            "env",
            "__remove_event_listener",
            |mut caller: Caller<'_, StoreData<R>>,
             target_id: i32,
             type_ptr: i32,
             callback_id: i32,
             options_flags: i32|
             -> Result<i32> {
                if target_id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidEventTarget, "negative target id")
                    }));
                }
                let event_type = match read_cstr(&mut caller, type_ptr) {
                    Ok(v) => v,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                let capture = (options_flags as u32) & 0b001 != 0;
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    match s.remove_event_listener(
                        target_id as u32,
                        stylo_atoms::Atom::from(event_type),
                        callback_id as u32,
                        capture,
                    ) {
                        Ok(()) => 0,
                        Err(code) => {
                            s.set_error(code, code.message());
                            code.as_i32()
                        }
                    }
                }))
            },
        )
        .expect("link __remove_event_listener");

    linker
        .func_wrap(
            "env",
            "__dispatch_event",
            |mut caller: Caller<'_, StoreData<R>>,
             target_id: i32,
             type_ptr: i32,
             bubbles: i32,
             cancelable: i32,
             composed: i32|
             -> Result<i32> {
                if target_id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidEventTarget, "negative target id")
                    }));
                }

                let event_type = match read_cstr(&mut caller, type_ptr) {
                    Ok(v) => v,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };

                // Check for re-entrant dispatch; on success clear any error.
                let precheck = with_state_i32(&mut caller, |s| {
                    if s.current_event
                        .as_ref()
                        .is_some_and(|e| e.dispatch_flag)
                    {
                        s.set_error(
                            HostErrorCode::EventAlreadyDispatching,
                            HostErrorCode::EventAlreadyDispatching.message(),
                        )
                    } else {
                        s.clear_error();
                        0
                    }
                });
                if precheck != 0 {
                    return Ok(precheck);
                }

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
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_mut() {
                    Some(ev) => {
                        ev.stop_propagation_flag = true;
                        0
                    }
                    None => s.set_error(
                        HostErrorCode::NoActiveEvent,
                        HostErrorCode::NoActiveEvent.message(),
                    ),
                }))
            },
        )
        .expect("link __event_stop_propagation");

    linker
        .func_wrap(
            "env",
            "__event_stop_immediate_propagation",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_mut() {
                    Some(ev) => {
                        ev.stop_propagation_flag = true;
                        ev.stop_immediate_propagation_flag = true;
                        0
                    }
                    None => s.set_error(
                        HostErrorCode::NoActiveEvent,
                        HostErrorCode::NoActiveEvent.message(),
                    ),
                }))
            },
        )
        .expect("link __event_stop_immediate_propagation");

    linker
        .func_wrap(
            "env",
            "__event_prevent_default",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_mut() {
                    Some(ev) => {
                        if ev.cancelable && !ev.in_passive_listener {
                            ev.canceled_flag = true;
                        }
                        0
                    }
                    None => s.set_error(
                        HostErrorCode::NoActiveEvent,
                        HostErrorCode::NoActiveEvent.message(),
                    ),
                }))
            },
        )
        .expect("link __event_prevent_default");

    // ── Event state accessors (read-only) ───────────────────────────

    linker
        .func_wrap(
            "env",
            "__event_target",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.target.map_or(-1, |id| u64::from(id) as i32),
                    None => HostErrorCode::NoActiveEvent.as_i32(),
                }))
            },
        )
        .expect("link __event_target");

    linker
        .func_wrap(
            "env",
            "__event_current_target",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.current_target.map_or(-1, |id| u64::from(id) as i32),
                    None => HostErrorCode::NoActiveEvent.as_i32(),
                }))
            },
        )
        .expect("link __event_current_target");

    linker
        .func_wrap(
            "env",
            "__event_phase",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.event_phase as i32,
                    None => HostErrorCode::NoActiveEvent.as_i32(),
                }))
            },
        )
        .expect("link __event_phase");

    linker
        .func_wrap(
            "env",
            "__event_bubbles",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.bubbles as i32,
                    None => HostErrorCode::NoActiveEvent.as_i32(),
                }))
            },
        )
        .expect("link __event_bubbles");

    linker
        .func_wrap(
            "env",
            "__event_cancelable",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.cancelable as i32,
                    None => HostErrorCode::NoActiveEvent.as_i32(),
                }))
            },
        )
        .expect("link __event_cancelable");

    linker
        .func_wrap(
            "env",
            "__event_default_prevented",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.default_prevented() as i32,
                    None => HostErrorCode::NoActiveEvent.as_i32(),
                }))
            },
        )
        .expect("link __event_default_prevented");

    linker
        .func_wrap(
            "env",
            "__event_composed",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<i32> {
                Ok(with_state_i32(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.composed as i32,
                    None => HostErrorCode::NoActiveEvent.as_i32(),
                }))
            },
        )
        .expect("link __event_composed");

    linker
        .func_wrap(
            "env",
            "__event_timestamp",
            |mut caller: Caller<'_, StoreData<R>>| -> Result<f64> {
                Ok(with_state(&mut caller, |s| match s.current_event.as_ref() {
                    Some(ev) => ev.time_stamp,
                    None => -1.0,
                })
                .unwrap_or(-1.0))
            },
        )
        .expect("link __event_timestamp");

    // ── Shadow DOM ──────────────────────────────────────────────────

    linker
        .func_wrap(
            "env",
            "__attach_shadow",
            |mut caller: Caller<'_, StoreData<R>>, host_id: i32, mode_ptr: i32| -> Result<i32> {
                if host_id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidParent, "negative host id")
                    }));
                }
                let mode = match read_cstr(&mut caller, mode_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    match s.attach_shadow(host_id as u32, &mode) {
                        Ok(id) => id as i32,
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __attach_shadow");

    linker
        .func_wrap(
            "env",
            "__get_shadow_root",
            |mut caller: Caller<'_, StoreData<R>>, host_id: i32| -> Result<i32> {
                if host_id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative id")
                    }));
                }
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    match s.get_shadow_root(host_id as u32) {
                        Some(id) => id as i32,
                        None => -1,
                    }
                }))
            },
        )
        .expect("link __get_shadow_root");

    linker
        .func_wrap(
            "env",
            "__add_shadow_stylesheet",
            |mut caller: Caller<'_, StoreData<R>>,
             shadow_root_id: i32,
             css_ptr: i32|
             -> Result<i32> {
                if shadow_root_id < 0 {
                    return Ok(with_state_i32(&mut caller, |s| {
                        s.set_error(HostErrorCode::InvalidChild, "negative shadow root id")
                    }));
                }
                let css = match read_cstr(&mut caller, css_ptr) {
                    Ok(value) => value,
                    Err(err) => {
                        return Ok(with_state_i32(&mut caller, |s| {
                            s.set_error(HostErrorCode::MemoryError, err.to_string())
                        }));
                    }
                };
                Ok(with_state_i32(&mut caller, |s| {
                    s.clear_error();
                    match s.add_shadow_stylesheet(shadow_root_id as u32, css) {
                        Ok(()) => 0,
                        Err(code) => s.set_error(code, code.message()),
                    }
                }))
            },
        )
        .expect("link __add_shadow_stylesheet");

    // -----------------------------------------------------------------------
    // WASI preview1 stubs — minimal shims so `std`-linked WASM guests
    // (e.g. yew) can use thread_local!, HashMap, and panic output.
    // -----------------------------------------------------------------------

    // `random_get` — HashMap seed randomisation via getrandom.
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "random_get",
            |mut caller: Caller<'_, StoreData<R>>, buf_ptr: i32, buf_len: i32| -> Result<i32> {
                with_memory_data_mut(&mut caller, |data| {
                    let start = buf_ptr as usize;
                    let end = start + buf_len as usize;
                    if end <= data.len() {
                        for (i, byte) in data[start..end].iter_mut().enumerate() {
                            *byte = (i.wrapping_mul(0x9E37_79B9) >> 24) as u8;
                        }
                    }
                    Ok(0i32) // __WASI_ERRNO_SUCCESS
                })
            },
        )
        .expect("link wasi random_get");

    // `environ_sizes_get` — zero environment variables.
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_sizes_get",
            |_caller: Caller<'_, StoreData<R>>,
             _count_ptr: i32,
             _buf_size_ptr: i32|
             -> Result<i32> { Ok(0) },
        )
        .expect("link wasi environ_sizes_get");

    // `environ_get` — no-op.
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_get",
            |_caller: Caller<'_, StoreData<R>>,
             _environ: i32,
             _environ_buf: i32|
             -> Result<i32> { Ok(0) },
        )
        .expect("link wasi environ_get");

    // `clock_time_get` — monotonic nanoseconds.
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "clock_time_get",
            |mut caller: Caller<'_, StoreData<R>>,
             _clock_id: i32,
             _precision: i64,
             time_ptr: i32|
             -> Result<i32> {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;
                with_memory_data_mut(&mut caller, |data| {
                    let offset = time_ptr as usize;
                    if offset + 8 <= data.len() {
                        data[offset..offset + 8].copy_from_slice(&nanos.to_le_bytes());
                    }
                    Ok(0i32)
                })
            },
        )
        .expect("link wasi clock_time_get");

    // `fd_write` — stdout/stderr for panic messages.
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_write",
            |mut caller: Caller<'_, StoreData<R>>,
             fd: i32,
             iovs_ptr: i32,
             iovs_len: i32,
             nwritten_ptr: i32|
             -> Result<i32> {
                let mut total: u32 = 0;
                with_memory_data_mut(&mut caller, |data| {
                    for i in 0..iovs_len as usize {
                        let iov = iovs_ptr as usize + i * 8;
                        if iov + 8 > data.len() {
                            break;
                        }
                        let bp =
                            u32::from_le_bytes(data[iov..iov + 4].try_into().unwrap()) as usize;
                        let bl =
                            u32::from_le_bytes(data[iov + 4..iov + 8].try_into().unwrap()) as usize;
                        if bp + bl <= data.len() && bl > 0 {
                            let chunk = &data[bp..bp + bl];
                            match fd {
                                1 => {
                                    use std::io::Write;
                                    let _ = std::io::stdout().write_all(chunk);
                                }
                                2 => {
                                    use std::io::Write;
                                    let _ = std::io::stderr().write_all(chunk);
                                }
                                _ => {}
                            }
                            total += bl as u32;
                        }
                    }
                    let np = nwritten_ptr as usize;
                    if np + 4 <= data.len() {
                        data[np..np + 4].copy_from_slice(&total.to_le_bytes());
                    }
                    Ok(0i32)
                })
            },
        )
        .expect("link wasi fd_write");

    // `proc_exit` — terminates WASM execution.
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "proc_exit",
            |_caller: Caller<'_, StoreData<R>>, code: i32| -> Result<()> {
                Err(format_err!("WASM guest called proc_exit({code})"))
            },
        )
        .expect("link wasi proc_exit");

    // `sched_yield` — no-op (single-threaded host).
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "sched_yield",
            |_caller: Caller<'_, StoreData<R>>| -> Result<i32> { Ok(0) },
        )
        .expect("link wasi sched_yield");

    linker
}

/// Implements the three-phase W3C event dispatch algorithm using wasmtime's
/// `Caller` for re-entrant WASM calls.
///
/// This function handles the borrow juggling required by wasmtime: each
/// `Caller::data()` / `Caller::data_mut()` borrow must be released before
/// calling into WASM (which re-enters host functions).
fn dispatch_event_wasm<R: EngineRenderer>(
    caller: &mut Caller<'_, StoreData<R>>,
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
    let Some(path_opt) =
        with_state(caller, |s| build_event_path(&s.doc, target_nid))
    else {
        return Ok(HostErrorCode::WrongThread as i32);
    };
    let path = match path_opt {
        Some(p) => p,
        None => {
            return Ok(with_state_i32(caller, |s| {
                s.set_error(
                    HostErrorCode::InvalidEventTarget,
                    "target not found in tree",
                )
            }));
        }
    };

    let target_index = path.len() - 1;

    // 2. Initialize event and store in RuntimeState
    if with_state(caller, |s| {
        let mut event = Event::new(event_type.clone(), bubbles, cancelable, composed);
        event.target = Some(target_nid);
        event.dispatch_flag = true;
        s.current_event = Some(event);
    })
    .is_none()
    {
        return Ok(HostErrorCode::WrongThread as i32);
    }

    // 3. Get the WASM export for listener invocation
    let invoke_fn = caller
        .get_export("__paws_invoke_listener")
        .and_then(|e| e.into_func());

    // 4. Capture phase: path[0..target_index]
    for &node_id in &path[..target_index] {
        let stop = with_state(caller, |s| {
            s.current_event
                .as_ref()
                .map(|e| e.stop_propagation_flag)
                .unwrap_or(false)
        })
        .unwrap_or(true);
        if stop {
            break;
        }

        with_state(caller, |s| {
            let ev = s.current_event.as_mut().unwrap();
            ev.event_phase = EventPhase::Capturing;
            ev.current_target = Some(node_id);
        });

        dispatch_listeners_on_node(
            caller,
            node_id,
            &event_type,
            EventPhase::Capturing,
            invoke_fn.as_ref(),
        )?;
    }

    // 5. At-target phase
    let stop_at_target = with_state(caller, |s| {
        s.current_event
            .as_ref()
            .map(|e| e.stop_propagation_flag)
            .unwrap_or(false)
    })
    .unwrap_or(true);
    if !stop_at_target {
        with_state(caller, |s| {
            let ev = s.current_event.as_mut().unwrap();
            ev.event_phase = EventPhase::AtTarget;
            ev.current_target = Some(target_nid);
        });
        dispatch_listeners_on_node(
            caller,
            target_nid,
            &event_type,
            EventPhase::AtTarget,
            invoke_fn.as_ref(),
        )?;
    }

    // 6. Bubble phase (only if bubbles)
    if bubbles {
        for i in (0..target_index).rev() {
            let stop = with_state(caller, |s| {
                s.current_event
                    .as_ref()
                    .map(|e| e.stop_propagation_flag)
                    .unwrap_or(false)
            })
            .unwrap_or(true);
            if stop {
                break;
            }

            with_state(caller, |s| {
                let ev = s.current_event.as_mut().unwrap();
                ev.event_phase = EventPhase::Bubbling;
                ev.current_target = Some(path[i]);
            });

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
    let canceled = with_state(caller, |s| {
        let canceled = {
            let ev = s.current_event.as_mut().unwrap();
            ev.dispatch_flag = false;
            ev.event_phase = EventPhase::None;
            ev.current_target = None;
            ev.default_prevented()
        };
        s.current_event = None;

        // 8. Clean up removed listeners
        for &node_id in &path {
            if let Some(node) = s.doc.get_node_mut(node_id) {
                node.event_listeners.retain(|l| !l.removed);
            }
        }
        canceled
    })
    .unwrap_or(false);

    // Return 1 if NOT canceled, 0 if canceled (matches W3C dispatchEvent return)
    Ok(if canceled { 0 } else { 1 })
}

/// Invokes matching listeners on a single node during WASM dispatch.
fn dispatch_listeners_on_node<R: EngineRenderer>(
    caller: &mut Caller<'_, StoreData<R>>,
    node_id: taffy::NodeId,
    event_type: &stylo_atoms::Atom,
    phase: engine::events::event::EventPhase,
    invoke_fn: Option<&wasmtime::Func>,
) -> Result<()> {
    use engine::events::dispatch::collect_matching_listeners;

    // Snapshot listeners (borrow released after)
    let Some(listeners) = with_state(caller, |s| {
        collect_matching_listeners(&s.doc, node_id, event_type, phase)
    }) else {
        return Ok(());
    };

    for snap in &listeners {
        // Re-check removed flag; possibly mark once listeners for removal;
        // set passive flag — all in one lock.
        let proceed = with_state(caller, |s| {
            let active = s
                .doc
                .get_node(node_id)
                .and_then(|n| n.event_listeners.get(snap.index))
                .is_some_and(|l| !l.removed);
            if !active {
                return false;
            }
            if snap.once {
                if let Some(node) = s.doc.get_node_mut(node_id) {
                    if let Some(entry) = node.event_listeners.get_mut(snap.index) {
                        entry.removed = true;
                    }
                }
            }
            let ev = s.current_event.as_mut().unwrap();
            ev.in_passive_listener = snap.passive;
            true
        })
        .unwrap_or(false);
        if !proceed {
            continue;
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

        // Clear passive flag and check stop-immediate in one lock.
        let stop = with_state(caller, |s| {
            let ev = s.current_event.as_mut().unwrap();
            ev.in_passive_listener = false;
            ev.stop_immediate_propagation_flag
        })
        .unwrap_or(true);
        if stop {
            break;
        }
    }

    Ok(())
}
