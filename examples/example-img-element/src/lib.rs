//! Creates an `<img>` element sourced from a `data:image/png;base64,…`
//! URL — the smallest example that exercises the iOS renderer's
//! `UIImageView` code path end-to-end.
//!
//! The embedded PNG is a 128×128 Paws paw-print silhouette (four toe
//! pads + a palm pad in `#0A84FF` on a white background). A
//! recognisable shape matters here: the first cut of this example
//! used a 16×16 solid blue block, which `.scaleAspectFit` rendered
//! as a flat colour square — visually indistinguishable from a
//! `UIView` with `backgroundColor`, defeating the point of the demo.
//! A detailed source bitmap at a displayed size of 240×240 makes it
//! obvious that the bytes actually decoded into a `UIImage`.

use rust_wasm_binding::*;

/// 128×128 paw-print PNG (accent blue on white), base64-encoded.
/// Generated offline with Python's zlib + a hand-authored
/// IHDR/IDAT/IEND wrapper; the bytes are embedded as a data URL so
/// the example needs no filesystem or network access to paint.
const PAW_PRINT_DATA_URL: &str = concat!(
    "data:image/png;base64,",
    "iVBORw0KGgoAAAANSUhEUgAAAIAAAACACAYAAADDPmHLAAACPElEQVR42u3dQXIDIQwEQJ3z",
    "7vw7qbwgXhsQMD1VOWNLbdZmF1I/Ep1SAgAEAAFAABAABAABQAAQAAQAAUAAEAC2zdd39vhx",
    "AP4K/t/fzeNHA3il+DOb0D1+NIAnxZ/RhO7xowG8U/yRTegePxrAJ8Uf0YTu8aMBjCj+J03o",
    "Hh8AAAAAIBTAyOK/04Tu8QEAAAAAAAAAAADiAMwo/pMmdI9vBjADnA9ghyXYmwDMAlQrP7EA",
    "9NVzKoAZRUgGsPKyUiub3/1F7AQAqy8t1XWddi9gfT2HAlhRlCQAXe+xdi5MCoDO91k+Gdkz",
    "Xbk2Zn/Xqc7m7/ztOOXXzjEAVv8+TlnvOArAyhWylBXP4wCsWiNPuedxPIDTcxSApPvkpzb/",
    "aT3NAGYAAAAAAAAAAAAAgP3XriEIvBkEAAAA7ARg1IuW/npu/UwgBBs/E/jJi5Z96rntvgAI",
    "DtkX8ORFy3713HZvoKypp9PCwwMAAAKAACAACAACgAAgAMQkfcWybm6sZxeDAMzcZXMziNL0",
    "bAyl8dkQSuOzIZTGZ0Mojc+GUJqfjaA0PhtCaX42gtL8bASl+dkIAABA85MRlOZnIwAAAM1P",
    "RgAAAJqfjAAAAAAAAAAAAABA8wMRmAHMAAAAAAAAAAAAAAAQuBcAAAAQeB4AAAAg8EwgBJ4K",
    "BgAACOwMgsDeQAjsDgbB+QAQOCEEBGcEgeCUMBCcEwiCk0KzMdwQp4U7LTw7/l+AACAACAAC",
    "gAAgAAgAAoAAIADI5fkFDLggfCc4DdYAAAAASUVORK5CYII=",
);

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let img_id = create_element("img")?;
            append_element(0, img_id)?;
            set_attribute(img_id, "src", PAW_PRINT_DATA_URL)?;
            set_attribute(img_id, "alt", "Paws paw-print")?;
            // Paws has no replaced-element intrinsic sizing yet, so the
            // author must size the <img> box explicitly or it stays at
            // zero-by-zero and the UIImageView has nothing to paint.
            set_inline_style(img_id, "width", "240px")?;
            set_inline_style(img_id, "height", "240px")?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
