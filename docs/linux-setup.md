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

You will authenticate with Apple and provide the path to **Xcode.xip**. After success:

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
cd examples/counter && xtool dev
```

## Cloud Agent / CI note

Hosted Linux CI in this repository runs **Rust tests and binding checks only**. iOS linking requires the Darwin SDK from `xtool setup`, which cannot be checked into the repo. Use a self-hosted runner with SDK installed for full iOS builds.
