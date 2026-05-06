import UIKit
import PawsRenderer

private enum WasmEntryPoint {
    static let run = "run"
}

final class ExampleRunnerViewController: UIViewController {
    private let entry: ExampleEntry
    private var rendererView: PawsRendererView?
    private let statusLabel = UILabel()
    private var wasmData: Data?
    private var hasPostedWasm = false

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

        loadWasm()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        postWasmIfReady()
    }

    private func loadWasm() {
        ExampleWasmCache.shared.load(entry) { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let data):
                wasmData = data
                postWasmIfReady()
            case .failure(let error):
                showError(error.localizedDescription)
            }
        }
    }

    private func postWasmIfReady() {
        guard !hasPostedWasm,
              let wasmData,
              let rendererView,
              rendererView.bounds.width > 0,
              rendererView.bounds.height > 0 else {
            return
        }

        hasPostedWasm = true
        rendererView.renderer.postRunWasm(wasmData, functionName: WasmEntryPoint.run)
    }

    private func showError(_ message: String) {
        statusLabel.text = message
        statusLabel.textColor = .systemRed
    }
}
