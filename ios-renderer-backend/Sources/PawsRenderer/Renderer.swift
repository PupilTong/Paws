/// High-level Swift API wrapping the Paws renderer C FFI.
///
/// Usage:
/// ```swift
/// let renderer = PawsRendererInstance(baseURL: "https://example.com")
/// renderer.setRootView(myUIView)
/// let div = renderer.createElement("div")
/// renderer.appendElement(parent: 0, child: div)
/// renderer.setInlineStyle(id: div, name: "width", value: "100px")
/// // Commit is triggered by the WASM program, not Swift.
/// ```

#if canImport(UIKit)
import UIKit
import PawsRendererFFI

/// A Paws renderer instance that manages a DOM, style engine, and UIKit view tree.
public final class PawsRendererInstance {
    private let handle: OpaquePointer

    /// Creates a new renderer with the given base URL for the document.
    public init(baseURL: String = "about:blank") {
        guard let ptr = baseURL.withCString({ paws_renderer_create($0) }) else {
            fatalError("paws_renderer_create returned null")
        }
        self.handle = ptr
    }

    deinit {
        paws_renderer_destroy(handle)
    }

    /// Creates a DOM element with the given tag name.
    ///
    /// Returns the element's node ID.
    @discardableResult
    public func createElement(_ tag: String) -> UInt32 {
        let result = tag.withCString { paws_renderer_create_element(handle, $0) }
        precondition(result > 0, "createElement failed with error code \(result)")
        return UInt32(result)
    }

    /// Creates a text node with the given content.
    ///
    /// Returns the node's ID.
    @discardableResult
    public func createTextNode(_ text: String) -> UInt32 {
        let result = text.withCString { paws_renderer_create_text_node(handle, $0) }
        precondition(result > 0, "createTextNode failed with error code \(result)")
        return UInt32(result)
    }

    /// Appends a child node to a parent node.
    public func appendElement(parent: UInt32, child: UInt32) {
        let result = paws_renderer_append_element(handle, parent, child)
        precondition(result == 0, "appendElement failed with error code \(result)")
    }

    /// Sets an inline CSS property on an element.
    public func setInlineStyle(id: UInt32, name: String, value: String) {
        let result = name.withCString { namePtr in
            value.withCString { valuePtr in
                paws_renderer_set_inline_style(handle, id, namePtr, valuePtr)
            }
        }
        precondition(result == 0, "setInlineStyle failed with error code \(result)")
    }

    /// Sets a DOM attribute on an element.
    public func setAttribute(id: UInt32, name: String, value: String) {
        let result = name.withCString { namePtr in
            value.withCString { valuePtr in
                paws_renderer_set_attribute(handle, id, namePtr, valuePtr)
            }
        }
        precondition(result == 0, "setAttribute failed with error code \(result)")
    }

    /// Adds a CSS stylesheet to the document.
    public func addStylesheet(_ css: String) {
        let result = css.withCString { paws_renderer_add_stylesheet(handle, $0) }
        precondition(result == 0, "addStylesheet failed with error code \(result)")
    }

    /// Sets the root `UIView` that the renderer will paint into.
    ///
    /// Call this once before the WASM program triggers its first commit.
    /// Pass `nil` to detach the renderer from its current root view.
    public func setRootView(_ view: UIView?) {
        let viewPtr = view.map { Unmanaged.passUnretained($0).toOpaque() }
        let result = paws_renderer_set_root_view(handle, viewPtr)
        precondition(result == 0, "setRootView failed with error code \(result)")
    }

    /// Destroys an element and removes it from the DOM.
    public func destroyElement(id: UInt32) {
        let result = paws_renderer_destroy_element(handle, id)
        precondition(result == 0, "destroyElement failed with error code \(result)")
    }
}

#endif
