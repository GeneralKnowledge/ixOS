# ADR 002: ForgeUI ships in the ForgeKit package

## Status

Proposed (2026-11-09). Waiting for Reuben.

## Context

ixOS is becoming a native application framework with a SwiftUI design system ([proposal](../research/forgeui-proposal.md)). ForgeUI (the design system) and later Forge AppKit (application composition) are Swift libraries that every app should be able to depend on, inside the checkout and outside it.

What already exists:

- ForgeKit, the Swift half of the application runtime, is a SwiftPM package whose only source is `forgekit/` in ixOS. `scripts/forgekit.sh release` copies it into the `GeneralKnowledge/ForgeKit` repository and tags it with forge-app's version. That repository holds releases only.
- SwiftPM needs `Package.swift` at the root of a repository to resolve a package by URL (Phase 0, [finding 3](../../spikes/phase0/RESULTS.md)). A package in a subfolder of ixOS can be used by path inside the checkout, but not by apps outside it.
- `forge init` pins new apps to one framework version (`forge-release.toml`), which already covers ForgeKit.

Options:

1. **A new product in the ForgeKit package.** `forgekit/Package.swift` gets a `ForgeUI` target with no dependencies and a `ForgeUI` library product.
2. **A separate Swift package, `forgeui/`, in ixOS,** released to a new `GeneralKnowledge/ForgeUI` repository with its own script and version.
3. **A root `Package.swift` in ixOS** holding ForgeKit and ForgeUI, so apps resolve ixOS itself.

## Decision

Option 1. ForgeUI is a library product of the ForgeKit package. Forge AppKit will be another (`ForgeAppKit`, never `AppKit`, which is Apple's macOS framework).

## Rationale

- **No new repository or release machinery.** The release script, its tests, the `app-model` CI job and `forge init`'s pin all work unchanged. Option 2 duplicates every one of them, and Reuben asked not to create another repository by default.
- **Independent where it matters.** SwiftPM builds only the targets a product needs, so an app that imports only `ForgeUI` builds no UniFFI bindings and links no Rust. ForgeUI can be used in a plain SwiftUI app.
- **One version.** Apps pin one framework version; ForgeUI and the runtime they were tested with move together. A design-system change that needs a runtime change (AppKit's load state over `ForgeHost`, for instance) lands in one PR and one release.
- **Option 3** would make every app resolve the whole ixOS repository (Rust crates, docs, examples and their history) to get a few Swift files, and reverses the 2026-11-09 decision that ForgeKit is its own repository.

## Consequences

- A ForgeUI-only change still needs a forge-app version bump to release, because the release is tagged with forge-app's version and refuses to reuse a tag. Forge-app's version is, in effect, the framework version; `docs/release-process.md` should say so in the ForgeUI PR.
- ForgeUI's SwiftUI files are wrapped in `#if canImport(SwiftUI)`, like ForgeKit's, so `swift build` and `swift test` of the package keep working on Linux. Its tokens and contrast maths import only Foundation and are tested there.
- The package name stays ForgeKit. If the framework is renamed later, that is a separate decision.
- If ForgeUI ever needs a release cadence of its own (for example, used by many apps that are not ixOS apps), option 2 is still open: the target moves out unchanged.
