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
5. After changing an app crate's public API (`examples/*/rust`), regenerate its bindings (`forge bindings generate --project <app>`, or `make bindings bindings-kotlin` for every example), then run `./scripts/check-app-crates.sh` and `./scripts/check-bindings.sh` and commit the `Generated/` updates. See [Binding drift](docs/ffi-policy.md#binding-drift) for what the check compares and how to regenerate.

## Toolchain and dependencies

- Rust is pinned in `rust-toolchain.toml`; rustup installs it automatically. Bump it in its own PR and fix any new Clippy lints there. CI also checks the declared MSRV (1.89).
- `cargo deny check` (install with `cargo install cargo-deny --locked`) audits advisories, licenses and sources using `deny.toml`. CI runs it on every PR that changes code.
- Dependabot opens grouped weekly PRs for Cargo and GitHub Actions (pinned by commit SHA) and monthly ones for Gradle.

## Host app tests

The view-model tests call the real Rust core through the generated bindings, so they catch binding and error-mapping mistakes that `cargo test` cannot.

- **Android (any OS with a JDK and Android SDK):** `cargo build --manifest-path examples/counter/rust/Cargo.toml`, then `cd examples/counter-android && ./gradlew :app:testDebugUnitTest` (or `make test-android-host`, or `forge test --project examples/counter-android`).
- **iOS (macOS only):** `./scripts/test-swift-host.sh examples/counter` (or `make test-swift-host` for every example). It builds the app's Rust crate for the Mac and runs `swift test` in the app.

CI runs both, plus unsigned `xcodebuild` device builds; see [CI](#ci) for when.

## CI

CI is kept lean on purpose: a pull request should be green in minutes, and is gated by Linux checks only. The supported stack is Linux, xtool and an iPhone, so the macOS job is a weekly backstop and an on-demand check, not a gate. Docs-only changes (`*.md`, `docs/`) run nothing.

| Workflow | Job | Pull requests | Pushes to main |
| --- | --- | --- | --- |
| Rust (Linux) | Rust: fmt, clippy, tests, app-owned crates, binding drift, Xcode project layout | Every code change | Every code change |
| Rust (Linux) | Fresh app outside the checkout | Every code change | Every code change |
| Rust (Linux) | MSRV (1.89) and cargo-deny | Every code change | Every code change |
| macOS | iOS (Swift tests, Xcode build): one job, `scripts/ci-macos.sh` | On demand | Weekly, every app |
| Android (Linux) | android-apk | Only when Android code changes | When shared Rust or Android code changes |
| Screenshots (macOS) | Simulator screenshots | On demand | On demand |

Anything path-filtered can be run by hand on any branch: Actions tab, pick the workflow, **Run workflow**. Use it for Android (frozen, work in progress), for the Swift host tests and Xcode device builds on a branch (macOS; optionally name the apps), and for simulator screenshots of a UI change (Screenshots, pick `starter`, `pocket-library` or `weather`; the images are uploaded and also printed as small JPEGs in the log). `./scripts/ci-macos.sh --list affected "$(git merge-base origin/main HEAD)"` shows which apps a branch's macOS run would check.

## Pull requests

- Keep Rust domain logic free of Swift/iOS and UniFFI dependencies (the Counter's `examples/counter/rust/src/domain` is the model).
- Do not edit UniFFI-generated Swift/C files by hand.
- Document behavioral changes in `docs/` when they affect developers.
- When you bump a toolchain or build-tool version, update [docs/compatibility.md](docs/compatibility.md) in the same PR.
- Record consequential packaging or architecture choices in `docs/adr/`.

## Code of conduct

Be respectful and precise in issues and reviews. Focus on reproducible toolchain evidence for iOS-related claims.
