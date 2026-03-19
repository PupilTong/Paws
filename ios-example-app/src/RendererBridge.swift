import UIKit

/// Swift wrapper around the Rust `rb_*` FFI functions.
///
/// Owns a renderer handle and a pre-allocated command buffer. Call
/// `tick(timestamp:)` each frame from a `CADisplayLink` callback to
/// receive the latest `LayerCmd` stream.
final class RendererBridge {

    /// Command buffer capacity.
    private static let bufferCapacity: UInt32 = 1024

    private let handle: UInt64
    private var cmdBuffer: UnsafeMutablePointer<LayerCmd>
    private var cmdCount: UInt32 = 0

    /// ScrollId for the main scroll container in the demo layout.
    static let mainScrollId: UInt64 = 2

    init(viewportSize: CGSize) {
        handle = rb_create(Self.bufferCapacity)
        cmdBuffer = .allocate(capacity: Int(Self.bufferCapacity))
        cmdBuffer.initialize(repeating: LayerCmd(), count: Int(Self.bufferCapacity))

        // Submit the built-in demo layout tree.
        rb_submit_demo_layout(
            handle,
            Float(viewportSize.width),
            Float(viewportSize.height),
            20
        )
    }

    deinit {
        cmdBuffer.deinitialize(count: Int(Self.bufferCapacity))
        cmdBuffer.deallocate()
        rb_destroy(handle)
    }

    /// Execute one pipeline frame. Returns a buffer pointer and count of
    /// commands produced.
    func tick(timestamp: UInt64) -> (UnsafePointer<LayerCmd>, Int) {
        cmdCount = 0
        rb_render_frame(handle, timestamp, cmdBuffer, &cmdCount)
        return (UnsafePointer(cmdBuffer), Int(cmdCount))
    }

    /// Forward a scroll offset update to the Rust pipeline.
    func updateScroll(scrollId: UInt64, x: Float, y: Float) {
        rb_update_scroll_offset(handle, scrollId, x, y)
    }
}
