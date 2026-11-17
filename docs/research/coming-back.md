# Coming back: apps that lose track while in the background

Plan, 17 November 2026. Reuben asked for it after the [recurring bugs](framework-candidates.md#recurring-bugs-2026-11-17) review found the same kind of bug fixed four times, once in each of four apps: something goes stale while the app is in the background, and the app doesn't notice when it comes back.

## What iOS does, and what the framework already handles

When an app leaves the screen, iOS suspends it within seconds: sockets drop, Bonjour adverts stop, timers stop, a request in flight may be cut off. When it comes back, the app's memory is as it left it, but the world has moved on.

The framework already covers the most common case. `ForgeLifecycle` reports each phase to the Rust runtime. On the way back, the runtime publishes `PhaseChanged { resumed: true }` and `ForgeHost` raises `changes`. Every hosted app re-reads its Rust state when `changes` moves. That is why Good Dog's Today and Pocket Library's "this year" are right after a night away: they are read again with the current clock.

What it doesn't cover is everything outside the Rust state the app re-reads. That falls into four kinds.

## The four kinds, app by app

| Kind | App | What goes stale | How it is handled today |
|---|---|---|---|
| **1. A connection iOS ended** | Relay, devices | the listening port and the Bonjour advert | Swift `scenePhase`: close on background, open on active (`ContentView`) |
| | Relay, send code | the send code's advert (fixed 2026-11-15) | Swift `scenePhase`: advertise again on active (`SendCodeView`) |
| | Relay, chats | server connections | Rust closes and reopens them itself (Chat's own lifecycle thread, A31). Swift also refreshes on `becameActive`, and the model refreshes again on `PhaseChanged`, so there are two paths |
| | FieldKit | a trip being recorded | Swift `scenePhase`: `resume()` re-reads the location permission and calls `core.resumeRecording()` |
| **2. Data from the network that ages** | Weather | the forecast, and where "My Location" is (fixed 2026-11-16) | Swift `scenePhase`: refresh if the report is older than 10 minutes, and find the phone again if the fix is old |
| | Commonplace | feeds | Swift `scenePhase`: `refreshIfStale` with its own age rule, and a refresh when the network comes back (#311) |
| | Soundscape | the song library from the take server | **Not handled**: it loads only when the Listen tab appears or the server changes, so picks made on the laptop don't show until a pull to refresh (found 2026-11-17, from the code) |
| **3. Something that changed outside the app** | FieldKit | location permission changed in Settings | re-read on `resume()` |
| | Device Lab | permissions on its dashboard | re-read when it comes to the foreground |
| **4. Work cut off while away** | Commonplace | a refresh interrupted by the background | open in the friction audit (#12, "needs a device test first"): nothing tries it again on return |
| | Relay | a transfer cut off | handled: the outbox retries |
| | Pocket Library | a catalogue import | handled: it fails with `Interrupted` and the library is unchanged |

The pattern behind the four fixes: **each app finds out it is back by watching SwiftUI's `scenePhase` in a view, then tells its model or its Rust core.** The rules end up in views (Relay's `ContentView`, `SendCodeView`), sometimes in two places for one screen, and nothing checks that a new screen with a connection does the same.

## What changes

### 1. Rust is told directly, with how long the app was away (framework)

This is the shared phase hook the [everyday capabilities](everyday-capabilities.md) plan deferred "until Relay's live connections need it" (A31). Relay now needs it in three places.

- **The runtime calls the core.** A core can register `on_transition`, which the runtime calls on its own thread with the phase, `resumed`, and **`away`: how long the app was in the background**. `PhaseChanged` carries `away` too, so Swift sees the same number.
- **Long-lived work reopens itself.** Relay's listener and adverts, the send code's advert and FieldKit's recording reopen and close in their cores, not in views. Chat's own lifecycle thread (A31) moves onto the hook, which removes Relay's second refresh path.
- **Behaviour test (Linux):** a `forge-app` test reports background, waits, reports active, and checks the hook saw `resumed` and the time away.

### 2. "Refresh if it's old" uses the time away (apps, with one helper)

Weather and Commonplace each wrote their own age rule, and Soundscape has none. With `away` in hand, the rule is the same everywhere: on resume, refresh what is older than the app's limit. The limit stays the app's choice (10 minutes for a forecast, longer for feeds).

- `ForgeHost` exposes `lastResume: (at: Date, away: Duration)?`.
- **Weather** and **Commonplace** keep their limits and read the time from there.
- **Soundscape** gains a refresh of the library on resume, which fixes the gap above.

### 3. Work cut off while away is tried again once (Commonplace)

Commonplace's interrupted refresh (friction #12) is tried again when the app comes back, using the same hook. This is the only open case of kind 4.

### 4. Keeping it from coming back

- **Parity rule `resume`:** an app's Swift doesn't use `scenePhase` to open or close Rust work. The rule flags `.onChange(of: scenePhase)` in app views. The uses that stay are listed with their reason: Device Lab's own dashboard, and UI-only uses such as pausing an animation.
- **Behaviour tests per app:** a model test that reports background and then active through the host, and checks the app's reaction: Relay reconnects and readvertises, Soundscape reloads the library, Commonplace retries an interrupted refresh.
- **A standing phone step:** "leave the app for a minute (lock the phone), then come back" is added to the device checklist template, so every phone sitting tries it once per app it tests.

## Order

1. **PR 1, the hook and Relay:** `on_transition` with `away`, Relay's devices and send code moved into Rust, Chat's lifecycle thread onto the hook, the rule, and the tests. Relay is the app that triggers the hook. Phone: lock for a minute in each Relay tab; devices are still found and the send code still reaches the laptop.
2. **PR 2, the other apps:**
   - FieldKit's recording moves onto the hook;
   - `lastResume` is used by Weather, Commonplace and Soundscape;
   - Commonplace's retry of an interrupted refresh.

   Phone: one minute away for each.

Each PR moves every app it touches, or lists the lag in `docs/app-parity.toml`. Neither touches the Now Playing work, which is separate ([now-playing.md](now-playing.md)).

## Decision for Reuben

Do it as above (recommended), or only fix the open gaps app by app: Soundscape's library and Commonplace's interrupted refresh. The second is smaller, but it leaves the rules in views, which is how the four fixes came about.
