// swift-tools-version: 6.0
// OpenGenerable: guided, structured generation for Swift on Linux.
// No dependencies, so it can move to its own repository unchanged.
import PackageDescription

let package = Package(
    name: "OpenGenerable",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [
        .library(name: "OpenGenerable", targets: ["OpenGenerable"])
    ],
    targets: [
        .target(name: "OpenGenerable"),
        .testTarget(name: "OpenGenerableTests", dependencies: ["OpenGenerable"]),
    ]
)
