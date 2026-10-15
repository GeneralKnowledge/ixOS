// swift-tools-version: 6.0

import PackageDescription

// Paths are relative to this package under examples/<name>/ inside the Forge workspace.
let iosLibSearch = "../../build/ios-device/lib"

let package = Package(
    name: "FORGE_PACKAGE_NAME",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .executable(name: "FORGE_PACKAGE_NAME", targets: ["AppHost"]),
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
            name: "AppHost",
            dependencies: ["ForgeFFI"],
            path: "Sources/AppHost"
        ),
    ]
)
