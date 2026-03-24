/// Decodes and executes the 32-byte op-code buffer produced by the Rust
/// background thread.
///
/// `OpExecutor` owns the mapping from engine node IDs to UIKit objects
/// (UIView, UIScrollView, CALayer). It is the sole mutator of the UIKit
/// view hierarchy during rendering — all mutations flow through the op buffer.
///
/// **Thread safety:** All methods must be called on the main thread.

#if canImport(UIKit)
import UIKit

// MARK: - Op-code tags (must match Rust OpTag in ops.rs)

private let OP_DECLARE_VIEW:       UInt8 = 0x01
private let OP_DECLARE_SCROLLVIEW: UInt8 = 0x02
private let OP_DECLARE_LAYER:      UInt8 = 0x03
private let OP_SET_VIEW_FRAME:     UInt8 = 0x04
private let OP_SET_LAYER_FRAME:    UInt8 = 0x05
private let OP_SET_BG_COLOR:       UInt8 = 0x06
private let OP_SET_CLIPS:          UInt8 = 0x07
private let OP_SET_CONTENT_SIZE:   UInt8 = 0x08
private let OP_DETACH_VIEW:        UInt8 = 0x09
private let OP_DETACH_LAYER:       UInt8 = 0x0A
private let OP_RELEASE_VIEW:       UInt8 = 0x0B
private let OP_RELEASE_SCROLLVIEW: UInt8 = 0x0C
private let OP_RELEASE_LAYER:      UInt8 = 0x0D
private let OP_ATTACH:             UInt8 = 0x0E

// ViewKind values (must match Rust ViewKind repr(u8))
private let KIND_VIEW:       UInt8 = 0
private let KIND_SCROLLVIEW: UInt8 = 1
private let KIND_LAYER:      UInt8 = 2

/// Slot size in bytes — must match Rust SLOT_SIZE.
private let SLOT_SIZE = 32

/// Sentinel parent ID meaning "attach to rootView".
private let ROOT_SENTINEL: UInt64 = UInt64.max

public final class OpExecutor {
    /// Maps engine node IDs to UIKit objects.
    private var viewMap: [UInt64: Entry] = [:]
    /// The root UIView to render into.
    private let rootView: UIView

    /// Optional callback invoked after each `execute()` completes.
    /// Used by tests to synchronize on op execution instead of sleeping.
    public var onExecute: (() -> Void)?

    private struct Entry {
        let obj: AnyObject    // UIView, UIScrollView, or CALayer
        let kind: UInt8       // OP_DECLARE_VIEW / SCROLLVIEW / LAYER tag
    }

    public init(rootView: UIView) {
        self.rootView = rootView
    }

    /// Executes a buffer of 32-byte op-code slots.
    ///
    /// Called on the main thread after the background thread produces ops.
    public func execute(ptr: UnsafePointer<UInt8>, byteCount: Int) {
        let opCount = byteCount / SLOT_SIZE

        for i in 0..<opCount {
            let base = ptr + i * SLOT_SIZE
            let tag = base[0]

            switch tag {
            case OP_DECLARE_VIEW, OP_DECLARE_SCROLLVIEW, OP_DECLARE_LAYER:
                let nodeId = readU64(base + 1)
                let parentId = readU64(base + 9)
                handleDeclare(nodeId: nodeId, parentId: parentId, tag: tag)

            case OP_SET_VIEW_FRAME, OP_SET_LAYER_FRAME:
                let nodeId = readU64(base + 1)
                let x = readF32(base + 9)
                let y = readF32(base + 13)
                let w = readF32(base + 17)
                let h = readF32(base + 21)
                let frame = CGRect(
                    x: CGFloat(x), y: CGFloat(y),
                    width: CGFloat(w), height: CGFloat(h)
                )
                if tag == OP_SET_VIEW_FRAME {
                    (viewMap[nodeId]?.obj as? UIView)?.frame = frame
                } else {
                    (viewMap[nodeId]?.obj as? CALayer)?.frame = frame
                }

            case OP_SET_BG_COLOR:
                let nodeId = readU64(base + 1)
                let r = readF32(base + 9)
                let g = readF32(base + 13)
                let b = readF32(base + 17)
                let a = readF32(base + 21)
                let color = UIColor(
                    red: CGFloat(r), green: CGFloat(g),
                    blue: CGFloat(b), alpha: CGFloat(a)
                )
                if let entry = viewMap[nodeId] {
                    if let view = entry.obj as? UIView {
                        view.backgroundColor = color
                    } else if let layer = entry.obj as? CALayer {
                        layer.backgroundColor = color.cgColor
                    }
                }

            case OP_SET_CLIPS:
                let nodeId = readU64(base + 1)
                let clips = base[9] != 0
                (viewMap[nodeId]?.obj as? UIView)?.clipsToBounds = clips

            case OP_SET_CONTENT_SIZE:
                let nodeId = readU64(base + 1)
                let w = readF32(base + 9)
                let h = readF32(base + 13)
                (viewMap[nodeId]?.obj as? UIScrollView)?.contentSize = CGSize(
                    width: CGFloat(w), height: CGFloat(h)
                )

            case OP_DETACH_VIEW:
                let nodeId = readU64(base + 1)
                (viewMap[nodeId]?.obj as? UIView)?.removeFromSuperview()

            case OP_DETACH_LAYER:
                let nodeId = readU64(base + 1)
                (viewMap[nodeId]?.obj as? CALayer)?.removeFromSuperlayer()

            case OP_RELEASE_VIEW:
                let nodeId = readU64(base + 1)
                viewMap.removeValue(forKey: nodeId)

            case OP_RELEASE_SCROLLVIEW:
                let nodeId = readU64(base + 1)
                viewMap.removeValue(forKey: nodeId)

            case OP_RELEASE_LAYER:
                let nodeId = readU64(base + 1)
                viewMap.removeValue(forKey: nodeId)

            case OP_ATTACH:
                let nodeId = readU64(base + 1)
                let parentId = readU64(base + 9)
                let childKind = base[17]
                let parentKind = base[18]
                attachToParent(
                    nodeId: nodeId, childKind: childKind,
                    parentId: parentId, parentKind: parentKind
                )

            default:
                break // Unknown op — skip
            }
        }

        onExecute?()
    }

