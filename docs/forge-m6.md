# Forge M6: an autonomous, evidence-driven dev loop

Status: plan, 2026-11-08, decisions taken the same day. M6.1 is built, hardened and Linux-tested; M6.2 is built, Linux-tested and CI-tested on a model of Good Dog; M6.3 is built and Linux-tested (its Swift side is compiled by macOS CI only); M6.4 is built and Linux-tested. None of them has run on the phone yet. It was written after reading `main` at 301359f (#106–#120 merged) and the open Good Dog GD1 PR (#121).

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

Two docs were behind the code: [dev-loop.md](dev-loop.md)'s status table (last checked 2026-11-07, said UI tests didn't pass on the phone) and `forge_diagnose`'s own description. M6.1 refreshed the description and M6.4 the table.

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

**Hardening (2026-11-08, after an audit of the M6.1 code; Linux-tested only):**

| Problem found | Fix | Regression test |
|---|---|---|
| A UI run or walk that returned an error (WebDriverAgent or the phone stopped answering, the app closing at launch) skipped `Watch::finish`: the log reader thread kept the phone's syslog service, and the log and crash reports collected so far were lost | The error path finishes the watch, pulls new crash reports, writes `device.log`, `crashes/` and `watch.json`, puts the watch in the result and names the folder in the error. Dropping an unfinished `Watch` stops and joins its reader the same way | `dropping_an_unfinished_watch_joins_its_reader`, `a_run_that_breaks_off_keeps_its_watch_and_says_where` |
| The log reader only noticed it should stop when the phone sent a line, so on a quiet phone it outlived the run | New `DeviceBackend::syslog_while(stop)`, whose contract is to return within about 100 ms of the stop flag whether or not a line arrives; the USB backend keeps it. `finish` and drop join the reader thread, waiting at most 2 s, and `watch.reader` says what happened: `stopped` (joined, connection closed), `detached` or `not_started`. Rust can't kill a thread, so a backend that breaks the contract is detached after the wait and reported (`reader: detached`, `log_status: unavailable` and a note; on drop, a line on stderr); it ends on the phone's next line, a disconnect, or when `forge` exits | `a_quiet_app_is_empty_not_missing_and_the_reader_has_ended_when_finish_returns` (the reader's last act is checked once, with no waiting, right after `finish` returns), `a_reader_that_wont_stop_is_left_after_the_wait_and_reported` |
| A watch that couldn't start (unreadable forge.toml), a log reader that failed, and crash reports that couldn't be listed all looked like a quiet app that didn't crash | `log_status` (`collected`, `empty`, `unavailable`), `crash_reports` (`not_checked`, `none_new`, `found`, `unavailable`), `symbolicated` on each crash, and `status: not_watched` with a `reason`. The failed step carries both statuses | `a_phone_that_cant_be_reached_is_unavailable_evidence_not_a_quiet_app`, `a_run_without_a_watch_says_why` |
| Correlation matched a crash to a changed file by file name: `crates/forge-runtime/src/training.rs` changed made a crash in `examples/good-dog/rust/src/training.rs` "in a changed file" | `culprit_source` resolves the crash path to a repository path (absolute in this checkout, absolute from another clone by a tail that starts at a top folder of the repository, relative to the app folder as forge-crash writes it, or repository-relative), normalising `\`, `.` and `..`. `changed` is `true` only on an exact path match, `false` when placed and unchanged or in toolchain or dependency code, and `null` when it can't be placed or the changes can't be listed | `the_same_file_name_in_another_folder_is_not_a_match`, `absolute_paths_in_this_checkout_and_in_another_resolve_to_the_same_file`, `code_outside_the_repository_is_said_to_be_outside`, `an_unplaceable_path_or_unknown_changes_are_unknown_not_unchanged`, `normalising_paths` |
| The changed-file list covered the app folder only, and was empty (read as "nothing changed") outside a git checkout | Changes across the whole repository since the server started; files already modified at the start are listed as `dirty_at_start`; outside git, `changes.unknown` says why | `changes_cover_the_whole_repository_and_say_when_git_is_missing` |
| Diagnose couldn't tell "no crash" from "crash reports not checked", nor say a crash came from another build or wasn't symbolicated | `collection`, `crash.symbolicated` and a `next` hint for each case; `evidence.exists` checks the bundle path | `crashes_from_another_build_or_without_symbols_say_so`, `a_failed_step_without_a_crash_check_doesnt_rule_out_a_crash` |
| A timed-out or cancelled MCP call looked like any failure, and a stopped UI run lost the path of what it had written | `stopped: timeout` or `cancelled`; UI run and explore results always name `data.out`, with `partial` when the run was stopped | `a_timeout_or_a_cancel_is_a_failure_that_says_which`, `a_stopped_run_still_names_its_folder_and_says_it_is_partial`, `ok_from_a_child_that_exits_non_zero_is_a_failure` |
| An explore bundle said `ok: true` for a walk that tried nothing (the command failed); `--actions 0` or `1` walked nothing | The bundle's `ok` needs a tried control; `--actions` below 2 is refused | `explore_refuses_options_that_would_check_nothing` |

Still unproven without the phone: `syslog_while` on a real syslog relay (that it stops within 100 ms and drops no lines while WDA runs), and that a real crash's DWARF path resolves through `culprit_source` (the laptop builds in its own clone, so `other_checkout` is the path expected). These join the M6.1 phone checks.

### M6.2 Exploration aimed at the change

The question: when an agent changes an app, can Forge explore the parts most likely to be affected, and find problems a script misses? Built on 2026-11-08, on Linux, without the phone.

**Status, kept apart:**

| State | What |
|---|---|
| Implemented | `forge ui focus`; `forge ui explore --since/--changed/--focus/--previous/--verify/--fixture`; `forge_ui_focus` and the new `forge_ui_explore` arguments in MCP |
| Linux-tested | Unit tests on synthetic trees (the Pocket Library model app in `explore.rs`) and on the real Good Dog source (`focus.rs`) |
| CI-tested | `scripts/demo-m6.2.sh` runs in `rust.yml` and fails if the aimed walk stops finding the planted crash, a seed stops repeating, or the fixed crash isn't rechecked |
| Simulated or fixture-tested | Every walk in the demo is on a model of Good Dog (`examples/good-dog/uitests/model/`): five screens recorded on the phone, five written by hand from the Swift source and marked `synthesized`, with the crash planted in the model. It proves how Forge prioritises, reproduces and rechecks, not how Good Dog behaves on a phone. Its bundles say `kind = "fixture"`, `forge verify` never counts them, and a phone walk never learns from or compares with them |
| iPhone-tested | Nothing yet |
| Human-accepted | Nothing yet |

**How it works:**

1. **What did I change, and which screens should I inspect?** `forge ui focus` reads the changed files (git since a commit, or `--changed`) and links them to accessibility identifiers, each link with the file it comes from and how:
   - *direct*: a changed Swift file declares the identifier (`.accessibilityIdentifier("record.save")`); an interpolated one (`"record.result.\(key)"`) is kept as its prefix, `record.result.*`.
   - *entry*: a Swift file uses a type the change declares (TodayView presents ExerciseView, so `today.activity.*` is a way in), or calls changed Rust through the UniFFI bindings, possibly through a model method (`MyDogView calls record() (GoodDogModel.swift), which calls recordSession; record_session (lib.rs) uses … from training.rs`). Exported Rust functions count where they are called, not where a property of the same name is read.
   - Every changed file with no link is listed with the reason (outside the app, generated bindings, not app source, deleted, or nothing connects it). A file name alone never ties code to a screen.
   - It also names the UI tests that use the linked identifiers and flags changed persistence or navigation code.
2. **Exploring the change first.** With a focus the explorer tries, in order: the change's own controls on the screen in front; routes from the previous walk (its issues to recheck first, then the shortest known ways to screens where it saw the change's identifiers, without the taps that changed nothing); the ways in on the screen in front; then the nearest screen with focus controls left. Only then does it walk the rest as before. Within each tier the seeded order holds, so the same seed, focus and previous walk give the same walk; without a focus the walk is exactly the old one.
3. **What did Forge actually exercise?** `explore.json` gets `focus`: the declared identifiers, which were seen and where, which were tried and at which step, the first step that tried one, and the ones never reached, with why that can happen ("not checked, so not passed"). Each journal step has a `phase` (`explore`, `route`, `recheck`, `verify`), and each screen its identifiers and route.
4. **What did it find, and can I reproduce it?** Each issue now has its tie to the change (`focus`: direct, entry, screen or none) and what that tie rests on (`focus_basis`: `declared` when the changed file holds the identifier, `inferred` for a chain of names matched as text; neither is a proven dependency, and `basis_note` says so in every report), `changed_files` with how (empty means no link was established, not "unrelated"), `confidence` (`observed` for what the phone showed; `heuristic` for `no_effect` and `dead_end`), `evidence` (the screen's tree and screenshot), its replay as steps (`route`) and its replay file. After the walk, up to `--verify` issues (default 3; errors first, then warnings on the focus) are replayed from a fresh launch, and `reproduced` says whether they came back. These replays aren't counted in `--actions`; audit findings aren't replayed.
5. **Did my change fix them?** With an earlier walk (`--previous`, or by default the app's latest under `.forge/runs` when the walk has a focus), `compare.json` sorts the earlier issues into `still_present`, `gone`, `new` and `not_rechecked`. An issue is `gone` only when this walk did the action that showed it (its recheck route does) and it didn't come back.

`forge_ui_explore` in MCP aims at the files changed since the server started by default (`focus_changes`), learns from the previous walk (`previous`) and replays 3 issues (`verify`). `forge_ui_focus` gives the mapping alone, without the phone.

**Demonstration** (`scripts/demo-m6.2.sh [OUT_DIR]`, about a minute, no phone):

1. `forge ui focus --changed Sources/AppHost/ExerciseView.swift` on Good Dog: 11 declared identifiers, the ways in from Today and Learn, the journey test that uses them.
2. An earlier plain walk of the model before the change: the map.
3. The model after the change, where Save on the "How did it go?" sheet closes the app. The aimed walk (seed 7, 20 actions) finds the crash at step 9, ties it to ExerciseView.swift (`declared in ExerciseView.swift`), and its replay brings it back. Its files are `repro/issue-N-app_closed.toml`, `screens/<id>.json`, `focus.json` and `evidence.json` with `kind = "fixture"`.
4. The same seed again: identical journal.
5. Plain against aimed, seeds 1 to 20, 20 actions each: the plain walk found the crash in 2 of 20 (steps 11 and 12); the aimed walk in 20 of 20 (steps 8 or 9), with no earlier walk to learn from.
6. After the fix, the aimed walk rechecks the earlier crash first: `gone`.

The planted crash is in the model, not in Good Dog. What the demo proves is how Forge chooses, records and compares; it doesn't prove that Good Dog behaves like the model, and it is no substitute for the phone run below.

**What it can't do yet:**

- The links are textual: whole-word names and brace-matched bodies, not a compiler's view. A common method name can over-link (a Rust file used by most exports, such as Good Dog's `training.rs`, links most screens as ways in), and identifiers built at run time, or set through a helper, are missed.
- It can't reach a screen whose way in isn't on screen: no scrolling below the fold, one row per list.
- State across a relaunch isn't checked: test-data mode starts fresh on every launch, so a relaunch can't show that data survived. That needs the keep-data launch flag (`-ForgeUITestKeepData`, Good Dog pain point 5), not built. Changed persistence code is flagged in `forge ui focus` only.
- No coverage map kept between walks beyond the previous walk's `explore.json`; no "skip unchanged screens".
- `--promote` (copy a replay into `uitests/`) isn't built; the replay files are ready to copy by hand.

**iPhone (owed, next sitting):** Pocket Library or Good Dog on test data: one plain walk, then a real change to one view, then `forge ui explore --since HEAD` reaches that view's controls within its first few actions, the same seed repeats the walk on the phone, and a fixed issue shows as `gone`. This is also the determinism check owed since #109 and #119. Until then, M6.2 is fixture-tested only.

### M6.3 Appearance and accessibility checks

**Status (2026-11-08):**

| State | M6.3 |
|---|---|
| Implemented | `-ForgeAppearance dark\|light` honoured by the template and every iOS example (`forgeLaunchAppearance()` on the root view); `appearance` on `forge_launch_app` and `forge_ui_explore`, `--appearance` on `forge ui explore`; baselines per variant; `small_tap_target` and a wider `clipped` in the audit; `forge ui audit` and `forge_ui_audit`; explore's comparison with the last clean walk |
| Linux-tested | Yes: `cargo test` covers the variant names, dark and light baselines kept apart, both audit findings and their exemptions, the audit on the GD1 journey's six recorded trees (no false findings), the walk comparison on PNG pairs, which earlier walk is compared with, and the MCP arguments |
| CI-tested | The Rust side in this PR's CI. The Swift side (`LaunchAppearance.swift`, the app roots) is compiled by macOS CI only; this session has no Swift toolchain |
| Simulated or fixture-tested | Only in the sense above: recorded trees and generated PNGs. No model walk is involved |
| iPhone-tested | No |
| Human-accepted | No |

**How it works:**

- **Dark and light at launch.** UIKit has no launch argument for the appearance, so it is the app's: `-ForgeAppearance dark` lands in `UserDefaults`, and `forgeLaunchAppearance()` on the root view turns it into `.preferredColorScheme`. Without it the app follows the phone. Weather forces dark by design, so the argument changes nothing there. An app made before M6.3 ignores it, which a screenshot shows at once.
- **Baselines per variant.** The launch step before a screenshot decides its variant: `-dark` or `-light`, then the text size (`-xs` to `-xxxl`, `-ax1` to `-ax5` for the accessibility sizes, Apple's AX1–AX5). `1-library@1290x2796-dark-ax5.png` sits beside `1-library@1290x2796.png`, and `forge ui accept` writes to the right one. Existing baselines, launched without either argument, keep their names.
- **`small_tap_target`** (warning): an enabled control under 44 x 44 pt. A control counts at the size of the row around it when that row holds no other control and hugs it within 24 pt (a Form row is tapped anywhere: Good Dog's 34 pt pickers sit in 44 pt rows). Left out: controls in navigation, tool and tab bars (the system widens their touch area) and links inside a sentence (WCAG 2.5.8 exempts them). The finding says what was measured (`44 x 18 pt, under 44 x 44`).
- **`clipped`** (warning) now also covers text drawn over other text, as well as text or a labelled control running off the side of the screen (which only counts while partly on screen, so a carousel's next page isn't flagged). Overlaps count only between texts in the same scrolling container: a list's rows scrolling under a fixed bar (Good Dog's "Done today" bar over the steps) are not an overlap. A text inside another (a row's label and its parts) isn't either.
- **`forge ui audit` and `forge_ui_audit`.** The screen's audit without the tree: counts by severity and issue, each finding with its element and what was measured, and `checks`, which says what isn't measured. A clean audit is clean for these checks only. It fails on an error (an unlabelled control).
- **Walks compared with the last clean one.** After a walk on the phone, each screen's screenshot is compared with the same screen (by fingerprint) in the newest earlier device walk that was launched with the same arguments (test data, appearance, text size) and found no error. `data.visual` lists `match` or `changed` with the changed areas and the elements in them, and the diff images go to `visual/`. It is information, never an issue: a difference is a change, not a defect, and the agent decides. A walk on a model has no screenshots and says so.

**Limits:**

- The audit reads the tree, not the pixels: contrast, text truncated with an ellipsis inside its own frame (the tree keeps the full label), and meaning carried by colour alone are not checked. Apple's own audit (XCTest `performAccessibilityAudit`, or the accessibility audit service the research found reachable from Linux) would cover those; it isn't used yet.
- The small-target exemptions are judgements about where the system widens the touch area. A person should confirm them on the phone (check 9 below).
- The walk comparison matches screens by their identifiers, so two visits to the same screen with different content (an empty list, then a full one) compare as a change.

**iPhone (owed, next sitting):** checks 8 to 10 in the table below.

### M6.4 The run record

**What:**

- `forge verify --run <start>` reads every bundle and the MCP journal since a start mark and writes one `forge.evidence/1` bundle of `kind = "run"`. It holds: app and repository, the commits made during the run and the dirty state at the end, builds and installs (from the journal), device and OS, scenarios and walks with their verdicts, screenshots and audit findings, crashes and logs, the failures and the commits that followed them (the repairs), and a `not_run` list with a reason for each check that was expected and didn't run. It also writes `report.md`, which is what a person reads.
- A `human` verdict: `forge verify --accept GD-3 --note "…"` records a person's acceptance as its own bundle (`kind = "human"`), refused when stdin isn't a terminal and absent from MCP, so no agent can record it. `forge verify` shows twin, device and person columns.
- `forge_verify` gains `run: true` for the report.
- Refresh dev-loop.md's status table and the M6 page's results.

**Relies on:** `verify.rs`, `evidence.rs`, the journal.

**Tested by:** Linux tests with recorded bundles and journals: a skipped check shows as not run, never as passed; a device verdict never fills the person column; a twin pass never fills the device column.

**iPhone:** none of its own; the M6.5 run produces the first real record.

**Status (2026-11-08):**

| State | M6.4 |
|---|---|
| Implemented | `forge verify --run <start> [--journal <file>]` writes `<app>/.forge/runs/<id>-run/evidence.json` (`kind = "run"`) and `report.md`; `forge verify --accept <ID> --note "…"` writes `<id>-accept/evidence.json` (`kind = "human"`); `forge verify` has a Person column; `forge_verify` takes `run: true` |
| Linux-tested | Yes: `cargo test` covers a skipped check listed as not run (never passed), a device pass leaving the person column empty and a fixture walk leaving the device column empty, a failure and the commits after it kept as a repair, the acceptance refused without a terminal or under `forge mcp`, refused unless the id is typed back, and the MCP server never passing `--accept` |
| CI-tested | The same tests in this PR's CI. No Swift is involved |
| Simulated or fixture-tested | The tests use recorded bundles and journals. A run on Good Dog's real `.forge/runs/` (empty in this checkout) gave "0 of 6 proven, 9 checks not run, 6 not accepted", which is right for a checkout with no runs |
| iPhone-tested | Not applicable: it reads what other commands wrote. The first real record comes from M6.5 |
| Human-accepted | No. Nothing has been accepted with `--accept` yet |

**How it works:**

- **The start** is a run id (`2026-11-08T10-00-00`, UTC), the same stamp as every run folder. `forge_verify run: true` uses the time the MCP server started and that server's journal, so an agent asks for the record of its own session. From a shell, `--journal` picks one file; without it every `*-mcp/journal.jsonl` started since the start, in the app's and the repository's `.forge/runs/`, is read.
- **What it reads:** the bundles in `<app>/.forge/runs/` whose id is at or after the start (twin runs, UI runs with their `watch` and `visual` sections, explore walks with device and fixture walks kept apart), the journal's build and install calls and every failed call, and `git log` and `git status` for the commits made and what was still uncommitted at the end. It runs nothing.
- **Repairs:** a failed tool call, then the commits made before the next successful call of the same tool, are recorded together as that failure's repair. A failure with no later success has `then_passed: null`: the commits after it are listed, but nothing shows they fixed it.
- **Not run:** for each requirement and each proof it names (twin, device), the reason it isn't proven in this run: nothing ran it since the start, it ran but `not_run` (needs Apple's frameworks), its result is for other files (stale), or the run had no phone (the server was started with `--no-device`). A requirement no person accepted is listed as not accepted. The verdict is `complete` only when every requirement is proven in the run and accepted by a person, so an agent's record alone always says `incomplete`.
- **The three columns never fill each other.** The twin column reads only `linux-twin` bundles, the device column only `device` bundles (a walk on a model is `fixture` and counts for neither), and the person column only `human` bundles.
- **Acceptance is a person's.** `--accept` needs stdin to be a terminal, is refused when `FORGE_MCP_CHILD` is set (the MCP server sets it on every `forge` it runs), names the commit and whether the files are uncommitted, and asks the person to type the requirement id back. The bundle records who (`git config user.name`, else `$USER`) and the note. It is for the app's files as they are: a later change makes it stale like any other verdict. No MCP tool takes it.

**Limits:**

- The terminal check stops an agent's tools and scripts, not a person who chooses to pipe a terminal into it. It records a claim by whoever typed it, not proof that they looked at the phone; the note is where they say what they looked at.
- Repairs are inferred from timing (commits between a failure and the next success of the same tool), not from what the commit changed.
- `git log --since` reads commit times, so commits made earlier and rebased or merged during the run are listed as made during it.
- Crashes and logs come from the UI runs' `watch` sections. A crash seen only through `forge device crashes` outside a UI run isn't in the record.

### M6.5 The Good Dog demonstration

**What:** an agent session (staged, on the laptop, through Remote Control) takes a real Good Dog task and runs the loop with everything above, ending with a run record. Reuben then reviews the app on the phone and records his acceptance.

**The task** comes from Good Dog's own next milestone, agreed with the Good Dog thread, so it advances the app instead of running beside it. Reuben (2026-11-08): once M6 is ready, use the loop to improve Good Dog substantially, because the app feels basic and its programmes are good step-by-step guides that read as lists. Good Dog's new direction is goal-led training (PR #127, docs/good-dog-demo.md), so the task follows that plan:

- **The task:** pausing, resuming and setting aside a training goal, with its history kept. It runs after GD2a (the Rust goals-and-pathways milestone, which the Good Dog thread builds first). It crosses Rust state, storage, SwiftUI, interaction and accessibility, and needs no new capability.
- **Fallback:** GD2b's goal journey screen, the screen that turns a goal's steps into a path you work through rather than a list.
- Earlier candidates (the My Dog progress section, CSV export for a trainer, now GD4, and reminder preferences, now GD5) still cross the same layers but no longer advance the product's next step.

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

## Outstanding iPhone checks (M6.1 to M6.4)

None of these has run; each says what it proves.

| # | Check | Proves | From |
|---|---|---|---|
| 1 | `forge device apps` against Settings > VPN & Device Management | The app list, profile team and days left, and the free-team slots in use, including whether the WebDriverAgent runner holds one | M6.1 |
| 2 | Good Dog `uitests/journey.toml` on test data: `watch.log_status` is `collected` and `log_lines` > 0, `device.log` holds only `GoodDogXtool` lines | The syslog stream and WebDriverAgent run together; `syslog_while` stops at the end of the run without dropping either. First run (Cati, 2026-11-08 15:48): 29/29, but all 5000 kept lines were WebDriverAgent's own (XCTAutomationSupport inside the app's process) and the first ~20 s were dropped, with no line from the app itself. Fixed in #126: runner lines are left out and counted (`runner_lines_skipped`), and dropped lines are counted with a note (`log_lines_dropped`). Rerun owed | M6.1, hardening |
| 3 | Device Lab `dashboard.crashTest` then `crashTest.rust` in a UI run (`m6/device-lab-crash-rust.toml`): the failed step has `crash` with `symbolicated: matched`, culprit `rust/src/crash_test.rs`, `crash_reports: found`, and the log tail has "crashing on purpose"; then `forge_diagnose` gives `culprit_source.resolved_by` `app_relative` or `other_checkout` and `layer: rust` | Crash correlation on a real report and a real DWARF path | M6.1, hardening |
| 4 | Unplug the phone during a UI run | The run breaks off with `watch.json`, `device.log` and the folder named in the error; `watch.reader` is `stopped` (joined, not detached) | Hardening |
| 5 | Good Dog or Pocket Library: a plain walk, a real change to one view, then `forge ui explore --since HEAD` | The aimed walk reaches that view's controls within its first few actions on real trees; `focus.not_reached` is right | M6.2 |
| 6 | The same walk with the same seed twice | Determinism on the phone (owed since #109 and #119) | M6.2 |
| 7 | An issue found, fixed, and the next aimed walk | `reproduced: true` before, `gone` after, on the phone | M6.2 |
| 8 | Good Dog (after a rebuild with M6.3) launched with `forge_launch_app appearance: dark, text_size: largest`, then `forge_ui_audit` on Today, an exercise and My Dog | The app honours `-ForgeAppearance` on the phone (the screenshot is dark); the audit's findings at AX5 are real, or show where its exemptions are wrong | M6.3 |
| 9 | A person looks at each `small_tap_target` and `clipped` finding from check 8 on the phone | Which findings are real. Only a person can say this, and it is recorded as theirs | M6.3 |
| 10 | Good Dog `journey.toml` with a dark launch step: `--visual record`, then a check run; then two explore walks with `--appearance dark` | Dark baselines are kept apart from the light ones (`-dark-ax5` names); the second walk compares with the first and lists `match` for unchanged screens | M6.3 |
| 11 | After check 2 passes and Reuben has used the journey on the phone, he runs `forge verify --project examples/good-dog --accept GD-3 --note "…"` in his own terminal on the laptop, then `forge verify` | A person's acceptance is recorded as `human` and only fills the Person column. Not a phone check itself: it needs the person, so it goes in the same sitting. Optional; M6.5 needs it anyway | M6.4 |

The runbook for 1 to 3 is in project files at `m6/m61-phone-checks.md`.

Found in the first sitting (Cati, 2026-11-08, forge at 9c8488a) and what became of it:

- Check 3 passed: a Rust panic in a UI run gave the failed step a symbolicated crash (`rust_panic`, `crash_test.rs:24`) and `forge_diagnose` layer `rust`. HangTracer filled the 5000-line log and pushed the panic's own line out; HangTracer is now left out like WebDriverAgent's lines (M6.4's PR).
- When the app had died, the failed step listed the home screen's icons and notification counts. A failure with the home screen in front now records neither its tree nor its screenshot, only a note saying why (M6.4's PR).
- Crash reports are pulled only when a step fails or the app closes: on a passing run `crash_reports` is `not_checked`, which means not looked, never "no crash". By design.
- Check 8 (Good Dog dark at AX5, audit on Today, an exercise and My Dog): no `small_tap_target` or `clipped` findings, but the audit only sees what is on screen, so at AX5 it covered the top of each screen. Today's plan title wrapped into three lines and ran under the tab bar unflagged. Reuben judged wrap and overlap by eye.
- A `kind` CollectionView scroll with the keyboard up swiped across it and typed into the name field ("Moss" became "MossSsss"). A scroll step now refuses while the keyboard is up (M6.4's PR).
- Check 10, dark journey: recorded 29/29; a check straight after differed on 3 of 6 screens: a session time on My Dog (needs a mask), 1.5% of small text areas on an exercise, and 18% of the glossary term screen, caught mid-push. Screenshot steps now wait for the screen to settle (M6.4's PR); the text areas need a second look once that is in.
- Explore determinism (#119) passed: two walks with the same seed gave identical journals, plain and dark. They flagged Good Dog's picker option rows (250 x 43 pt) as small targets. Those are system list rows, 44 pt with the separator, so the check now allows 1 pt (M6.4's PR). Their missing identifiers are Good Dog's to fix.
- Check 5, first try, FAILED: an aimed walk on a planted change to My Dog, in a fresh worktree with no earlier walks, never got past onboarding (test data starts there) and spent its 40 actions on the dog form. No route was known and nothing made it prefer leaving the form. `--previous <explore.json>` from another worktree supplies routes. Open: prefer controls that leave a screen once its form has been tried, when no route reaches the focus; an "Undo" control the walk tapped needs its place in the tree before it can be filtered.
- Open: a whole-screen audit (scroll, then audit each part); the journey needs scroll steps at AX5, where `onboarding.start`, `exercise.step.0` and `dog.sessionsThisWeek` are below the fold; a `kind` scroll in a CollectionView took 7 to 11 s each; `forge_diagnose` called while another tool is still running answers about the last call that finished, not the running one.

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
