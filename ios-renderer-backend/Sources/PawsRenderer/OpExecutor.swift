/// Decodes and executes the 32-byte op-code buffer produced by the Rust
/// background thread.
///
/// `OpExecutor` owns the mapping from engine node IDs to UIKit objects
/// (UIView, UIScrollView, CALayer, CATextLayer). It is the sole mutator
/// of the UIKit view hierarchy during rendering — all mutations flow
/// through the op buffer.
///
/// **Thread safety:** All methods must be called on the main thread.

#if canImport(UIKit)
import UIKit
import QuartzCore

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

// Text ops
private let OP_DECLARE_TEXT:       UInt8 = 0x10
private let OP_SET_TEXT_CONTENT:   UInt8 = 0x11
private let OP_SET_TEXT_FONT:      UInt8 = 0x12
private let OP_SET_TEXT_COLOR:     UInt8 = 0x13
private let OP_DETACH_TEXT:        UInt8 = 0x14
private let OP_RELEASE_TEXT:       UInt8 = 0x15

// Image ops
private let OP_DECLARE_IMAGE:      UInt8 = 0x20
private let OP_SET_IMAGE_DATA:     UInt8 = 0x21

// ViewKind values (must match Rust ViewKind repr(u8))
private let KIND_VIEW:       UInt8 = 0
private let KIND_SCROLLVIEW: UInt8 = 1
private let KIND_LAYER:      UInt8 = 2
private let KIND_TEXT:       UInt8 = 3
private let KIND_IMAGE:      UInt8 = 4

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
        let obj: AnyObject    // UIView, UIScrollView, CALayer, or CATextLayer
        let kind: UInt8       // OP_DECLARE_* tag
    }

    public init(rootView: UIView) {
        self.rootView = rootView
    }

    /// Executes a buffer of 32-byte op-code slots with an optional string table.
    ///
    /// Called on the main thread after the background thread produces ops.
    public func execute(
        ptr: UnsafePointer<UInt8>, byteCount: Int,
        stringsPtr: UnsafePointer<UInt8>?, stringsLen: Int
    ) {
        let opCount = byteCount / SLOT_SIZE

        for i in 0..<opCount {
            let base = ptr + i * SLOT_SIZE
            let tag = base[0]

            switch tag {
            case OP_DECLARE_VIEW, OP_DECLARE_SCROLLVIEW, OP_DECLARE_LAYER:
                let nodeId = readU64(base + 1)
                let parentId = readU64(base + 9)
                handleDeclare(nodeId: nodeId, parentId: parentId, tag: tag)

            case OP_DECLARE_TEXT:
                let nodeId = readU64(base + 1)
                let parentId = readU64(base + 9)
                handleDeclareText(nodeId: nodeId, parentId: parentId)

            case OP_DECLARE_IMAGE:
                let nodeId = readU64(base + 1)
                let parentId = readU64(base + 9)
                handleDeclareImage(nodeId: nodeId, parentId: parentId)

            case OP_SET_IMAGE_DATA:
                let nodeId = readU64(base + 1)
                let offset = readU32(base + 9)
                let len = readU32(base + 13)
                if let sPtr = stringsPtr, Int(offset) + Int(len) <= stringsLen {
                    let imageData = Data(bytes: sPtr + Int(offset), count: Int(len))
                    if let image = UIImage(data: imageData),
                       let imageView = viewMap[nodeId]?.obj as? UIImageView {
                        imageView.image = image
                    }
                    // Silent skip on decode failure — the UIImageView
                    // stays empty, matching the "no image" case. The
                    // engine side emitted the op because the bytes were
                    // validated earlier; a failure here means the blob
                    // was not a format UIKit understands.
                }

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

            case OP_SET_TEXT_CONTENT:
                let nodeId = readU64(base + 1)
                let offset = readU32(base + 9)
                let len = readU32(base + 13)
                if let sPtr = stringsPtr, Int(offset) + Int(len) <= stringsLen {
                    let textData = Data(bytes: sPtr + Int(offset), count: Int(len))
                    if let text = String(data: textData, encoding: .utf8) {
                        (viewMap[nodeId]?.obj as? CATextLayer)?.string = text
                    }
                }

            case OP_SET_TEXT_FONT:
                let nodeId = readU64(base + 1)
                let fontSize = readF32(base + 9)
                let fontWeight = readF32(base + 13)
                if let textLayer = viewMap[nodeId]?.obj as? CATextLayer {
                    textLayer.fontSize = CGFloat(fontSize)
                    let uiWeight = cssWeightToUIFontWeight(fontWeight)
                    textLayer.font = UIFont.systemFont(ofSize: CGFloat(fontSize), weight: uiWeight)
                }

            case OP_SET_TEXT_COLOR:
                let nodeId = readU64(base + 1)
                let r = readF32(base + 9)
                let g = readF32(base + 13)
                let b = readF32(base + 17)
                let a = readF32(base + 21)
                let color = UIColor(
                    red: CGFloat(r), green: CGFloat(g),
                    blue: CGFloat(b), alpha: CGFloat(a)
                )
                (viewMap[nodeId]?.obj as? CATextLayer)?.foregroundColor = color.cgColor

            case OP_DETACH_VIEW:
                let nodeId = readU64(base + 1)
                (viewMap[nodeId]?.obj as? UIView)?.removeFromSuperview()

            case OP_DETACH_LAYER, OP_DETACH_TEXT:
                let nodeId = readU64(base + 1)
                (viewMap[nodeId]?.obj as? CALayer)?.removeFromSuperlayer()

            case OP_RELEASE_VIEW, OP_RELEASE_SCROLLVIEW, OP_RELEASE_LAYER, OP_RELEASE_TEXT:
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

    private func handleDeclareText(nodeId: UInt64, parentId: UInt64) {
        if let existing = viewMap[nodeId], existing.kind == OP_DECLARE_TEXT {
            return
        }

        if viewMap[nodeId] != nil {
            viewMap.removeValue(forKey: nodeId)
        }

        let textLayer = CATextLayer()
        textLayer.isWrapped = true
        textLayer.truncationMode = .end
        textLayer.contentsScale = UIScreen.main.scale
        // Default to black text
        textLayer.foregroundColor = UIColor.black.cgColor

        viewMap[nodeId] = Entry(obj: textLayer, kind: OP_DECLARE_TEXT)
    }

    private func handleDeclareImage(nodeId: UInt64, parentId: UInt64) {
        if let existing = viewMap[nodeId], existing.kind == OP_DECLARE_IMAGE {
            return
        }

        if viewMap[nodeId] != nil {
            viewMap.removeValue(forKey: nodeId)
        }

        let imageView = UIImageView()
        // `.scaleAspectFit` matches the browser's "contain the image
        // inside the content box without distorting it" default — the
        // closest single contentMode to CSS `object-fit: contain`. We
        // don't plumb `object-fit` yet; authors can resize the `<img>`
        // box itself via CSS `width`/`height` to control the fit.
        imageView.contentMode = .scaleAspectFit
        imageView.clipsToBounds = true

        viewMap[nodeId] = Entry(obj: imageView, kind: OP_DECLARE_IMAGE)
    }

    private func attachToParent(
        nodeId: UInt64, childKind: UInt8,
        parentId: UInt64, parentKind: UInt8
    ) {
        guard let childEntry = viewMap[nodeId] else { return }

        // Resolve parent — ROOT_SENTINEL means rootView.
        let parentObj: AnyObject
        var effectiveParentKind: UInt8
        if parentId == ROOT_SENTINEL {
            parentObj = rootView
            effectiveParentKind = KIND_VIEW
        } else {
            guard let parentEntry = viewMap[parentId] else { return }
            parentObj = parentEntry.obj
            effectiveParentKind = parentKind
        }

        // UIImageView is a UIView — collapse it onto KIND_VIEW so the
        // existing (parent, child) dispatch table doesn't need to list
        // every KIND_IMAGE combination separately. `addSubview` and
        // `view.layer` both work transparently on UIImageView.
        var effectiveChildKind = childKind
        if effectiveChildKind == KIND_IMAGE { effectiveChildKind = KIND_VIEW }
        if effectiveParentKind == KIND_IMAGE { effectiveParentKind = KIND_VIEW }

        switch (effectiveParentKind, effectiveChildKind) {
        case (KIND_VIEW, KIND_VIEW), (KIND_VIEW, KIND_SCROLLVIEW),
             (KIND_SCROLLVIEW, KIND_VIEW), (KIND_SCROLLVIEW, KIND_SCROLLVIEW):
            if let parent = parentObj as? UIView, let child = childEntry.obj as? UIView {
                parent.addSubview(child)
            }

        case (KIND_VIEW, KIND_LAYER), (KIND_SCROLLVIEW, KIND_LAYER),
             (KIND_VIEW, KIND_TEXT), (KIND_SCROLLVIEW, KIND_TEXT):
            // Layer/TextLayer child → view.layer.addSublayer
            if let parent = parentObj as? UIView, let child = childEntry.obj as? CALayer {
                parent.layer.addSublayer(child)
            }

        case (KIND_LAYER, KIND_LAYER), (KIND_LAYER, KIND_TEXT):
            if let parent = parentObj as? CALayer, let child = childEntry.obj as? CALayer {
                parent.addSublayer(child)
            }

        case (KIND_LAYER, KIND_VIEW), (KIND_LAYER, KIND_SCROLLVIEW):
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

private func readU32(_ ptr: UnsafePointer<UInt8>) -> UInt32 {
    var value: UInt32 = 0
    withUnsafeMutableBytes(of: &value) { buf in
        buf.copyBytes(from: UnsafeBufferPointer(start: ptr, count: 4))
    }
    return UInt32(littleEndian: value)
}

private func readF32(_ ptr: UnsafePointer<UInt8>) -> Float {
    var bits: UInt32 = 0
    withUnsafeMutableBytes(of: &bits) { buf in
        buf.copyBytes(from: UnsafeBufferPointer(start: ptr, count: 4))
    }
    return Float(bitPattern: UInt32(littleEndian: bits))
}

/// Maps CSS font-weight (100–900) to UIFont.Weight.
private func cssWeightToUIFontWeight(_ cssWeight: Float) -> UIFont.Weight {
    switch cssWeight {
    case ..<150:  return .ultraLight
    case ..<250:  return .thin
    case ..<350:  return .light
    case ..<450:  return .regular
    case ..<550:  return .medium
    case ..<650:  return .semibold
    case ..<750:  return .bold
    case ..<850:  return .heavy
    default:      return .black
    }
}

#endif
