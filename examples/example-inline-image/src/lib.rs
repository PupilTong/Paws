//! Demonstrates `inline_image!` + `create_object_url_with_raw_data`:
//! reads a PNG at compile time (no base64 bloat), registers it with
//! the host as a blob URL at runtime, and places that URL in an
//! `<img src>` attribute. The iOS renderer decodes the bytes through
//! the engine's `ResourceResolver` path and paints them into a
//! `UIImageView`.
//!
//! Contrast with `example-img-element`, which embeds the same image
//! as a multi-kilobyte base64 string literal. The raw-bytes +
//! object-URL path here saves ~33% binary size and skips runtime
//! base64 decoding entirely.

use rust_wasm_binding::*;

const LOGO: (&[u8], &str) = inline_image!("assets/paws-logo.png");

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            // Register the inlined bytes with the host. `url` is a
            // `blob:paws/<hex>` URL that stays valid for the
            // lifetime of the engine (or until revoked).
            let url = create_object_url_with_raw_data(LOGO.0, LOGO.1);

            let img = create_element("img")?;
            append_element(0, img)?;
            set_attribute(img, "src", &url)?;
            set_attribute(img, "alt", "Paws logo")?;
            // Paws has no replaced-element intrinsic sizing yet, so the
            // author must size the <img> box explicitly or it stays
            // zero-by-zero and nothing paints.
            set_inline_style(img, "width", "240px")?;
            set_inline_style(img, "height", "240px")?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
