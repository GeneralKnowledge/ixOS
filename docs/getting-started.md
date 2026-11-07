# Getting started: your first iPhone app from Linux

This page takes you from a Linux machine with nothing installed to a Forge app running on your iPhone, then through changing it. No Mac is needed at any point. It takes about an hour the first time, most of it downloading Xcode.xip and the Swift toolchain.

If you get stuck, `forge doctor` tells you what is missing and the command that fixes it, and every failed `forge` command ends with a "What went wrong" section. [troubleshooting.md](troubleshooting.md) has the longer explanations.

## 1. Install the toolchain (once)

Follow [linux-setup.md](linux-setup.md). In short:

1. Rust with rustup, then `rustup target add aarch64-apple-ios`.
2. Swift 6.2 from swift.org.
3. xtool, then `sudo apt-get install usbmuxd libimobiledevice-utils`.
4. Download **Xcode 26** as `Xcode.xip` from developer.apple.com (needs a free Apple ID) and run `xtool setup`. It asks for your Apple ID and the path to `Xcode.xip`, and installs the Darwin SDK. Xcode 16's SDK is too old for Swift 6.2.
5. Clone Forge and install its CLI:

   ```bash
   git clone https://github.com/GeneralKnowledge/ixOS.git ~/src/forge
   cargo install --path ~/src/forge/crates/forge-cli --locked
   ```

   The installed `forge` remembers where the checkout is. If you move the checkout, reinstall or set `FORGE_WORKSPACE`.

## 2. Check the machine

```bash
forge doctor
```

You want these two lines to say `ready`:

```
iOS from Linux (xtool, device builds): ready
Run on an iPhone (Apple sign-in, connected device): ready
```

Anything else is listed underneath with its fix. The second line needs the iPhone: connect it by USB, unlock it and tap **Trust**, and turn on **Developer Mode** (Settings > Privacy & Security > Developer Mode, then restart). `forge devices` lists what is connected.

## 3. Create an app

Anywhere outside the Forge checkout:

```bash
cd ~/projects
forge init MyApp --bundle-id org.example.myapp
cd MyApp
```

`forge init` prints the next steps. The bundle identifier must be unique to you (reverse DNS, like `com.yourname.myapp`). A free Apple account can register 10 new ones a week, so reuse one while experimenting.

The app has its own Rust crate in `rust/`, a SwiftUI screen in `Sources/AppHost/`, and a `README.md` that maps out the rest. See [app-crate.md](app-crate.md) for the Rust side.

## 4. Run it on the iPhone

```bash
forge run ios
```

This checks the toolchain, your Apple sign-in and the iPhone first (seconds), then builds the Rust library for iOS, regenerates the Swift bindings, compiles the app with xtool, signs it with your Apple ID and installs it. It does not open the app (xtool cannot launch apps on iOS 17 and later): tap its icon on the home screen.

The first install of each app needs one step on the phone before it opens: Settings > General > VPN & Device Management, tap your Apple ID, then **Trust**.

With a free Apple account the signature lasts 7 days; run `forge run ios` again to renew it. A free account can also have only 3 such apps installed at a time.

## 5. Change it

The loop is Rust first, then the screen:

```bash
$EDITOR rust/src/lib.rs                     # add a method to AppCore, a field to a record...
cargo test --manifest-path rust/Cargo.toml  # seconds, no SDK or phone
forge bindings generate                     # optional: the new Swift API, for your editor
$EDITOR Sources/AppHost/NotesModel.swift    # call it from the view model
forge check swift                           # seconds: the model + bindings + their tests, plain Swift, no SDK
$EDITOR Sources/AppHost/ContentView.swift   # show it
forge run ios
```

`forge run ios` always rebuilds the Rust and the bindings, so a Rust change cannot reach the phone without its Swift API, or the other way round.

`forge test` runs your crate's tests from anywhere in the app. On a Mac it also runs the XCTest suite in `Tests/`; on Linux that part is skipped and CI runs it.

`forge check swift` catches most Swift mistakes at the Swift–Rust boundary without the iOS SDK, a Mac or the phone. It builds your Rust for this computer and compiles every Swift file that imports nothing Apple-only (in the starter, `NotesModel.swift`, `Note+Display.swift` and the UniFFI bindings) with the plain Swift toolchain. It then runs the tests that need no SwiftUI: 9 of the starter's 10, in about 15 seconds. It lists the files it left out. SwiftUI views still need `forge run ios` or `forge build ios`. Without Swift installed, `FORGE_SWIFT_DOCKER=swift:6.2 forge check swift` runs it in Docker. If the app has a `requirements.toml`, it also reports which requirements those tests prove and writes an evidence bundle to `.forge/runs/` ([evidence.md](evidence.md)).

## 6. When something fails

Every `forge build ios` / `forge run ios` failure names the layer that failed and the fix:

```
error: `xtool dev run` failed (exit status: 1)

What went wrong:
  [Signing and provisioning] A free Apple account can have at most 3 sideloaded apps on a device at once.
    Fix: Delete one of your other development apps from the iPhone (long-press its icon, Remove App), then run again.
```

The layers are:

| Layer | Typical problems |
|-------|------------------|
| Rust library | iOS target missing, Darwin SDK missing, your Rust does not compile |
| Swift build | SDK too old for Swift 6.2, bindings out of step with the library |
| Signing and provisioning | xtool not signed in, free-account limits, a revoked certificate |
| iPhone | not connected or not trusted, locked, Developer Mode off, profile not trusted, usbmuxd stuck |

If a check before the build is wrong about your setup, `FORGE_SKIP_PREFLIGHT=1 forge run ios` skips it.

For scripts and coding agents, add `--json` to any command: stdout then carries one result object (`ok`, the failed `stage`, compiler `diagnostics` with file and line, the `hints` above, and the path of a full log), progress goes to stderr, and the exit code names the stage: 10 Rust, 11 Swift, 12 signing, 13 iPhone, 14 tests, 2 a wrong command line. The format is in [forge-mcp.md](forge-mcp.md#the-result-contract).

## Next

- [builds.md](builds.md): every way to build, debug and release, and what is not supported yet.
- [ios-signing.md](ios-signing.md): Apple accounts, signing and devices in more depth.
- [android-setup.md](android-setup.md): the same app on Android (`forge init MyApp --platform android`). Android is work in progress.
- On a Mac, the app's `Xcode/` project gives you the debugger and Instruments ([ios-xcode.md](ios-xcode.md)). It is optional.
