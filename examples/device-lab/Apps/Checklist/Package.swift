// swift-tools-version: 6.0
// Checklist inside the Device Lab showcase: the app's own Swift (Sources/, symlinked
// from examples/checklist) over its own bindings (Generated/, written by
// `forge bindings generate --project examples/device-lab`). Device Lab imports
// it with moduleAliases, so its ForgeFFI and AppHost modules keep their names
// here. The Rust is linked into Device Lab's library, so nothing links here.
import PackageDescription

let forgeFFI: Target = .target(
    name: "ForgeFFI",
    dependencies: ["checklist_coreFFI", .product(name: "ForgeApp", package: "forgekit")],
    path: "Generated/ForgeFFI",
    // Generated UniFFI code is built in Swift 5 mode, as in the app.
    swiftSettings: [.swiftLanguageMode(.v5)]
)

let package = Package(
    name: "Checklist",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "ChecklistEmbed", targets: ["ChecklistEmbed"]),
    ],
    dependencies: [
        .package(path: "../../../../forgekit"),
    ],
    targets: [
        .systemLibrary(name: "checklist_coreFFI", path: "Generated/checklist_coreFFI"),
        forgeFFI,
        .target(name: "ChecklistEmbed", dependencies: ["ChecklistApp", "ForgeFFI"], path: "Sources/ChecklistEmbed"),
        .target(
            name: "ChecklistApp",
            dependencies: ["ForgeFFI", .product(name: "ForgeKit", package: "forgekit")],
            path: "Sources/ChecklistApp",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
