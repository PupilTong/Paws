// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "PawsRenderer",
    platforms: [.iOS(.v15)],
    products: [
        .library(
            name: "PawsRenderer",
            targets: ["PawsRenderer"]
        ),
    ],
    targets: [
        // C module wrapping the Rust static library and cbindgen-generated header.
        .target(
            name: "PawsRendererFFI",
            path: "ios-renderer-backend/Sources/PawsRendererFFI",
            publicHeadersPath: "include",
            linkerSettings: [
                .linkedLibrary("ios_renderer_backend"),
            ]
        ),
        // Swift wrapper providing a native API over the C FFI.
        .target(
            name: "PawsRenderer",
            dependencies: ["PawsRendererFFI"],
            path: "ios-renderer-backend/Sources/PawsRenderer"
        ),
    ]
)
