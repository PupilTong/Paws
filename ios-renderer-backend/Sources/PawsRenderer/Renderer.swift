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
/// Copies the op buffer and string table, then dispatches to the main
/// queue for execution via `OpExecutor`.
private func pawsCompletionCallback(
    opsPtr: UnsafePointer<UInt8>?,
    opsLen: Int,
    stringsPtr: UnsafePointer<UInt8>?,
    stringsLen: Int,
    ctx: UnsafeMutableRawPointer?
) {
    guard let opsPtr = opsPtr, let ctx = ctx, opsLen > 0 else { return }

    // Copy both buffers — they're only valid during this callback invocation.
    let opsData = Data(bytes: opsPtr, count: opsLen)
    let stringsData: Data? = if let stringsPtr = stringsPtr, stringsLen > 0 {
        Data(bytes: stringsPtr, count: stringsLen)
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
                        ptr: basePtr, byteCount: opsLen,
                        stringsPtr: strPtr, stringsLen: stringsLen
                    )
                }
            } else {
                executor.execute(
                    ptr: basePtr, byteCount: opsLen,
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
                paws_renderer_post_run_wasm(handle, basePtr, wasmData.count, funcPtr)
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

}

#endif
