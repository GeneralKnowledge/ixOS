# Evidence: proving an app does what it should

Status: format version 1, 2026-11-08. The Linux half (the headless twin) is built and runs in CI for Pocket Library. `forge ui run` writes device bundles and `forge verify` reads both kinds; both are tested on Linux, and the first device bundle was written on an iPhone on 2026-11-08 (Pocket Library PL-10, `uitests/m0.toml`, 11 of 11 steps on iOS 26).

"The build passed" says the code compiles. It does not say the app works. This page defines how a run of Forge records what it proved, so that an agent can show that a feature works instead of saying so, and a person can check the claim in a minute. It is the capability ledger's rule ("an API is not proof") applied to apps, from discoveries D4 and D5 in the [agent capability audit](research/forge-agent-capability-audit.md).

## Three kinds of proof

| Kind | What runs | Where | What it proves | Sets |
|---|---|---|---|---|
| **Twin** | The app's real Rust core and its portable Swift view model, driven by XCTests | Linux, no SDK, no phone, in seconds (`forge check swift`) | The behaviour: rules, state, persistence, errors, cancellation, events across the Swift–Rust boundary | The requirement's `twin` verdict |
| **Device** | The installed app, driven by a UI scenario through its accessibility identifiers | A physical iPhone (`forge ui run`, #61) | That the screens reach that behaviour on the phone: the control exists, is labelled, responds, and the state shows | The requirement's `device` verdict |
| **Person** | Someone looks at the evidence and the phone | | That it is right | The capability ledger's "iPhone: Yes". Only a person sets it |

The kinds are never mixed up. A twin pass leaves the device verdict `unverified`. A device pass is still not "iPhone: Yes" in the ledger until a person reports it ([AGENTS.md](../AGENTS.md) rule 7).

Most of a feature's behaviour belongs in the twin, because it is cheap: Pocket Library's seven model tests run against the real Rust library in 0.05 seconds, and the whole check takes about 10 seconds with a warm Swift image. The device run then only has to show that the screen is wired to that behaviour, which keeps UI scenarios short and the phone free.

## The headless twin

The twin is what `forge check swift` already builds: the app's Rust crate for this computer, the UniFFI bindings, and every Swift file in `Sources/AppHost` and `Tests/AppHostTests` that imports nothing Apple-only, in a plain SwiftPM package. It now also reads the XCTest results and judges the app's requirements against them.

For an app to have a useful twin, its view model must build without SwiftUI or UIKit ([ios-ui.md](ios-ui.md)). Getting Pocket Library's twin running found two things that only built on Apple platforms, both fixed here:

- `LibraryModel` used `ErrorText`, which lived in `Presentation.swift` next to SwiftUI helpers. It is now in its own Foundation-only `ErrorText.swift`.
- `URL.startAccessingSecurityScopedResource()` doesn't exist in Linux Foundation. It is now inside `#if canImport(Darwin)`; on Linux there is no sandbox to leave.

Neither broke the iPhone build; both stopped the model from being tested anywhere except macOS CI.

## Requirements

An app lists what it must do in `requirements.toml`, next to `forge.toml`. Each requirement has an id, one sentence a person can check, and the tests or scenarios that prove it:

```toml
[[requirement]]
id = "PL-1"
statement = "Books and reading progress survive closing and reopening the app."
twin = ["LibraryModelTests.testBooksAndProgressSurviveARelaunch"]
device = ["uitests/relaunch.toml"]   # a #61 scenario; Pocket Library has uitests/m0.toml
```

- `twin` names XCTests as `Class.method`. The class must live in a file of the same name (the convention every app here follows), which is how Forge tells "this test needs Apple's frameworks" from "this test no longer exists".
- `device` names UI scenario files, relative to the app. The scenario format is #61's; it doesn't change.
- Every requirement needs at least one of the two. Ids are unique. Unknown keys are refused, so a typo can't quietly drop the proof.

Pocket Library's are in [`examples/pocket-library/requirements.toml`](../examples/pocket-library/requirements.toml).

## Verdicts

| Verdict | Means | Fails the command |
|---|---|---|
| `passed` | Every named test ran and passed | No |
| `failed` | A named test ran and failed | Yes (stage `test`, exit 14) |
| `not_run` | A named test is in a file this run couldn't build, because it needs Apple's frameworks. This run says nothing about it; macOS CI or the phone must | No |
| `missing` | A named test is in no file at all: renamed or deleted | Yes (stage `test`, exit 14) |
| `no_twin` | Only a device scenario is named | No |
| `unverified` | The device verdict, until a device run records one | No |

## The evidence bundle

Every run that executes tests writes a directory:

```
<app>/.forge/runs/<id>-<command>/
  evidence.json        what was proved (below)
  swift-test.log       the raw output it was judged from
  <name>.png           device runs: each screenshot step's picture
  <name>.tree.json     device runs: the accessibility tree saved with it
  visual/<name>.diff.png  device runs: where a screenshot differs from its baseline
  device.log           device runs: the app's log lines for the run, from Forge Device     (proposed)
```

`<id>` is the UTC start time (`2026-11-07T18-48-25`), the same stamp as `.forge/logs/`. `.forge/` is git-ignored; CI keeps Pocket Library's bundle as the `pocket-library-evidence` artifact on every run of the Rust workflow.

The command's `--json` result (`forge.result/1`, [forge-mcp.md](forge-mcp.md#the-result-contract)) stays the short answer and points at the bundle: `data.evidence` is the path of `evidence.json`, `data.requirements` lists each id with its verdict, and `data.results` counts the tests. An agent reads the result; a person, or `forge verify`, reads the bundle.

### `evidence.json`, version 1

```json
{
  "schema": "forge.evidence/1",
  "id": "2026-11-07T18-48-25",
  "command": "check swift",
  "kind": "linux-twin",
  "ok": true,
  "duration_ms": 5323,
  "where": { "host": "x86_64-linux", "device": null },
  "build": {
    "commit": "b8183cd…", "dirty": false, "tree": "4b825dc…",
    "forge": "0.1.0", "swift": "Swift version 6.2.3 (swift-6.2.3-RELEASE)"
  },
  "tests": {
    "passed": 7, "failed": 0,
    "cases": [ { "name": "LibraryModelTests.testBooksAndProgressSurviveARelaunch", "passed": true, "seconds": 0.014 } ]
  },
  "requirements": [
    {
      "id": "PL-1",
      "statement": "Books and reading progress survive closing and reopening the app.",
      "twin": { "verdict": "passed", "tests": ["LibraryModelTests.testBooksAndProgressSurviveARelaunch"], "notes": [] },
      "device": { "verdict": "unverified", "scenarios": [] }
    }
  ],
  "artifacts": [ { "kind": "log", "path": "swift-test.log", "sha256": "9f2c…", "bytes": 4810 } ]
}
```

| Field | Means |
|---|---|
| `kind` | `linux-twin` or `device`. Decides which verdict a run may set |
| `ok` | No test failed and no requirement is `failed` or `missing` |
| `where.device` | `null` on Linux. On a device run: `{ "udid", "name", "ios" }` |
| `build.commit`, `build.dirty` | The commit the run was built from, and whether the app folder had uncommitted changes |
| `build.tree` | A git tree hash of the app folder's files as they were on disk, committed or not (git-ignored files such as `.forge/` left out). `forge verify` compares it with the folder now: same files, same hash. Evidence for other files is stale |
| `build.forge`, `build.swift` | The forge version, and the Swift that ran the twin tests (`docker <image>` with `FORGE_SWIFT_DOCKER`) |
| `build.installed_as` | Device runs: the bundle id the app is installed as (xtool renames it `XTL-<team>.<id>`) |
| `tests.cases` | Every XCTest case the run reported, by `Class.method` |
| `requirements[].notes` | Why a verdict isn't `passed`, in words |
| `artifacts` | Files in the bundle, with a `kind` (`log`; for device runs `screenshot`, `ui_tree` and `visual_diff`, later `device_log`, `crash`, `audit`), its `sha256` and its size in `bytes` |
| `visual` | Device runs: each screenshot compared with its approved baseline: `match`, `new`, `changed` or `recorded`, the changed areas and elements ([visual-regression.md](visual-regression.md)) |

Readers must ignore fields they don't know. A change that removes or changes the meaning of a field raises the version to `forge.evidence/2`.

## Device runs (`forge ui run`)

When the scenario file is inside an app (a folder above it has `forge.toml`), `forge ui run <scenario>`:

1. Writes to `<app>/.forge/runs/<id>-ui-run/` unless `--out` names another folder. Outside an app it still defaults to `forge-ui-out` and writes no bundle. The files it writes there are unchanged: the script's named screenshots, and at a failed step `failure-stepN.png`, `failure-stepN-tree.json` and `failure-stepN-tree.txt`.
2. Writes `evidence.json` there with `kind = "device"`, `where.device` (`udid`, `ios_major`), `build` (read before the run, plus `installed_as`), `scenario` (relative to the app) and, instead of `tests`, `steps`: `{ "passed": N, "failed": null | { "step", "action", "error", "on_screen", "tree" } }`. Screenshots and the failure tree are listed under `artifacts`.
3. Lists only the requirements whose `device` names this scenario, each with `device.verdict` `passed` or `failed`. Twin verdicts are left out of a device bundle.
4. Compares each screenshot with the scenario's baseline (`uitests/baselines/<test>/`) and records the result under `visual`. A changed screen, or one with a new accessibility finding, fails the run and its requirements like a failed step, with a note naming the screen ([visual-regression.md](visual-regression.md)). `--visual off` skips this.

`forge_run_ui_test` in `forge mcp` writes to the same place, so an agent's UI tests count as evidence. `forge_ui_act`'s one-step files don't: they live in `.forge/mcp/` and no requirement names them.

The scenario TOML is unchanged. The link between a requirement and a scenario lives only in `requirements.toml`, so neither format has to change when the other does.

## `forge verify`

```
forge verify --project examples/pocket-library            # twin and iPhone evidence required
forge verify --project examples/pocket-library --twin-only # a session without the phone
```

It runs nothing. It reads `requirements.toml` and every bundle in `<app>/.forge/runs/`, and prints one row per requirement: the newest twin verdict (from the newest `linux-twin` bundle that judged it), the newest device verdict (from the newest `device` bundle that lists it), and `proven` or `open`. A verdict recorded for different files than the app has now (another `build.tree`) is shown as `passed (stale)` and doesn't count; `?` means it can't be told (not a git checkout, or a bundle from before `tree` was recorded).

A requirement is `proven` when every proof it names passed for the current files. With `--twin-only` the device column is shown but not required, and a requirement nothing on Linux can prove (only a device scenario, or a twin test that is `not_run` because it needs Apple's frameworks, like Pocket Library's PL-9) is `not here` rather than open. The command fails (stage `test`, exit 14) while any requirement is open; `not here` doesn't fail it but is listed. Its `--json` `data.requirements` has each row with its `status`. It is the agent's stop condition ("every requirement has current evidence") and the page a person reviews; in `forge mcp` it is `forge_verify`.

## Next steps (not built)

- **Linux repro** (audit A8): pull the app's data from the phone (`house_arrest`) into a twin test, so a bug seen on the device is reproduced on Linux in seconds.
- **MCP resource**: a `forge://runs/{id}` resource returning a bundle's files as they are ([forge-mcp.md](forge-mcp.md)).
- **More apps**: neither of the other two apps with model tests builds its twin on Linux yet (checked 2026-11-07 with `forge check swift`). Device Lab's `Format.swift` uses `Measurement.formatted`, which Linux Foundation lacks. Chat's `LocalChatNotifier.swift` imports only Foundation but uses `ChatRoute` and the notification adapters, which live in Apple-only files. Each is a small move like Pocket Library's, then a `requirements.toml`.
- **A report**: `verify`'s table is the summary today. An HTML or Markdown export of a bundle (screenshots inline) is worth building once people review device runs regularly.

Where the code lives: requirement parsing, XCTest parsing, provenance and bundle writing are in `crates/forge-cli/src/evidence.rs`, called by `forge check swift` and `forge ui run` (both in forge-cli, so nothing needed extracting). `forge verify` is `crates/forge-cli/src/verify.rs`.
