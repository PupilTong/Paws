//! `<img src>` resolution for the iOS renderer.
//!
//! Supports two synchronous URL schemes today:
//!
//! - `data:*;base64,*` — decoded inline via
//!   [`engine::decode_data_url`]. Kept for backward compatibility and
//!   for guests that embed images through the legacy base64 path.
//! - `blob:paws/*` — looked up through the engine's
//!   [`ResourceResolver`](engine::ResourceResolver) view over the host-
//!   owned [`ResourceManager`](engine::ResourceManager). This is the
//!   path the `inline_image!` + `create_object_url_with_raw_data`
//!   pair produces.
//!
//! The returned [`ImageSource`] distinguishes an owned `Vec<u8>`
//! (freshly decoded from a data URL) from an `Arc<Vec<u8>>` (shared
//! with the resource manager). The caller copies the bytes into its
//! op buffer either way — the distinction is mostly about avoiding an
//! unnecessary clone when the resource manager already holds the
//! bytes.
//!
//! Any failure (unknown scheme, malformed data URL, revoked blob
//! URL) returns `None`; the renderer treats that as "no bitmap to
//! draw" and leaves the `UIImageView` empty.

use std::sync::Arc;

/// Bytes backing an `<img src>` after resolution. Owned vs shared is
/// surfaced so callers that can handle either can skip a clone in
/// the shared case.
pub(crate) enum ImageSource {
    /// Decoded inline (data URL). Caller owns the bytes.
    Owned(Vec<u8>),
    /// Shared with the resource manager (blob URL or cached network
    /// fetch). The Arc keeps the bytes alive for the duration of the
    /// render pass.
    Shared(Arc<Vec<u8>>),
}

/// Resolves an `<img src>` string to its raw bytes.
pub(crate) fn decode_image_src(
    src: &str,
    resources: &dyn engine::ResourceResolver,
) -> Option<ImageSource> {
    if src.starts_with("blob:paws/") {
        return resources.resolve(src).map(ImageSource::Shared);
    }
    if let Ok(bytes) = engine::decode_data_url(src) {
        return Some(ImageSource::Owned(bytes));
    }
    // Unknown scheme — the renderer will leave the UIImageView empty.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::ResourceManager;

    #[test]
    fn decodes_data_url_to_owned_bytes() {
        let rm = ResourceManager::new();
        let bytes = decode_image_src("data:image/png;base64,aGk=", &rm).unwrap();
        match bytes {
            ImageSource::Owned(b) => assert_eq!(b, b"hi"),
            ImageSource::Shared(_) => panic!("data URL should produce Owned bytes"),
        }
    }

    #[test]
    fn resolves_blob_url_from_registry() {
        let mut rm = ResourceManager::new();
        let url = rm.create_object_url(b"shared".to_vec(), "image/png".into());
        let bytes = decode_image_src(&url, &rm).unwrap();
        match bytes {
            ImageSource::Shared(arc) => assert_eq!(arc.as_slice(), b"shared"),
            ImageSource::Owned(_) => panic!("blob URL should produce Shared bytes"),
        }
    }

    #[test]
    fn returns_none_for_unknown_scheme() {
        let rm = ResourceManager::new();
        assert!(decode_image_src("https://example.com/img.png", &rm).is_none());
        assert!(decode_image_src("data:text/plain,hello", &rm).is_none());
        assert!(decode_image_src("data:image/png;base64,!!!!", &rm).is_none());
    }

    #[test]
    fn revoked_blob_url_returns_none() {
        let mut rm = ResourceManager::new();
        let url = rm.create_object_url(b"gone".to_vec(), "image/png".into());
        rm.revoke_object_url(&url);
        assert!(decode_image_src(&url, &rm).is_none());
    }
}
