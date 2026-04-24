//! Creates an `<img>` element sourced from a `data:image/png;base64,…`
//! URL — the smallest example that exercises the iOS renderer's
//! `UIImageView` code path end-to-end.
//!
//! The embedded PNG is a 16×16 solid `#0A84FF` block (Paws accent
//! blue, matching `example-styled-element`). The `<img>` box is sized
//! to 120×120 via inline styles; `.scaleAspectFit` on the underlying
//! `UIImageView` scales the tiny bitmap up to fill the box.

use rust_wasm_binding::*;

/// 16×16 solid `#0A84FF` PNG, base64-encoded. Generated offline with
/// Python's zlib + a hand-authored IHDR/IDAT/IEND wrapper; the bytes
/// are embedded as a data URL so the example needs no filesystem or
/// network access to paint something visible.
const BLUE_SQUARE_DATA_URL: &str = concat!(
    "data:image/png;base64,",
    "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAGUlEQVR42mPgavn/",
    "nxLMMGrAqAGjBgwXAwAElYwfbLKyegAAAABJRU5ErkJggg==",
);

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let img_id = create_element("img")?;
            append_element(0, img_id)?;
            set_attribute(img_id, "src", BLUE_SQUARE_DATA_URL)?;
            set_attribute(img_id, "alt", "Paws accent-blue square")?;
            // Paws has no replaced-element intrinsic sizing yet, so the
            // author must size the <img> box explicitly or it stays at
            // zero-by-zero and the UIImageView has nothing to paint.
            set_inline_style(img_id, "width", "120px")?;
            set_inline_style(img_id, "height", "120px")?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
