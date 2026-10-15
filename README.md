# Forge Mobile

Linux-first, Rust-first framework for building native iOS applications with **Rust** (domain logic), **UniFFI** (FFI), **SwiftUI** (UI), and **xtool** (build/sign/install from Linux).

This repository (`ixOS` on GitHub) ships a working **Counter** example, developer CLI, scripts, and documentation. It is an integration reference, not a full mobile platform yet.

## Quick start (Linux)

1. Install [Rust](https://www.rust-lang.org/), [Swift](https://www.swift.org/), and [xtool](https://github.com/xtool-org/xtool) — see [docs/linux-setup.md](docs/linux-setup.md).
2. Run `xtool setup` with your **Xcode.xip** to install the Darwin SDK.
3. Validate: `./scripts/validate-toolchain.sh`
4. Test Rust: `cargo test --workspace`
5. Generate bindings: `./scripts/generate-bindings.sh`
6. Build Counter for iOS: `cargo run -p forge-cli -- build ios --project examples/counter`

## Repository layout

```
crates/forge-core      # Domain logic (testable on Linux)
crates/forge-storage   # JSON snapshot persistence
crates/forge-ffi       # UniFFI boundary
crates/forge-cli       # `forge` CLI
examples/counter       # SwiftUI + xtool app
templates/ios-app      # `forge init` template
scripts/               # Toolchain and build scripts
docs/                  # Architecture, setup, validation
```

## Forge CLI

```bash
cargo run -p forge-cli -- doctor
cargo run -p forge-cli -- init MyApp --bundle-id org.example.myapp
cargo run -p forge-cli -- test
cargo run -p forge-cli -- bindings generate
cargo run -p forge-cli -- build ios --project examples/counter
cargo run -p forge-cli -- devices
```

Install the binary: `cargo install --path crates/forge-cli --locked` (optional).

## Configuration

Example apps include `forge.toml` — see [examples/counter/forge.toml](examples/counter/forge.toml).

New apps: `cargo run -p forge-cli -- init MyApp --bundle-id org.example.myapp` — see [docs/project-generation.md](docs/project-generation.md).

## License

Dual licensed under **MIT OR Apache-2.0** — see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## Known limitations

- Device-first; simulator off by default in config.
- iOS Rust linking requires xtool’s Darwin SDK (Xcode.xip).
- GitHub Actions CI runs Rust + binding checks, not full iOS device builds.
- No App Store / Android / networking in this release.

See [docs/toolchain-validation.md](docs/toolchain-validation.md) for verified commands and blockers.
