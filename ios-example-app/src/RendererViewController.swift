import UIKit

/// Main view controller that drives the Rust rendering pipeline via
/// the push model: loads `demo.wat`, executes it through the full
/// WASM → DOM → Style → Layout → Renderer → UIKit pipeline.
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

        // Load and execute demo.wat through the real WASM pipeline.
        guard let watURL = Bundle.main.url(forResource: "demo", withExtension: "wat"),
              let watData = try? Data(contentsOf: watURL)
        else {
            fatalError("demo.wat not found in app bundle")
        }

        let status = bridge.runWasmApp(watData)
        if status != 0 {
            fatalError("rb_run_wasm_app failed with status \(status)")
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
