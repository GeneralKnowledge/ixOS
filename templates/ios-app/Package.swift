// swift-tools-version: 6.0

import PackageDescription

// Paths are relative to this package under examples/<name>/ inside the Forge workspace.
let iosLibSearch = "../../build/ios-device/lib"
// Host (macOS) static library for `swift test`; see scripts/test-swift-host.sh.
let macosLibSearch = "../../build/macos-host/lib"

let package = Package(
    name: "FORGE_PACKAGE_NAME",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .executable(name: "FORGE_PACKAGE_NAME", targets: ["AppHostLauncher"]),
        .library(name: "AppHost", targets: ["AppHost"]),
    ],
    targets: [
        .target(
            name: "ForgeFfiC",
            path: "Generated",
            publicHeadersPath: ".",
            cSettings: [
                .headerSearchPath("."),
            ],
            linkerSettings: [
                .linkedLibrary("forge_ffi"),
                .unsafeFlags(["-L", iosLibSearch], .when(platforms: [.iOS])),
                .unsafeFlags(["-L", macosLibSearch], .when(platforms: [.macOS])),
            ]
        ),
        .target(
            name: "ForgeFFI",
            dependencies: ["ForgeFfiC"],
            path: "Generated",
            sources: ["forge_ffi.swift"]
        ),
        .target(
            name: "AppHost",
            dependencies: ["ForgeFFI"],
            path: "Sources/AppHost"
        ),
        .executableTarget(
            name: "AppHostLauncher",
            dependencies: ["AppHost"],
            path: "Sources/AppHostLauncher"
        ),
        .testTarget(
            name: "AppHostTests",
            dependencies: ["AppHost"],
            path: "Tests/AppHostTests"
        ),
    ]
)
