import UIKit
import PawsRenderer

private enum WasmEntryPoint {
    static let run = "run"
}

class ViewController: UIViewController {
    private var rendererView: PawsRendererView!

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .white

        rendererView = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 37, y: 100, width: 300, height: 300)
        )
        view.addSubview(rendererView)

        // Run the demo WASM module.
        // This is async: ops will be dispatched to the main thread when ready.
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let url = Bundle.main.url(forResource: "demo", withExtension: "wasm") else {
                fatalError("demo.wasm not found in bundle — ensure the Cargo build phase ran successfully.")
            }
            guard let data = try? Data(contentsOf: url) else {
                fatalError("Failed to read demo.wasm from bundle.")
            }
            DispatchQueue.main.async {
                self?.rendererView.renderer.postRunWasm(data, functionName: WasmEntryPoint.run)
            }
        }
    }
}
