//! `<img src>` data-URL decoding for the iOS renderer.
//!
//! The actual decoder lives in the engine crate
//! ([`engine::decode_data_url`]) so the logic is shared with any
//! other backend — the renderer exposes a thin adapter that folds
//! the engine's structured [`engine::IoError`] back to the simple
//! `Option` shape the renderer's call sites already use. Keeping
//! the crate surface stable here also means a future switch from
//! "silently drop undecodable blobs" to "report the error in
//! telemetry" stays local to this file.

/// Decodes a `data:...;base64,...` URL to its raw bytes. Returns
/// `None` if the URL is not a recognisable base64-encoded `data:`
/// URL; the renderer treats any failure as "no bitmap to draw" and
/// leaves the `UIImageView` empty.
pub(crate) fn decode_data_url(src: &str) -> Option<Vec<u8>> {
    engine::decode_data_url(src).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapts_engine_decoder_success() {
        let bytes = decode_data_url("data:image/png;base64,aGk=").unwrap();
        assert_eq!(bytes, b"hi");
    }

    #[test]
    fn adapts_engine_decoder_failure_to_none() {
        assert!(decode_data_url("https://example.com/img.png").is_none());
        assert!(decode_data_url("data:text/plain,hello").is_none());
        assert!(decode_data_url("data:image/png;base64,!!!!").is_none());
    }
}
