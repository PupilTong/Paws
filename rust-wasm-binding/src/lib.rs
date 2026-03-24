//! Rust WASM binding for Paws host functions.
//!
//! Provides safe wrappers around all host-imported functions that WASM guests
//! can call to manipulate the DOM, set styles, and trigger layout.
//!
//! Targets `wasm32-wasip1-threads` (or `wasm32-wasip1`). This crate is
//! `#![no_std]` and uses a static scratch buffer for C-string passing.

#![no_std]

pub use view_macros::css;

// ---------------------------------------------------------------------------
// Panic handler for no_std WASM targets
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// ---------------------------------------------------------------------------
// Raw extern declarations (private)
// ---------------------------------------------------------------------------

#[link(wasm_import_module = "env")]
extern "C" {
    fn __CreateElement(name_ptr: *const u8) -> i32;
    fn __SetInlineStyle(id: i32, name_ptr: *const u8, value_ptr: *const u8) -> i32;
    fn __SetAttribute(id: i32, name_ptr: *const u8, value_ptr: *const u8) -> i32;
    fn __AppendElement(parent: i32, child: i32) -> i32;
    fn __AppendElements(parent: i32, ptr: *const i32, len: i32) -> i32;
    fn __DestroyElement(id: i32) -> i32;
    fn __AddStylesheet(css_ptr: *const u8) -> i32;
    fn __Commit() -> i32;
}

#[link(wasm_import_module = "paws")]
extern "C" {
    fn paws_add_parsed_stylesheet(ptr: *const u8, len: usize);
}

// ---------------------------------------------------------------------------
// Scratch buffer for C-string passing
// ---------------------------------------------------------------------------

const SCRATCH_SIZE: usize = 8192;

use core::cell::UnsafeCell;

struct ScratchBuffer {
    buf: UnsafeCell<[u8; SCRATCH_SIZE]>,
    offset: UnsafeCell<usize>,
}

// SAFETY: WASM is single-threaded; the scratch buffer is never accessed
// concurrently. This impl is required for a static, but no actual sharing
// occurs.
unsafe impl Sync for ScratchBuffer {}

static SCRATCH: ScratchBuffer = ScratchBuffer {
    buf: UnsafeCell::new([0; SCRATCH_SIZE]),
    offset: UnsafeCell::new(0),
};

/// Writes a Rust `&str` into the scratch buffer as a null-terminated C-string.
///
/// Returns a pointer into WASM linear memory that the host can read.
///
/// # Panics
///
/// Panics if the scratch buffer does not have enough space for `s.len() + 1`
/// bytes. Call [`reset_scratch`] to reclaim space.
fn write_cstr(s: &str) -> *const u8 {
    let needed = s.len() + 1; // +1 for null terminator

    // SAFETY: Single-threaded WASM execution — no concurrent access to the
    // scratch buffer. We obtain raw pointers from UnsafeCell and perform
    // bounded writes within the buffer.
    unsafe {
        let offset_ptr = SCRATCH.offset.get();
        let off = *offset_ptr;
        assert!(off + needed <= SCRATCH_SIZE, "scratch buffer overflow");
        let buf_ptr = SCRATCH.buf.get() as *mut u8;
        let dst = buf_ptr.add(off);
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, s.len());
        *dst.add(s.len()) = 0; // null terminator
        *offset_ptr = off + needed;
        dst as *const u8
    }
}

