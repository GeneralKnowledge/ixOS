# Contributing

Thank you for contributing to Forge Mobile.

## Development

1. Install toolchain per [docs/linux-setup.md](docs/linux-setup.md).
2. `cargo test --workspace`
3. `cargo fmt --all`
4. `cargo clippy --workspace --all-targets -- -D warnings`
5. After changing `forge-ffi` public API, regenerate both Swift and Kotlin bindings (`make bindings bindings-kotlin`), then run `./scripts/check-bindings.sh` and commit the `Generated/` updates.

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
- Record consequential packaging or architecture choices in `docs/adr/`.

## Code of conduct

Be respectful and precise in issues and reviews. Focus on reproducible toolchain evidence for iOS-related claims.
