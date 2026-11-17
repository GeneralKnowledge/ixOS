# Plan: an app as a specification

**Status (2026-11-17):** plan only, nothing built. Asked for by Reuben on 2026-11-17: pick a relatively simple existing iOS app, map it through its accessibility tree, rebuild it with ixOS, and compare the two, so that every difference is evidence about the platform.

This extends the [replication benchmarks](../../examples/commonplace/docs/benchmark/benchmarks.md) that started with NetNewsWire and Commonplace on 2026-11-10. Their rules still hold: behaviour, not implementation; our own name, icon and look; nothing copied from the reference; the real iPhone, never a simulator; every workaround becomes a gap record.

## Where we are

The outside pitch describes a tool that reads an app's accessibility tree, discovers its interactions, walks its screens breadth first, records state hashes and screenshots, replays sequences and compares an original with its recreation. Most of that has been built once. What has never existed is the comparison, and the explorer that did the walking is parked.

| Piece | Today | Where |
|---|---|---|
| Read any app's accessibility tree | Works, by cable. `forge ui tree --app <bundle id>` reads a third-party app through WebDriverAgent; first done on the Claude app on 2026-11-08 | ixOS `forge-uitest` |
| Act in any app, safely | Works. `forge ui run --any-app` puts every action through the person's `~/.config/forge/device-control.toml` policy (observe, operate, full) and its hard refusals | ixOS `forge_uitest::policy` |
| Replay a sequence | Works. `forge ui run` test files find by identifier, then label, and `expect` checks a value | ixOS `forge ui run` |
| Discover interactions and walk screens | Built and run on the phone (Pocket Library, Good Dog, NetNewsWire in observe mode), then **parked** on 2026-11-15. The library is in forge-lab `crates/forge-explore`; its command glue is in `parked/` and does not compile | forge-lab |
| State hashes and screenshots | Built into the explorer: a screen's id is a hash of its structure (identifiers, kinds, labels of controls without ids), never its pixels or values, plus a screenshot and tree per new screen and a `graph.json` of transitions | forge-lab `forge-explore` |
| Breadth-first traversal | Not quite. The explorer shuffles each screen's controls with a seed and replays the shortest known path to reach unfinished screens. Breadth-first order would be a small option on the same walk | forge-lab `forge-explore` |
| A spec and gap records for a reference | Exists for NetNewsWire: spec, fixture feeds, a ten-minute person's walk and 15 gap records. The reference has **never been walked**, so every spec row is still "Expected" | `examples/commonplace/docs/benchmark/` |
| Compare an original with its recreation | **Missing.** The two apps share no identifiers, so a mapping between their screens is needed, and nothing reads two walks side by side | |

