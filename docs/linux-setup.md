# Linux setup

Forge Mobile targets **Linux-first** development for native iOS apps using Rust, UniFFI, SwiftUI, and [xtool](https://github.com/xtool-org/xtool).

## Requirements

| Tool | Purpose |
|------|---------|
| Rust (stable, see `rust-toolchain.toml`) | Core logic, CLI, tests |
| Swift 6.2+ (6.4 recommended by xtool upstream) | SwiftPM host |
| xtool | iOS build, sign, install |
| usbmuxd | USB communication with iPhone |
| Xcode `.xip` | Supplies iOS SDK via `xtool setup` (download in browser; license acceptance required) |
| Apple ID / Developer credentials | `xtool setup` / `xtool auth` |

Tested versions of each tool are in [compatibility.md](compatibility.md).

## Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-apple-ios
```

## Install Swift (Ubuntu 24.04 example)

Download the latest **ubuntu24.04** tarball from [swift.org/download](https://www.swift.org/download/) and extract to `/usr/local`:

```bash
curl -fL -o /tmp/swift.tar.gz \
  "https://download.swift.org/swift-6.2.3-release/ubuntu2404/swift-6.2.3-RELEASE/swift-6.2.3-RELEASE-ubuntu24.04.tar.gz"
sudo tar -xzf /tmp/swift.tar.gz -C /usr/local
sudo ln -sf /usr/local/swift-6.2.3-RELEASE-ubuntu24.04/usr/bin/swift /usr/local/bin/swift
sudo apt-get install -y libncurses6
swift --version
```

## Install xtool

```bash
curl -fL \
  "https://github.com/xtool-org/xtool/releases/latest/download/xtool-$(uname -m).AppImage" \
  -o /tmp/xtool
chmod +x /tmp/xtool
sudo mv /tmp/xtool /usr/local/bin/xtool
xtool --version
```

## One-time xtool configuration

Follow [xtool Linux installation](https://github.com/xtool-org/xtool/blob/main/Documentation/xtool.docc/Installation-Linux.md):

```bash
sudo apt-get install -y usbmuxd libimobiledevice-utils
xtool setup
```

You will authenticate with Apple and provide the path to **Xcode.xip** (use Xcode 26; the iOS 18 SDK from Xcode 16 fails to link with Swift 6.2, see [troubleshooting.md](troubleshooting.md)). After success:

```bash
swift sdk list
# darwin
```

## Forge Mobile bootstrap

```bash
git clone <repo>
cd ixOS   # repository name; product is Forge Mobile
./scripts/validate-toolchain.sh
cargo test --workspace
./scripts/generate-bindings.sh
```

## Build Counter example

```bash
cargo run -p forge-cli -- build ios --project examples/counter
```

Or step by step:

```bash
./scripts/package-ios-library.sh examples/counter/Generated
cd examples/counter && xtool dev run
```

## CI note

Hosted Linux CI runs the Rust checks, binding drift, Android JVM tests and a debug APK build; macOS CI builds the Xcode projects and runs the Swift host tests. Linux iOS linking needs the Darwin SDK from `xtool setup`, which cannot be checked into the repo or downloaded in hosted CI. Use a self-hosted runner with the SDK installed for full Linux iOS builds.

## Hosted agent VMs (`environment.json`)

`environment.json` at the repo root is the bootstrap for hosted coding-agent VMs (fresh Ubuntu 24.04 images with the repo at `/workspace`). Its single `install` command runs the steps on this page non-interactively:

1. `apt-get install` of `libncurses6`, `usbmuxd`, `libimobiledevice-utils`, `curl`, `ca-certificates`
2. `rustup target add aarch64-apple-ios`
3. Download of the latest xtool AppImage to `/usr/local/bin/xtool`
4. Swift 6.2.3 from swift.org into `/usr/local` (skipped if already present)
5. `cargo fetch`

It holds no credentials or machine-specific paths, so it stays in the repo. It does **not** run `xtool setup` (that needs an Apple ID and an `Xcode.xip`), so agent VMs can run Rust, binding and template checks but not Linux iOS builds.

Two known gaps: xtool is fetched as `latest` rather than the version in [compatibility.md](compatibility.md), and neither download is checksum-verified. If you change the install steps on this page, update `environment.json` to match.
