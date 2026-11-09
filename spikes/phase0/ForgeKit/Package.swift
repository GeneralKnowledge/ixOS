// swift-tools-version: 6.0
// Phase 0 spike (docs/app-model-review.md): the Swift half of the ixOS
// application runtime as a real, versioned package, instead of files copied
// into each app. Apps depend on it with `.package(url:..., exact: "0.3.0")`.
import PackageDescription

let package = Package(
    name: "ForgeKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        // The shared runtime contract's UniFFI bindings (AppPhase,
        // RuntimeEvents, Cancellation, ErrorKind). Generated once per Forge
        // release from the forge-app crate; app bindings import this module.
        .library(name: "ForgeApp", targets: ["ForgeApp"]),
        // The host: owns the Rust core for the process, reports the app
        // lifecycle, runs the event loop, forwards cancellation.
        .library(name: "ForgeKit", targets: ["ForgeKit"]),
    ],
    targets: [
        .systemLibrary(name: "forge_appFFI", path: "Sources/forge_appFFI"),
        .target(
            name: "ForgeApp",
            dependencies: ["forge_appFFI"],
            path: "Sources/ForgeApp",
            // Generated UniFFI code is built in Swift 5 mode, as in apps today.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(
            name: "ForgeKit",
            dependencies: ["ForgeApp"],
            path: "Sources/ForgeKit",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
