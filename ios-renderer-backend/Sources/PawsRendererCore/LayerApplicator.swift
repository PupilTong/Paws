#if canImport(UIKit)
import UIKit
import CIOSRendererBackend

/// Reads `LayerCmd` values from the Rust pipeline and applies them to
/// a live `UIView` hierarchy.
///
/// Maintains a dictionary mapping `LayerId` → `UIView` so that
/// incremental updates (create, update, remove, reparent, reorder)
/// are applied in O(1) per command.
@MainActor
public final class LayerApplicator {

    private var views: [UInt64: UIView] = [:]
    private let rootView: UIView
    public weak var scrollDelegate: UIScrollViewDelegate?

    public init(rootView: UIView) {
        self.rootView = rootView
    }

    /// Process a batch of commands from an `UnsafeBufferPointer`.
    public func apply(_ commands: UnsafeBufferPointer<LayerCmd>) {
        for cmd in commands {
            switch cmd.tag {
            case CreateLayer:
                handleCreate(cmd.create_layer)
            case UpdateLayer:
                handleUpdate(cmd.update_layer)
            case RemoveLayer:
                handleRemove(cmd.remove_layer)
            case AttachScroll:
                handleAttachScroll(cmd.attach_scroll)
            case SetZOrder:
                handleSetZOrder(cmd.set_z_order)
            case ReparentLayer:
                handleReparent(cmd.reparent_layer)
            default:
                break
            }
        }
    }

    /// Process a batch of commands from a raw pointer + count.
    public func apply(commands: UnsafePointer<LayerCmd>, count: Int) {
        let buffer = UnsafeBufferPointer(start: commands, count: count)
        apply(buffer)
    }

    // MARK: - Command Handlers

    private func handleCreate(_ body: CreateLayer_Body) {
        let id = body.id
        guard views[id] == nil else { return }

        let view: UIView
        switch body.kind {
        case ScrollView:
            let scrollView = UIScrollView()
            scrollView.delegate = scrollDelegate
            scrollView.tag = Int(id)
            view = scrollView
        default:
            view = UIView()
        }

        views[id] = view
        rootView.addSubview(view)
    }

    private func handleUpdate(_ body: UpdateLayer_Body) {
        guard let view = views[body.id] else { return }
        let props = body.props

        view.frame = CGRect(
            x: CGFloat(props.frame.x),
            y: CGFloat(props.frame.y),
            width: CGFloat(props.frame.width),
            height: CGFloat(props.frame.height)
        )

        view.alpha = CGFloat(props.opacity)

        view.backgroundColor = UIColor(
            red: CGFloat(props.background.r),
            green: CGFloat(props.background.g),
            blue: CGFloat(props.background.b),
            alpha: CGFloat(props.background.a)
        )

        view.layer.cornerRadius = CGFloat(props.border_radius)
        if props.border_radius > 0 {
            view.clipsToBounds = true
        }

        if props.has_transform {
            var t = CATransform3DIdentity
            let m = props.transform.m
            t.m11 = CGFloat(m.0);  t.m12 = CGFloat(m.1);  t.m13 = CGFloat(m.2);  t.m14 = CGFloat(m.3)
            t.m21 = CGFloat(m.4);  t.m22 = CGFloat(m.5);  t.m23 = CGFloat(m.6);  t.m24 = CGFloat(m.7)
            t.m31 = CGFloat(m.8);  t.m32 = CGFloat(m.9);  t.m33 = CGFloat(m.10); t.m34 = CGFloat(m.11)
            t.m41 = CGFloat(m.12); t.m42 = CGFloat(m.13); t.m43 = CGFloat(m.14); t.m44 = CGFloat(m.15)
            view.layer.transform = t
        } else {
            view.layer.transform = CATransform3DIdentity
        }

        if props.has_clip {
            view.clipsToBounds = true
        }
    }

    private func handleRemove(_ body: RemoveLayer_Body) {
        guard let view = views.removeValue(forKey: body.id) else { return }
        view.removeFromSuperview()
    }

    private func handleAttachScroll(_ body: AttachScroll_Body) {
        guard let scrollView = views[body.id] as? UIScrollView else { return }
        scrollView.contentSize = CGSize(
            width: CGFloat(body.content_size.width),
            height: CGFloat(body.content_size.height)
        )
    }

    private func handleSetZOrder(_ body: SetZOrder_Body) {
        guard let view = views[body.id] else { return }
        view.layer.zPosition = CGFloat(body.index)
    }

    private func handleReparent(_ body: ReparentLayer_Body) {
        guard let view = views[body.id] else { return }
        let newParent = views[body.new_parent] ?? rootView
        if view.superview !== newParent {
            view.removeFromSuperview()
            newParent.addSubview(view)
        }
    }
}
#endif
