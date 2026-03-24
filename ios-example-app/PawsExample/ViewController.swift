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

        // Run the demo WAT module — creates the flex container with 4 colored divs.
        // This is async: ops will be dispatched to the main thread when ready.
        rendererView.renderer.postRunWat(demoWat, functionName: "run")
    }
}
