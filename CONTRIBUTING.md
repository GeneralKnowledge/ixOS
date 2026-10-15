# Contributing

Thank you for contributing to Forge Mobile.

## Development

1. Install toolchain per [docs/linux-setup.md](docs/linux-setup.md).
2. `cargo test --workspace`
3. `cargo fmt --all`
4. `cargo clippy --workspace --all-targets -- -D warnings`
5. After changing `forge-ffi` public API: `./scripts/generate-bindings.sh` and commit `Generated/` updates.

## Pull requests

- Keep Rust domain logic free of Swift/iOS dependencies in `forge-core`.
- Do not edit UniFFI-generated Swift/C files by hand.
- Document behavioral changes in `docs/` when they affect developers.
- Record consequential packaging or architecture choices in `docs/adr/`.

## Code of conduct

Be respectful and precise in issues and reviews. Focus on reproducible toolchain evidence for iOS-related claims.
