import Foundation
import PawsRenderer

struct ExampleEntry {
    let displayName: String
    let description: String
    let wasmResourceName: String
    let symbolName: String
}

struct ExampleSection {
    let title: String
    let footer: String?
    let entries: [ExampleEntry]
}

enum ExampleCatalog {
    static let sections: [ExampleSection] = [
        ExampleSection(
            title: "Core Primitives",
            footer: "Hand-written WASM components that exercise the rust-wasm-binding API directly.",
            entries: [
                ExampleEntry(
                    displayName: "Basic Element",
                    description: "Creates a single <div> on the document root.",
                    wasmResourceName: "example_basic_element",
                    symbolName: "square"
                ),
                ExampleEntry(
                    displayName: "Styled Element",
                    description: "<div> with inline width and height via setInlineStyle.",
                    wasmResourceName: "example_styled_element",
                    symbolName: "paintpalette"
                ),
                ExampleEntry(
                    displayName: "Nested Elements",
                    description: "Parent <div> with three <span> children via batch append.",
                    wasmResourceName: "example_nested_elements",
                    symbolName: "square.stack.3d.up"
                ),
                ExampleEntry(
                    displayName: "Stylesheet Cascade",
                    description: "Adds a stylesheet `div { height: 77px; }` via add_stylesheet.",
                    wasmResourceName: "example_stylesheet_cascade",
                    symbolName: "doc.text"
                ),
                ExampleEntry(
                    displayName: "Parsed Stylesheet",
                    description: "css!() macro drives a flexbox layout.",
                    wasmResourceName: "example_parsed_stylesheet",
                    symbolName: "curlybraces"
                ),
                ExampleEntry(
                    displayName: "Attributes",
                    description: "Sets class and id on a <div>.",
                    wasmResourceName: "example_attributes",
                    symbolName: "tag"
                ),
                ExampleEntry(
                    displayName: "Destroy & Rebuild",
                    description: "Creates, destroys, and recreates child elements.",
                    wasmResourceName: "example_destroy_rebuild",
                    symbolName: "arrow.triangle.2.circlepath"
                ),
                ExampleEntry(
                    displayName: "Full Commit Pipeline",
                    description: "DOM → style → layout with explicit commit().",
                    wasmResourceName: "example_commit_full",
                    symbolName: "checkmark.seal"
                ),
                ExampleEntry(
                    displayName: "Namespaces",
                    description: "SVG and MathML via create_element_ns.",
                    wasmResourceName: "example_namespace",
                    symbolName: "globe"
                ),
                ExampleEntry(
                    displayName: "Event Dispatch",
                    description: "Button with a click listener; dispatches a synthetic click.",
                    wasmResourceName: "example_event_dispatch",
                    symbolName: "hand.tap"
                ),
                ExampleEntry(
                    displayName: "Image Element",
                    description: "<img> backed by a data: URL; rendered via UIImageView.",
                    wasmResourceName: "example_img_element",
                    symbolName: "photo"
                ),
                ExampleEntry(
                    displayName: "Inline Image",
                    description: "inline_image!() + createObjectURL: raw PNG bytes embedded at compile time, blob URL minted at runtime.",
                    wasmResourceName: "example_inline_image",
                    symbolName: "photo.on.rectangle"
                ),
            ]
        ),
        ExampleSection(
            title: "Yew Framework",
            footer: "React-style components from the yew crate, running on Paws' virtual-DOM reconciler.",
            entries: [
                ExampleEntry(
                    displayName: "Counter",
                    description: "Classic button-plus-counter with use_state.",
                    wasmResourceName: "example_yew_counter",
                    symbolName: "plus.circle"
                ),
                ExampleEntry(
                    displayName: "use_state Counter",
                    description: "use_state with multiple setters and reads.",
                    wasmResourceName: "example_yew_use_state_counter",
                    symbolName: "number.circle"
                ),
                ExampleEntry(
                    displayName: "Multi-State Setters",
                    description: "Several setters mutating the same state in one frame.",
                    wasmResourceName: "example_yew_multi_state_setters",
                    symbolName: "square.grid.2x2"
                ),
                ExampleEntry(
                    displayName: "use_state_eq",
                    description: "Equality-gated state updates (no rerender on equal value).",
                    wasmResourceName: "example_yew_use_state_eq",
                    symbolName: "equal.circle"
                ),
                ExampleEntry(
                    displayName: "UB Deref Regression",
                    description: "Guards against use-after-free in state derefs.",
                    wasmResourceName: "example_yew_ub_deref",
                    symbolName: "exclamationmark.shield"
                ),
                ExampleEntry(
                    displayName: "Stale Read Regression",
                    description: "Guards against reading state after it was updated.",
                    wasmResourceName: "example_yew_stale_read",
                    symbolName: "clock.arrow.circlepath"
                ),
                ExampleEntry(
                    displayName: "Child Rerender",
                    description: "Parent state change that must rerender a child subtree.",
                    wasmResourceName: "example_yew_child_rerender",
                    symbolName: "arrow.down.forward.square"
                ),
                ExampleEntry(
                    displayName: "Photo Cycle",
                    description: "Three inlined PNGs with createObjectURL on mount; click cycling lands when the event system does.",
                    wasmResourceName: "example_yew_photo_cycle",
                    symbolName: "photo.stack"
                ),
            ]
        ),
    ]
}

