# Exploratory UI testing (`forge ui explore`)

`forge ui run` checks what someone wrote down. `forge ui explore` looks for what nobody wrote down: it walks an app's screens on the iPhone, tries each safe control once, checks every screen it reaches, and writes each problem as a `forge ui run` file that replays it. It is [UI Testing M5](forge-ui-testing.md#milestones) and the crawler in [general-device-control.md](research/general-device-control.md#the-crawler-on-the-unified-ui-model), and it is how an agent finds problems in an app it just changed without being told where to look.

**Status (2026-11-08):** built and tested on Linux. First run on Cati the same day: it walked Pocket Library (27 screens in 79 s) and found real accessibility issues, but **it changed Reuben's real library** (rated a book, changed its status, opened it in the reader, saved "Explored Book" entries). Exploring now needs the app's test-data mode or `--real-data`, which only looks ([Test data](#test-data)); that version has not run on the phone yet. See [Phone checks](#phone-checks).

## Use

From Linux, with the phone set up for UI testing (`forge ui setup`, see [forge-ui-testing.md](forge-ui-testing.md)):

```bash
forge ui explore --project examples/pocket-library --actions 60 --minutes 10 \
  --text bookForm.title="Explored Book"
```

The app must start on throwaway data (its `[ui_test] test_data_args`), or you pass `--real-data` to only look around ([Test data](#test-data)).

It prints each action and where it led, then a summary, and writes to `<app>/.forge/runs/<UTC>-ui-explore/` (or `--out`):

| File | What |
|---|---|
| `explore.json` | The whole report: screens, issues, the journal of every action (`--json` prints the summary) |
| `screens/<id>.png`, `screens/<id>.json` | A screenshot and the accessibility tree of every new screen |
| `visual/<id>.diff.png` | Screens that look different from the last clean walk launched the same way (M6.3; `data.visual` lists them). Information, never an issue: a difference is a change, not a defect |
| `repro/issue-<n>-<kind>.toml` | A `forge ui run` file per issue: launch, the shortest known way to the screen, the action, a screenshot |

It exits with an error when it found an error-severity issue (a crash, an unlabelled control), so an agent or a script can treat it like a test. Re-run a finding with `forge ui run <out>/repro/issue-3-app_closed.toml`.

| Option | Default | |
|---|---|---|
| `--actions N` | 60 | Stop after N actions (taps, typing, launches) |
| `--minutes M` | 10 | Hard time limit, whatever is left |
| `--seed S` | the clock | The order controls are tried in; printed, so a walk can be repeated |
| `--text ID=TEXT` | `Forge` | What to type into the field with that identifier |
| `--skip ID_OR_LABEL` | | Never touch this control |
| `--allow-destructive` | off | Also tap delete, remove, reset, sign out… (test data only) |
| `--real-data` | off | Explore the data already on the phone, only looking: nothing that saves, edits, rates, types or changes a setting |
| `--largest-text` | off | Launch at the largest accessibility text size (UIKit's own launch argument) |
| `--appearance dark\|light` | the phone's | Launch in dark or light mode (`-ForgeAppearance`, which Forge apps honour from their root view; M6.3) |
| `--launch-arg <ARG>` | none | One of the app's own launch arguments, after Forge's; repeat for more. For an app that can start past onboarding with sample data, so the walk reaches the rest of the app (seen on Cati: a Good Dog walk never left onboarding) |
| `--per-screen N` | 15 | At most N controls tried per screen |
| `--no-screenshots` | | Faster; trees are still saved |

## How it walks

The explorer is `forge_uitest::explore`. It drives any `Driver`, so the same code runs against today's per-command WebDriverAgent runner, the long-lived session that is being built, and later Apple's accessibility service (`AxDriver`).

- **A screen is its structure, not its pixels or data.** Its id is a hash of the visible identifiers (with data parts such as row ids and UUIDs replaced by `*`, so `library.book.42` and `library.book.43` are one control), the kinds and labels of controls without identifiers, and whether an alert, sheet, navigation bar or tab bar is showing. Values, frames and the keyboard are left out. The same detail screen for two different books is one screen.
- **Each control once.** On a new screen it lists the visible, enabled controls (buttons, cells, links, tabs, switches, menu items, text fields), shuffles them with the seed, and puts back and dismiss controls last. Rows of a list count once. Controls with neither an identifier nor a label are not tapped: Forge never uses coordinates (AGENTS.md rule 4), and the audit reports them.
- **Typing** uses `--text` for that identifier, else a value that fits the field: digits for a field that shows a number or is named like one (pages, goal, amount, year…), an address for email, a URL for web, a phone number for phone, else `Forge`. WebDriverAgent's tree doesn't give the keyboard type, so this goes by identifier, label and current value. Then it puts the keyboard away (WebDriverAgent's `/wda/keyboard/dismiss`).
- **Only the app's own part of the screen.** The keyboard, its keys and the panels a keyboard extension puts above them (anything from 100 pt above the top of the keys down) are cut from every tree before the walk looks at it, so they are never tapped, never fingerprinted and never audited, even when the keyboard can't be dismissed.
- **Moving on.** When a screen has nothing left to try, the explorer leaves through its back or dismiss control (twice in a row at most), or relaunches the app and replays the shortest known path to the nearest screen with something left. If a path no longer leads where it did, that screen's remaining controls are dropped rather than looped on.
- **Stopping.** At the action or time budget, or when nothing reachable is left (`stopped` in the report says which). Every wait is bounded (`find_timeout`, 5 s per control); there is no polling loop beyond the existing bounded `wait_for`.

## What it checks

| Issue | Severity | When |
|---|---|---|
| `app_closed` | error | The home screen is in front after an action: a crash or an exit. The crash report is for `forge device crashes` (and the crash-diagnosis work) |
| `no_label` | error | A visible control VoiceOver has no name for (Forge's tree audit) |
| `left_app` | warning | Another app came to the front (a link opened Safari). The explorer comes back with `activate`, else relaunches |
| `error_alert` | warning | An alert whose text says something failed (error, failed, couldn't, unable, invalid…) |
| `action_failed` | warning | WebDriverAgent refused the action |
| `dead_end` | warning | A screen with no back or dismiss control from which nothing tried led anywhere: only relaunching gets out |
| `clipped` | warning | A labelled control or text running off the side of the screen, or text drawn over other text in the same list (useful with `--largest-text`) |
| `small_tap_target` | warning | A control under 44 x 44 pt, counting a Form row as the target; bar buttons and links inside a sentence are left out (M6.3) |
| `no_identifier`, `duplicate_identifier` | warning | Forge's tree audit |
| `no_effect` | info | A button or link whose tap changed nothing in the tree |

Each issue is reported once, on the screen where it was first seen, with the actions that lead there.

## Test data

An explorer that taps "Save", stars and "Mark as finished" changes whatever data the app holds. On Cati on 2026-11-08 the first version did exactly that to Reuben's own Pocket Library. So `forge ui explore` has two modes and picks neither by default:

- **Test data** (the normal mode). The app's forge.toml declares the launch arguments that start it on fresh, throwaway data:

  ```toml
  [ui_test]
  test_data_args = ["-ForgeUITestData"]
  ```

  The explorer passes them on every launch (and the replay files include them), so every launch starts from the same empty state. That also makes a seed repeat the same walk. With test data the explorer may save, add, edit, rate and type; destructive controls still need `--allow-destructive`. It is the app's job to honour the argument. Pocket Library uses an `Application Support/PocketLibrary-UITest` folder, emptied at launch, instead of `PocketLibrary` (`LibraryModel.testDataArgument`). Settings kept in `UserDefaults` (the selected tab, reader text size, voice) are not separated.
- **`--real-data`**: for an app without a test-data mode, or to look at the person's real content. The walk only navigates and reads. It skips any control whose identifier or label contains a data word (save, add, create, new, edit, update, rename, set, apply, confirm, done, rate, star, favourite, like, bookmark, flag, pin, mark, checkmark, status, progress, finish, complete, read, reading, continue, resume, start, import, export, move, archive, restore, upload, download, record, sync, refresh). It also skips switches, sliders, steppers, pickers and segmented controls, typing into anything but a search field, and every destructive control. `--allow-destructive` is refused.

Without either, the command stops and says which to add. The word list errs towards skipping, so `--real-data` explores less. That is the point.

## Safety

The explorer touches the app the way a curious person would, so it has a floor of rules that no option removes:

- **Only Forge apps for now.** `--project` names an app folder with a `forge.toml`; there is no bundle id option. Other developers' apps come next: the any-app policy (`forge_uitest::policy`, merged in #107) exists, and the explorer has to ask its gate before each action, as `forge ui run --any-app` does, before it gets a bundle id option.
- **Never** starts a system service: dictation (it turns the microphone on), the camera, the keyboard switcher (Next keyboard, globe, emoji) or the system file picker (import), which is another app's interface. This is matched as whole words in every mode.
- **Only cancels** iOS's Shake to Undo alert ("Undo Typing"): it is the system's, and tapping its Undo once emptied a field the walk had filled (Good Dog on Cati, 2026-11-08).
- **Never** types into secure fields, and **never** taps a control whose identifier or label (as whole words, camelCase split) says it acts outside the phone or on someone else: send, post, reply, pay, buy, purchase, checkout, subscribe, transfer, share, call, publish, submit, invite, follow, report, block, donate.
- **Never changes the person's data**: see [Test data](#test-data).
- **Skips destructive controls** (delete, remove, erase, reset, clear, wipe, discard, trash, sign out, log out) unless `--allow-destructive`, which only works with test data.
- Every skipped control is listed per screen in `explore.json` with the reason, so coverage gaps are visible.
- Nothing typed is logged beyond the fixture text the person chose; trees and screenshots stay in the app's `.forge/runs`, which is not committed.

The word lists are a floor, not the policy. Wiring the explorer to the any-app policy's commit gate (`policy::is_commit`) is the next step; then one list serves both.

## How it fits with the work alongside it

| Work | What the explorer gets from it |
|---|---|
| Persistent WDA session | Merged: `forge ui explore` uses a running `forge ui start` session's runner when there is one, else starts one for the whole walk. Either way the 2 s start-up is paid at most once, not per action |
| Any-app safety policy | The gate before each action, and with it apps outside the project |
| Visual regression | A screenshot per screen id is already saved; comparing them with a baseline turns "it looks worse" into an issue kind |
| Crash diagnosis | `app_closed` already has the step and the replay; pulling and symbolicating the crash report at that step makes it self-explanatory |
| Evidence bundles ([evidence.md](evidence.md)) | `explore.json` becomes a `forge.evidence/1` bundle with every step linked to its screen and screenshot |

## Phone checks

### First run, 2026-11-08 (Cati, 748b35d, before test data)

Three walks on Pocket Library: seed 1 (79 s, 40 actions, 27 screens, 44 issues, 2 errors), the same seed again (126 s, 14 screens, 30 issues, 9 errors), and `--largest-text` (100 s, 22 screens, 51 issues, 9 errors). No crash and no other app opened.

- **Real findings to confirm:** controls with no label (a "10" text field, the "Online catalogue search" switch, an unlabelled button, the book form's title and author fields at the largest text size); duplicate identifiers (`star` ×5, the book form's save and cancel on a wrapper and the button); controls without identifiers (Edit, More, Continue Reading, Erase Library…, a settings switch, the tab buttons as WDA sees them); `tab.activity` with no visible effect; 2 dead ends.
- **Determinism failed** and a replay file failed at step 4, both for the same reason: the first walk had changed the library, so the next launch began on different data.
- **It changed Reuben's library**, which led to [Test data](#test-data).

### Second run, 2026-11-08 (Cati, c467e0d plus #116, test data)

On test data, with Pocket Library's own labels fixed by #116: 0 `no_label` errors in the app itself. But after typing in the library search the walk carried on in the keyboard. It tapped Next keyboard into third-party keyboards (Czech, GIF Keyboard's "Search Tenor", Grammarly's panel), typed on their keys, opened the EPUB import picker, and tapped `dictation`, which **turned the microphone on**. Most of the 73 issues were keyboard keys. In the second walk, WebDriverAgent closed the connection right after `dictation` and the phone dropped off USB, so no journal was saved and determinism is still unproven. Fixed by the keyboard cut, keyboard dismissal and the system-service list above; not yet re-run on the phone.

### Still to run

Run through the iPhone test session, not separately. Each proves something the Linux tests cannot:

0. Before anything else: Pocket Library launched with `-ForgeUITestData` shows an empty library and Reuben's own books are still there after the walk (proves the test-data mode).
1. `forge ui explore --project examples/pocket-library --actions 40 --seed 1` on Cati: the walk reaches the library, the add form, a book and settings; screen ids stay stable across visits (proves the fingerprint on real WDA trees, including the system back button and sheets).
2. The same seed twice gives the same journal (proves determinism on real timing).
3. `--largest-text`: the app comes up at the largest size (proves UIKit's launch argument through WDA's launch).
4. A replay file from step 1 passes or fails the same way under `forge ui run` (proves the reproductions).
5. A person confirms which reported issues are real ([acceptance (a)](research/general-device-control.md#priorities): "the crawler finds real issues in Pocket Library that a person confirms").

## Not built yet

- Scrolling to controls below the fold, and long lists beyond the first row.
- Invariants an app declares (for example "the count on the badge equals the rows"), and checks across a whole walk.
- Performance during the walk (A3 FPS and memory).

Since M6.1 ([forge-m6.md](forge-m6.md), Linux-tested, not yet run on the phone): agents call `forge_ui_explore` in `forge mcp` (no destructive taps and no typed text of their own); every walk writes a `forge.evidence/1` bundle next to `explore.json`; the app's log lines are kept as `device.log`, and when the app closes its new crash reports are pulled and symbolicated into `crashes/`. A walk that tried no control fails rather than passing.

## Exploring what changed (M6.2)

Since M6.2 ([forge-m6.md](forge-m6.md#m62-exploration-aimed-at-the-change), Linux- and CI-tested on a model of Good Dog, not yet on the phone), a walk can be aimed at a change:

```bash
forge ui focus --project examples/good-dog --since HEAD           # what the uncommitted work touches
forge ui explore --project examples/good-dog --since HEAD          # try that first
forge ui explore --project examples/good-dog --changed Sources/AppHost/ExerciseView.swift
```

- The controls the changed Swift files declare go first, then routes from the previous walk (its issues are rechecked first), then the ways into the change (views that present it or call into it), then the rest of the app as before.
- `explore.json` says what of the change was seen, tried and never reached; each issue says its tie to the change and the files behind it, its confidence (`observed` or `heuristic`), and after a replay whether it came back (`--verify N`, default 3).
- Every tie says what it rests on (`focus_basis` on an issue, `basis` on a link): `declared` when the changed file contains the control's identifier (that control's code changed; which screen shows it is not proven) and `inferred` when names were matched as words along a chain (a view that uses the changed code) or by the same screen part. Both are read from the source as text, not compiled: leads for where to look first, never proven dependencies. `forge ui focus` prints that first, and groups the inferred ways in by the reason, so each can be checked. `basis_note` in `focus.json` and `explore.json` says the same for an agent.
- With an earlier walk (`--previous FILE`, or the app's latest when the walk has a focus; `--no-previous` turns it off), `compare.json` lists issues gone, still there, new and not rechecked. "Gone" needs this walk to have done what showed the issue.
- `--fixture MODEL.json` walks a model of the app (recorded trees and where taps lead) instead of the phone. It proves how Forge explores, never how the app behaves: its bundle and result say `kind = "fixture"`, `forge verify` never counts it as a device result, a walk learns only from earlier walks of its own kind (a phone walk never compares with a model's issues, and `--previous` naming the other kind is refused), and MCP has no way to start one. `scripts/demo-m6.2.sh` is the worked example.

- Seeded random walks as well as the systematic one, for flaky-state bugs.
