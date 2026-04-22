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
    /// via the `OpExecutor`. The viewport is propagated to the engine via
    /// `layoutSubviews` so Taffy lays out unstyled block elements to the
    /// view's bounds width instead of collapsing to intrinsic content size.
    public init(baseURL: String = "about:blank", frame: CGRect = .zero) {
        super.init(frame: frame)
        self.renderer = PawsRendererInstance(baseURL: baseURL, rootView: self)
        if frame.width > 0 && frame.height > 0 {
            renderer.setViewport(width: frame.width, height: frame.height)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PawsRendererView does not support Interface Builder")
    }

    /// Propagate the host bounds to the engine as the layout viewport. The
    /// engine captures the viewport once when the background thread starts,
    /// so this only matters for the first meaningful layout pass — the one
    /// that happens before `postRunWasm` triggers engine startup.
    public override func layoutSubviews() {
        super.layoutSubviews()
        let size = bounds.size
        if size.width > 0 && size.height > 0 {
            renderer.setViewport(width: size.width, height: size.height)
        }
    }
}

#endif
