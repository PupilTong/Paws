use stylo_atoms::Atom;

/// Decoded options for `addEventListener`.
///
/// Encoded as a bitfield for FFI: bit 0 = capture, bit 1 = passive, bit 2 = once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerOptions {
    pub capture: bool,
    pub passive: bool,
    pub once: bool,
}

impl ListenerOptions {
    /// Decodes options from an FFI bitfield.
    pub fn from_bits(bits: u32) -> Self {
        Self {
            capture: bits & 0b001 != 0,
            passive: bits & 0b010 != 0,
            once: bits & 0b100 != 0,
        }
    }
}

/// A registered event listener on a DOM node.
///
/// Per the W3C spec, listeners are identified by the triple
/// `(event_type, callback_id, capture)` for deduplication.
/// Listeners fire in registration order.
#[derive(Debug, Clone)]
pub struct EventListenerEntry {
    /// The event type this listener is registered for.
    pub event_type: Atom,

    /// Opaque identifier for the callback. In the WASM context this is
    /// an index into the guest's listener table; in tests it can be any
    /// unique u32.
    pub callback_id: u32,

    /// `true` if this listener was registered for the capture phase.
    pub capture: bool,

    /// `true` if this listener is passive (cannot call `preventDefault`).
    pub passive: bool,

    /// `true` if this listener should be removed after its first invocation.
    pub once: bool,

    /// Set to `true` when removed during an active dispatch. The dispatch
    /// loop checks this flag before invoking each listener.
    pub removed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_options_from_bits() {
        let opts = ListenerOptions::from_bits(0b000);
        assert!(!opts.capture);
        assert!(!opts.passive);
        assert!(!opts.once);

        let opts = ListenerOptions::from_bits(0b111);
        assert!(opts.capture);
        assert!(opts.passive);
        assert!(opts.once);

        let opts = ListenerOptions::from_bits(0b010);
        assert!(!opts.capture);
        assert!(opts.passive);
        assert!(!opts.once);
    }
}
