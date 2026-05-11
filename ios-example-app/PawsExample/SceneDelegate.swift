import UIKit

class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        let nav = UINavigationController(rootViewController: WelcomeViewController())
        nav.navigationBar.prefersLargeTitles = true
        window.rootViewController = nav
        window.makeKeyAndVisible()
        self.window = window

        // Screenshot/automation hook: when launched with
        // `PAWS_AUTO_OPEN_EXAMPLE=<wasmResourceName>` in the environment,
        // push the matching runner so a single `simctl launch --env …`
        // can drive the simulator straight into any example without a tap.
        if let resource = ProcessInfo.processInfo.environment["PAWS_AUTO_OPEN_EXAMPLE"],
           let entry = ExampleCatalog.sections
               .flatMap({ $0.entries })
               .first(where: { $0.wasmResourceName == resource }) {
            nav.pushViewController(ExampleRunnerViewController(entry: entry), animated: false)
        }
    }
}
