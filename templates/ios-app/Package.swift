// swift-tools-version: 6.0

import PackageDescription

let iosLibSearch = "../../build/ios-device/lib"

let package = Package(
    name: "Counter",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .executable(name: "Counter", targets: ["CounterApp"]),
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
            ]
        ),
        .target(
            name: "ForgeFFI",
            dependencies: ["ForgeFfiC"],
            path: "Generated",
            sources: ["forge_ffi.swift"]
        ),
        .executableTarget(
            name: "CounterApp",
            dependencies: ["ForgeFFI"],
            path: "Sources/CounterApp"
        ),
    ]
)
