# iPhone acceptance: the integrated framework

One runbook for checking the integrated framework (#32–#36, combined on the `claude/project-thread-mtlahv` branch, PR #37) on a real iPhone, from Linux with xtool. It replaces the separate device steps in `examples/pocket-library/ACCEPTANCE.md`, `docs/native-services-device-check.md` and the starter runbook from #32; those still describe their own app in more detail.

Each check says where it can be verified:

- **Automated**: Rust tests on Linux, run in CI and locally.
- **Linux local**: run by hand on a Linux machine (generation, xtool builds).
- **macOS CI**: XCTest against the real Rust library, unsigned Xcode device builds, simulator screenshots. The simulator renders are a review aid, not device results.
- **Physical iPhone**: only a real phone can show it.

The Status column records what has actually been run. Leave a cell as "Not run" until someone has done the step; fill in the date and what was seen.

## Before you start

- A Linux machine with the Darwin SDK from Xcode 26 (`xtool setup`), usbmuxd, and the iPhone plugged in and unlocked, with Developer Mode on and your developer profile trusted. [getting-started.md](../getting-started.md) covers the one-time setup.
- **The patched xtool** from the `ixos` branch of GeneralKnowledge/xtool (1.24.0-ixos.1). It records iOS SDK 26.0 in the app, so iOS 26 shows the Liquid Glass look; stock xtool 1.24.0 records 17.0 and iOS may run the app in compatibility mode. It also skips the relink when nothing changed. Put it first on `PATH` so `forge` uses it:

  ```bash
  export PATH="$HOME/xtool-ixos:$PATH"    # the directory holding the patched xtool binary
  xtool --version                         # record it below
  ```

- `xtool dev run` (and so `forge run ios`) **installs the app but does not open it**. Open it from the home screen.
- A free Apple ID can have **3 sideloaded apps** at a time. This runbook installs up to three (the starter, the services check and Pocket Library); delete older test apps first.
- Every build warns that Swift 6.2.3 is older than the 6.4 xtool supports on Linux. That warning is expected.

Record the versions:

| Item | Value |
|------|-------|
| Date | |
| Branch and commit (`git rev-parse --short HEAD`) | |
| xtool (`xtool --version`) | |
| Swift (`swift --version`) | |
| iPhone model and iOS version | |
| Apple ID type (free or paid) | |

```bash
cd ~/ixOS
git fetch origin && git checkout claude/project-thread-mtlahv && git pull
cargo install --path crates/forge-cli --locked     # or prefix each forge command with: cargo run -p forge-cli --
forge doctor                                       # every iOS line should be ready, including the iPhone
```

## What has already run

| What | Where | Result |
|------|-------|--------|
| Patched xtool build of the starter app (#32 branch, before integration) | Reuben's laptop, 2026-11-07 | Built cleanly in about 1m30s; app records iOS SDK 26.0. So `@Observable` and the starter's Swift build with xtool on Linux |
| Patched xtool build of the services check app (#34 branch, before integration) | Reuben's laptop, 2026-11-07 | Built in 1m36s; SDK 26.0 |
| Patched xtool build of Pocket Library (#33 branch, before integration) | Reuben's laptop, 2026-11-07 | Built in 2m11s; SDK 26.0 |
| Patched xtool build of Counter, Weather and Chat (main) | Reuben's laptop, 2026-11-07 | Built in about 1m30s each; SDK 26.0. A no-op Counter rebuild took 2.9s with no relink (stock xtool: 13.1s with a relink) |
| Installing any of these on the iPhone | | Not yet recorded |
| Anything built from the integrated branch | | Not yet run |

## A. Fresh application: generate, build, install

| # | Step | Kind | Status |
|---|------|------|--------|
| A1 | `forge init` creates an app outside the checkout with its own Rust crate, bindings and tests | Automated (CI "Fresh app outside the checkout") and Linux local | Pass in CI and in the cloud Linux checks on the integrated branch |
| A2 | The generated app's XCTest suite and an unsigned Xcode device build | macOS CI | Pass on the combined branch (PR #37) |
| A3 | `forge run ios` builds it with xtool on Linux | Linux local | Not run on the integrated branch |
| A4 | It installs, opens from the home screen, and shows "No Notes" | Physical iPhone | Not run |
| A5 | The app records iOS SDK 26.0 and has the iOS 26 look (navigation bar and buttons in Liquid Glass) | Linux local (SDK) and Physical iPhone (look) | Not run on the integrated branch |

```bash
cd ~
forge init NotesCheck --bundle-id org.example.notescheck     # creates ~/NotesCheck
cd NotesCheck
forge test            # Rust tests (XCTest is skipped on Linux)
forge run ios         # preflight, Rust build, xtool build, install
```

- `forge run ios` first checks the toolchain, the Apple sign-in and the iPhone. If one fails, it says which and how to fix it. Note any wording that does not match what xtool or Apple printed: some of it was written without a real failure to copy from.
- To check the SDK version recorded in the app: `xtool dev build` leaves the app under `xtool/`; `otool -l` or `llvm-objdump --macho --private-headers` on the binary shows `LC_BUILD_VERSION` with `sdk 26.0`.

## B. Native starter UI

All on the NotesCheck app from A. Notes and edits are Rust calls; the screens only show Rust's results.

| # | Check | Kind | Status |
|---|-------|------|--------|
| B1 | **Navigation.** Tap a note to open it; swipe from the left edge to go back. The gear opens Settings | Physical iPhone (macOS CI covers the model) | Not run |
| B2 | **New Note sheet.** Opens with the keyboard up; Add stays disabled until you type; once you've typed, the sheet can't be swiped away, but Cancel closes it | Physical iPhone | Not run |
| B3 | **Add.** The note appears, with a light success tap | Physical iPhone (XCTest covers adding) | Not run |
| B4 | **Edit in place.** Change a note's text, go back: the list shows the change. Done saves and hides the keyboard | Physical iPhone (XCTest covers `update`) | Not run |
| B5 | **Delete.** From the detail screen (••• › Delete) it asks first; swipe a row left to delete; long-press for Share and Delete | Physical iPhone | Not run |
| B6 | **Search.** Narrows the list (Rust `search_notes`); no matches shows "No Results for …" | Automated (Rust) + macOS CI + Physical iPhone | Rust and XCTest pass; phone not run |
| B7 | **Persistence.** Force-quit from the app switcher and reopen: the notes are still there | Automated (Rust, XCTest) + Physical iPhone | Rust and XCTest pass; phone not run |
| B8 | **Dark mode** looks right: no white patches, readable text | macOS CI (simulator render) + Physical iPhone | Simulator render reviewed on #32; phone not run |
| B9 | **Largest text size** (Settings › Accessibility › Display & Text Size › Larger Text at maximum): rows stack and wrap | macOS CI (simulator render) + Physical iPhone | Simulator render reviewed on #32; phone not run |
| B10 | **VoiceOver** reads each row's text and date, and the buttons by name | Physical iPhone | Not run |

## C. Lifecycle

The starter reports SwiftUI's scene phase to Rust (`.forgeLifecycle`). Going to the background stops requests started on screen with `Interrupted`; coming back re-reads the notes; termination shuts Rust down.

| # | Check | Kind | Status |
|---|-------|------|--------|
| C1 | Phases and resume: Active → Background → Inactive emits `PhaseChanged`, with `resumed` on the way back | Automated (Rust) + macOS CI (XCTest) + Android CI (JVM) | Pass |
| C2 | **Background interrupts a request.** In Settings, set the URL to `https://httpbin.org/delay/10` (a public test server that answers after ten seconds) and tap Check Connection. Within a few seconds, swipe up to the home screen. Wait five seconds and reopen the app: Settings shows "Stopped when the app went to the background. Try again." and no spinner | Automated + macOS CI (against a silent local server) + Physical iPhone | Automated pass; phone not run |
| C3 | **Request cancellation.** Start the same check and close Settings straight away. Reopen Settings: no spinner, no late result | Automated + macOS CI + Physical iPhone | Automated pass; phone not run |
| C4 | **Recovery after returning.** Back in the foreground after C2, Check Connection with `https://example.com` shows "HTTP 200"; adding a note still works | Physical iPhone | Not run |
| C5 | **Termination.** Force-quit while a note is being typed in the sheet, reopen: earlier notes are intact and nothing is half-written. (iOS does not always tell the app it is terminating; every command is saved before it returns, so nothing depends on it) | Automated (Rust `terminated_is_shutdown`) + Physical iPhone | Automated pass; phone not run |

## D. Native services

Every generated app installs the services (`AppServices.install()`), but the starter's screens do not use them, so the device check uses a throwaway app whose screen calls each one through the real Rust → Swift path. Follow [native-services-device-check.md](../native-services-device-check.md) for the details; the summary:

```bash
cd ~/ixOS
scripts/make-services-check-app.sh ~/forge-services-check
forge run ios --project ~/forge-services-check
```

Open **Services check** and tap **Run checks**.

| # | Check | Kind | Status |
|---|-------|------|--------|
| D0 | Services check app generated from the integrated templates; its Rust tests pass | Linux local | Pass (cloud Linux, integrated branch) |
| D0b | It compiles for iOS (unsigned Xcode build) | macOS CI | Pass on the combined branch (PR #37) |
| D1 | **Keychain.** `PASS keychain write/read`, `PASS keychain delete` | Automated (Rust fakes) + macOS CI (XCTest) + Physical iPhone | Automated pass; phone not run |
| D2 | **Permissions.** The notification prompt appears; `PASS notification permission: NotDetermined -> Granted`. Then turn notifications off in Settings and run again: `FAIL ... Denied -> Denied` and a typed `PermissionDenied` (expected); **Open Settings** goes to the app's page | Physical iPhone | Not run |
| D3 | **Notifications.** About five seconds after the run, a "Forge services check" banner appears, app open or phone locked | Physical iPhone | Not run |
| D4 | **Deep links.** Tapping the banner adds `PASS forgecheck://check/notification?from=tap → route ["check", "notification"]`; typing `forgecheck://check/url` in Safari adds a second PASS line | Automated (Rust parsing) + Physical iPhone | Automated pass; phone not run |
| D5 | **Sharing.** Share the file Rust wrote → Save to Files: `services-check.txt` opens with Rust's text | Physical iPhone | Not run |
| D6 | **File import.** Import a file: `PASS Rust read the imported file: N bytes` | Physical iPhone | Not run |

## E. Pocket Library

```bash
cd ~/ixOS/examples/pocket-library
cargo test --manifest-path rust/Cargo.toml     # 45 pass
forge run ios
```

| # | Check | Kind | Status |
|---|-------|------|--------|
| E0 | Rust crate: model, rules, persistence and recovery, catalogue parsing, EPUB extraction, history (45 tests); bindings match the crate | Automated (CI `check-app-crates.sh`) | Pass |
| E0b | SwiftUI host: 7 XCTests against the real Rust; unsigned device build; screenshots of every tab, light, dark and largest text | macOS CI | Pass on #33 and on the combined branch (PR #37) |
| E0c | Builds with xtool on Linux | Linux local | Built on Reuben's laptop from the #33 branch before integration (2m11s, SDK 26.0); not yet from the integrated branch |
| E1 | **First launch.** The Library tab says "Your library is empty" | Physical iPhone | Not run |
| E2 | **Create.** + → Enter Details: title, author, 300 pages, current page 20. Save. The book opens with status Reading (Rust starts it because the page is past 0) | Physical iPhone | Not run |
| E3 | **Edit and delete.** Edit a book's title and rating. Swipe left on another in the Library → Delete → confirm | Physical iPhone | Not run |
| E4 | **Catalogue search.** + → Search Online Catalogue → "the hobbit" → ⊕ on a result. It opens with a cover and description; searching again shows a green check on it | Physical iPhone (parsing is automated on canned responses; openlibrary.org has never been reached from CI) | Not run |
| E5 | **EPUB import.** Put an `.epub` in Files, then + → Import EPUB File: title, author, cover and an estimated page count appear | Automated (synthetic EPUB) + Physical iPhone | Automated pass; phone not run |
| E6 | **Progress and history.** Save a page, use the stepper, then the last page: Finished, with a haptic; Activity shows the session and the finish | Automated + Physical iPhone | Automated pass; phone not run |
| E7 | **Search, filter, sort.** Part of an author's name; the filter menu shows one status and changes the order | Automated + Physical iPhone | Automated pass; phone not run |
| E8 | **Persistence.** Swipe the app away, reopen: everything is as you left it | Automated + Physical iPhone | Automated pass; phone not run |
| E9 | **Offline.** Airplane mode: browsing, adding, editing and deleting work, covers still show; catalogue search says "You're offline" with Try Again and Enter Details by Hand | Automated (failure types) + Physical iPhone | Automated pass; phone not run |
| E10 | **Export.** Settings → Export Library → Share Export File → save to Files; the file opens. Turning off online search disables the catalogue item | Physical iPhone | Not run |
| E11 | **Release build.** `forge run ios --release`; repeat E1, E6 and E8 briefly | Linux local + Physical iPhone | Not run |

## F. Rebuild: a Rust change reaches the phone

| # | Check | Kind | Status |
|---|-------|------|--------|
| F1 | `forge` rebuilds the Rust library when its sources change, and only then (including on Linux, where the SDK path used to force a rebuild every time) | Automated (`scripts/test-native-rebuild.sh`) | Pass |
| F2 | The change below shows on the phone after `forge run ios` | Linux local + Physical iPhone | Not run |
| F3 | Pocket Library: the same with its own Rust (below) | Linux local + Physical iPhone | Not run |

On NotesCheck from A:

1. In `rust/src/lib.rs`, in `fn note_text`, change `let text = text.trim().to_string();` to `let text = text.trim().to_uppercase();`.
2. `forge run ios`. The output should show the Rust library being rebuilt, then the xtool build and install.
3. Open the app and add a note in lower case: it is saved in capitals. Older notes are unchanged.
4. Revert the change and run `forge run ios` again; new notes keep their case.

On Pocket Library: in `rust/src/domain.rs`, change `SESSION_GAP_MS` to `5 * 1000`, run `forge run ios`, update a book's page, wait ten seconds and update it again. Activity shows two entries instead of one. Revert it.

## Reporting back

Fill in the Status cells with the date and what you saw, or paste the results into the project thread. For a failure, the first error is enough; `forge run ios` ends a failed step with a "What went wrong" section, which is worth pasting whole. Screenshots of the starter list, a note, the New Note sheet and Pocket Library's tabs, in light and dark, can go into the docs.

Not covered here: Android (the test phone is under repair; it has its own device runbook for when it is back), App Store distribution, and the iOS Simulator, which Forge does not support for running apps.
