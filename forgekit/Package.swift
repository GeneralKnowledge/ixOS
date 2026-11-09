// swift-tools-version: 6.0
// ForgeKit: the Swift half of the ixOS application runtime
// (docs/app-model-review.md). This folder is the source of truth; the
// GeneralKnowledge/ForgeKit repository is a release artefact made from it by
// scripts/forgekit.sh, tagged with the forge-app version. Never edit the
// release repository by hand.
//
// Apps outside this checkout depend on a release:
//     .package(url: "https://github.com/GeneralKnowledge/ForgeKit.git", exact: "<version>")
// Examples inside the checkout use this folder: .package(path: "../../forgekit")
import PackageDescription

let package = Package(
    name: "ForgeKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        // The runtime contract's UniFFI bindings (AppPhase, ErrorKind,
        // RuntimeEvent, RuntimeEvents, Cancellation), generated from the
        // forge-app crate. App bindings import this module. The symbols are
        // in the app's own Rust library, which links forge-app.
        .library(name: "ForgeApp", targets: ["ForgeApp"]),
        // The host: owns the Rust core for the process, reports the app
        // lifecycle, runs the event loop, forwards cancellation, presents
        // errors by kind.
        .library(name: "ForgeKit", targets: ["ForgeKit"]),
    ],
    targets: [
        .systemLibrary(name: "forge_appFFI", path: "Sources/forge_appFFI"),
        .target(
            name: "ForgeApp",
            dependencies: ["forge_appFFI"],
            path: "Sources/ForgeApp",
            // Generated UniFFI code is built in Swift 5 mode, as in apps.
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
