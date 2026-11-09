// swift-tools-version: 6.0
// Phase 0 spike app: the shape `forge init` would generate on the proposed
// model. ForgeKit is a versioned package dependency; nothing in this app is a
// copy of framework code, and nothing refers to a Forge checkout.

import Foundation
import PackageDescription

let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
// The app's Rust library, staged by the build (host build for `swift test`).
let iosLibSearch = "\(packageDir)/build/ios-device/lib"
let hostLibSearch = "\(packageDir)/build/host/lib"

let package = Package(
    name: "SpikeApp",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .executable(name: "SpikeApp", targets: ["AppHostLauncher"]),
        .library(name: "AppHost", targets: ["AppHost"]),
    ],
    dependencies: [
        // The framework version this app is built on: exact, with
        // Package.resolved committed. (Phase 0 has no ForgeKit repository
        // yet; CI serves this URL from a local git repository tagged 0.3.0.)
        .package(url: "https://github.com/GeneralKnowledge/ForgeKit.git", exact: "0.3.0"),
    ],
    targets: [
        .systemLibrary(name: "forge_ffiFFI", path: "Generated/forge_ffiFFI"),
        .target(
            name: "ForgeFFI",
            dependencies: ["forge_ffiFFI", .product(name: "ForgeApp", package: "ForgeKit")],
            path: "Generated/ForgeFFI",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .linkedLibrary("forge_ffi"),
                .linkedFramework("Security", .when(platforms: [.iOS, .macOS])),
                .linkedFramework("CoreFoundation", .when(platforms: [.iOS, .macOS])),
                .unsafeFlags(["-L", iosLibSearch], .when(platforms: [.iOS])),
                .unsafeFlags(["-L", hostLibSearch], .when(platforms: [.macOS, .linux])),
            ]
        ),
        .target(
            name: "AppHost",
            dependencies: ["ForgeFFI", .product(name: "ForgeKit", package: "ForgeKit")],
            path: "Sources/AppHost",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .executableTarget(
            name: "AppHostLauncher",
            dependencies: ["AppHost", .product(name: "ForgeKit", package: "ForgeKit")],
            path: "Sources/AppHostLauncher",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "AppHostTests",
            dependencies: ["AppHost", "ForgeFFI", .product(name: "ForgeKit", package: "ForgeKit")],
            path: "Tests/AppHostTests",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
