/// Errors returned by the iOS renderer backend.
///
/// Uses a distinct range (-100..) to avoid collision with [`engine::HostErrorCode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum RendererError {
    /// A null or invalid opaque handle was passed.
    InvalidHandle = -100,
    /// The operation was called from a non-main thread.
    ThreadViolation = -101,
    /// A Swift callback returned an error or null unexpectedly.
    CallbackFailed = -102,
    /// The LayoutBox tree was empty or malformed.
    InvalidLayout = -103,
    /// Engine initialization failed.
    EngineFailed = -104,
}

impl RendererError {
    pub(crate) fn as_i32(self) -> i32 {
        self as i32
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::InvalidHandle => "null or invalid opaque handle",
            Self::ThreadViolation => "operation called from non-main thread",
            Self::CallbackFailed => "Swift callback returned error or null",
            Self::InvalidLayout => "LayoutBox tree was empty or malformed",
            Self::EngineFailed => "engine initialization failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RendererError;

    #[test]
    fn test_error_codes() {
        assert_eq!(RendererError::InvalidHandle.as_i32(), -100);
        assert_eq!(RendererError::ThreadViolation.as_i32(), -101);
        assert_eq!(RendererError::CallbackFailed.as_i32(), -102);
        assert_eq!(RendererError::InvalidLayout.as_i32(), -103);
        assert_eq!(RendererError::EngineFailed.as_i32(), -104);
    }

    #[test]
    fn test_error_messages_non_empty() {
        let variants = [
            RendererError::InvalidHandle,
            RendererError::ThreadViolation,
            RendererError::CallbackFailed,
            RendererError::InvalidLayout,
            RendererError::EngineFailed,
        ];
        for variant in variants {
            assert!(
                !variant.message().is_empty(),
                "{variant:?} should have a non-empty message"
            );
        }
    }
}