/// Resets the scratch buffer offset to zero, reclaiming all space.
///
/// Call this at the start of each frame or operation batch.
pub fn reset_scratch() {
    // SAFETY: Single-threaded WASM execution — no concurrent access.
    unsafe {
        *SCRATCH.offset.get() = 0;
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Converts a host return code to `Result`: 0 → `Ok(())`, negative → `Err(code)`.
#[inline]
fn check(code: i32) -> Result<(), i32> {
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}

// ---------------------------------------------------------------------------
// Safe public wrappers
// ---------------------------------------------------------------------------

/// Creates a new DOM element with the given tag name.
///
/// Returns the element's numeric ID on success, or a negative host error code.
pub fn create_element(name: &str) -> Result<i32, i32> {
    let ptr = write_cstr(name);
    // SAFETY: `ptr` points to a null-terminated string in WASM linear memory.
    // The host reads from this memory region during the call.
    let id = unsafe { __CreateElement(ptr) };
    if id < 0 {
        Err(id)
    } else {
        Ok(id)
    }
}

/// Sets an inline CSS property on an element.
pub fn set_inline_style(id: i32, name: &str, value: &str) -> Result<(), i32> {
    let name_ptr = write_cstr(name);
    let value_ptr = write_cstr(value);
    // SAFETY: Both pointers are null-terminated strings in WASM linear memory.
    let code = unsafe { __SetInlineStyle(id, name_ptr, value_ptr) };
    check(code)
}

/// Sets a DOM attribute on an element (e.g. `class`, `id`).
pub fn set_attribute(id: i32, name: &str, value: &str) -> Result<(), i32> {
    let name_ptr = write_cstr(name);
    let value_ptr = write_cstr(value);
    // SAFETY: Both pointers are null-terminated strings in WASM linear memory.
    let code = unsafe { __SetAttribute(id, name_ptr, value_ptr) };
    check(code)
}

/// Appends a child element to a parent element.
pub fn append_element(parent: i32, child: i32) -> Result<(), i32> {
    // SAFETY: No memory pointers involved — only integer IDs.
    let code = unsafe { __AppendElement(parent, child) };
    check(code)
}

/// Appends multiple children to a parent element in one call.
///
/// The `children` slice is passed as a contiguous i32 array in WASM linear memory.
pub fn append_elements(parent: i32, children: &[i32]) -> Result<(), i32> {
    // SAFETY: `children.as_ptr()` points to a valid i32 slice in WASM linear
    // memory. The host reads `len` i32 values starting from this pointer.
    let code = unsafe { __AppendElements(parent, children.as_ptr(), children.len() as i32) };
    check(code)
}

/// Destroys an element and all its descendants.
pub fn destroy_element(id: i32) -> Result<(), i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let code = unsafe { __DestroyElement(id) };
    check(code)
}

/// Adds a CSS stylesheet from a string (parsed at runtime by the host).
pub fn add_stylesheet(css: &str) -> Result<(), i32> {
    let ptr = write_cstr(css);
    // SAFETY: `ptr` points to a null-terminated CSS string in WASM linear memory.
    let code = unsafe { __AddStylesheet(ptr) };
    check(code)
}

/// Triggers style resolution and layout computation.
///
/// Returns `Ok(())` on success.
pub fn commit() -> Result<(), i32> {
    // SAFETY: No arguments — triggers host-side style+layout pass.
    let code = unsafe { __Commit() };
    check(code)
}

/// Applies a pre-parsed CSS stylesheet (rkyv-encoded IR bytes) to the engine.
///
/// Use with the [`css!`] macro: `apply_css(css!(r#"div { color: red; }"#))`.
pub fn apply_css(css_bytes: &[u8]) {
    // SAFETY: `css_bytes` is a valid byte slice in WASM linear memory.
    // The host reads `len` bytes starting from `ptr`.
    unsafe {
        paws_add_parsed_stylesheet(css_bytes.as_ptr(), css_bytes.len());
    }
}

// ---------------------------------------------------------------------------
// Tests (run on host, not wasm — only test the macro / IR round-trip)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use view_macros::css;

    #[test]
    fn test_css_macro_outputs_bytes() {
        let stylesheet_bytes = css!(
            r#"
            div {
                color: red;
                display: flex;
            }
            .classy {
                font-size: 16px;
            }
            "#
        );

        assert!(
            !stylesheet_bytes.is_empty(),
            "CSS macro should generate byte slice"
        );

        let ir =
            rkyv::from_bytes::<paws_style_ir::StyleSheetIR, rkyv::rancor::Error>(stylesheet_bytes)
                .unwrap();
        assert_eq!(ir.rules.len(), 2);

        match &ir.rules[0] {
            paws_style_ir::CssRuleIR::Style(s) => {
                assert_eq!(s.selectors, "div");
                assert_eq!(s.declarations.len(), 2);
                assert_eq!(
                    s.declarations[0].name,
                    paws_style_ir::CssPropertyName::Color
                );
                match &s.declarations[0].value {
                    paws_style_ir::PropertyValueIR::Raw(tokens) => match &tokens[..] {
                        [paws_style_ir::CssToken::Ident(val)] => {
                            assert_eq!(val, "red");
                        }
                        other => panic!("Expected Raw Ident token, got: {other:?}"),
                    },
                    other => panic!("Expected Raw value for color, got: {other:?}"),
                }
            }
            _ => panic!("Expected Style rule"),
        }

        match &ir.rules[1] {
            paws_style_ir::CssRuleIR::Style(s) => {
                assert_eq!(s.selectors, ".classy");
            }
            _ => panic!("Expected Style rule"),
        }
    }
}