    // MARK: - Private helpers

    private func handleDeclare(nodeId: UInt64, parentId: UInt64, tag: UInt8) {
        if let existing = viewMap[nodeId], existing.kind == tag {
            // Same kind — already exists, nothing to create.
            return
        }

        // Different kind or new node — create fresh.
        if viewMap[nodeId] != nil {
            // Kind changed — old object was already detached+released by
            // preceding Detach+Release ops, but remove from map just in case.
            viewMap.removeValue(forKey: nodeId)
        }

        let obj: AnyObject
        switch tag {
        case OP_DECLARE_VIEW:
            obj = UIView()
        case OP_DECLARE_SCROLLVIEW:
            obj = UIScrollView()
        case OP_DECLARE_LAYER:
            obj = CALayer()
        default:
            return
        }

        viewMap[nodeId] = Entry(obj: obj, kind: tag)
    }

    private func attachToParent(
        nodeId: UInt64, childKind: UInt8,
        parentId: UInt64, parentKind: UInt8
    ) {
        guard let childEntry = viewMap[nodeId] else { return }

        // Resolve parent — ROOT_SENTINEL means rootView.
        let parentObj: AnyObject
        let effectiveParentKind: UInt8
        if parentId == ROOT_SENTINEL {
            parentObj = rootView
            effectiveParentKind = KIND_VIEW
        } else {
            guard let parentEntry = viewMap[parentId] else { return }
            parentObj = parentEntry.obj
            effectiveParentKind = parentKind
        }

        switch (effectiveParentKind, childKind) {
        case (KIND_VIEW, KIND_VIEW), (KIND_VIEW, KIND_SCROLLVIEW),
             (KIND_SCROLLVIEW, KIND_VIEW), (KIND_SCROLLVIEW, KIND_SCROLLVIEW):
            // View/ScrollView parent + View/ScrollView child → addSubview
            if let parent = parentObj as? UIView, let child = childEntry.obj as? UIView {
                parent.addSubview(child)
            }

        case (KIND_VIEW, KIND_LAYER), (KIND_SCROLLVIEW, KIND_LAYER):
            // View/ScrollView parent + Layer child → view.layer.addSublayer
            if let parent = parentObj as? UIView, let child = childEntry.obj as? CALayer {
                parent.layer.addSublayer(child)
            }

        case (KIND_LAYER, KIND_LAYER):
            // Layer parent + Layer child → addSublayer
            if let parent = parentObj as? CALayer, let child = childEntry.obj as? CALayer {
                parent.addSublayer(child)
            }

        case (KIND_LAYER, KIND_VIEW), (KIND_LAYER, KIND_SCROLLVIEW):
            // Layer parent + View child — edge case, treat as layer-to-layer fallback
            if let parent = parentObj as? CALayer, let child = childEntry.obj as? UIView {
                parent.addSublayer(child.layer)
            }

        default:
            break
        }
    }
}

// MARK: - Binary helpers

private func readU64(_ ptr: UnsafePointer<UInt8>) -> UInt64 {
    var value: UInt64 = 0
    withUnsafeMutableBytes(of: &value) { buf in
        buf.copyBytes(from: UnsafeBufferPointer(start: ptr, count: 8))
    }
    return UInt64(littleEndian: value)
}

private func readF32(_ ptr: UnsafePointer<UInt8>) -> Float {
    var bits: UInt32 = 0
    withUnsafeMutableBytes(of: &bits) { buf in
        buf.copyBytes(from: UnsafeBufferPointer(start: ptr, count: 4))
    }
    return Float(bitPattern: UInt32(littleEndian: bits))
}

#endif
