import UIKit
import PawsRenderer

private enum WasmEntryPoint {
    static let run = "run"
}

final class ExampleRunnerViewController: UIViewController {
    private let entry: ExampleEntry
    private var rendererView: PawsRendererView?
    private let statusLabel = UILabel()
    private var hasRunWasm = false

    init(entry: ExampleEntry) {
        self.entry = entry
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ExampleRunnerViewController does not support Interface Builder")
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        title = entry.displayName
        navigationItem.largeTitleDisplayMode = .never
        view.backgroundColor = .systemBackground

        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        statusLabel.text = entry.description
        statusLabel.font = .preferredFont(forTextStyle: .footnote)
        statusLabel.textColor = .secondaryLabel
        statusLabel.numberOfLines = 0
        view.addSubview(statusLabel)

        let host = PawsRendererView(baseURL: "about:blank")
        host.translatesAutoresizingMaskIntoConstraints = false
        host.backgroundColor = .secondarySystemBackground
        host.layer.cornerRadius = 12
        host.layer.cornerCurve = .continuous
        host.clipsToBounds = true
        view.addSubview(host)
        self.rendererView = host

        let guide = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            statusLabel.topAnchor.constraint(equalTo: guide.topAnchor, constant: 12),
            statusLabel.leadingAnchor.constraint(equalTo: guide.leadingAnchor, constant: 16),
            statusLabel.trailingAnchor.constraint(equalTo: guide.trailingAnchor, constant: -16),
            host.topAnchor.constraint(equalTo: statusLabel.bottomAnchor, constant: 16),
            host.leadingAnchor.constraint(equalTo: guide.leadingAnchor, constant: 16),
            host.trailingAnchor.constraint(equalTo: guide.trailingAnchor, constant: -16),
            host.bottomAnchor.constraint(equalTo: guide.bottomAnchor, constant: -16),
        ])
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        guard !hasRunWasm else { return }
        hasRunWasm = true
        runWasm()
    }

    override func willMove(toParent parent: UIViewController?) {
        super.willMove(toParent: parent)
        if parent == nil {
            rendererView?.removeFromSuperview()
            rendererView = nil
        }
    }

    private func runWasm() {
        guard let host = rendererView else { return }
        let resource = entry.wasmResourceName
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let url = Bundle.main.url(
                forResource: resource,
                withExtension: "wasm",
                subdirectory: "Examples"
            ) else {
                DispatchQueue.main.async {
                    self?.showError("\(resource).wasm not found in bundle")
                }
                return
            }
            guard let data = try? Data(contentsOf: url) else {
                DispatchQueue.main.async {
                    self?.showError("Failed to read \(resource).wasm")
                }
                return
            }
            DispatchQueue.main.async {
                host.renderer.postRunWasm(data, functionName: WasmEntryPoint.run)
            }
        }
    }

    private func showError(_ message: String) {
        statusLabel.text = message
        statusLabel.textColor = .systemRed
    }
}
