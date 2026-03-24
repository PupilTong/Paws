/// Errors returned by the iOS renderer backend.
///
/// Uses a distinct range (-100..) to avoid collision with [`engine::HostErrorCode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum RendererError {
    /// A null or invalid opaque handle was passed.
    InvalidHandle = -100,
    /// Engine initialization failed.
    EngineFailed = -104,
}

impl RendererError {
    pub(crate) fn as_i32(self) -> i32 {
        self as i32
    }
}

#[cfg(test)]
mod tests {
    use super::RendererError;

    #[test]
    fn test_error_codes() {
        assert_eq!(RendererError::InvalidHandle.as_i32(), -100);
        assert_eq!(RendererError::EngineFailed.as_i32(), -104);
    }
}
