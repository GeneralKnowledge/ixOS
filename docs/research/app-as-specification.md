# Plan: an app as a specification

**Status (2026-11-18):** R0 done on the phone (exit met): Calculator's [profile](expeditions/calculator/profile.md), [findings](expeditions/calculator/findings.md) and [evidence](expeditions/calculator/evidence/README.md). Nothing built. Asked for by Reuben on 2026-11-17: pick a relatively simple existing iOS app, map it through its accessibility tree, rebuild it with ixOS, and compare the two, so that every difference is evidence about the platform.

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

## The method: an expedition

Reuben's framing (2026-11-17): the question is not only "can we rebuild it?" but "what can you learn about a piece of software by exploring everything it lets you observe?" Calculator is the first specimen; the tools are the expedition's equipment.

**Five activities**, and the phase each happens in:

| Activity | What it means | Phase |
|---|---|---|
| Survey the terrain | Screenshots and trees: what screens and controls are there, before deciding what they mean | R0 |
| Collect evidence | Labels, roles, values, element relationships, the result of each interaction | R0, R1 |
| Build a map | Connect screens, controls and sequences; separate what is known from what is inferred and what is unexplored | R1 (by hand), R3 (the explorer, if revived) |
| Reconstruct the artefact | Rebuild from the evidence alone, and find out which assumptions were wrong | R2 |
| Verify the findings | Replay the same interactions, compare, and go back to the original whenever something doesn't make sense | R2, R3 |

**Five representations** the app gives of itself, each showing something different and none the whole story: visual (screenshots), structural (the accessibility tree: roles, labels, relationships), behavioural (what taps and sequences do), navigational (which screens lead where) and temporal (what changes after repeated presses, a relaunch, or time). R0 to R2 collect the first four for Calculator and the temporal ones that are cheap (repeated equals, history after a relaunch).

**Every finding is tagged** in the findings register and the [log](../blog/calculator/log.md):

- **Observed:** captured directly, with where it came from (a tree file, a screenshot, a recorded result).
- **Inferred:** believed from the evidence, with the evidence named. An inferred rule becomes Observed only when a sequence that tests it has been recorded.
- **Unknown:** not worked out yet. Unknowns are listed, not guessed: a guess in the rebuild is marked as one.

**The tree is one map, not the boundary.** The method is to investigate everything the app makes observable: the accessibility tree, screenshots and layout, its answers to unusual interactions, what survives a relaunch, its effects on the system (permissions, the clipboard, notifications), and its errors, omissions and controls that can't be reached. A phase that only looked at the tree has skipped part of its job.

**A failed investigation is a finding.** A control with no useful label, a value the tree won't give up, a screen WebDriverAgent can't see: each is recorded as an Observed limitation, with what was tried, and kept apart from any assumption made to work around it. Nothing found about a specimen is thrown away when the expedition moves to another one.

### What every expedition produces

The same six outputs for every specimen, so the next expedition starts from a template rather than inventing the process again. They live in `docs/research/expeditions/<specimen>/`, created at R0 and filled in as the phases go; the recreation's code links to it.

| Output | What it holds | Started in |
|---|---|---|
| `profile.md`: specimen profile | The app's identity (name, bundle id, version), the phone and iOS version, how it was installed, the policy level used, and the constraints (licence, data, what we won't copy) | R0 |
| `evidence/`: evidence catalogue | Screenshots, saved trees and recorded values, each named by date, screen and what produced it. Trees and screenshots of an app holding personal data are not committed | R0 |
| `map.md`: field map | Screens, controls and transitions | R1 |
| `findings.md`: findings register | Every finding with its tag (Observed, Inferred, Unknown), its source, and the representation it came from, failed investigations included | R0 |
| `reconstruction.md`: reconstruction record | Implementation decisions, and each assumption the recreation makes where the evidence ran out | R2 |
| `verification.md`: verification report | Confirmed behaviours, differences and their verdicts (our bug, or a platform gap with a record in `gaps.toml`) | R2 |

This is a folder of Markdown and saved files, not a tool. Nothing new is built for it before R0.

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
| Size | One main screen, a history list, unit conversion, a scientific mode (assumed to be the landscape layout; R0 found landscape is the basic keypad, see the findings) | Feeds, timelines, reader, settings, search | A recording list, recording, playback, rename, delete |
| Deterministic | Fully: keys in, display out | Yes with the fixture feeds served from the laptop | Mostly; recordings are made of real audio |
| Accessibility | Apple's own labels on every key (to confirm in R0) | Good, and already partly read on 2026-11-10 | Apple's own (to confirm) |
| What it tests in ixOS | The Rust and Swift boundary on every key press, a ForgeUI grid of buttons, Dynamic Type, rotation, history persistence | Networking, parsing, SQLite, the reader, navigation, settings: most of the platform | Audio recording and playback, files, permissions, the lock screen |
| Recreation already exists | No | Commonplace, slices 1 and 2 | No |
| Risk | Teaches little about the platform beyond the basics | Too large to prove the method on | Microphone and real data, so `operate` level and a watched walk |

