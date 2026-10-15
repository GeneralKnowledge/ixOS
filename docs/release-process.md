# Release process (initial)

1. Bump crate versions in workspace `Cargo.toml` files if needed.
2. Run full Linux quality gate:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ./scripts/check-bindings.sh
   ```
3. On a machine with Darwin SDK + device, run the manual checklist in [toolchain-validation.md](toolchain-validation.md).
4. Tag release in git; publish crates only if/when crates.io publishing is configured (not required for source-only v0.1).
5. Document known limitations in release notes (no simulator default, no App Store pipeline).
