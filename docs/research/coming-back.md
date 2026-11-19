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
| | Pocket Library | voices downloaded in Settings | Swift `scenePhase`: Settings > Read Aloud re-reads the voices on active (found while moving the apps) |
| | Device Lab | permissions on its dashboard | re-read when it comes to the foreground |
| **4. Work cut off while away** | Commonplace | a refresh interrupted by the background | open in the friction audit (#12, "needs a device test first"): nothing tries it again on return |
| | Relay | a transfer cut off | handled: the outbox retries |
| | Pocket Library | a catalogue import | handled: it fails with `Interrupted` and the library is unchanged |
| | Pocket Library | a whole-book summary | Swift `scenePhase`: the sheet starts it again from its saved progress (found while moving the apps) |

The pattern behind the four fixes: **each app finds out it is back by watching SwiftUI's `scenePhase` in a view, then tells its model or its Rust core.** The rules end up in views (Relay's `ContentView`, `SendCodeView`), sometimes in two places for one screen, and nothing checks that a new screen with a connection does the same.

## What changes

### 1. The host tells the models, with how long the app was away (framework)

A closer look before starting (2026-11-17, 22:45) changed where the hook goes. Most of what goes stale is not in Rust: Relay's Bonjour adverts and browsing use Apple's Network framework in Swift (`RelayNetwork`), and Weather's and Commonplace's age rules are in their Swift models. Only Relay's listening port and the chats' connections are in Rust, and Rust already closes and reopens the chats itself (A31's lifecycle thread). So the hook goes in `ForgeHost`, which already receives every phase from `ForgeLifecycle`:

- `ForgeHost` notes when the app goes to the background. On the way back it sets `lastResume` (when, and how long it was away) and calls the handlers models registered with `onResume { away in … }`, in the same `report(_:)` call that already tells Rust.
- Models react there, not views. A test drives it with `host.report(.background)` then `host.report(.active)`, with an injected clock, on Linux.
- A view still runs what only a view can (Relay's `RelayNetwork` lives in a view, because model files may not import Network): it reacts to a value the model sets in `onResume`, not to `scenePhase`. The decision is in the model and tested. The view only carries it out.
- Rust is not changed. A core that needs the phase already gets it (`set_phase`, `PhaseChanged`, `rt.lifetime`).

### 2. "Refresh if it's old" uses the time away (apps)

Weather and Commonplace each wrote their own age rule, and Soundscape has none. With `away` in hand, the rule is the same everywhere: on resume, refresh what is older than the app's limit. The limit stays the app's choice (10 minutes for a forecast, longer for feeds).

- **Weather** and **Commonplace** move their `scenePhase` handling into `onResume`.
- **Soundscape** reloads the library on resume, which fixes the gap above.

### 3. Work cut off while away is tried again once (Commonplace)

Commonplace's interrupted refresh (friction #12) is tried again in `onResume`. This is the only open case of kind 4.

### 4. Keeping it from coming back

- **Parity rule `resume`:** no `.onChange(of: scenePhase)` in an app's views. The few that stay are listed with their reason: Device Lab's own dashboard (it reports the phase for its lab services, not a model's work) and Device Lab's transfer test.
- **Behaviour tests per app:** a model test that reports background, moves the clock, reports active, and checks the app's reaction:
  - Relay asks for its port and adverts again, and refreshes the chats;
  - Soundscape reloads the library;
  - Commonplace refreshes when stale and retries an interrupted refresh;
  - Weather refreshes an old forecast;
  - FieldKit resumes a recording.
- **A standing phone step:** "leave the app for a minute (lock the phone), then come back" is added to the device checklist template, so every phone sitting tries it once per app it tests.

## Order

1. **PR 1, the hook and Relay:** `ForgeHost.onResume` and `lastResume`; Relay's devices screen, send code and chats driven from their models; the rule (with the other apps listed as lag for PR 2); and the tests. Relay has the most cases. Phone: lock for a minute in each Relay tab; devices are still found, the send code still reaches the laptop, and the chats are up to date.
2. **PR 2, the other apps:** FieldKit, Weather, Commonplace (with its retry) and Soundscape move onto `onResume`, which empties the lag list. Phone: one minute away in each.

Each PR moves every app it touches, or lists the lag in `docs/app-parity.toml`. Neither touches the Now Playing work, which is separate ([now-playing.md](now-playing.md)).

## Done (2026-11-17)

Both PRs went in as one, on the same branch as this plan, because the hook was small and every app's move was a few lines:

- `ForgeHost.onBackground`, `onResume` and `lastResume` (with `clock`). A handler belongs to a model and goes with it; the same model registering again replaces its handler.
- Relay: `RelayModel.follow` sets `isAway` and counts `returns`; the devices screen and the send code react to those. Chat refreshes from `onResume`, one path instead of two.
- Weather, FieldKit, Commonplace and Soundscape: `follow(host)` on the model, called where the screen makes it. Commonplace's refresh stopped by the background no longer shows an alert and is tried again on return (friction #12).
- Pocket Library: the book summary carries on from `onResume` in its state; Settings re-reads voices on `host.lastResume`.
- Rule `resume`, with Device Lab not applicable. Tests: Relay (port and adverts, time away), Weather (a long time away finds the phone again), Commonplace (the retry), Soundscape (the reload). FieldKit's `resume()` is unchanged and only its caller moved, so it has no new test.

## Decision

Reuben chose the full plan on 2026-11-17 ("We should fix this"), over fixing only the two gaps.
