// swift-tools-version: 6.0

// Tests for the Swift helpers that every app gets a copy of
// (scripts/generate-bindings.sh copies helpers/ into Generated/ForgeFFI).
// Only the helpers that build on Foundation alone are on Linux; ImageIO and
// SwiftUI parts compile to nothing there and are tested on macOS.
//
//     swift test --package-path crates/forge-services/swift

import PackageDescription

let package = Package(
    name: "ForgeServicesHelpers",
    platforms: [.iOS(.v17), .macOS(.v14)],
    targets: [
        .target(
            name: "ForgeServicesHelpers",
            path: "helpers",
            // UIKit, WebKit and AVFoundation helpers aren't tested here.
            exclude: ["ForgeAudioSession.swift", "ForgeDocuments.swift", "ForgeWeb.swift"],
            // The apps build Generated/ in Swift 5 mode; test the same way.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "HelperTests",
            dependencies: ["ForgeServicesHelpers"],
            path: "Tests/HelperTests",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
