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
    public init(baseURL: String = "about:blank", frame: CGRect = .zero) {
        self.renderer = PawsRendererInstance(baseURL: baseURL)
        super.init(frame: frame)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PawsRendererView does not support Interface Builder")
    }

    /// Commits pending DOM changes and updates the UIKit view tree.
    public func commitChanges() {
        renderer.commit(rootView: self)
    }
}

#endif
