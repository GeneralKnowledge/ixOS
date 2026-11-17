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
