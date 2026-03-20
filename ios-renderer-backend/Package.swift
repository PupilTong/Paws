// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PawsRendererCore",
    platforms: [.iOS(.v15)],
    products: [
        .library(name: "PawsRendererCore", targets: ["PawsRendererCore"]),
    ],
    targets: [
        // C header module exposing the Rust FFI surface.
        .target(
            name: "CIOSRendererBackend",
            path: "Sources/PawsRendererCore/include",
            publicHeadersPath: "."
        ),
        // Swift wrapper around the Rust static library.
        .target(
            name: "PawsRendererCore",
            dependencies: ["CIOSRendererBackend"],
            path: "Sources/PawsRendererCore",
            exclude: ["include"],
            linkerSettings: [
                .linkedLibrary("ios_renderer_backend"),
            ]
        ),
    ]
)
