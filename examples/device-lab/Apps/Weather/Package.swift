// swift-tools-version: 6.0
// Weather inside the Device Lab showcase: the app's own Swift (Sources/, symlinked
// from examples/weather) over its own bindings (Generated/, written by
// `forge bindings generate --project examples/device-lab`). Device Lab imports
// it with moduleAliases, so its ForgeFFI and AppHost modules keep their names
// here. The Rust is linked into Device Lab's library, so nothing links here.
import PackageDescription

let forgeFFI: Target = .target(
    name: "ForgeFFI",
    dependencies: ["weather_coreFFI", .product(name: "ForgeApp", package: "forgekit")],
    path: "Generated/ForgeFFI",
    // Generated UniFFI code is built in Swift 5 mode, as in the app.
    swiftSettings: [.swiftLanguageMode(.v5)]
)

let package = Package(
    name: "Weather",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "WeatherEmbed", targets: ["WeatherEmbed"]),
    ],
    dependencies: [
        .package(path: "../../../../forgekit"),
    ],
    targets: [
        .systemLibrary(name: "weather_coreFFI", path: "Generated/weather_coreFFI"),
        forgeFFI,
        // SwiftPM 6.2 gives the alias only to the targets this one names, so
        // it names WeatherBridge too, though WeatherApp already depends on it.
        .target(name: "WeatherEmbed", dependencies: ["WeatherApp", "WeatherBridge", "ForgeFFI"], path: "Sources/WeatherEmbed"),
        .target(name: "WeatherBridge", dependencies: ["ForgeFFI"], path: "Sources/WeatherBridge"),
        .target(name: "WeatherApp", dependencies: ["WeatherBridge"], path: "Sources/WeatherApp"),
    ]
)
