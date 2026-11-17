# Calculator expedition: field notes

What actually happened while exploring and recreating Calculator, for the blog post ([draft](draft.md)) and [the plan](../../research/app-as-specification.md). Times are UK time. Claims about the phone are only what a person saw on it.

Every finding carries a tag:

- **Observed:** captured directly; says where (a tree file, a screenshot, a recorded result, a file in the repository).
- **Inferred:** believed from the evidence; names the evidence.
- **Unknown:** not worked out yet.

Each entry also names the activity it belongs to: survey, collect, map, reconstruct or verify.

## 2026-11-17: before the expedition

**20:10.** Reuben is sent a blog idea: a tool that reads an app's accessibility tree, walks its screens breadth first, records state hashes and screenshots, replays sequences, and compares an original with its recreation. His reaction: "I don't think we are quite there."

**20:13.** He asks for a plan: find a relatively simple app, map it through the accessibility tree, try to rebuild it. **20:16:** the plan goes up as [PR #318](https://github.com/GeneralKnowledge/ixOS/pull/318).

Checking the equipment before setting off:

- **Observed** (repository, forge-uitest): reading any app's tree and driving it safely already works, `forge ui tree --app` and `forge ui run --any-app` behind a policy file the person writes. First done on the Claude app on 2026-11-08.
- **Observed** (forge-lab `parked/`): the explorer that walked screens, hashed them by structure and saved screenshots and a transition graph was built and run on the phone, then parked on 2026-11-15. It has no command today.
- **Observed** (`examples/commonplace/docs/benchmark/spec.md`): an earlier expedition, NetNewsWire against our reader Commonplace, was planned on 2026-11-10 and half built, but the original app was never walked. Every row of its spec is still "Expected".
- **Observed:** nothing has ever compared two apps.
- **Observed:** one of our own docs said the parked explorer "still works". It doesn't. Corrected in #318.

Choosing the specimen:

- **Observed:** Calculator ships with iOS, so it needs none of the free team's three sideload slots, which are all in use.
- **Inferred** (it's a calculator): the same keys always give the same display, which is what makes it comparable. To be confirmed in R1 by recording the same sequences twice.
- **Unknown:** whether WebDriverAgent can read an Apple app's tree at all. No Apple app has been tried. This is what R0 answers.
- **Unknown:** whether every key has a label, and whether the display's value is readable from the tree.

The plan's key idea: compare values before screens. A table of key sequences and what each leaves on the display, recorded from the original and replayed on the copy, needs no explorer at all and can also run as a plain Rust test on Linux.

**20:19.** Reuben decides the explorer question waits until R3, once the value comparison has results.

**20:29.** Reuben shares his draft of the post, "silly" and fictional by his own description. Three points didn't match the plan and were corrected: the tree is read through WebDriverAgent over a USB cable (xtool builds our copy, it doesn't inspect the original); the copy reproduces behaviour, not Apple's look; and screenshots aren't compared, because the two apps are meant to look different.

**20:37.** Reuben reframes the whole thing as an expedition: what can you learn about software by exploring everything it lets you observe? Five activities (survey, collect, map, reconstruct, verify), five representations (visual, structural, behavioural, navigational, temporal), and every finding tagged Observed, Inferred or Unknown. These notes, the draft and the plan now follow that.

**20:40.** Reuben reviews #318 and keeps the direction, with three tightenings, all now in the plan and the draft:

- Every expedition produces the same six outputs (specimen profile, evidence catalogue, field map, findings register, reconstruction record, verification report), so the next one starts from a template. They'll live in `docs/research/expeditions/<specimen>/`.
- The accessibility tree is one map, not the boundary: screenshots, unusual interactions, persistence across a relaunch, effects on the rest of the phone, and errors and unreachable controls all count. R0 now includes a quick relaunch check.
- A failed investigation is a finding. If Calculator's tree can't be read, that goes in its findings as an Observed limitation, and those findings are kept even if the expedition moves to Voice Memos.

He also asked for no new infrastructure before R0: get the phone connected and see what the calculator gives up.

## Next

R0, survey: ten minutes with the phone on the cable, reading Calculator's tree and taking screenshots on the main screen, the history and in landscape.
