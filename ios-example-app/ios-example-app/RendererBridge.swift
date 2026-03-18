import Foundation

/// Swift wrapper around the Rust `example_*` FFI functions.
///
/// Owns a renderer handle and a pre-allocated command buffer. Call
/// `tick(timestamp:)` each frame from a `CADisplayLink` callback to
/// receive the latest `LayerCmd` stream.
final class RendererBridge {

    /// Command buffer capacity (must match `POOL_CAPACITY` in Rust).
    private static let bufferCapacity = 1024

    private let handle: UInt64
    private var cmdBuffer: UnsafeMutablePointer<LayerCmd>
    private var cmdCount: UInt32 = 0

    init() {
        handle = example_create()
        cmdBuffer = .allocate(capacity: Self.bufferCapacity)
        cmdBuffer.initialize(repeating: LayerCmd(), count: Self.bufferCapacity)
    }

    deinit {
        cmdBuffer.deinitialize(count: Self.bufferCapacity)
        cmdBuffer.deallocate()
        example_destroy(handle)
    }

    /// Execute one pipeline frame. Returns a buffer pointer and count of
    /// commands produced.
    func tick(timestamp: UInt64) -> (UnsafePointer<LayerCmd>, Int) {
        cmdCount = 0
        example_tick(handle, timestamp, cmdBuffer, &cmdCount)
        return (UnsafePointer(cmdBuffer), Int(cmdCount))
    }

    /// Forward a scroll offset update to the Rust pipeline.
    func updateScroll(x: Float, y: Float) {
        example_update_scroll(handle, x, y)
    }
}
