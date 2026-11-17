# Evaluation framework

Status: v0.1, 2026-11-17. Part of the [External Projects Atlas](README.md).

One scorecard, so very different projects can be compared fairly, and one evidence block, so a verdict can be checked later against what was actually inspected.

## Scorecard

Each dimension gets **Strong**, **Adequate**, **Weak** or **Unknown**, with one line of reason. Unknown is an honest answer; it is not scored as Weak or Adequate.

| Dimension | What we assess |
|---|---|
| Capability | Does it do something ixOS cannot do today? |
| Product value | Does it materially improve a person's experience in an ixOS app? |
| Architecture | Does it fit Rust owning state and SwiftUI presenting it, ForgeUI and ForgeAppKit, and the FFI rules? |
| Platform | Does it work on a real iPhone (iOS version, memory, the free Apple team's limits)? |
| Development | Can it be built and tested with the Linux and xtool workflow, with no Mac? |
| Maintenance | Is it actively maintained and technically credible? |
| Licence | Is its licence compatible with how ixOS is distributed (MIT apps and ForgeKit)? |
| Security | Does it add a new trust boundary, network surface or data risk? |
| Integration cost | How much code, binary size and ongoing upkeep would it add? |
| Reusability | Could more than one ixOS app use it? |

## Five outcomes

| Outcome | Meaning | Recorded in |
|---|---|---|
| **Adopt** | Integrate into an existing app or framework module, in its own implementation PR | [decisions/adopted.md](decisions/adopted.md) |
| **Borrow** | Reproduce a technique, design or algorithm without adding a dependency | [decisions/borrowed.md](decisions/borrowed.md) |
| **Study** | Keep as a product or architectural reference | The [register](prior-research-register.md) |
| **Watch** | Interesting, but not mature or relevant enough yet; says what would change that | [decisions/watchlist.md](decisions/watchlist.md) |
| **Reject** | Unsuitable or redundant, with the reason | [decisions/rejected.md](decisions/rejected.md) |

An Adopt verdict is a recommendation. The dependency still needs a second use, a Linux and xtool build, a phone run, and a capability ledger entry when it changes what ixOS can do.

## Evidence every assessment records

| Field | What | Example |
|---|---|---|
| Date | When it was inspected | 2026-11-18 |
| Inspected | Repository and commit, or release tag | `owner/repo` at `abc1234` (v1.2.0) |
| Licence | As the repository states it that day, and where | MIT (`LICENSE`) |
| Activity | Last commit, release cadence, open issues, as seen that day | Last commit 2026-11-01 |
| Read or run | Source read, built, run on Linux, run on iPhone: which ones | Source read; built with xtool; not run on iPhone |
| Build evidence | The command and its result, or "not built" | `scripts/check-ios-xtool.sh ...` passed on the laptop |
| Platform evidence | Device, iOS version and what was seen, or "not run". Only a person's report counts for a phone result | iPhone 15 Pro Max, iOS 26.3.1, Reuben, 2026-11-18 |
| ixOS baseline | The ixOS commit and the code it was compared with | main at `98a8446`; `examples/relay/rust/src/outbox.rs` |
| Verdict and reason | One of the five outcomes, and why, in a sentence or two | Borrow: ... |
| Reopen when | The requirement change that would justify looking again | Prefetching becomes a real need |

Results not seen are written as not run, never as passing (AGENTS.md rule 8).

## Assessment template

Copy to `assessments/<area>/<project>.md`.

```markdown
# <Project>: <the question in a few words>

Status: <date>. Part of the [External Projects Atlas](../../README.md). Research only.

## Question

<What ixOS wants to know, and which app or capability it is for.>

## Evidence

| Field | Value |
|---|---|
| Date | |
| Inspected | |
| Licence | |
| Activity | |
| Read or run | |
| Build evidence | |
| Platform evidence | |
| ixOS baseline | |

## What it does

<Short, from the source, with file paths.>

## Compared with ixOS

<What ixOS has today, what this adds, where it would belong (app, ForgeUI, ForgeAppKit, Rust crate, check, docs).>

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | | |
| Product value | | |
| Architecture | | |
| Platform | | |
| Development | | |
| Maintenance | | |
| Licence | | |
| Security | | |
| Integration cost | | |
| Reusability | | |

## Verdict

**<Adopt / Borrow / Study / Watch / Reject>.** <Reason.>

Reopen when: <condition>.

## Next step, if any

<One proposal with its size, or none.>
```
