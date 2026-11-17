# Calculator recreation: what actually happened

A factual log for the blog post about recreating Calculator ([the plan](../../research/app-as-specification.md)). Each entry says what was run, what came back and what surprised us. Nothing goes in here that wasn't seen; claims about the phone are only what a person saw on it. Times are UK time.

## 2026-11-17: the idea and the plan

- **20:10.** Reuben is sent a blog idea: a tool that reads an app's accessibility tree, walks its screens breadth first, records state hashes and screenshots, replays sequences, and compares an original app with its recreation. His reaction: "I don't think we are quite there."
- **20:13.** He asks for a plan: find a relatively simple app, map it through the accessibility tree, try to rebuild it.
- **20:16.** The plan goes up as [PR #318](https://github.com/GeneralKnowledge/ixOS/pull/318). Checking the repository first turned up more than expected:
  - Reading any app's tree and driving it safely already worked (`forge ui tree --app`, `forge ui run --any-app` behind a per-app policy file the person writes). First done on the Claude app on 2026-11-08.
  - The explorer that walked screens, hashed them by structure and saved screenshots and a transition graph had been built, run on the phone, and then parked on 2026-11-15 when agent tooling moved out to forge-lab. It no longer has a command.
  - An earlier benchmark (NetNewsWire against our reader, Commonplace) was planned on 2026-11-10 and half built, but the original app was never actually walked.
  - Comparing two apps had never existed in any form.
- **Surprise:** a doc still said the parked explorer "still works". It didn't; corrected in the same PR.
- **Choice of app:** Apple's Calculator. It ships with iOS, so it needs none of the free team's three sideload slots (all in use), and the same keys always give the same display, which makes it comparable. Nobody has yet read an Apple app's tree this way, so that is the first thing to check.
- **The key idea from planning:** compare values before screens. A table of key sequences and what each leaves on the display, recorded from the original and replayed on the copy, needs no explorer at all, and can also run as a plain Rust test on Linux.
- **20:19.** Reuben decides the explorer question can wait until phase R3, once the value comparison has results.

## Next

R0: ten minutes with the phone on the cable, reading Calculator's tree and taking screenshots.
