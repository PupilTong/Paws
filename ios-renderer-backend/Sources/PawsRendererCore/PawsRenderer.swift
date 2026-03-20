import Foundation
import CIOSRendererBackend

/// High-level Swift wrapper around the Rust renderer backend.
///
/// Uses the push model: register a callback via ``setRenderCallback(_:)``,
/// then call ``runWasmApp(_:)`` or ``triggerRender()`` to produce commands.
public final class PawsRenderer: @unchecked Sendable {

    private let handle: UInt64

    /// Prevent premature deallocation of the callback closure box.
    private var callbackBox: CallbackBox?

    /// Create a new renderer instance.
    ///
    /// - Parameter poolCapacity: Pre-allocation size for internal command
    ///   buffers. 1024 is a reasonable default.
    public init(poolCapacity: UInt32 = 1024) {
        handle = rb_create(poolCapacity)
    }

    deinit {
        rb_destroy(handle)
    }

    // MARK: - Push-model API

    /// Register a closure that receives layer commands whenever the
    /// renderer produces a new frame.
    ///
    /// The closure is called on the same thread that triggers the render
    /// (typically main). It receives an `UnsafeBufferPointer<LayerCmd>`
    /// that is valid only for the duration of the call.
    public func setRenderCallback(_ callback: @escaping @Sendable (UnsafeBufferPointer<LayerCmd>) -> Void) {
        let box_ = CallbackBox(callback)
        callbackBox = box_  // prevent deallocation
        let userdata = Unmanaged.passUnretained(box_).toOpaque()
        rb_set_render_callback(handle, trampolineFn, userdata)
    }

    /// Clear the render callback.
    public func clearRenderCallback() {
        rb_set_render_callback(handle, nil, nil)
        callbackBox = nil
    }

    /// Trigger a render frame and push the result via the registered callback.
    public func triggerRender() {
        rb_trigger_render(handle)
    }

    /// Load and execute a WASM application.
    ///
    /// The WASM module must export a `run() -> i32` function. After
    /// execution the DOM is laid out and the result is pushed to Swift
    /// via the render callback (if set).
    ///
    /// - Returns: 0 on success, or a negative error code.
    @discardableResult
    public func runWasmApp(_ bytes: Data) -> Int32 {
        bytes.withUnsafeBytes { raw in
            guard let ptr = raw.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return Int32(-1)
            }
            return rb_run_wasm_app(handle, ptr, UInt(raw.count))
        }
    }

    // MARK: - Pull-model (backward compat)

    /// Execute one frame of the rendering pipeline (pull model).
    ///
    /// Writes commands into `buffer` and returns the number written.
    public func renderFrame(
        timestamp: UInt64,
        into buffer: UnsafeMutablePointer<LayerCmd>,
        count: UnsafeMutablePointer<UInt32>
    ) {
        rb_render_frame(handle, timestamp, buffer, count)
    }

    // MARK: - Scroll

    /// Forward a scroll offset update from `UIScrollViewDelegate`.
    public func updateScrollOffset(scrollId: UInt64, x: Float, y: Float) {
        rb_update_scroll_offset(handle, scrollId, x, y)
    }

    // MARK: - Layout submission

    /// Submit a demo layout tree (scrollable colored rows).
    public func submitDemoLayout(viewportWidth: Float, viewportHeight: Float, rowCount: UInt32) {
        rb_submit_demo_layout(handle, viewportWidth, viewportHeight, rowCount)
    }
}

// MARK: - Callback trampoline

/// Box holding the Swift closure, bridged through `Unmanaged` as `user_data`.
private final class CallbackBox: @unchecked Sendable {
    let closure: @Sendable (UnsafeBufferPointer<LayerCmd>) -> Void
    init(_ closure: @escaping @Sendable (UnsafeBufferPointer<LayerCmd>) -> Void) {
        self.closure = closure
    }
}

/// C-compatible trampoline that forwards to the Swift closure.
private nonisolated(unsafe) let trampolineFn: @convention(c) (
    UnsafePointer<LayerCmd>?, UInt32, UnsafeMutableRawPointer?
) -> Void = { cmds, count, userData in
    guard let cmds, let userData else { return }
    let box_ = Unmanaged<CallbackBox>.fromOpaque(userData).takeUnretainedValue()
    let buffer = UnsafeBufferPointer(start: cmds, count: Int(count))
    box_.closure(buffer)
}
