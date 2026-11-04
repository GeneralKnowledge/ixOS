# Contributing

Thank you for contributing to Forge Mobile.

## Quick path: a green local build

Assumes Linux (or macOS) with `rustup`, `git` and `make`. Nothing else: no Swift, xtool, Xcode, Android SDK or device is needed for this.

```bash
git clone https://github.com/GeneralKnowledge/ixOS.git && cd ixOS
cargo test --workspace     # rustup installs the pinned toolchain on first run
make validate              # rustfmt, clippy, tests, binding drift, template sync
```

That is the same set of checks as the Rust and Bindings CI jobs. `make validate` runs `cargo fmt --all`, so commit any formatting it applies. If something fails, compare your versions with [docs/compatibility.md](docs/compatibility.md).

Going further:

- Android JVM unit tests need a JDK 17 and the Android SDK: `make test-android-host` ([docs/android-setup.md](docs/android-setup.md)).
- Swift host tests need macOS with Xcode: `make test-swift-host`.
- Building an iOS app on Linux needs Swift, xtool and the Darwin SDK ([docs/linux-setup.md](docs/linux-setup.md)).
- Looking for something to work on? See [docs/roadmap.md](docs/roadmap.md).

## Development

1. Install the full toolchain per [docs/linux-setup.md](docs/linux-setup.md) when you need more than the quick path.
2. `cargo test --workspace`
3. `cargo fmt --all`
4. `cargo clippy --workspace --all-targets -- -D warnings`
5. After changing `forge-ffi` public API, regenerate both Swift and Kotlin bindings (`make bindings bindings-kotlin`), then run `./scripts/check-bindings.sh` and commit the `Generated/` updates. See [Binding drift](docs/ffi-policy.md#binding-drift) for what the check compares and how to regenerate.

## Toolchain and dependencies

- Rust is pinned in `rust-toolchain.toml`; rustup installs it automatically. Bump it in its own PR and fix any new Clippy lints there. CI also checks the declared MSRV (1.88).
- `cargo deny check` (install with `cargo install cargo-deny --locked`) audits advisories, licenses and sources using `deny.toml`. CI runs it on every PR.
- Dependabot opens grouped weekly PRs for Cargo and GitHub Actions (pinned by commit SHA) and monthly ones for Gradle.

## Host app tests

The view-model tests call the real Rust core through the generated bindings, so they catch binding and error-mapping mistakes that `cargo test` cannot.

- **Android (any OS with a JDK and Android SDK):** `cargo build -p forge-ffi`, then `cd examples/counter-android && ./gradlew :app:testDebugUnitTest` (or `make test-android-host`).
- **iOS (macOS only):** `./scripts/test-swift-host.sh` (or `make test-swift-host`). It builds `forge-ffi` for the Mac and runs `swift test` in `examples/counter`.

CI runs both, plus an unsigned `xcodebuild` of the example and of a freshly generated iOS template app, on every pull request (`.github/workflows/macos.yml`, `.github/workflows/android.yml`).

## Pull requests

- Keep Rust domain logic free of Swift/iOS dependencies in `forge-core`.
- Do not edit UniFFI-generated Swift/C files by hand.
- Document behavioral changes in `docs/` when they affect developers.
- When you bump a toolchain or build-tool version, update [docs/compatibility.md](docs/compatibility.md) in the same PR.
- Record consequential packaging or architecture choices in `docs/adr/`.

## Code of conduct

Be respectful and precise in issues and reviews. Focus on reproducible toolchain evidence for iOS-related claims.
