// swift-tools-version: 6.0

import PackageDescription

let iosLibSearch = "../../build/ios-device/lib"
// Host (macOS) static library for `swift test`; see scripts/test-swift-host.sh.
let macosLibSearch = "../../build/macos-host/lib"

let package = Package(
    name: "Counter",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .executable(name: "Counter", targets: ["CounterLauncher"]),
        .library(name: "CounterApp", targets: ["CounterApp"]),
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
            name: "CounterApp",
            dependencies: ["ForgeFFI"],
            path: "Sources/CounterApp"
        ),
        .executableTarget(
            name: "CounterLauncher",
            dependencies: ["CounterApp"],
            path: "Sources/CounterLauncher"
        ),
        .testTarget(
            name: "CounterAppTests",
            dependencies: ["CounterApp"],
            path: "Tests/CounterAppTests"
        ),
    ]
)
