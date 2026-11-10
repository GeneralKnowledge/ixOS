// swift-tools-version: 6.0
// The ForgeUI showcase: every ForgeUI component, live, and three example
// screens (settings, a list, a create/edit form) built only from ForgeUI and
// SwiftUI. It is an ordinary SwiftUI app: no Rust core, no generated
// bindings, no ForgeKit runtime. That is the point: ForgeUI must be useful in
// any SwiftUI app (docs/adr/002-forgeui-in-forgekit.md).
//
// Build for an iPhone from Linux with xtool, from this folder: `xtool dev`.
// Inside the checkout ForgeUI comes from ../../forgekit; an app outside it
// uses the release:
//     .package(url: "https://github.com/GeneralKnowledge/ForgeKit.git", exact: "<version>")

import PackageDescription

let package = Package(
    name: "ForgeUIShowcase",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        // The gallery and example screens, usable from any app.
        .library(name: "ForgeUIShowcase", targets: ["ForgeUIShowcase"]),
        // What xtool builds: @main in a library product (xtool.yml).
        .library(name: "ForgeUIShowcaseXtool", targets: ["ForgeUIShowcaseXtoolApp"]),
    ],
    dependencies: [
        .package(path: "../../forgekit"),
    ],
    targets: [
        // Models/ is Foundation and Observation only (tested on Linux); the
        // views are wrapped in `#if canImport(SwiftUI)`.
        .target(
            name: "ForgeUIShowcase",
            dependencies: [.product(name: "ForgeUI", package: "forgekit")],
            path: "Sources/ForgeUIShowcase",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(
            name: "ForgeUIShowcaseXtoolApp",
            dependencies: ["ForgeUIShowcase"],
            path: "Sources/ForgeUIShowcaseXtoolApp",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "ForgeUIShowcaseTests",
            dependencies: ["ForgeUIShowcase", .product(name: "ForgeUI", package: "forgekit")],
            path: "Tests/ForgeUIShowcaseTests",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
