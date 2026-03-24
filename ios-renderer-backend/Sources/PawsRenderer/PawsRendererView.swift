/// A `UIView` subclass that hosts a Paws renderer.
///
/// Provides a convenient entry point for embedding a Paws-rendered UI
/// inside an existing UIKit view hierarchy.

#if canImport(UIKit)
import UIKit

/// A UIView that owns a `PawsRendererInstance` and renders into itself.
///
/// The renderer's `OpExecutor` uses this view as the root for attaching
/// UIKit objects (UIView, CALayer) generated from the op-code buffer.
public class PawsRendererView: UIView {
    /// The renderer instance managing the DOM, style engine, and background thread.
    public private(set) var renderer: PawsRendererInstance!

    /// Creates a new `PawsRendererView` with the given base URL.
    ///
    /// The view automatically registers itself as the renderer's root view
    /// via the `OpExecutor`.
    public init(baseURL: String = "about:blank", frame: CGRect = .zero) {
        super.init(frame: frame)
        self.renderer = PawsRendererInstance(baseURL: baseURL, rootView: self)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PawsRendererView does not support Interface Builder")
    }
}

#endif
