import UIKit

/// Main view controller that drives the Rust rendering pipeline via
/// `CADisplayLink` and applies the resulting `LayerCmd` stream to a
/// live `UIView` hierarchy.
final class RendererViewController: UIViewController, UIScrollViewDelegate {

    private var bridge: RendererBridge!
    private var applicator: LayerApplicator!
    private var displayLink: CADisplayLink?
    private var frameCount: UInt64 = 0

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .white

        bridge = RendererBridge()
        applicator = LayerApplicator(rootView: view)
        applicator.scrollDelegate = self

        // Initial frame — produce all creation commands.
        let (cmds, count) = bridge.tick(timestamp: 0)
        applicator.apply(commands: cmds, count: count)

        // Start the display link for subsequent frames.
        let link = CADisplayLink(target: self, selector: #selector(displayLinkFired(_:)))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        displayLink?.invalidate()
        displayLink = nil
    }

    @objc private func displayLinkFired(_ link: CADisplayLink) {
        frameCount += 1
        let timestampNs = UInt64(link.timestamp * 1_000_000_000)
        let (cmds, count) = bridge.tick(timestamp: timestampNs)
        if count > 0 {
            applicator.apply(commands: cmds, count: count)
        }
    }

    // MARK: - UIScrollViewDelegate

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        let offset = scrollView.contentOffset
        bridge.updateScroll(x: Float(offset.x), y: Float(offset.y))
    }
}
