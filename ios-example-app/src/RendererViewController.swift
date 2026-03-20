import UIKit

/// Main view controller that drives the Rust rendering pipeline via
/// the push model: submits a layout tree, which triggers the pipeline
/// and pushes `LayerCmd` commands back to Swift via a callback.
final class RendererViewController: UIViewController, UIScrollViewDelegate {

    private var bridge: RendererBridge!
    private var applicator: LayerApplicator!

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .white

        bridge = RendererBridge()
        applicator = LayerApplicator(rootView: view)
        applicator.scrollDelegate = self

        // Set up push-model callback.
        bridge.setRenderCallback { [weak self] cmds, count in
            guard let self else { return }
            self.applicator.apply(commands: cmds, count: count)
        }

        // Try loading a WASM app first; fall back to the built-in demo layout.
        var usedWasm = false
        if let watURL = Bundle.main.url(forResource: "demo", withExtension: "wat"),
           let watData = try? Data(contentsOf: watURL)
        {
            let status = bridge.runWasmApp(watData)
            usedWasm = (status == 0)
        }

        if !usedWasm {
            // Use the built-in demo layout (always available).
            bridge.submitDemoLayout(
                viewportWidth: Float(view.bounds.width),
                viewportHeight: Float(view.bounds.height),
                rowCount: 20
            )
            bridge.triggerRender()
        }
    }

    // MARK: - UIScrollViewDelegate

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        let offset = scrollView.contentOffset
        bridge.updateScroll(
            scrollId: UInt64(scrollView.tag),
            x: Float(offset.x),
            y: Float(offset.y)
        )
        bridge.triggerRender()
    }
}
