# App Intents, Shortcuts and Spotlight from Linux (spike, 2026-11-11)

**Question:** can an app built on Linux with xtool offer App Intents (actions in Shortcuts, Siri and Spotlight)?

**Answer so far:** the Swift compiles, but iOS only finds an app's intents through a `Metadata.appintents` folder in the bundle that Xcode writes after compiling. xtool doesn't write it. An open-source replacement for Xcode's tool exists and runs on Linux, so the gap looks closable, but nothing here has been tried with the SDK or on an iPhone yet.

## How Xcode does it

1. `swiftc` is run with `-emit-const-values-path <file>.swiftconstvalues` (and a list of the App Intents protocols to gather), which writes the literal values in each intent's declaration (title, parameters, phrases) as JSON. This is in the open-source compiler, so the Linux toolchain has it.
2. Xcode's `appintentsmetadataprocessor` reads those files and the binary and writes `Metadata.appintents/` (`extract.actionsdata`, `version.json`) into the app. Shortcuts and Siri read it without launching the app ([Swift Forums](https://forums.swift.org/t/what-are-the-compiler-technologies-that-support-the-app-intents-framework/59981)).
3. Without that step intents don't show up: Apple's reply to apps whose intents vanished after moving into a Swift package was that the package build skips extraction ([Apple Developer Forums](https://developer.apple.com/forums/thread/759160)).

## The replacement

[re-appintentsmetadataprocessor](https://codeberg.org/viraptor/re-appintentsmetadataprocessor) (MIT, Rust, one maintainer, 1.0.0, packaged in nixpkgs for Linux) reimplements Xcode's tool with the same arguments and says it writes byte-identical output for what it covers. Gaps it lists: no `AppShortcuts.xcstrings` merging (so App Shortcut phrases are English only), no `Metadata.generativefunctions`, no `FileEntity` content types.

## What Forge would do

1. Build the app target with `-Xswiftc -emit-const-values-path` per file (or SwiftPM's equivalent) during `forge build ios`.
2. Run the replacement tool on the outputs and the built binary.
3. Add `Metadata.appintents` to the bundle through `xtool.yml` `resources` (copied to the bundle root, which is where Xcode puts it), as Forge already does for compiled String Catalogs ([localization.md](../localization.md)).

The intents themselves stay plain Swift in the app (`AppIntent`, `AppShortcutsProvider`) calling the Rust core, as [native-capability-roadmap.md](../native-capability-roadmap.md) item 12 says. Nothing is needed in Rust.

## To find out next (needs the laptop with the iOS SDK, then the iPhone)

- Does SwiftPM pass `-emit-const-values-path` through for every source file, or does `forge build ios` need to compile once more to get the files? (laptop, ~15 minutes)
- Does the tool's output make one intent appear in Shortcuts on the iPhone? Add one intent and one App Shortcut to the services check app, build, install, and search Shortcuts for it. (phone, ~5 minutes)
- If yes: build it into `forge build ios`, then Pocket Library's "Log reading" intent.
- If no: App Intents stay a Mac-built feature, documented as such, like App Store upload.

Recommendation: do the two checks at the next laptop sitting before writing any Forge code; it's a small risk to find out and a large one to build blind.
