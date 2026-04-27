/// High-level Swift API wrapping the Paws renderer C FFI.
///
/// The rendering pipeline runs on a background thread. DOM mutations
/// (createElement, setInlineStyle, etc.) send commands to the background
/// thread. Commits produce an op-code buffer that is dispatched to the
/// main thread for UIKit execution via `OpExecutor`.
///
/// Usage:
/// ```swift
/// let view = PawsRendererView(baseURL: "about:blank", frame: frame)
/// view.renderer.postRunWat(demoWat)
/// ```

#if canImport(UIKit)
import UIKit
import PawsRendererFFI

/// C completion callback — called from the Rust background thread.
///
/// `UInt` params match Rust's `usize` in `CompletionFn`; the `@convention(c)`
/// closure form is required for passing to a C function pointer argument.
private let pawsCompletionCallback: @convention(c) (
    UnsafePointer<UInt8>?,
    UInt,
    UnsafePointer<UInt8>?,
    UInt,
    UnsafeMutableRawPointer?
) -> Void = { opsPtr, opsLen, stringsPtr, stringsLen, ctx in
    guard let opsPtr = opsPtr, let ctx = ctx, opsLen > 0 else { return }

    let opsLenInt = Int(opsLen)
    let stringsLenInt = Int(stringsLen)

    let opsData = Data(bytes: opsPtr, count: opsLenInt)
    let stringsData: Data? = if let stringsPtr = stringsPtr, stringsLen > 0 {
        Data(bytes: stringsPtr, count: stringsLenInt)
    } else {
        nil
    }
    let executor = Unmanaged<OpExecutor>.fromOpaque(ctx).takeUnretainedValue()

    DispatchQueue.main.async {
        opsData.withUnsafeBytes { rawBuffer in
            guard let basePtr = rawBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return
            }
            if let stringsData = stringsData {
                stringsData.withUnsafeBytes { strBuf in
                    let strPtr = strBuf.baseAddress?.assumingMemoryBound(to: UInt8.self)
                    executor.execute(
                        ptr: basePtr, byteCount: opsLenInt,
                        stringsPtr: strPtr, stringsLen: stringsLenInt
                    )
                }
            } else {
                executor.execute(
                    ptr: basePtr, byteCount: opsLenInt,
                    stringsPtr: nil, stringsLen: 0
                )
            }
        }
    }
}

/// A Paws renderer instance that manages a DOM, style engine, and UIKit view tree.
///
/// All engine state lives on a background thread. This class holds only the
/// FFI handle (channel sender) and the `OpExecutor` reference.
public final class PawsRendererInstance {
    private let handle: OpaquePointer
    /// Retained reference to the OpExecutor to prevent deallocation.
    /// The Rust background thread holds an unretained pointer to this.
    private let executorRef: Unmanaged<OpExecutor>

    /// The `OpExecutor` that processes op-code buffers on the main thread.
    public let executor: OpExecutor

    /// Creates a new renderer with the given base URL and root view.
    ///
    /// The renderer spawns a background thread for the engine pipeline.
    /// Op-code buffers are dispatched to the main thread and executed
    /// against the given root view via `OpExecutor`.
    public init(baseURL: String = "about:blank", rootView: UIView) {
        let opExecutor = OpExecutor(rootView: rootView)
        self.executor = opExecutor

        // Retain the executor for the lifetime of this renderer.
        // The Rust background thread holds an unretained pointer to it.
        let retained = Unmanaged.passRetained(opExecutor)
        self.executorRef = retained
        let ctx = retained.toOpaque()

        guard let ptr = baseURL.withCString({ urlPtr in
            paws_renderer_create(urlPtr, pawsCompletionCallback, ctx)
        }) else {
            fatalError("paws_renderer_create returned null")
        }
        self.handle = ptr
    }

    deinit {
        // Shut down the background thread first (this blocks until the
        // thread exits, so no more callbacks will fire after this).
        paws_renderer_destroy(handle)
        // Release the retained executor reference.
        executorRef.release()
    }



    // MARK: - Async operations (non-blocking)

    /// Sets the viewport size that the engine's layout step will use for
    /// this renderer. Call this before `postRunWasm` — the viewport is
    /// captured when the engine thread starts and later calls are no-ops.
    ///
    /// Without a viewport, Taffy lays out every block element at its
    /// intrinsic content size, so unstyled `<div>`s collapse to the width
    /// of their text (often under 10 pixels). Passing the host view's
    /// bounds makes unstyled elements fill the available width, matching
    /// browser-like behaviour.
    public func setViewport(width: CGFloat, height: CGFloat) {
        let result = paws_renderer_set_viewport(handle, Float(width), Float(height))
        precondition(result == 0, "setViewport failed with error code \(result)")
    }

    /// Asynchronously compiles and runs a WASM module, then auto-commits.
    ///
    /// The `OpExecutor` will be called on the main thread when ops are ready.
    ///
    /// - Parameters:
    ///   - wasmData: WASM binary data to execute.
    ///   - functionName: The exported function to call (default: `"run"`).
    public func postRunWasm(_ wasmData: Data, functionName: String = "run") {
        let result = wasmData.withUnsafeBytes { rawBuffer -> Int32 in
            guard let basePtr = rawBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return -1
            }
            return functionName.withCString { funcPtr in
                paws_renderer_post_run_wasm(handle, basePtr, UInt(wasmData.count), funcPtr)
            }
        }
        precondition(result == 0, "postRunWasm failed with error code \(result)")
    }

    /// Convenience: compiles WAT text to WASM and runs it.
    ///
    /// Primarily for testing — production code should use `postRunWasm`.
    public func postRunWat(_ watText: String, functionName: String = "run") {
        let result = watText.withCString { watPtr in
            functionName.withCString { funcPtr in
                paws_renderer_post_run_wat(handle, watPtr, funcPtr)
            }
        }
        precondition(result == 0, "postRunWat failed with error code \(result)")
    }

    /// Posts a host-driven click at `point` (in CSS-pixel /
    /// PawsRendererView-local coordinate space) to the engine thread.
    ///
    /// The engine runs hit-test against the laid-out document, finds
    /// the deepest element under the point, and dispatches a synthetic
    /// `click` event through the W3C three-phase pipeline. Listeners
    /// registered by the guest fire on the engine thread and any DOM
    /// mutations they make are committed back through the existing op
    /// pipeline.
    ///
    /// Returns `true` if the click was queued, `false` if the engine
    /// thread is no longer accepting messages (shut down) or
    /// `postRunWasm` has not been called yet. Non-finite coordinates
    /// also return `false`.
    @discardableResult
    public func dispatchClick(at point: CGPoint) -> Bool {
        guard point.x.isFinite, point.y.isFinite else { return false }
        let result = paws_renderer_dispatch_click(handle, Float(point.x), Float(point.y))
        return result == 0
    }

}

#endif
