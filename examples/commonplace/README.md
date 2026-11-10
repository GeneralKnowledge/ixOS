# Commonplace

A native, local-first reading application built with ixOS: subscribe to RSS, Atom and JSON feeds, read their articles, keep track of what you've read and bookmark what you want to keep. Everything stays on the device.

It is also the first **replication benchmark** for ixOS: its behaviour is benchmarked against NetNewsWire, an open-source iOS feed reader, to find what ixOS is missing for a real app. Commonplace is written from scratch, with its own name and look; it uses none of NetNewsWire's code or assets. The benchmark's brief, specification, gap log and pattern inventory are in [docs/benchmarks/netnewswire/](../../docs/benchmarks/netnewswire/README.md).

**Slice 1 (now):** subscriptions, refresh with HTTP caching, parsing (RSS, Atom, JSON Feed), the timelines (All Unread, Bookmarks, All Articles, each feed), read and unread, bookmarks, SQLite on the device, and navigation. The reader shows plain paragraphs behind one boundary (`ArticleReader` in Swift, `ArticleText` in Rust) until the native reader from the capability roadmap replaces it. Search and settings come later.

## Run it

From this directory:

```bash
forge doctor     # is the toolchain ready? (xtool, Darwin SDK, Apple sign-in, iPhone)
forge run ios    # build the Rust and Swift, sign and install on the iPhone
forge test       # Rust tests (and the XCTest suite on macOS)
```

`forge run ios` checks the toolchain, your Apple sign-in and the connected iPhone before it starts building, and when a step fails it says which part failed (Rust, Swift, signing or the iPhone) and how to fix it.

The first time you run on an iPhone:

1. Connect it by USB, unlock it and tap **Trust**. `forge devices` should list it.
2. Turn on **Developer Mode** (Settings > Privacy & Security > Developer Mode), then restart the phone.
3. Sign xtool in to your Apple account: `xtool auth login`. Any Apple ID works; a free one can have 3 apps installed at a time and needs a re-install every 7 days.
4. xtool installs the app but does not open it: tap its icon on the home screen. After the first install, trust your developer profile: Settings > General > VPN & Device Management > your Apple ID > Trust.

## Where things are

| Path | What it is |
|------|------------|
| `rust/src/lib.rs` | Your app's logic, state, persistence and networking. Everything marked `#[uniffi::export]` is callable from Swift. |
| `rust/Cargo.toml` | Rust dependencies. Add crates here as usual (`cargo add --manifest-path rust/Cargo.toml <crate>`). The Forge crates (`forge-app`, the runtime the core embeds, and the others) are pinned to a framework release, as ForgeKit is in `Package.swift`. |
| `rust/src/feed.rs`, `rust/src/store.rs` | Feed parsing and plain text; the SQLite library (`library.sqlite`). |
| `Sources/AppHost/` | `AppHost.swift`, where ForgeKit's host opens the Rust core once and follows the app lifecycle; the SwiftUI screens (`ContentView.swift` is the root; `FeedListView`, `TimelineView`, `ArticleView` with the `ArticleReader` boundary, `AddFeedView`); and `LibraryModel.swift`, the model that calls Rust. |
| `Tests/AppHostTests/` | XCTest suite against the real Rust library (macOS only). |
| `uitests/` | UI tests that drive the app on a plugged-in iPhone from Linux (`forge ui run uitests/smoke.toml`). They find controls by their accessibility identifiers (`feeds.add`, `addFeed.save`). |
| `Generated/` | Swift bindings generated from `rust/`. Do not edit; `forge run ios` and `forge bindings generate` rewrite them. |
| `forge.toml` | App name, bundle identifier, version, minimum iOS, permissions, entitlements (`forge config show`). |
| `Info.plist`, `App.entitlements`, `xtool.yml` | Generated from `forge.toml` by every `forge build ios` / `forge run ios`. Edit `forge.toml` instead. |
| `Xcode/` | An Xcode project, for when you are on a Mac and want the debugger. Never required. |
| `build/`, `rust/target/` | Build output. Safe to delete. |
| `AGENTS.md`, `CLAUDE.md`, `.mcp.json` | Not here by default. If you use an AI coding agent, `forge agent setup` adds its instructions (with a place for your project notes) and tool setup; nothing else needs them. |

## The loop

1. Change the Rust in `rust/src/lib.rs`. `cargo test --manifest-path rust/Cargo.toml` tests it in seconds, with no SDK or phone.
2. If you changed what is exported, `forge bindings generate` updates the Swift API so your editor sees it.
3. Use it from `Sources/AppHost/`, then `forge run ios`.

## Help

- Something missing: `forge doctor` lists each missing piece and the command that fixes it.
- A build or install failed: the error ends with "What went wrong" and a fix. The Forge checkout's `docs/troubleshooting.md` has more.
- Getting started from scratch: `docs/getting-started.md` in the Forge checkout.
