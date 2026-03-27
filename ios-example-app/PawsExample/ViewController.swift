import UIKit
import PawsRenderer

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
        if let url = Bundle.main.url(forResource: "demo", withExtension: "wasm"),
           let data = try? Data(contentsOf: url) {
            rendererView.renderer.postRunWasm(data, functionName: "run")
        } else {
            print("Failed to load demo.wasm. Please ensure it is built and added to the app bundle.")
        }
    }
}
