# Framework candidates, 17 November 2026

This is a survey of what the apps built that now belongs in the framework, judged by the [extraction gates](../roadmap.md): a second app has to use the feature, both uses must have passed on an iPhone, the app's identity stays in the app, and Apple's frameworks are wrapped, never replaced. Reuben asked for this on 2026-11-17, agreed with the two feature extractions, and asked for the lower-level items to be done or planned.

## Feature extractions (agreed, each gets its own plan PR)

| What | Apps | Waiting on |
|---|---|---|
| Photo picker: turning a picked photo into a temporary file Rust reads, next to `ForgePickedFile` | Relay (Send, and photos in Chats), Soundscape (song artwork) | Soundscape's artwork picker passing on the phone. Relay's photo send passed on 2026-11-17. Soundscape, not FieldKit, is the second app: it already has its own copy (`LibraryView.swift`) |
| Now Playing: lock screen, headphone controls and interruptions (everyday capabilities phase 5) | Pocket Library (Read Aloud), Soundscape (song player) | #306 merging and Soundscape's lock screen passing a recheck (it failed on 2026-11-17) |

Already in the framework: web images (`ForgeRemoteImage`, #298), Undo banners (`ForgeBanner`, #294), haptics (`ForgeFeedback`, #309).

The following need a second app first: full-text search (Commonplace has it, Pocket Library hasn't built its own) and chart styling (FieldKit only; Good Dog's progress would be the second).

These stay in their apps: Relay's transfers and outbox, Device Lab's Checks, Servers and Logs (tooling), the eSIM compatibility check, the lifecycle hook Chat needed (Chat now lives inside Relay, so that is still one app), and each app's error wording and glossary links.

## Lower-level items

### 1. Network error conversion: done

`forge_app::app_error_conversions!` has a `NetError` arm. Commonplace and Pocket Library each wrote the same `From<NetError>` by hand, and both now use the arm. Weather (a missing TLS set-up is `Internal`) and Soundscape (its own server is `Unreachable`) word network failures differently, so they keep their own conversions. Commonplace's own network monitor moved onto the `connectivity` service in the same PR, and the parity rule `connectivity` keeps it from coming back.

### 2. Swift command helpers: adopt what exists; don't add a new helper

**Cancelling Rust work from Swift.** ForgeKit already has `withRustCancellation`. It makes a `Cancellation`, runs the Rust call, and cancels the call when the Swift task is cancelled. Seven model methods in three apps still write that out by hand with `Cancellation()` and `withTaskCancellationHandler`:

- Commonplace: `add(url:)` and `refresh()`
- Relay: `pair(code:)` and `sending(_:)`
- Soundscape: `waveform`, `refreshLibrary` and `song`

Plan, one PR:

1. Move all seven onto `withRustCancellation`. Commonplace's refresh keeps its Stop button by storing the `Cancellation` it is handed (`refreshing = cancellation`) inside the closure.
2. Add a parity rule, `rust-cancellation`. It fails any app whose Swift pairs `Cancellation()` with `withTaskCancellationHandler`. Soundscape's `LoopEngine` uses `withTaskCancellationHandler` for an audio continuation with no Rust call, and Weather's client doesn't use it at all, so neither one trips the rule.
3. Proof: the Linux model tests still pass. The combined Device Lab build compiles the change for the iPhone. On the phone, one cancellation per app: close Add Feed while it is fetching, leave the Send screen while it is preparing, and leave a song while it is loading.

**The `perform` function.** Seven models have one: run a Rust command, reload, then show the error. The copies differ in what matters. Each puts the error somewhere different (`alert`, `actionError`, `errorMessage`, `report`), some play a haptic on success, Good Dog passes the clock in, and Chat and AI Lab run synchronously. A shared version would need a parameter for each of those differences and would save about eight lines per app, so it stays in the apps. Revisit it only if a bug turns up in one copy that the others share.

### 3. Distance maths: not yet

`forge_services::sensors::distance_m` lives in the native services crate. Moving it would only help a pure-Rust core that needs distances without any native service (friction log A4). Both users today, FieldKit and Device Lab, already select the `location` service, so they depend on that crate anyway. Nothing would get simpler now.

The trigger is the first app that needs geo maths without location. When that happens, move `distance_m` into a small `geo` module in `forge-runtime` and keep a re-export at the old path so nothing breaks. Until then it stays where it is, and A4 stays "accept for now".

## Apps not using what the framework already has

On 2026-11-17 Reuben asked to look for places where apps do by hand what the framework already offers. Each item below moves the apps onto the existing helper and adds an app-parity rule so the hand-written version can't come back.

| # | What the apps do by hand | Framework helper | Where | Plan |
|---|---|---|---|---|
| 1 | Make a `Cancellation` and wire it to `withTaskCancellationHandler` | ForgeKit `withRustCancellation` | 7 places: Commonplace, Relay, Soundscape | PR A, rule `rust-cancellation` (item 2 above) |
| 2 | Error text in raw `.red` (`Text(problem).foregroundStyle(.red)`) | ForgeUI `.forgeForeground(.danger)`, which passes contrast in light and dark | 12 places: Device Lab, FieldKit, Relay, Sketchbook, Soundscape | PR A, widen the `toned-text` rule to red. A red that marks recording (Soundscape's "Recording one pass") is a state colour, not an error; it moves to the same tone |
| 3 | Swift logging through `print` and `NSLog`, which bypass the log pipeline (`forge device logs`, crash reports) | `ForgeLog` (crates call `forge_runtime::log_export!()`) | Pocket Library (model), Relay (`ChatModel`). Pocket Library and Checklist don't export `ForgeLog` at all | PR A, rule `swift-logging` (no `print` or `NSLog` in `Sources/AppHost`; launchers and Device Lab's crash test are exempt) |
| 4 | Empty, missing and failed screens built directly with `ContentUnavailableView` | ForgeUI `ForgeStateView` (`empty`, `noResults`, `loading`, `failed` with a retry button and identifier) | 22 places in 9 apps; Pocket Library and Relay have the most | PR B on its own, since screens change look slightly: a glance at each app in Device Lab. Rule `state-views` |
| 5 | Soundscape sets `AVAudioSession` itself | `ForgeAudioSession` (no music mode yet) | Soundscape | Part of the Now Playing extraction, which adds the music mode |

Already in flight: outcome haptics through `ForgeFeedback` in every app (#309).

These were checked and are already used where they apply: share sheets, Keychain, notifications, permissions, URLSession requests (`http`), location, picked files, web images, file storage (`FileStore`, `JsonFile`, atomic writes), Rust logging and date formatting. Plain SwiftUI `ShareLink` for app-made content is fine: `ForgeShareLink` is for share items that come from Rust.

**PR A merged (#313, items 1 to 3):** rules `rust-cancellation`, `swift-logging` and a wider `toned-text`. Soundscape's mixdown moved onto `withRustCancellation` too, which also makes leaving its screen stop the mixdown in Rust; before, only Stop did.

Order: PR A (items 1 to 3, mechanical, Linux tests plus the iPhone build) after #311 merges and passes its phone check, then PR B. Neither PR touches the photo picker or Now Playing: those stay gated on their second-app phone checks.

### Reuben's review (2026-11-17)

He approved the direction and the two-PR split, with four tightenings and one rule:

**A parity rule proves adoption, not correct use.** Banning `print` shows the old way has gone, not that useful logs arrive. Each rule below is paired with a test of the behaviour where one can be written.

**PR A, cancellation: behaviour, not just compiling.** Tests and phone checks show that:

- cancelling while the Rust operation runs stops it in Rust (`Cancelled` comes back), not only the Swift task;
- closing the screen mid-operation cancels it;
- nothing updates the screen after a cancellation (no late `lastRefresh`, no alert for `Cancelled`);
- Commonplace's Stop still stops the refresh itself. The `Cancellation` handed to `withRustCancellation` is stored for Stop, and a Linux model test starts a refresh against a slow fixture, presses Stop, and checks that Rust reports `Cancelled` and nothing was saved.

**PR A, red text: only error presentation.** The rule doesn't ban `.red`. It matches error text: `Text(<problem|error|failure>…).foregroundStyle(.red)`, and a `.red` foreground on a line that shows a model's problem or error message. A state colour such as Soundscape's recording indicator is left alone and doesn't count as a lag. Fills and tints stay any colour, as `toned-text` already allows.

**PR A, logging: the bridge in every affected app.** Pocket Library and Checklist gain `forge_runtime::log_export!()` and regenerated bindings, so `ForgeLog` exists in them. Pocket Library's `print` and Relay's `NSLog` become `ForgeLog` calls. The rule exempts exactly two named files, `*Launcher/AppMain.swift` (command-line launchers, which print to a terminal) and Device Lab's `LabModel.crashTestSwift`, and nothing else. Behaviour test: a Linux test per app writes through `ForgeLog` and reads the record back from `forge_runtime::logging`'s recent records.

**PR B, state views: an interaction audit before any change.** For each of the 22 screens, a table records what it shows today (empty, no results, missing, failed, loading), its actions, and its identifiers. The migration keeps:

- empty, no results and failed as distinct states;
- Try Again calling the operation that failed (a model test per app with a retry);
- loading never showing briefly as empty (state comes from the model's load state, not from an empty list);
- every existing navigation and recovery action;
- identifiers that stay stable, so existing UI scenarios still find them; new ones are named `screen.element`.

Then a look at each affected app in Device Lab.

**What stays out.** This is not a campaign against every app-specific implementation. `perform`, the distance maths, each app's error wording and other app behaviour stay where they are.

## PR B: the state-view audit

Done before any screen changed. Each row is a place an app drew a state with `ContentUnavailableView`, what it showed, its actions and identifiers, and what it is now. The 22 counted on 17 November grew to 33 with screens added since (Device Lab's dashboard and logs, Relay's assistant and trusted devices). 31 moved; Checklist's 2 stay.

Four things ForgeUI's `ForgeStateView` couldn't do, which several apps needed, were added to it rather than left as exceptions: a second action (Pocket Library's "Enter Details by Hand", Weather's "Search for a City"), advice and an action on a search with no results (Find a Book), an action that is a menu (Relay's "Start a Chat", Pocket Library's "Add Book"), and a title or message from the app's data shown as written (a place name, a reason from Rust). `noResults` now draws the system's own search state, in the system's own language, instead of a copy of its English words.

| App | Screen | Showed | Actions (identifiers) | Now |
|---|---|---|---|---|
| Device Lab | Audio Events | empty | none | `empty` |
| Device Lab | Heat Changes | empty | none | `empty` |
| Device Lab | Recent Changes (network) | empty | none | `empty` |
| Device Lab | Logs | empty (nothing logged), no results (filter or search) | none | `empty`, `noResults` with the search words |
| Relay | Assistant | empty | Start a Chat menu (`assistant.new`, `assistant.character.<id>`) | `empty` with a menu action, same identifiers |
| Relay | Chats search | loading, no results | none | `loading`, `noResults` |
| Relay | Chats list | loading until first listed (`hasListed`), empty | none | `loading`, `empty`; a list error stays its own banner |
| Relay | Chat search | no results | none | `noResults` |
| Relay | Pair a Device | empty (no Wi-Fi, no code) | Done in the toolbar (`code.done`) | `empty` |
| Relay | Chats tab, core failed to open | failed | Try Again → `chat.open()` (`content.retry`) | `failed` with retry; tested by `testALoadFailureShowsWhyAndTryAgainOpens` |
| Relay | Trusted device | missing (no longer trusted) | back | `empty` |
| Relay | Device | missing (forgotten) | back | `empty` |
| Weather | A place with no forecast | failed, or empty (no location yet) | Try Again → `refresh(page)` (`forecast.retry`) or Use My Location (`forecast.useLocation`); Search for a City (`forecast.search`) | `failed` when the page has an error, else `empty`; same actions and identifiers; tested by `testALoadFailureShowsAMessageAndTryAgainLoads` |
| Good Dog | Exercise | missing | back | `empty` |
| Good Dog | Glossary term | missing | back | `empty` |
| Good Dog | Learn search | no results | none | `noResults` |
| Pocket Library | Find a Book, before searching | empty | none | `empty` |
| Pocket Library | Find a Book, search failed | failed | Try Again when retryable (`catalogue.retry`), Enter Details by Hand (`catalogue.manual`) | `failed` with retry and a second action; tested by `testTryAgainRepeatsTheFailedCatalogueSearch` |
| Pocket Library | Find a Book, no results | no results with its own advice | Enter Details by Hand (`catalogue.noResults.manual`) | `noResults` with its advice and the action |
| Pocket Library | Book | missing (deleted) | back | `empty` |
| Pocket Library | Chapter summary, book summary | unavailable (reason from the system) | none | `empty` with the reason as written |
| Pocket Library | Reader | failed to open | back | `failed` with no retry, as before |
| Pocket Library | Library | empty (no books), empty (none with this status), no results | Add Book menu (`library.empty.add`, `library.add.catalogue`, `.manual`, `.epub`) | `empty` with the menu, `empty`, `noResults`; the library shows only once it has loaded (`phase == .open`) |
| Soundscape | Project | missing (deleted) | back | `empty` |
| Commonplace | Search | no results; loading stays a row in the list | none | `noResults` |
| Sketchbook | Sketch | missing (deleted) | back | `empty` |
| Starter template | Note | missing (deleted) | back | `empty` |
| Checklist | Items | empty, no results | none | unchanged: Checklist uses ForgeKit without ForgeUI, so the rule doesn't apply to it |

What changes on screen: failed states now use ForgeUI's warning triangle instead of each app's own symbol, and their buttons use ForgeUI's styles instead of `.borderedProminent`. Everything else (wording, actions, identifiers, when each state shows) is as it was. The rule `state-views` fails an app that uses ForgeUI and draws `ContentUnavailableView` itself.

Reuben's review of PR B (2026-11-17) approved it. Two follow-ups are left for later, outside this PR:

- A menu action is a `ForgeStateAction` with an empty `action`. The type could tell a button from a menu explicitly the next time ForgeUI's API changes anyway.
- Pocket Library's retry test checks the request and the Rust search, not the button. A UI scenario that taps Try Again on a failed Find a Book search would cover the whole interaction.

The `state-views` rule shows that the old view has gone, not that every state is the right one; the audit table above is the record of that.

## Recurring bugs (2026-11-17)

Reuben asked to look out for bugs that keep coming back. This comes from the friction log (A28 to A42), today's fixes and the phone results. Each kind is listed with where it has shown up and what now stops it.

| Kind | Seen in | What stops it now |
|---|---|---|
| A search says "No Results" while it is still running, or a replaced search's late answer overwrites the newer one | Weather (fixed 2026-11-17), Pocket Library's Find a Book, Commonplace, Relay's chat list (each guarded by hand). **Still open until this PR:** Relay's search inside a chat and Checklist's search | Both fixed here by remembering which words the results answer, as Relay's chat list and Commonplace already do. No parity rule: a script can't tell a search from a list. `ForgeStateView`'s documentation and the [state-view audit](#pr-b-the-state-view-audit) say loading is never shown as empty; `ForgeLoad` is the shared shape for new searches |
| Audio rules that only the phone shows | Soundscape's three Studio crashes (A38), the song player's lock screen (failed, then fixed by #306), Read Aloud's blank lock screen (fixed in #313), and Read Aloud not pausing for calls or unplugged headphones (found today, from the code) | The [Now Playing plan](now-playing.md): one helper with Soundscape's rules, which passed on the phone, and a Linux test of those rules |
| Failures nobody sees | Alerts behind pushed screens (A33), dropped `try?` and empty catches (A34), alerts under sheets (A39) | Parity rules `alerts-on-top`, `errors-shown`, `alerts-over-sheets`. Nothing new found today |
| Swift only the iPhone build catches | A28, Soundscape's `flatMap` (2026-11-16), AI Lab's capture (2026-11-17), and the published-ForgeKit Checklist job in #313 | Process: the combined Device Lab build compiles a PR's Swift before or straight after it merges. Model files compile on Linux in CI |
| State gone stale after the app comes back from the background | Relay's send code (2026-11-15), Weather's My Location (2026-11-16), Relay's chats reconnecting, Commonplace's offline refresh (#311) | Fixed for every app in the same PR as this table: `ForgeHost.onResume`, with each app's model following it, and parity rule `resume` ([coming-back.md](coming-back.md)) |
| A bar floating over a list hides its last rows | Soundscape's mini-player over a long album (seen on the phone; fixed by hand in #320). Commonplace's Undo banner over a pushed timeline (seen on the phone 2026-11-18), and possibly Weather's bottom bar over its pages | Planned in [screen-space.md](screen-space.md): ForgeUI's `forgeBar` gives its height to every scroll view below it, `forgeBanner` is built on it, and rule `floating-bars` plus a "tap the last row" UI scenario keep it fixed |

The first row's two fixes are in the same PR as the Now Playing plan. Their phone check: type a search in a Relay chat and in Checklist; while it runs, it shows a spinner, never "No Results".