**Calculator first** because it proves the loop end to end (map, specify, rebuild, compare) on something finished in a week, and its edge cases (repeated equals, percent, sign change, very large numbers, the display's formatting) are exactly the fidelity questions the method is meant to catch. **NetNewsWire second** because Commonplace already exists and the method then pays off on a real app. Voice Memos is the alternative if Calculator's tree turns out to be unreadable, and Calculator's findings are kept either way.

The Calculator recreation would be a benchmark target, not a new product. It goes in Device Lab under a Benchmarks heading with its own plain name, never in the product roadmap, so it does not break the rule that existing apps expand before new ones are added. Its look is our own ForgeUI, not Apple's round orange keys.

## Phases

Each phase ends at its exit criterion and waits for Reuben before the next starts.

### R0: look (one phone sitting, about 10 minutes, no code)

With the phone on the cable, in the next sitting already planned for Device Lab:

1. Add `"com.apple.calculator" = "observe"` to `~/.config/forge/device-control.toml` on the laptop (bundle id to confirm with `forge device apps`).
2. `forge ui tree --app com.apple.calculator` and `forge ui screenshot` on the main screen, the history and in landscape.
3. While the cable is in, a few observations outside the tree: close Calculator from the app switcher and reopen it (is the last number or the history still there?), and note anything it asks for or changes outside itself.
4. Same two commands on NetNewsWire's feeds list, if it is still installed.
5. Start `profile.md`, `evidence/` and `findings.md` for Calculator from what came back.

**Proves:** whether Apple's own app exposes every key and the display through WebDriverAgent, which no Apple app has been tried with yet. **Exit:** every key has a label and the display's value can be read from the tree. If not, that is recorded as an Observed limitation in Calculator's findings (kept, not discarded) and Voice Memos is tried in the same sitting.

### R1: map by hand

**Before running R1, change the policy on purpose, and change it back after.** `run_sequences.py` presses Delete without asking, and Forge only allows that when Calculator is at `full` in the person's own `~/.config/forge/device-control.toml`:

1. Back up the file, then change only Calculator's line to `"com.apple.calculator" = "full"`. At `full` there is no commit gate for that one app (secure fields and the hard refusals still apply). Only the person decides this; Reuben agreed for this run on 2026-11-18 ("yes, full, then back to operate").
2. Run the passes (below).
3. Put the line back to `"com.apple.calculator" = "operate"`, or restore the backup, as soon as the passes finish or a step fails.

Without step 1, the first Delete step stops at the commit gate ("Allow it? [y/N]") and, with nobody at the terminal, fails.


1. Write the field map from the R0 evidence: screens, controls, where each leads (`map.md`), and move what it shows into the findings register.
2. Write the sequence table: about 40 key sequences covering ordinary sums, the edge cases and some nobody sensible would press, plus a relaunch in the middle of a few to test what persists (`evidence/sequences.toml`).
3. A small script runs each sequence on the reference through `forge ui run --any-app` (tapping keys by their ids: Seven, Add, Equals…) and reads the display with `forge --json ui tree` in portrait (R0 found the answer is missing from the tree in landscape), stripping every U+200E, writing `evidence/reference-results.toml`. It uses only commands ixOS already has, so a person can run it without an agent.
4. Forge does the pressing, not the person: keys (Delete included), History and Close, through `forge ui run` steps. Landscape is left out for now (Reuben, 2026-11-18). Unattended Delete needs Calculator at `full` in the person's policy file, because at `operate` the commit gate stops at the key's "Delete" label; that is the person's call, and the taps go into the real History. The only thing left to the person is opening Calculator, until Forge's `--app` can launch a system app (R0 finding 22).

**Exit:** the table recorded twice on the phone gives identical results, which shows the reference is deterministic enough to compare against, and every rule in the field map is in the findings register, tagged Observed, Inferred or Unknown.

### R2: rebuild and compare values

1. The calculator engine in Rust, owning all state and rules; a SwiftUI and ForgeUI view; an identifier on every key (`calculator.key.7`); hosted in Device Lab.
2. `evidence/reference-results.toml` becomes a Rust unit test of the engine, so most differences show up on Linux in `cargo test`, before the phone.
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
