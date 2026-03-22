/// A `UIView` subclass that hosts a Paws renderer.
///
/// Provides a convenient entry point for embedding a Paws-rendered UI
/// inside an existing UIKit view hierarchy.

#if canImport(UIKit)
import UIKit

/// A UIView that owns a `PawsRendererInstance` and renders into itself.
public class PawsRendererView: UIView {
    /// The renderer instance managing the DOM and view tree.
    public let renderer: PawsRendererInstance

    /// Creates a new `PawsRendererView` with the given base URL.
    ///
    /// The view automatically registers itself as the renderer's root view.
    public init(baseURL: String = "about:blank", frame: CGRect = .zero) {
        self.renderer = PawsRendererInstance(baseURL: baseURL)
        super.init(frame: frame)
        renderer.setRootView(self)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PawsRendererView does not support Interface Builder")
    }
}

#endif
