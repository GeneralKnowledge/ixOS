// swift-tools-version: 6.0
// Good Dog inside the Device Lab showcase: the app's own Swift (Sources/, symlinked
// from examples/good-dog) over its own bindings (Generated/, written by
// `forge bindings generate --project examples/device-lab`). Device Lab imports
// it with moduleAliases, so its ForgeFFI and AppHost modules keep their names
// here. The Rust is linked into Device Lab's library, so nothing links here.
import PackageDescription

let forgeFFI: Target = .target(
    name: "ForgeFFI",
    dependencies: ["good_dog_appFFI", .product(name: "ForgeApp", package: "forgekit")],
    path: "Generated/ForgeFFI",
    // Generated UniFFI code is built in Swift 5 mode, as in the app.
    swiftSettings: [.swiftLanguageMode(.v5)]
)

let package = Package(
    name: "GoodDog",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "GoodDogApp", targets: ["AppHost", "ForgeFFI"]),
    ],
    dependencies: [
        .package(path: "../../../../forgekit"),
    ],
    targets: [
        .systemLibrary(name: "good_dog_appFFI", path: "Generated/good_dog_appFFI"),
        forgeFFI,
        .target(
            name: "AppHost",
            dependencies: ["ForgeFFI", .product(name: "ForgeUI", package: "forgekit")],
            path: "Sources/AppHost"
        ),
    ]
)
