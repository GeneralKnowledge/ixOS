# Evidence: proving an app does what it should

Status: format version 1, 2026-11-07. The Linux half (the headless twin) is built and runs in CI for Pocket Library. The device half is a proposal for `forge ui run` ([#61](https://github.com/GeneralKnowledge/ixOS/pull/61), merged) once it has had its first phone run. `forge verify` is not built.

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
  screens/             device runs: <step>-<name>.png            (proposed)
  trees/               device runs: the accessibility tree at each check and failure, JSON  (proposed)
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
  "build": { "commit": "b8183cd…", "dirty": false },
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
  "artifacts": [ { "kind": "log", "path": "swift-test.log" } ]
}
```

| Field | Means |
|---|---|
| `kind` | `linux-twin` or `device`. Decides which verdict a run may set |
| `ok` | No test failed and no requirement is `failed` or `missing` |
| `where.device` | `null` on Linux. On a device run: `{ "udid", "name", "ios" }` |
| `build.commit`, `build.dirty` | What the run was built from. A requirement proved by a dirty build, or an older commit than the one under review, is stale evidence |
| `tests.cases` | Every XCTest case the run reported, by `Class.method` |
| `requirements[].notes` | Why a verdict isn't `passed`, in words |
| `artifacts` | Files in the bundle, with a `kind`: `log`, and for device runs `screenshot`, `ui_tree`, `device_log`, `crash`, `audit` |

Readers must ignore fields they don't know. A change that removes or changes the meaning of a field raises the version to `forge.evidence/2`.

## Device runs (proposed, for `forge ui run`)

Nothing below is built. It is written down so that #61 and the twin produce one format, and it asked nothing of #61 before it merged.

Once #61 (merged) has run on the phone, `forge ui run <scenario>` would:

1. Default its `--out` directory to `.forge/runs/<id>-ui-run/`. The files it already writes there stay as they are and are listed under `artifacts`: the script's named screenshots, and at a failed step `failure-stepN.png`, `failure-stepN-tree.json` and `failure-stepN-tree.txt`. (The `screens/` and `trees/` folders in the layout above are for later runs that take a tree at every check.)
2. Write `evidence.json` with `kind = "device"`, `where.device` from the phone, and `build.installed_as` (xtool's renamed bundle id). Instead of `tests`, a device run has `steps`: one entry per scenario step, `{ "n", "action", "passed", "error", "screenshot", "tree" }`, taken from what `forge ui run --json` already returns (`passed_steps`, `failed_step` with `on_screen` and `tree`, `screenshots`).
3. Look up which requirements list the scenario under `device`, and set their `device` verdict (`passed` or `failed`). The twin verdict is left out of a device bundle.

The scenario TOML stays as #61 has it. The link between a requirement and a scenario lives only in `requirements.toml`, so neither format has to change when the other does.

## Next steps (not built)

- **`forge verify --project <app>`**: reads the newest bundle of each kind and prints one table: requirement, twin verdict, device verdict, which build each came from, and whether that is the current commit. It is the agent's stop condition ("every requirement has current evidence") and the page a person reviews.
- **Linux repro** (audit A8): pull the app's data from the phone (`house_arrest`) into a twin test, so a bug seen on the device is reproduced on Linux in seconds.
- **MCP**: `forge_verify` and a `forge://runs/{id}` resource return these files as they are ([forge-mcp.md](forge-mcp.md)).
- **More apps**: neither of the other two apps with model tests builds its twin on Linux yet (checked 2026-11-07 with `forge check swift`). Device Lab's `Format.swift` uses `Measurement.formatted`, which Linux Foundation lacks. Chat's `LocalChatNotifier.swift` imports only Foundation but uses `ChatRoute` and the notification adapters, which live in Apple-only files. Each is a small move like Pocket Library's, then a `requirements.toml`.

Where the code lives: requirement parsing, XCTest parsing and bundle writing are in `crates/forge-cli/src/evidence.rs`, called by `forge check swift`. When `forge ui run` writes bundles too, the shared types move into `forge-uitest` (or a small crate both use), following the extraction rule in [architecture.md](architecture.md#framework-principles): extract on the second real use, not before.
