// swift-tools-version: 6.0
// Commonplace inside the Device Lab showcase: the app's own Swift (Sources/, symlinked
// from examples/commonplace) over its own bindings (Generated/, written by
// `forge bindings generate --project examples/device-lab`). Device Lab imports
// it with moduleAliases, so its ForgeFFI and AppHost modules keep their names
// here. The Rust is linked into Device Lab's library, so nothing links here.
import PackageDescription

let forgeFFI: Target = .target(
    name: "ForgeFFI",
    dependencies: ["commonplace_coreFFI", .product(name: "ForgeApp", package: "forgekit")],
    path: "Generated/ForgeFFI",
    // Generated UniFFI code is built in Swift 5 mode, as in the app.
    swiftSettings: [.swiftLanguageMode(.v5)]
)

let package = Package(
    name: "Commonplace",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "CommonplaceApp", targets: ["AppHost", "ForgeFFI"]),
    ],
    dependencies: [
        .package(path: "../../../../forgekit"),
    ],
    targets: [
        .systemLibrary(name: "commonplace_coreFFI", path: "Generated/commonplace_coreFFI"),
        forgeFFI,
        .target(
            name: "AppHost",
            dependencies: [
                "ForgeFFI",
                .product(name: "ForgeKit", package: "forgekit"),
                .product(name: "ForgeAppKit", package: "forgekit"),
            ],
            path: "Sources/AppHost"
        ),
    ]
)