enum ExampleWasmCacheError: LocalizedError {
    case missing(String)
    case readFailed(String)
    case precompileFailed(String)

    var errorDescription: String? {
        switch self {
        case .missing(let resource):
            return "\(resource).wasm not found in bundle"
        case .readFailed(let resource):
            return "Failed to read \(resource).wasm"
        case .precompileFailed(let resource):
            return "Failed to precompile \(resource).wasm"
        }
    }
}

final class ExampleWasmCache {
    static let shared = ExampleWasmCache()

    private let stateQueue = DispatchQueue(label: "dev.paws.example.wasm-cache.state")
    private let queue: OperationQueue = {
        let queue = OperationQueue()
        queue.name = "dev.paws.example.wasm-cache"
        queue.qualityOfService = .utility
        queue.maxConcurrentOperationCount = 1
        return queue
    }()
    private var dataByResource: [String: Data] = [:]
    private var completionsByResource: [String: [(Result<Data, Error>) -> Void]] = [:]
    private var operationsByResource: [String: Operation] = [:]

    private init() {}

    func prewarm(_ entries: [ExampleEntry]) {
        for entry in entries {
            preload(entry, priority: .veryLow)
        }
    }

    func preload(_ entry: ExampleEntry, priority: Operation.QueuePriority = .high) {
        load(entry, priority: priority) { _ in }
    }

    func load(
        _ entry: ExampleEntry,
        priority: Operation.QueuePriority = .high,
        completion: @escaping (Result<Data, Error>) -> Void
    ) {
        let resource = entry.wasmResourceName
        stateQueue.async { [self] in
            if let cached = dataByResource[resource] {
                DispatchQueue.main.async {
                    completion(.success(cached))
                }
                return
            }

            completionsByResource[resource, default: []].append(completion)

            if let existing = operationsByResource[resource] {
                if existing.queuePriority.rawValue < priority.rawValue {
                    existing.queuePriority = priority
                }
                return
            }

            let operation = BlockOperation { [self] in
                finish(resource: resource, result: loadAndPrecompile(resource: resource))
            }
            operation.queuePriority = priority
            operationsByResource[resource] = operation
            queue.addOperation(operation)
        }
    }

    private func finish(resource: String, result: Result<Data, Error>) {
        let completions = stateQueue.sync { [self] in
            if case .success(let data) = result {
                dataByResource[resource] = data
            }

            operationsByResource.removeValue(forKey: resource)
            return completionsByResource.removeValue(forKey: resource) ?? []
        }

        DispatchQueue.main.async {
            completions.forEach { $0(result) }
        }
    }

    private func loadAndPrecompile(resource: String) -> Result<Data, Error> {
        guard let url = Bundle.main.url(
            forResource: resource,
            withExtension: "wasm",
            subdirectory: "Examples"
        ) else {
            return .failure(ExampleWasmCacheError.missing(resource))
        }

        do {
            let data = try Data(contentsOf: url)
            guard PawsRendererInstance.precompileWasm(data) else {
                return .failure(ExampleWasmCacheError.precompileFailed(resource))
            }
            return .success(data)
        } catch {
            return .failure(ExampleWasmCacheError.readFailed(resource))
        }
    }
}
