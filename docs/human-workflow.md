# Developing an app without an AI agent

Forge is a framework for people first. Every ixOS app can be created, built, tested, installed, debugged and reviewed by a person with a terminal, a text editor and the `forge` CLI. No AI coding agent, MCP server, LLM API key, agent configuration file or hosted service is needed at any step.

Agents use the same commands. `forge mcp` and `forge agent setup` are optional layers over them ([Optional agent integration](#optional-agent-integration)).

This page is that workflow, step by step. `scripts/check-human-workflow.sh` runs it, and CI runs the script on every push. The [results table](#what-is-checked-where) says which steps are automated, which need hardware, and which have been seen working on an iPhone.

## The workflow

The example creates an app called Shelf and adds a reading time to each note: a rule in Rust, shown by SwiftUI.

### 1. Install Forge

Follow [getting-started.md](getting-started.md) and [linux-setup.md](linux-setup.md). Building from a checkout:

```bash
cargo install --path crates/forge-cli    # or: cargo build -p forge-cli, then use target/debug/forge
forge doctor                             # what is ready: Rust, Swift, xtool, the Darwin SDK, the iPhone
forge setup ios                          # installs what it safely can, asking before sudo
```

### 2. Create an app

```bash
forge init Shelf --bundle-id org.example.shelf
cd Shelf
```

The app has its Rust crate (`rust/`), its SwiftUI screens (`Sources/AppHost/`), tests, a starter UI test and `forge.toml`. It has no agent files. `README.md` in the app explains each folder.

### 3. Inspect it

```bash
forge inspect            # Swift files and their imports, accessibility identifiers, UI tests, generated files
forge config show        # bundle id, version, permissions, entitlements, signing
```

`forge inspect --json` prints the same facts for scripts. It reports what it can point at: it doesn't guess which files are "views", and it says that it finds only identifiers written as literals.

### 4. Change the Rust business logic

Edit `rust/src/lib.rs`. For example, add a rule and its test:

```rust
/// Minutes to read `text` at 200 words a minute, rounded up; at least 1.
#[uniffi::export]
pub fn reading_minutes(text: String) -> u32 {
    let words = text.split_whitespace().count() as u32;
    words.div_ceil(200).max(1)
}
```

```bash
cargo test --manifest-path rust/Cargo.toml   # seconds, no SDK or phone
forge bindings generate                      # the Swift API now has readingMinutes(text:)
```

### 5. Change the SwiftUI presentation

In `Sources/AppHost/NoteListView.swift`, show what Rust works out (Swift adds no rule of its own):

```swift
Text("\(readingMinutes(text: note.text)) min read")
```

Add a test the Linux twin can run (`Tests/AppHostTests/ReadingTimeTests.swift`), and name it in `requirements.toml` so the evidence can be reviewed later:

```toml
[[requirement]]
id = "RT-1"
statement = "Each note in the list says how long it takes to read, rounded up to whole minutes."
twin = ["ReadingTimeTests.testReadingTimeRoundsUp"]
```

### 6. Run the tests and checks

```bash
forge test                # the Rust crate (and XCTest on macOS)
forge check swift         # the model, bindings and tests with plain Swift on Linux; writes twin evidence
forge ui focus --changed Sources/AppHost/NoteListView.swift   # which controls and UI tests a change touches (no phone)
```

`forge check swift` doesn't compile SwiftUI views: only the iOS SDK does (step 7).

### 7. Build for iOS

```bash
forge build ios           # Rust for iOS, the bindings, then xtool builds the Swift (needs the Darwin SDK)
```

### 8. Sign and install

```bash
forge run ios             # builds, then signs with your Apple ID and installs on the plugged-in iPhone
```

It compiles before it looks for the phone, so you can start it before you fetch the iPhone: with no phone connected it stops with `Built …, ready to install`, and running it again with the phone plugged in rebuilds only what changed and installs ([troubleshooting.md](troubleshooting.md#build-without-the-phone-install-when-its-there)).

The first install needs **Trust** on the phone (Settings > General > VPN & Device Management). A free Apple team allows 3 sideloaded apps and re-signing every 7 days ([ios-signing.md](ios-signing.md)).

### 9. Launch and inspect the app

```bash
forge ui setup                       # once per phone restart: the UI testing runner
forge ui launch                      # opens the app (--appearance dark, --largest-text)
forge ui screenshot shelf.png
forge ui tree                        # the accessibility tree
forge ui audit                       # unlabelled controls, small tap targets, clipped text
forge ui tap --id notes.add          # one action at a time, no test file:
forge ui type --id compose.text "Hi" #   tap, type, clear, scroll, swipe, long-press,
forge ui key dismiss                 #   double-tap, drag, pinch, pick, key, expect...
forge ui expect --id compose.save --enabled true
forge ui alert accept --text Camera  # a permission prompt; also press home, open-url, back
forge ui tap --at 200,640            # a point, for a control with no identifier (terminal only)
```

### 10. Read logs and diagnose

```bash
forge device logs --process ShelfXtool --for 20s
forge device crashes --pull ./crashes      # symbolicated against your build
forge crash ./crashes/<report>.ips
forge diagnose                             # last run, the phone, the install, the newest crash, the app's log
```

Every `forge` command on the app also appends a line to `.forge/history.jsonl` (the command, when, whether it passed, its log), so you can see later what ran.

### 11. Run a UI test and explore

```bash
forge ui run uitests/smoke.toml      # writes a device evidence bundle to .forge/runs/
forge ui explore --real-data         # look-only walk of the screens; or on test data, see forge-explore.md
```

### 12. Review the evidence

```bash
forge verify                         # each requirement: Linux twin, iPhone and person, and whether current
forge verify --run 2026-11-09T10-00-00     # a record of everything since then, with report.md
forge verify --accept RT-1 --note "Checked on my phone: 3 min read on a long note"
```

`--accept` is yours alone: it needs a terminal, asks for the id to be typed back, and is refused inside `forge mcp` ([evidence.md](evidence.md)).

## What is checked where

| Step | `check-human-workflow.sh` on Linux (CI) | Needs | Seen on an iPhone |
|---|---|---|---|
| 1 Install, `forge doctor` | Yes | | n/a |
| 2 `forge init` (default starter), no agent files | Yes | | n/a |
| 3 `forge inspect`, `forge config show` | Yes | | n/a |
| 4 Rust rule, `cargo test`, `forge bindings generate` | Yes | | n/a |
| 5 Swift model test, `requirements.toml` | Yes (the view edit is applied, not compiled) | | n/a |
| 6 `forge test`, `forge check swift`, `forge ui focus` | Yes (`check swift` in the `swift:6.2` image) | Swift or Docker | n/a |
| 7 `forge build ios` | No | xtool, the Darwin SDK | Yes, 2026-11-10 |
| 8 `forge run ios` | No | an iPhone, an Apple ID | Yes, 2026-11-10 |
| 9 `forge ui launch`, `screenshot`, `tree`, `audit` | No | an iPhone, `forge ui setup` | Yes, 2026-11-10 (`launch`, `screenshot`, `audit`); the row showed the reading time |
| 10 `device logs`, `device crashes`, `diagnose` | No | an iPhone | Partly, 2026-11-10: `diagnose` passed; a 10 s log capture was empty, so logs are unproven |
| 11 `forge ui run`, `forge ui explore` | No | an iPhone | `ui run` yes, 2026-11-10 (smoke test 9/9); `explore` not run |
| 12 `forge verify`, `--run` | Yes (twin column) | | n/a |
| 12 `forge verify --accept` | No: a person types it | a person | n/a |

With xtool and a phone, `FORGE_HUMAN_DEVICE=1 scripts/check-human-workflow.sh` runs steps 7–11 too. It ran on an iPhone 15 Pro Max (iOS 26.3.1) on 2026-11-10, and a person checked the reading time on the phone. Without `FORGE_HUMAN_DEVICE=1`, the script lists those steps as manual.

The script also checks that nothing agent-shaped took part: no `AGENTS.md`, `CLAUDE.md` or `.mcp.json` in the app, no `forge mcp` journal, and no MCP call in the command history.

## Optional agent integration

| Human-facing (the framework) | Optional agent layer |
|---|---|
| The `forge` CLI, `--json` results (`forge.result/1`) | `forge mcp`: MCP tools that run those commands and return their results |
| `forge inspect`, `forge ui launch`, `forge diagnose`, `forge verify` | `forge_inspect_app`, `forge_launch_app`, `forge_diagnose` (adds the session's last failure), `forge_verify` |
| `forge ui tap`, `type`, `scroll`, `swipe`, `long-press`, `double-tap`, `drag`, `pinch`, `rotate`, `pick`, `key`, `press`, `open-url`, `alert`, `back`, `app-state`, `reset-permission`, `orientation`, `settle`, `expect` | `forge_ui_act` (one tool, the same actions; coordinates, `lock` and `unlock` stay in the terminal) |
| The app's `README.md` and Forge's `docs/` | `forge agent setup`: `AGENTS.md` (the app's easy-to-miss rules and the commands), `CLAUDE.md`, `.mcp.json` (`--apple-docs` adds a hosted documentation server) |
| `forge init` | `forge init --agent` (both at once); `forge_create_app` passes it |

The safety rules are the same whoever runs a command: exploration on real data is read-only ([forge-explore.md](forge-explore.md#test-data)), only a person accepts a requirement, and `forge mcp` installs nothing unless the person started it with `--allow install`.