Two constraints shape the plan. UI automation is cable only (Reuben's call on 2026-11-15, after WebDriverAgent died 30 seconds after unplugging). And the free team's three sideload slots are full, which matters less than it looks: a reference installed from the App Store or shipped with iOS takes no slot, and the recreation lives in Device Lab like every other ixOS app.

## Two levels of comparison

A recreation can differ from its reference in two ways, and the plan keeps them apart because they need different tools.

1. **Screens and navigation.** Which screens exist, which controls each one offers, and where each control leads. This is the explorer's graph. Values are deliberately left out of a screen's hash, so the graph says "a calculator screen" rather than "a calculator showing 42".
2. **State and values.** For the same inputs, does the app end up showing the same thing? This is a table of sequences and the value each one leaves on screen, recorded from the reference and replayed on the recreation. It needs no explorer at all, only `forge ui run` and `forge ui tree`.

The second level is the cheaper and more revealing one for a small app, so it comes first.

## Candidate apps

What makes a good first target: small enough to rebuild in days, native UIKit or SwiftUI with Apple's own accessibility labels, deterministic (the same taps always give the same screen), installable without a sideload slot, and with no account, network or personal data to protect.

| | Apple Calculator (recommended first) | NetNewsWire (recommended second) | Apple Voice Memos (alternative) |
|---|---|---|---|
| Install | Ships with iOS, no slot | App Store, no slot | Ships with iOS, no slot |
| Source | Closed; behaviour is public | Open source, MIT | Closed |
| Size | One main screen, a history list, unit conversion, a scientific layout in landscape | Feeds, timelines, reader, settings, search | A recording list, recording, playback, rename, delete |
| Deterministic | Fully: keys in, display out | Yes with the fixture feeds served from the laptop | Mostly; recordings are made of real audio |
| Accessibility | Apple's own labels on every key (to confirm in R0) | Good, and already partly read on 2026-11-10 | Apple's own (to confirm) |
| What it tests in ixOS | The Rust and Swift boundary on every key press, a ForgeUI grid of buttons, Dynamic Type, rotation, history persistence | Networking, parsing, SQLite, the reader, navigation, settings: most of the platform | Audio recording and playback, files, permissions, the lock screen |
| Recreation already exists | No | Commonplace, slices 1 and 2 | No |
| Risk | Teaches little about the platform beyond the basics | Too large to prove the method on | Microphone and real data, so `operate` level and a watched walk |

**Calculator first** because it proves the loop end to end (map, specify, rebuild, compare) on something finished in a week, and its edge cases (repeated equals, percent, sign change, very large numbers, the display's formatting) are exactly the fidelity questions the method is meant to catch. **NetNewsWire second** because Commonplace already exists and the method then pays off on a real app. Voice Memos is the alternative if Calculator's tree turns out to be unreadable.

The Calculator recreation would be a benchmark target, not a new product. It goes in Device Lab under a Benchmarks heading with its own plain name, never in the product roadmap, so it does not break the rule that existing apps expand before new ones are added. Its look is our own ForgeUI, not Apple's round orange keys.

## Phases

Each phase ends at its exit criterion and waits for Reuben before the next starts.

### R0: look (one phone sitting, about 10 minutes, no code)

With the phone on the cable, in the next sitting already planned for Device Lab:

1. Add `"com.apple.calculator" = "observe"` to `~/.config/forge/device-control.toml` on the laptop (bundle id to confirm with `forge device apps`).
2. `forge ui tree --app com.apple.calculator` and `forge ui screenshot` on the main screen, the history and in landscape.
3. Same two commands on NetNewsWire's feeds list, if it is still installed.

**Proves:** whether Apple's own app exposes every key and the display through WebDriverAgent, which no Apple app has been tried with yet. **Exit:** every key has a label and the display's value can be read from the tree. If not, try Voice Memos in the same sitting.

### R1: map by hand

1. Write the spec from the R0 trees: screens, controls, where each leads (in the recreation's own `docs/benchmark/` folder, in the format of the NetNewsWire spec, so nothing waits for the R2 code).
2. Write the sequence table: about 40 key sequences covering ordinary sums and the edge cases, one per line (`sequences.toml`).
3. A small script runs each sequence on the reference through `forge ui run --any-app` (tapping keys by label) and reads the display with `forge --json ui tree`, writing `reference-results.toml`. It uses only commands ixOS already has, so a person can run it without an agent.

**Exit:** the table recorded twice on the phone gives identical results, which shows the reference is deterministic enough to compare against.

### R2: rebuild and compare values

1. The calculator engine in Rust, owning all state and rules; a SwiftUI and ForgeUI view; an identifier on every key (`calculator.key.7`); hosted in Device Lab.
2. `reference-results.toml` becomes a Rust unit test of the engine, so most differences show up on Linux in `cargo test`, before the phone.
3. On the phone, the same sequences replayed on the recreation by identifier, compared with the reference results.

**Exit:** every sequence matches, or each difference is written down as either a bug in the recreation (fixed) or a gap in ixOS (a record in `gaps.toml`: layer, cost, whether a second app would need it). A finding that needed a workaround outside ForgeUI or the FFI rules is a gap even if the result matches.

### R3: compare screens automatically (the explorer question is decided here)

1. Wire the parked explorer into a `forge-lab explore` command, add a breadth-first order option, and walk both apps.
2. A `forge-lab compare` that reads two `graph.json` files and a hand-written screen mapping (reference screen to recreation screen), and reports screens, controls and transitions present in one and not the other.

**Exit:** comparing the recreation with itself reports no differences, and comparing it with the reference reports only differences that are real when a person checks them.

### R4: the real test

Run R1 to R3 on NetNewsWire and Commonplace, with the fixture feeds. This is where most platform gaps should appear, and where the [NetNewsWire spec](../../examples/commonplace/docs/benchmark/spec.md) rows finally move from Expected to Seen.

## The decision: the explorer

**Decided 2026-11-17 (Reuben): decide at R3.** R0 to R2 go ahead without it.

The explorer was frozen by the reinvention audit (2026-11-11) and moved to forge-lab (2026-11-15), where it "can grow on its own" but nothing has been wired back in. R0 to R2 do not need it. R3 does.

| Option | What happens |
|---|---|
| **Decide at R3** (recommended) | R0 to R2 use only `forge ui tree` and `forge ui run`, which are not frozen. The explorer question comes back with R2's evidence, at or after the 2026-12-15 freeze review |
| Revive it in forge-lab now | Wire `parked/` into a `forge-lab explore` command alongside R1, so the screen graph is available from the start. More work up front, before the method has proved itself |
| Never revive it | A person walks the screens, as the NetNewsWire walk already does, and only the value table is automated |

## What stays out

- No AI in the loop. Nothing asks a model whether two screens look alike; the comparison is structural and value based.
- No pixel comparison between the two apps: they are meant to look different.
- No coordinate taps (AGENTS.md rule 4). If a reference control has neither identifier nor label, it is reported, not tapped.
- No new tool in ixOS. Device tooling here gets bug fixes only; anything new for walking and comparing lives in forge-lab.
- No Wi-Fi automation; the cable is enough for a ten-minute sitting.

## First step

R0, in the next phone sitting: one policy line on the laptop and about ten minutes of `forge ui tree` and `forge ui screenshot` on Calculator. Nothing is built before it.
