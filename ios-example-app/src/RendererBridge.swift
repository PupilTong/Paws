import Foundation

/// Swift wrapper around the Rust `rb_*` FFI functions.
///
/// Supports both push and pull models. The push model uses a callback
/// registered via ``setRenderCallback(_:)``; the pull model uses
/// ``tick(timestamp:)``.
final class RendererBridge {

    /// Command buffer capacity.
    private static let bufferCapacity: UInt32 = 1024

    private let handle: UInt64

    // Push-model state: prevent deallocation of the closure box.
    private var callbackBox: CallbackBox?

    init() {
        handle = rb_create(Self.bufferCapacity)
    }

    deinit {
        rb_destroy(handle)
    }

    // MARK: - Push model

    /// Register a callback that receives layer commands when the renderer
    /// produces a new frame.
    func setRenderCallback(_ callback: @escaping (UnsafePointer<LayerCmd>, Int) -> Void) {
        let box_ = CallbackBox(callback)
        callbackBox = box_  // prevent deallocation
        let userdata = Unmanaged.passUnretained(box_).toOpaque()
        rb_set_render_callback(handle, renderTrampoline, userdata)
    }

    /// Trigger a render and push the result via the registered callback.
    func triggerRender() {
        rb_trigger_render(handle)
    }

    /// Load and execute a WASM application (WAT or WASM bytes).
    @discardableResult
    func runWasmApp(_ bytes: Data) -> Int32 {
        bytes.withUnsafeBytes { raw in
            guard let ptr = raw.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return Int32(-1)
            }
            return rb_run_wasm_app(handle, ptr, UInt(raw.count))
        }
    }

    /// Submit a built-in demo layout tree (mirrors demo.wat).
    func submitDemoLayout(viewportWidth: Float, viewportHeight: Float) {
        rb_submit_demo_layout(handle, viewportWidth, viewportHeight)
    }

    // MARK: - Scroll

    /// Forward a scroll offset update to the Rust pipeline.
    func updateScroll(scrollId: UInt64, x: Float, y: Float) {
        rb_update_scroll_offset(handle, scrollId, x, y)
    }
}

// MARK: - Callback trampoline

private final class CallbackBox {
    let closure: (UnsafePointer<LayerCmd>, Int) -> Void
    init(_ closure: @escaping (UnsafePointer<LayerCmd>, Int) -> Void) {
        self.closure = closure
    }
}

private let renderTrampoline: @convention(c) (
    UnsafePointer<LayerCmd>?, UInt32, UnsafeMutableRawPointer?
) -> Void = { cmds, count, userData in
    guard let cmds, let userData else { return }
    let box_ = Unmanaged<CallbackBox>.fromOpaque(userData).takeUnretainedValue()
    box_.closure(cmds, Int(count))
}
