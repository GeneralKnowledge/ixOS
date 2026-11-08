# Forge M6: an autonomous, evidence-driven dev loop

Status: plan, 2026-11-08, decisions taken the same day. M6.1 is built and Linux-tested; its phone checks are pending. It was written after reading `main` at 301359f (#106–#120 merged) and the open Good Dog GD1 PR (#121).

The brief (Reuben, 2026-11-08): an AI agent takes a real task on an existing Rust and SwiftUI app, changes it, builds and installs it on the iPhone, inspects and tests it, finds a real problem, diagnoses and fixes it, proves the fix on the phone, and leaves a truthful record of all of it. Good Dog is the demonstration app. The measure is how much of the real development process an agent completes reliably, safely and on its own, not how many tools Forge has.

## The short answer

- **Most of the loop already exists and has already run unattended twice.** Tipster (M0, 2026-11-07) and Murmur (M2, 2026-11-08) were built, installed, inspected, tested and fixed by an agent through `forge mcp`, and both found real problems from device evidence (keyboard covering a total, a password sheet covering the app, 44×18 pt tap targets, taps doing nothing in pushed screens) and fixed them ([forge-mcp.md](forge-mcp.md#m2-run-murmur)). M6 is not starting from zero.
- **What those runs didn't have is what landed since:** exploration (#109, #119), visual regression (#110), crash symbolication (#111, #114), the any-app policy (#107), unlabelled-control locators (#120) and `forge verify`. They work as separate commands. Several of them are not reachable from `forge mcp`, none of them feeds the others, and nothing joins their output into one record of a run.
- **So M6 is mostly wiring, plus four real additions:** exploration aimed at what changed, failure correlation (action, screen, logs, crash, changed files), appearance variants (dark mode is not settable today), and a run record that says what was changed, what ran, what didn't, and what a person accepted.
- **Good Dog isn't on the phone yet.** GD1 is PR #121 (Linux-checked; SwiftUI only syntax-checked so far). M6's demonstration starts after GD1's own phone run, and its task should come from Good Dog's next milestone rather than be invented for M6.
- **Good Dog's pain-points log is part of M6** (Reuben, 2026-11-08): loop-related items go into the milestones that need them, and the rest form a cleanup milestone, M6.P.
- **Plan:** seven milestones. A baseline run with today's tools comes first, so the gap list is measured rather than guessed. Infrastructure is proven on Pocket Library and Device Lab, which are already on the phone, so it doesn't wait for Good Dog.

## What exists today, verified

"Linux" means automated tests that ran in CI. "iPhone" means a run on Cati whose result someone saw. "Person" means Reuben confirmed it himself. A merged PR is none of these.

| Capability | Where | Linux | iPhone | Person |
|---|---|---|---|---|
| Rust logic, UniFFI, `forge check swift` twin, `requirements.toml` | `forge-cli` `evidence.rs`, `swift_check.rs` | Yes | n/a | n/a |
| Build, sign, install (`forge build ios`, `forge run ios --json`), stall detection after `[Installing] 100%` | `ios_builder.rs`, xtool fork | Yes (recorded output) | Yes (Tipster, Murmur, Pocket Library) | Yes |
| Launch, tree, screenshot, tap and type by identifier (`forge ui run`, WDA) | `forge-uitest` | Yes | Yes (Pocket Library m0 11/11) | Yes |
| Persistent UI session (`forge ui start`) | `ui_session.rs`, `session.rs` | Yes | Partial: worked, not confirmed by a person | No |
| Unlabelled-control locators (`kind`, `index`, `near`) | `forge-uitest` (#120) | Yes | Partial: `kind` only; needs the rerun | No |
| Exploration (`forge ui explore`), test-data mode, keyboard cut | `explore.rs` (#109, #119) | Yes | Partial: two walks, the test-data and keyboard fixes not rerun | No |
| Any-app policy and commit gate | `policy.rs` (#107) | Yes | Yes | Yes (watched) |
| Visual regression (baselines, tree diff, `forge ui accept`) | `visual.rs`, `baseline.rs` (#110) | Yes | Partial: match and a real change reported, not confirmed by a person | No |
| Crash pull and symbolication (Swift and Rust frames) | `forge-crash`, `forge device crashes --pull` (#111, #114) | Yes | Yes (Device Lab) | Yes (Reuben tapped both crashes) |
| Live device logs (`forge device logs --for`) | `forge-device` | Yes | Yes | n/a |
| Evidence bundles `forge.evidence/1` (`linux-twin`, `device`) and `forge verify` | `evidence.rs`, `verify.rs` | Yes | Device bundles from `forge ui run`: yes | No |
| `forge mcp`: 18 tools, run journal `forge.journal/1`, install allowance | `forge-mcp` | Yes | Yes (Tipster, Murmur) | Yes (Tipster, Murmur) |
| Staged agent runs (`claude -p --resume`, phone stages of 15–20 min) | `scripts/agent-runs/murmur/` | n/a | Yes (Murmur 7/7 stages) | Yes |

Two docs are behind the code: [dev-loop.md](dev-loop.md)'s status table (last checked 2026-11-07, says UI tests don't pass on the phone) and `forge_diagnose`'s own description. M6.4 refreshes them.

## Gaps against the acceptance criteria

| # | Criterion | Today | Gap |
|---|---|---|---|
| 1–3 | Task, inspect, plan, implement in Rust and SwiftUI | Works (Murmur's change stages). `forge_inspect_app`, `forge://conventions`, app `AGENTS.md` | Good Dog must be on the phone first (GD1). Nothing for M6 to build |
| 4 | Linux checks | `forge_check` (Rust tests, twin) | Views are only type-checked by xtool or macOS CI. The agent runs on the laptop, where `forge_build_ios` type-checks them, so this isn't blocking. `forge_check` reports no test counts (seen in Murmur) |
| 5 | Build and install | `forge_build_ios`, `forge_install_device` | Free team: 3 slots and no way to see them (`forge device apps` not built). Good Dog needs a slot |
| 6 | Launch and inspect | `forge_launch_app` (with `text_size`), `forge_inspect_ui`, `forge_screenshot` | Dark mode can't be set. No app-state view (`forge_app_state` waits on the devtools pairing, never run on the phone) |
| 7 | Scenarios and exploration | `forge_run_ui_test`; `forge ui explore` from the shell only | **No `forge_ui_explore` tool.** Exploration walks everything in shuffled order, with no idea what changed. No scrolling below the fold. A repro file isn't linked to a requirement. Test data is emptied on every launch, so a device test can't check that data survives a relaunch (Good Dog pain point 13) |
| 8 | Visual and accessibility | Baselines per scenario; tree audit (no label, duplicate id, missing id) | No baselines per appearance or text size. Explore screenshots aren't compared with anything. The audit has no small-target or truncation checks. `forge_ui_audit` not built |
| 9–10 | Discover, diagnose, repair | Murmur did this by reading screenshots and trees | **`forge_diagnose` is "the last failure and its log tail".** A crash in a UI run or a walk doesn't pull the crash report; logs aren't captured around the failing action; nothing names the changed files |
| 11 | Rerun and verify on the device | `forge_run_ui_test` again, `forge_verify` | Explore repros can't be promoted into the app's tests. `forge verify` only knows twin and device verdicts |
| 12–13 | One evidence record; pass, fail, skip, human kept apart | Per-command bundles plus the MCP journal | **No run-level record.** Explore writes `explore.json`, not `forge.evidence/1`. Device bundles have no logs or crashes (`device.log` is listed as proposed). There is no `human` kind, so a person's acceptance can't be recorded next to the machine verdicts. Nothing lists the checks that weren't run |
| 14 | Repeatable | The Murmur run script | Run scripts are per-app copies. No generic "M6 run" with a task file |

## Principles for the plan

- **No new orchestrator.** The agent is the orchestrator; it already chooses the next tool. Forge's job is to give each tool a structured answer that makes the next choice obvious (what failed, where, what changed, what's untested), and to refuse what's unsafe. A fixed pipeline command would cut against the brief.
- **Extend the bundle, don't invent a format.** Explore and the run record become `forge.evidence/1` bundles with new optional fields. Readers already ignore fields they don't know, so this needs no version change.
- **Few new tools.** Five MCP tools at most across M6 (explore, appearance, audit, record acceptance, run report), each over an existing CLI command or a flag on one.
- **Safety stays where it is.** Exploration keeps its floor (external words, destructive words, secure fields, keyboard, system services) and test-data mode. M6 adds no allowance. Installs still need `--allow install`. Destructive actions still need `--allow-destructive` on test data, which an MCP agent can't grant itself.

## Milestones

Infrastructure (M6.1–M6.4) and the Good Dog work (M6.0, M6.5) are separate PRs. Each infrastructure milestone is proven on an app already on the phone, so none of it waits for Good Dog.

### M6.0 Baseline run with today's tools

**What:** after GD1's own phone run (owned by the Good Dog thread), one staged agent session on Good Dog with a small real task, using only what's on `main`. The Murmur pattern: build stages without the phone, short phone stages, one resumed session. The agent is told to use explore from the shell and to note every time it had to guess.

**Why first:** it shows the demonstration early and turns the gap table above into measured friction. Anything the run doesn't hit drops down the list.

**Relies on:** `forge mcp`, `forge ui explore`, `scripts/agent-runs/murmur/` as the template.

**Tested by:** the run itself, with the journal and bundles kept.

**iPhone:** yes, about 30 minutes in one sitting, after GD1 is installed.

**Output:** a short results section in this page; no Forge code.

### M6.1 Diagnostics in the loop

**What:**

- `forge_ui_explore`: an MCP tool over `forge --json ui explore`, always on test data, with the same refusals as the CLI.
- Explore writes a `forge.evidence/1` bundle (`kind = "device"`, `command = "ui explore"`), with each issue linked to its step, screen and screenshot, and its repro file listed as an artifact.
- When a UI run step or an explore action closes the app, Forge pulls the new crash report for that app, symbolicates it (`forge-crash`), and attaches it to that step: `kind`, `culprit` file and line, and `build.status`.
- Device logs for the run window: `forge ui run` and `explore` capture the app's own log lines while they run and store `device.log` (the artifact already proposed in [evidence.md](evidence.md)). Only the app's process, and app output is never copied into the journal.
- `forge_diagnose` correlates: the failed step or action, the screen id and its tree, log lines from a few seconds before it, the crash summary, and `git diff --stat` of the app since the bundle's `build.commit`.
- `forge device apps`: installed developer apps with their profile expiry, so the agent knows whether an install will need a slot before it tries (Good Dog pain point 15). Removing an app stays a person's step.

**Relies on:** `forge-crash`, `forge device logs`, `forge device crashes`, `explore.rs` `app_closed`, `evidence.rs`.

**Tested by:** Linux tests with recorded WDA trees and `.ips` fixtures (the existing fixture dSYM). A fake driver that "crashes" after a given action checks the correlation end to end.

**iPhone:** Device Lab's Crash Test buttons in a UI scenario: the bundle must name the step, `crash_test.rs:24` and the log lines before it. Then one explore walk on Pocket Library test data writing a bundle. Proves correlation on real reports and real timing.

**Built (2026-11-08, Linux-tested only; not yet run on the iPhone):**

| Piece | Where |
|---|---|
| `forge_ui_explore` (no destructive taps, no agent-typed text; `data.out` is the run folder) | `crates/forge-mcp/src/tools.rs` |
| Explore bundle: `evidence.json` with `command = "ui explore"`, `explore`, `watch`, and artifacts `explore_report`, `screenshot`, `ui_tree`, `repro`, `device_log`, `crash`. A walk that tried no control fails | `evidence.rs` `write_explore_bundle`, `ui.rs` |
| The watch: the app's own log lines during `forge ui run` and `forge ui explore` (`device.log`), and its new crash reports, pulled and symbolicated when a step fails or the app closes. The failed step gets `log_tail` and `crash`. `--no-watch` (one-step MCP actions use it) | `crates/forge-cli/src/watch.rs` |
| `forge_diagnose` `data.correlation`: layer, failed step, explore errors with replay files, crash with its app frame, log tail, files changed since the session started, and whether the crash is in one of them | `crates/forge-mcp/src/correlate.rs` |
| `forge device apps`: installed apps, profile team, lifetime and days left, free-team slots in use; `forge_device_status` adds them with a note when every slot is taken or a profile expired | `forge-device` (installation_proxy, misagent), `device.rs` |
| `forge run ios --wait-for-device MINUTES` builds Rust and Swift first, then waits for the phone; every run writes `<app>/.forge/status/run-ios.json` (phase, pid, heartbeat every 10 s, the error when it fails) | `commands.rs`, `status.rs` |
| `forge check swift`: `swiftc -parse` of the view files, Docker-not-running named | `swift_check.rs` |

What the phone has to show, in the next coordinated sitting: the syslog stream and WDA running together without either dropping; a Device Lab crash named in the failed step with `crash_test.rs:24`; `forge device apps` matching what Settings shows (including whether the WebDriverAgent runner holds a free slot); and the misagent profile read on iOS 26.3.1.

### M6.2 Exploration aimed at the change

**What:**

- `--focus`: start from the screens and controls the task touched. Forge reads the changed files (`git diff` since a commit) for accessibility identifiers and finds the screens holding them in earlier walks' maps. The walk goes there first, by the shortest known path.
- A coverage map kept between walks (`.forge/explore-map.json`: screens, controls tried, last result, missing labels). New and changed screens, untried controls and controls that failed before come first. Screens unchanged since the last clean walk are skipped unless asked.
- Scroll to controls below the fold (named in forge-explore.md as not built).
- `forge ui explore --promote <issue>` copies a repro into the app's `uitests/` with a name the agent gives it. The agent then names it in `requirements.toml`, which keeps requirements in the agent's hands.
- A launch flag that keeps test data across a relaunch (`-ForgeUITestKeepData`), so a device test can check persistence. This is the app-template helper from Good Dog pain point 5, and the second use (Pocket Library and Good Dog) justifies it.

**Relies on:** the explorer's screen fingerprint and replay, `forge ui run`.

**Tested by:** Linux tests on recorded trees: same seed and map give the same walk, a changed identifier puts its screen first, and an unchanged screen is skipped.

**iPhone:** Pocket Library: one full walk to build the map, then a change to one screen, then `--focus` reaches that screen in its first few actions and skips the rest. This is also the determinism check still owed from #109 and #119. Proves focus on real trees.

### M6.3 Appearance and accessibility checks

**What:**

- Dark and light at launch: a `-ForgeAppearance dark|light` launch argument that the template's app root honours with `.preferredColorScheme`. This changes app code, so it goes in the template and in each app. `forge_launch_app` gets `appearance` next to `text_size`.
- Baselines per variant: `<name>@<W>x<H>[-dark][-xxxl].png`, so a scenario can be run in four variants without the baselines colliding.
- Two more tree audit findings: tap targets under 44×44 pt (Murmur's agent found these by eye) and text clipped at the largest size (a label's frame narrower than its neighbour's or than the screen, measured from the tree, not pixels).
- `forge_ui_audit`: the current screen's audit as a tool (planned in forge-mcp.md since M0).
- Explore compares each screen with its last clean walk's screenshot and reports `changed` as information, not an issue. The brief is right that a difference is a change, not a defect, so the agent decides.

**Relies on:** `visual.rs`, `baseline.rs`, `tree.rs` audit, `forge_launch_app`'s `text_size`.

**Tested by:** Linux tests with recorded trees and PNG pairs.

**iPhone:** Pocket Library in dark mode at the largest text size: screenshots differ from light mode as expected, and the new audit findings are checked against what a person sees. A person confirms which findings are real.

### M6.4 The run record

**What:**

- `forge verify --run <start>` reads every bundle and the MCP journal since a start mark and writes one `forge.evidence/1` bundle of `kind = "run"`. It holds: app and repository, the commits made during the run and the dirty state at the end, builds and installs (from the journal), device and OS, scenarios and walks with their verdicts, screenshots and audit findings, crashes and logs, the failures and the commits that followed them (the repairs), and a `not_run` list with a reason for each check that was expected and didn't run. It also writes `report.md`, which is what a person reads.
- A `human` verdict: `forge verify --accept GD-3 --note "…"` records a person's acceptance as its own bundle (`kind = "human"`), refused when stdin isn't a terminal and absent from MCP, so no agent can record it. `forge verify` shows twin, device and person columns.
- `forge_verify` gains `run: true` for the report.
- Refresh dev-loop.md's status table and the M6 page's results.

**Relies on:** `verify.rs`, `evidence.rs`, the journal.

**Tested by:** Linux tests with recorded bundles and journals: a skipped check shows as not run, never as passed; a device verdict never fills the person column; a twin pass never fills the device column.

**iPhone:** none of its own; the M6.5 run produces the first real record.

### M6.5 The Good Dog demonstration

**What:** an agent session (staged, on the laptop, through Remote Control) takes a real Good Dog task and runs the loop with everything above, ending with a run record. Reuben then reviews the app on the phone and records his acceptance.

**The task** is chosen after GD1 is on the phone and agreed with the Good Dog thread, from Good Dog's own next milestone, so it advances GD instead of running beside it. Candidates that cross Rust, storage, SwiftUI, interaction and accessibility without a new capability spike:

- The plain-English progress section on My Dog with weekly history (the data half of GD3, before the charts).
- History export for a trainer as CSV through the share sheet (GD3).
- Reminder preferences stored in Rust and shown in settings, without the notification delivery (the Rust half of GD2).

Not a planted bug. If the run finds no genuine problem, the record says so and M6 isn't complete on that point; a second task follows.

**Passes when** the brief's acceptance criteria 1–14 are met, checked against the run record, with Reuben's acceptance recorded as `human`.

**iPhone:** yes, staged phone sittings of 15–20 minutes, planned with the iPhone session thread.

### M6.6 Repeat

**What:** a second run in a fresh agent session, on a different Good Dog task, from a generic run script (`scripts/agent-runs/m6/` with a task file, replacing the per-app copies). Ledger rows for the tools M6 touched are updated from what actually ran.

**Passes when** the second run completes with no Forge change between stages. Any Forge fix needed counts against repeatability and is recorded.

**iPhone:** yes, one more set of staged sittings.

### Silent failures

Reuben (2026-11-08): silent failures break unattended automation, so look for all of them. The trigger was Good Dog pain point 18: a laptop request was refused at 13:07, nobody noticed, and he waited 20 minutes at the phone believing it was building. The sweep covered the forge CLI and MCP server, device and WebDriverAgent calls, the build and CI scripts, and the workflows, looking for swallowed errors, ignored exit codes, timeouts that end as success, and empty results treated as passes.

| Found | Effect | Status |
|---|---|---|
| A request to the laptop that never starts looks the same as one that is building | A person waits at the phone for nothing (pain point 18) | Fixed: `forge run ios` writes `.forge/status/run-ios.json` at once and a heartbeat every 10 s. Whoever sends a laptop run (a person, an agent, Remote Control) reads it within a minute and says "building" only after seeing `phase: building` |
| `forge run ios` asks for the phone before anything is built | The phone waits on the build (pain point 17) | Fixed: `--wait-for-device` |
| `ci-macos.sh affected`: a base commit that can't be fetched made `git diff` fail inside a process substitution, which reads as "no files changed" | No Swift CI at all, reported as "Nothing to check" | Fixed: a failed diff checks every app |
| `ci-macos.sh`: an example not in its hand-kept list (Good Dog) fell into the shared-code branch | A change to Good Dog's Swift built the template and Device Lab, never Good Dog (pain point 11) | Fixed: apps with `forge.toml` and an Xcode project are discovered |
| `rust.yml` ran the headless twin for Pocket Library only | A new app's `requirements.toml` was never checked in CI (pain point 11) | Fixed: every `examples/*/requirements.toml` |
| `forge check swift`: `swift test` that exited 0 but whose results couldn't be parsed | Reported "passed" with nothing proved | Fixed: fails and says so |
| `forge check swift` with `FORGE_SWIFT_DOCKER` and Docker stopped | An unexplained Docker error (pain point 10) | Fixed: named, with the fix |
| A UI test file with no steps (for example a typo in `[[step]]`) | Passed, and set its requirements' device verdict to passed | Fixed: refused when loaded |
| `forge ui explore` that tried no control (all skipped or unreachable) | Exit 0, an empty walk read as clean | Fixed: fails |
| `forge run ios` stopping xtool after `[Installing] 100%` | `ok: true` with only `data.verified: false` | Fixed: a hint in the result as well |
| `forge mcp`: a child that printed `ok: true` and then exited non-zero | Trusted as a success | Fixed: becomes a failure naming the exit code |
| `forge run ios --wait-for-device` when `xtool devices` itself fails | Would have said "no iPhone connected" | Fixed: says the listing failed (usbmuxd) |
| A Rust enum case added without the Swift picker noticing (UniFFI enums aren't `CaseIterable`) | A missing option, no error (pain point 6) | Open, in M6.P |
| `forge_run_ui_test` with a screenshot that has no baseline yet | `ok: true`, verdict `new` | Kept: by design, and `data.visual` says `new`; M6.4's run record lists it under not proven |
| A screenshot step whose tree can't be saved | The visual check notes "no accessibility tree was saved" | Kept: reported, not silent |
| Remote Control requests refused by the session layer | Not visible to Forge at all | Process: the status file above is the check; the iPhone session thread and agent run scripts read it before saying anything is building |

Checks for each fix are Linux tests (`cargo test`), plus a real `forge check swift` on Pocket Library in Docker that syntax-checked 13 view files and caught a planted syntax error. The workflow changes run on this PR's CI.

### Good Dog pain points in M6

Reuben (2026-11-08) asked for the Good Dog build's pain-points log (`docs/good-dog-pain-points.md`, in #121) to be fixed as part of M6. Each item goes where it helps the loop, and the rest form one cleanup milestone. Framework extractions (`Store<T>`, test data, `LocalTime`) start only after #121 merges, because rule 1 extracts from merged code.

| Log item | Where in M6 | What |
|---|---|---|
| 1, 9: SwiftUI views not type-checked on Linux | M6.1 (cheap part, done), M6.P (the rest) | `forge check swift` syntax-checks the view files with `swiftc -parse`. Type-checking against SwiftUI stubs is a spike in M6.P; if it isn't feasible, the laptop xtool build stays the check (decision 1) |
| 5, 13: test-data and clock code copied between apps; no way to keep test data across a relaunch | M6.2 | `ForgeTestData.directory(app:)` with the keep-data flag in the template, and `forge_runtime::LocalTime` with an injectable clock. Pocket Library and Good Dog move onto them |
| 12: `expect` can't combine an id with a label check | M6.2 | `id` plus `label_contains` in one target in `forge ui run` |
| 14: a requirement can't say its device check isn't written yet | M6.4 | `device = []` with a `pending` note; `forge verify` lists it as open with the note, so the run record names it under not run |
| 15: no view of app slots | M6.1 (done) | `forge device apps` |
| 18: a refused laptop request went unnoticed for 20 minutes | M6.1 (done) | The status marker and heartbeat; see Silent failures |
| 17: the phone was asked for before the laptop had built the app | M6.1 (done) | `forge run ios` builds first and waits for the phone only for the install. Phone sittings are booked once the build has passed (`forge_build_ios` before `forge_install_device` in the run scripts) |
| 10: Docker not running gives a confusing error | M6.1 (done in `forge check swift`) | `forge check swift` names it and the fix |
| 2, 3: `forge init` gives a notes app to delete; generated README and AGENTS name the starter's files | M6.P | `forge init --blank`: an empty `AppCore` with lifecycle, events and logging wired, one screen, one model test, docs listing what is actually there |
| 4: `forge_storage::Store<T>` (save, then publish) | M6.P | Extracted from Good Dog's `change` helper and Pocket Library's `store.rs` (the second use) |
| 4, 11: adding an app to CI takes five hand edits | M6.1 (done, as a silent failure) | `ci-macos.sh` and the twin step discover apps; the Xcode project validation and cache list in `rust.yml` are still per app (M6.P) |
| 6, 7: UniFFI enums aren't `CaseIterable`; error case casing | M6.P | `forge bindings generate` adds `CaseIterable` to field-less enums; the casing is documented in ios-ui.md |
| 1 (prebuilt bindgen), 8, 16: slow first `forge init`, bundled content, shared core crate | M6.P (docs), later (prebuilt bindgen) | Document bundled content and a shared core crate in app-crate.md. A prebuilt `uniffi-bindgen` waits until the first-run time matters to an agent run |

### M6.P Pain-point cleanup

**What:** the items marked M6.P above, as two or three small PRs (starter and bindings; storage; CI). Where it's practical they are done by the M6 agent loop itself, as warm-up tasks before M6.5, which also tests the loop on framework code.

**Relies on:** #121 merged (for the extractions), `check-app-crates.sh`'s auto-discovery as the CI pattern.

**Tested by:** `make validate`, `scripts/check-fresh-app.sh` with `--blank`, Good Dog and Pocket Library tests passing on `Store<T>` unchanged.

**iPhone:** one install of a `--blank` app and of Good Dog after the `Store<T>` move, in the next coordinated sitting. Proves the starter and the moved storage still run on the phone.

## Order and phone time

```
M6.1 ─┬─ M6.2 ─┐
      └─ M6.3 ─┼─ M6.4 ── M6.5 ── M6.6
GD1 phone run ── M6.0 ─┘
```

M6.1 starts now. M6.0 waits only for GD1 on the phone. M6.2 and M6.3 can run side by side. M6.P runs alongside them once #121 has merged. Phone checks for M6.1–M6.3 go into one coordinated sitting through the iPhone session thread, about 30 minutes, together with the reruns already owed for #119 and #120.

## Risks

- **Free team slots.** Cati has 3 app slots. Good Dog, Pocket Library and the agent sandbox may be all of them, and Device Lab's crash check needs one too. Each install that needs a slot is Reuben's call (decision 3).
- **Good Dog's pace.** If GD1 slips or needs a big fix on the phone, M6.5 waits. M6.1–M6.4 don't.
- **No genuine defect.** The brief forbids planting one. The Murmur and Tipster runs each found several, so the odds are good, but the plan allows for a second task.
- **The agent's judgement.** Focused exploration and correlation make good choices easier; they don't guarantee them. The run record is how a person sees what the agent decided and why.
- **Scope.** The temptation is a pipeline command. The principle above says no: if M6.0 shows the agent can't sequence the tools, that is evidence for better results and prompts first.

## Decisions

All five were decided by Reuben on 2026-11-08 ("go with your recommendations"):

1. **Where the agent runs:** on the laptop through Remote Control, as Murmur did, so SwiftUI errors come from the xtool build in a minute. A cloud agent would see them only through macOS CI, about 35 minutes later.
2. **The demo task:** taken from Good Dog's next milestone and agreed with the Good Dog thread after GD1 is on the phone, not an M6-only feature.
3. **Phone slots:** Good Dog keeps its own bundle id (`org.forgemobile.gooddog`), and the agent sandbox's slot is freed for it during M6. Removing the sandbox app is done in a coordinated phone sitting.
4. **Human acceptance:** `forge verify --accept`, which only a person at a terminal can run, so acceptance sits in the same record as the machine verdicts.
5. **M6.1 starts now,** before GD1 reaches the phone. It is proven on Pocket Library and Device Lab.
