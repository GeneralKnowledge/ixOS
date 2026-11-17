# External Projects Atlas

Status: v0.1, 2026-11-17, written against main at `98a8446`. Research only: nothing here is a dependency, and nothing enters an app because it appears here.

ixOS looks at a lot of other people's work: libraries, complete apps, design systems, protocols. This area keeps track of what we looked at, what we decided, and what we might look at next, so a project we already settled isn't investigated again as if it were new, and a project that looked promising months ago doesn't quietly become a recommended dependency.

It is separate from [ixOS Experience Research](../experience/README.md) (#285), which studies apps for UX, and from the UI programme's implementation PRs. Where a project was studied there, the register here links to that work instead of repeating it.

## Four different states

A project being **known**, **researched**, **borrowed from** and **adopted** are four different things, and every entry says which one applies.

| State | Meaning | Example |
|---|---|---|
| Known | Named as a candidate; nobody has read its code or built it | Most of the [atlas](atlas.md) |
| Researched | Someone read it, or ran it, and wrote down what they found | Drift, Daak (#297) |
| Borrowed | We reproduced an idea, technique or design without adding the dependency | swift-navigation's state-driven destinations in ForgeAppKit |
| Adopted | It is a dependency in shipped ixOS code | iroh and iroh-blobs in Relay |

## The pages

| Page | What it holds |
|---|---|
| [prior-research-register.md](prior-research-register.md) | Everything already investigated, where, and what was decided. **Read this first**: an entry here is not restarted without new requirements |
| [atlas.md](atlas.md) | The candidate catalogue: 12 areas, each project with what it might give and its state. New finds are marked *unverified* |
| [capability-map.md](capability-map.md) | Capabilities ixOS could gain across apps, whether or not an external project is reused |
| [evaluation-framework.md](evaluation-framework.md) | The scorecard, the five outcomes, and the evidence every assessment must record |
| [decisions/](decisions/) | One list per outcome: [adopted](decisions/adopted.md), [borrowed](decisions/borrowed.md), [rejected](decisions/rejected.md), [watchlist](decisions/watchlist.md) |
| [assessments/](assessments/) | One file per evidence-backed assessment, in a folder per area |

## Rules

1. **The atlas is a discovery shortlist, not an endorsement.** A classification in [atlas.md](atlas.md) is provisional until an assessment exists.
2. **Prior decisions stand.** A project in the [register](prior-research-register.md) is reopened only when a real requirement changes, and the reopening says which one. GRDB and ScanCodes were rejected; Nuke was not rejected as a library, but ForgeImages was extended instead (#298), and that decision stands unless requirements change.
3. **No dependency from research.** An assessment can recommend Adopt; the dependency is added in its own implementation PR, under the normal rules (AGENTS.md: a real second use, Linux and xtool first, the capability ledger, app parity).
4. **Every assessment carries its evidence** (see [evaluation-framework.md](evaluation-framework.md#evidence-every-assessment-records)): the date, the commit or release inspected, the licence, build evidence, platform evidence and the reason for the verdict. An assessment without them is a note, not a verdict.
5. **Linux and xtool are part of the question.** "Works on iOS" is not enough; whether it builds with xtool on Linux and runs on a physical iPhone with the free team is recorded separately.
6. **Licences as found.** Licences are as the repository states them on the date of the assessment. AGPL projects (Signal, Ice Cubes, Bitwarden's apps) are read for behaviour and structure only; no code is copied.

## Adding an assessment

1. Check the [register](prior-research-register.md). If the project is there, start from that work.
2. Copy the template in [evaluation-framework.md](evaluation-framework.md#assessment-template) to `assessments/<area>/<project>.md`.
3. Fill in the evidence block before the verdict.
4. Move the project's line in [atlas.md](atlas.md) from *unverified* to the verdict, add it to the matching list in [decisions/](decisions/), and add it to the register.

## Priority 0 investigations

Each is its own docs PR with one assessment, run one at a time:

| Order | Project | Question | Assessment |
|---|---|---|---|
| 1 | CVS Health SwiftUI accessibility techniques (a11y-check) | Can its rules become a mandatory check next to `forge check swift`, on Linux? | Not started |
| 2 | FluidAudio | Can a shared local speech capability build with xtool and run on the iPhone? | Not started |
| 3 | Lightning P2P | What does it do that Relay's [delivery outbox](../../../examples/relay/docs/relay-delivery-outbox.md) (#299) doesn't, for resume, retry and chunk verification? | Not started |
| 4 | Offscan | How is its scan, OCR, edit and PDF pipeline built on Apple frameworks alone? | Not started |
