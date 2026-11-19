# ixOS App Studio: specification and forge-lab audit

Status: **proposal, not approved, nothing built.** Written 2026-11-19 from Reuben's App Studio message (project thread, 2026-11-19 08:59). Checked against ixOS `main` at `a4b3b4f` and forge-lab `b8d8bca` on its default branch `claude/project-thread-jt3hrz` (it has no `main`; pinned to ixOS `1291083`). Every claim about an outside project was checked on its own page on 2026-11-19 ([sources](#sources)).

App Studio is the idea that a person describes an app, an AI coding agent builds it as a real Forge project, and the app is built, tested and installed on the iPhone through the normal `forge` commands. This page specifies it, says where it collides with standing decisions, and audits what forge-lab already offers for Reuben's first milestone (M0: an agent can create, modify, build and test a Forge app through forge-lab).

## Summary

1. **M0 has already been done once, on 2026-11-07 and 2026-11-08.** A headless Claude Code agent created, built, installed, UI-tested and fixed Tipster through Forge MCP, unattended, and Reuben confirmed it worked. The next day the same agent built Murmur (a local microblogging app with SQLite, migrations and photos) over seven staged sessions, all passing, with about 19 minutes of phone time ([forge-lab docs/forge-mcp.md](https://github.com/GeneralKnowledge/forge-lab/blob/claude/project-thread-jt3hrz/docs/forge-mcp.md#first-tipster-run-on-cati-2026-11-07)). So App Studio's M0 is **re-qualifying** that loop after the 2026-11-15 move to forge-lab, not building it.
2. **The loop has drifted since.** Of the 23 MCP tools, 20 work as they are on today's `forge`; `forge_ui_explore` and `forge_ui_focus` call removed commands (known, parked), and `forge_verify` with `run: true` passes `--run` and `--journal`, which `forge verify` no longer accepts, and `forge_device_status` breaks when the server is started with `--udid`. forge-lab pins an ixOS commit 291 commits behind `main`. Its own tests pass against current ixOS docs (42 of 42, run for this page), but those use a stub `forge`.
3. **The phone is the real constraint, not the tooling.** All three free-team slots are in use (Device Lab, WebDriverAgent, the VPN app). The agent sandbox app (`org.forgemobile.agentlab`) needs a slot, and joining Device Lab is four hand edits to an app that ships a combined build. M0 needs a decision about where generated apps run ([decision D1](#open-decisions)).
4. **App Studio collides with four standing decisions** (the tooling freeze, P6, the Nov 18 direction review and "expand existing apps before new ones"). The proposed fit: a **separate companion repository**, no ixOS code changes beyond bug fixes, and **M0 only until the 2026-12-15 freeze review** ([conflicts](#where-this-conflicts-with-standing-decisions)).
5. **None of the four external tools is the engine.** Craft Anything is the closest match, but it is early alpha, drives Claude Code as its builder, and doesn't target iOS or SwiftUI. REA investigates software and builds nothing. The OpenHands CLI is unmaintained (its README says so) in favour of Agent Canvas. Aider has no MCP support. The coding agent that already passed M0 (Claude Code, headless) stays the reference backend, behind a small adapter so others can be tried ([assessments](#external-tools-checked)).

## What App Studio is and isn't

**Is:** a workflow and, later, an interface that turns a description into a Forge app and keeps improving that same app on request. Each generated app is an ordinary Forge project that a person can build, read and change with no AI, using the commands in [human-workflow.md](../human-workflow.md).

**Isn't:**

- A new app framework, renderer or web-app platform. SwiftUI stays the UI ([roadmap](../roadmap.md), decided 2026-11-09). Dioxus is out for that reason.
- A change to ixOS. ixOS stays a framework; App Studio depends on it and on forge-lab, never the reverse ([AGENTS.md rule 11](../../AGENTS.md), [areas.md](../areas.md)).
- A way to give the host extra powers. A generated app gets exactly what its `forge.toml` declares, through the same `forge config` generation, and a person approves every permission or entitlement change ([safety](#safety-and-permissions)).
- An App Store pipeline. Apps run on Reuben's own phone under the free team (3 slots, 7-day signing, 10 new App IDs a week).

## Where this conflicts with standing decisions

| Standing decision | Where App Studio collides | Proposed fit |
|---|---|---|
| **Tooling freeze** (2026-11-11, [roadmap](../roadmap.md#tooling-freeze-and-reuse-rules)): `forge ui`, `forge device`, `forge crash` get bug fixes only until P1 to P4 are met; review on **2026-12-15** | Visual feedback (M2) wants explore, focus and screen comparison, which are frozen or parked | M0 and M1 use only existing commands. Anything M2 needs is listed now and raised **at the freeze review**, not before |
| **Agent tooling deferred to P6 and moved to forge-lab** (2026-11-15) | App Studio is agent tooling by definition | It lives **outside ixOS**: a new companion repository (working name `app-studio`) that depends on forge-lab, or a folder in forge-lab until it earns its own repo ([D2](#open-decisions)). No ixOS crate, command or doc beyond this page |
| **Direction review** (2026-11-18): next milestone is making ixOS feel complete: real-device validation, then app parity, then completing existing apps. "Framework work must not substitute for product work" | App Studio is neither validation nor completing an existing app | Sequence it **after** the device-validation baseline. Until then, only this page and, if Reuben wants, one M0 re-qualification sitting, which doubles as a device-validation exercise of `forge run ios`, `forge ui` and `forge diagnose` |
| **Expand existing apps before new ones** (2026-11-16) | App Studio's whole output is new apps | Generated apps are **experiments, not ixOS apps**: they live in the studio workspace, not `examples/`, and never join the parity check or the ledger. A good one becomes an ixOS app only by a normal PR with Reuben's go-ahead |
| **Laptop sessions only run builds; code changes only in cloud PRs** | Fast SwiftUI fixes need the agent on the laptop (xtool there reports errors in a minute or two; macOS CI takes about 35) | The rule is about ixOS code. Generated apps aren't ixOS code, so a laptop agent editing **only the studio workspace** doesn't break it. Needs Reuben's confirmation ([D3](#open-decisions)) |
| **One combined Device Lab build; never touch the GPU job; one build at a time** | An agent building repeatedly | Every build already goes through `forge queue`; the agent never builds Device Lab, and studio builds wait their turn. Studio sittings only when Reuben asks |

## Architecture

```
 Person ──► App Studio (CLI first, UI later)
              │  brief, decisions, approvals, history
              ▼
           Orchestrator (stages, gates, budgets, records)
              │ prompts + allowed tools           │ reference study (optional)
              ▼                                   ▼
           Coding-agent adapter               Reference tools (opt-in, per brief)
           (Claude Code first; others         (Craft Anything, REA: study only,
            behind the same contract)          never write the app)
              │ MCP (stdio)
              ▼
           forge-lab MCP server ──► forge --json ──► xtool, cargo, usbmuxd, WebDriverAgent
              │                                        │
              ▼                                        ▼
           Studio workspace (one Forge project       iPhone (by cable, one slot)
           per app, git, its own history)
```

- **App Studio** owns the brief, the stages, the approvals and the record of each app. It is a `studio` CLI before it is anything visual, so the whole flow works from a terminal ([human-workflow.md](../human-workflow.md) applies to it too).
- **The orchestrator** runs one stage at a time with a turn and time budget, stops at every gate that needs a person, and writes what happened.
- **The coding agent** writes the code with its own file tools, inside the app's folder only, and reaches Forge only through forge-lab's MCP tools. It gets no shell outside the project.
- **forge-lab** stays what it is: MCP tools that run `forge --json`. App Studio adds no build, install or device logic of its own.
- **ixOS** is unchanged: framework, templates and `forge`.

## The generated app

A generated app is a Forge project made by `forge init`, the same as a person's:

| Part | Where | Rule |
|---|---|---|
| Rust crate | `rust/` | Owns state, rules, persistence (forge-storage) and networking (forge-net / URLSession service). Tests in the crate |
| SwiftUI | `Sources/AppHost/` | ForgeUI and ForgeAppKit first ([forgeui.md](../forgeui.md)); models import Foundation, Observation and the bindings only; an accessibility identifier on every control |
| Bindings | `Generated/` | Only from `forge bindings generate` (or a build), never edited |
| Configuration | `forge.toml` | The only place permissions, entitlements and background modes are declared. Info.plist, entitlements and `xtool.yml` come from it |
| Tests | `rust/` tests, `uitests/*.toml`, `requirements.toml` | The brief's requirements become `requirements.toml` lines with a UI scenario each ([evidence.md](../evidence.md)) |
| History | git, one commit per stage | The studio record links each commit to the stage and request that made it |
| Bundle id | `org.forgemobile.agentlab` | Every generated app shares the sandbox id, so it never spends a new App ID; installing one replaces the previous one ([D1](#open-decisions)) |

The studio workspace is a git repository of its own (one folder per app), outside the ixOS checkout, pinned to an ixOS commit like forge-lab is.

## Stages

Each stage has an input, an output, the forge-lab tools it may use and a gate. The agent may loop inside a stage; it may not skip a gate.

| Stage | Input | Output | Tools | Gate |
|---|---|---|---|---|
| **1 Describe** | The person's brief; optional screenshots; optional reference app | `brief.md`: purpose, users, screens wanted, data, offline needs, **capabilities requested** (camera, location, notifications…) | None | The person confirms the brief and the capability list |
| **2 Design** | Brief | `design.md`: screens and navigation, Rust data model and commands, persistence (with schema version), the ForgeUI components used, `requirements.toml` draft | MCP resources only (forge://docs, conventions, capabilities, limits) | The person approves (or the run is in "auto" mode, [D4](#open-decisions)) |
| **3 Generate** | Design | The project: `forge_create_app`, Rust core, SwiftUI, `forge.toml` limited to the approved capabilities, Rust tests, UI tests | `forge_create_app`, `forge_inspect_app`, `forge_configure_app`, `forge_check` | `forge_check` (Rust and Swift twin) passes; `forge_inspect_app` has no convention errors |
| **4 Build and test** | Project | An iOS build, Rust and model tests green, requirements proven on the Linux twin | `forge_build_ios`, `forge_check`, `forge_verify` (twin only) | `forge_build_ios` passes on the laptop |
| **5 Preview** | Build | The app on the phone; screenshots and trees at default and largest text, light and dark; UI tests run; logs and crashes read | `forge_install_device` (needs `--allow install`), `forge_launch_app`, `forge_screenshot`, `forge_inspect_ui`, `forge_ui_audit`, `forge_ui_act`, `forge_run_ui_test`, `forge_read_logs`, `forge_read_crashes`, `forge_diagnose` | The person plugs in only when the build is ready; a phone stage is 15 to 20 minutes at most |
| **6 Refine** | A request ("the timeline feels cramped", "add bookmarks") | Changes to the same project, a migration if the data changed, tests extended | Same as 3 to 5 | Same gates; a schema change needs a migration test with the previous version's data |

This is the Murmur run's shape, which worked: everything without the phone first (`forge-lab mcp --no-device`), then short phone stages resuming the same agent session.

## Safety and permissions

The rule from the brief, "don't let an AI agent inject arbitrary capabilities into the host runtime", made concrete:

1. **Capabilities come only from `forge.toml`.** The agent may edit `forge.toml`, but App Studio diffs its `[ios.permissions]`, entitlements and background modes after every stage against the capability list the person approved in stage 1. Anything extra stops the run and asks. (forge-lab's `forge_configure_app` today regenerates and checks, but doesn't compare against an approved list: **gap G6**.)
2. **No host or framework edits.** The agent's write scope is the app folder. It never edits ixOS, ForgeKit, forge-lab, Device Lab or `Generated/`. Enforced by the agent host's sandbox (Claude Code permission rules), checked after each stage with `git status` outside the app folder.
3. **Installing is the person's** (forge-lab's tier 2): the server starts with `--allow install --bundle-id org.forgemobile.agentlab` only for a preview sitting the person asked for. Removing apps, pairing, Apple ID, signing and secrets stay out of reach (tier 3).
4. **Acceptance is the person's.** `forge verify --accept` already refuses inside an MCP server; App Studio never records a phone result it didn't see.
5. **Reference tools are opt-in per brief**, run on software the person owns or may examine, in clean-room mode by default, and their output is study material, never copied code. Recreating a specific existing app's name, look or assets is out of scope.
6. **Secrets** (API keys for a hosted model, Jamendo-style client ids) are never written into a generated app's repository; the app asks for them at run time like Soundscape does.

## Provider independence

The orchestrator talks to a coding agent through one contract: start a session in a folder with an MCP server config and a prompt; resume it with another prompt; stop it; get back a transcript and a final message. Claude Code (`claude -p`, `--resume`) already does all of this and is the reference. Other backends are added only when one passes the same M0 run:

| Backend | Fits the contract? | What it would need |
|---|---|---|
| Claude Code (headless) | Yes; passed Tipster and Murmur | Nothing |
| OpenHands CLI (`--headless`, MCP servers) | Yes on paper, but the CLI README says it is no longer actively maintained and points to Agent Canvas | Wait; reassess Agent Canvas if a second backend is wanted |
| Aider (`--message`, `--yes`) | Partly: no MCP, so it can't call forge-lab's tools; it would have to run `forge --json` itself | A shell bridge, which breaks the "agent reaches Forge only through forge-lab" rule. Not now |
| A local model (laptop GPU, AI Lab) | Through an MCP-capable agent that accepts an OpenAI-compatible endpoint | AI Lab A1 first; a local model doing a whole M0 run is a research question |

## External tools checked

Checked on 2026-11-19 on each project's own page. None is added as a dependency, forked or merged; App Studio would call a tool as a separate program at most. Each gets a line in the [prior research register](external-projects/prior-research-register.md#tooling).

| Tool | What it actually is | Corrections to the brief | Verdict |
|---|---|---|---|
| [Craft Anything](https://github.com/storytold/craft-anything) (MIT or Apache-2.0, Rust 1.97+) | "See how any website, app or game works, then have AI build your own version": study, build, check, refine. Every tool is a CLI subcommand and an MCP tool. Runs your own `claude` as the builder; Codex adapter "hasn't been verified"; other agents over ACP. Linux builds (AppImage, deb, rpm). "Early alpha" | It doesn't build the app itself: it drives Claude Code, the same agent App Studio would use. "Phone apps" are a study target only; iOS and SwiftUI aren't output targets. Comparison is page screenshots at three widths for websites; app and game checks are "partly there" | **Study, then maybe Borrow.** Its study-build-check loop is the model for stages 2 to 6, and its reference study could feed stage 1. Not the engine: it would add a second orchestrator around the same agent. Reassess after its first public release |
| [REA](https://github.com/morluto/rea) (MIT, Node 22.19+ / 24.11+) | Lets an agent inspect native binaries (via Hopper or Ghidra), JavaScript and Electron, .NET, websites and packages (ZIP, APK, IPA, ASAR). CLI and MCP. Linux supported (Ubuntu 24.04+) | Right that it isn't a builder: its README says it "does not claim to recover original source code or automatically clone an application" | **Watch.** Optional reference study for M2 or later, only on software the person may examine. Note: the two GitHub pages fetched showed wildly different star counts, so popularity figures in articles about it are unreliable |
| [OpenHands CLI](https://github.com/OpenHands/OpenHands-CLI) (MIT, Python 3.12) | Headless mode (`--headless -t`, `--json`), MCP servers, bring-your-own model key. Latest 1.16.0 (2026-05-08) | Its README: "This project is no longer actively maintained", recommending Agent Canvas | **Watch** (Agent Canvas). Not a backend while the CLI is unmaintained |
| [Aider](https://github.com/Aider-AI/aider) (Apache-2.0) | Terminal pair programmer; `--message` does one edit and exits; git auto-commits; many providers including local models (Ollama). Newest numbered release in its history: v0.86.1 | No MCP support in its README or release notes, so it can't use forge-lab | **Reject as a backend for now.** Fine as a person's own editor on a generated app |
| [Dioxus](https://dioxuslabs.com) | Cross-platform Rust UI | Agreed in the brief: wrong direction | **Reject:** SwiftUI is the UI (roadmap, 2026-11-09) |

## forge-lab audit: what an agent can do today

Checked by building `forge` from ixOS `main` (`a4b3b4f`) and calling `--help` on every command and flag forge-lab's MCP server passes ([crates/forge-mcp/src/tools.rs](https://github.com/GeneralKnowledge/forge-lab/blob/claude/project-thread-jt3hrz/crates/forge-mcp/src/tools.rs)), plus `FORGE_WORKSPACE=<ixOS> cargo test -p forge-mcp` (42 passed). Nothing ran on the laptop or the phone for this page.

| Need | Tool | Runs | State on today's `forge` |
|---|---|---|---|
| Check the machine | `forge_doctor` | `forge doctor` | OK |
| Create | `forge_create_app` | `forge init <Name> --bundle-id org.forgemobile.agentlab --path … --agent` | OK. Refuses an existing folder; scoped to the server root |
| Read the project | `forge_inspect_app` | `forge inspect` | OK |
| Modify | (the agent's own file tools) | | No MCP tool, by design. Write scope is the agent host's job ([safety 2](#safety-and-permissions)) |
| Apply `forge.toml` | `forge_configure_app` | `forge config sync`, `forge config check` | OK; no approved-capability comparison (G6) |
| Test on Linux | `forge_check` | `forge test`, `forge check swift` | OK. Murmur noted it reports no test counts (G7) |
| Requirements | `forge_verify` | `forge verify` | **Broken with `run: true`**: passes `--run` and `--journal`, which moved out with run records. `twin_only` works (G2) |
| Build for iOS | `forge_build_ios` | `forge build ios` (queues itself) | OK |
| Install | `forge_install_device` | `forge run ios` | OK; refused unless `--allow install`, limited by `--bundle-id`. Needs a free slot (G1) |
| Phone state | `forge_device_status` | `forge device list`, `forge device apps`, `forge doctor` | **Broken when the server has `--udid`**: it passes `--udid` to `forge device list`, which has no such flag (G3) |
| Launch | `forge_launch_app` | `forge ui launch` | OK (terminates first; appearance and text size) |
| See the screen | `forge_screenshot`, `forge_inspect_ui`, `forge_ui_audit` | `forge ui screenshot`, `ui tree`, `ui audit` | OK |
| Drive it | `forge_ui_act` | one-step `forge ui run` | OK; coordinate taps refused |
| UI tests and baselines | `forge_run_ui_test`, `forge_accept_screenshots` | `forge ui run`, `forge ui accept` | OK |
| Logs, crashes, diagnosis | `forge_read_logs`, `forge_read_crashes`, `forge_diagnose` | `forge device logs`, `device crashes`; diagnose is the server's own correlation | OK |
| Explore, focus | `forge_ui_explore`, `forge_ui_focus` | `forge ui explore`, `forge ui focus` | **Broken**: commands removed from `forge` (parked in forge-lab, known). Not needed for M0 |
| Docs as resources | `forge://conventions`, `capabilities`, `limits`, `docs/<page>` | Read from the ixOS checkout | OK; every page forge-lab names exists on `main` |

### Gaps for M0

| | Gap | Where it's fixed | Size |
|---|---|---|---|
| **G1** | **No phone slot** for `org.forgemobile.agentlab`: Device Lab, WebDriverAgent and the VPN app fill all three | A decision, not code ([D1](#open-decisions)) | — |
| **G2** | `forge_verify` `run: true` calls removed flags | forge-lab: drop the option or rebuild run records in forge-lab from the parked code | Small |
| **G3** | `--udid` passed to `forge device list` when the server has one (no such flag) | forge-lab: don't add it in `device_status` | Small |
| **G4** | forge-lab pins ixOS `1291083`, 291 commits behind | forge-lab: bump the rev, run its tests, run one Linux-only create-check-build in a scratch folder | Small |
| **G5** | Stale pointers: forge-lab's README says to copy `templates/agent/mcp.json`, which ixOS no longer has; the server's refusal text says `forge mcp` (now `forge-lab mcp`) | forge-lab docs and strings | Small |
| **G6** | No check that `forge.toml` capabilities match what the person approved | App Studio's orchestrator (not forge-lab, not ixOS) | Small |
| **G7** | `forge_check` doesn't report test counts | ixOS `forge test --json` data, a bug-fix-sized change allowed under the freeze, or left | Small |
| **G8** | No studio record: brief, design, stage results, commits, phone results in one place | App Studio | Medium; the parked run-record code is a starting point |

Nothing on this list needs a new `forge` command. G2 to G5 are forge-lab maintenance; G6 and G8 are App Studio itself.

## Milestones

| | Deliverable | Done when | Phone |
|---|---|---|---|
| **M0 Re-qualify the loop** | G2 to G5 fixed in forge-lab; one headless agent run on the laptop | A headless agent, through forge-lab tools only, creates a small app from a written prompt (a tip calculator again, or a new one Reuben picks), passes `forge_check` and `forge_build_ios` with `--no-device`, then in one phone stage of at most 20 minutes installs, launches, screenshots at largest text, runs its UI test and fixes one thing it found; then makes one requested change and checks it. Reuben opens the app and agrees it works | One sitting, needs D1 |
| **M1 App generation** | `studio` CLI with stages 1 to 4 and gates; capability check (G6); studio record (G8); brief and design templates | Three different briefs (a list-and-detail app, one with persistence and a migration, one asking for one permission) each produce a project that passes stage 4 without a person editing code; the permission one stops at the capability gate | None |
| **M2 Visual feedback** | Stages 5 and 6 automated: screenshots and trees compared with the design, ForgeUI audit, refine loop | Two refine requests per app land with a passing UI test each. Anything here that needs frozen tooling (explore, focus, tree diffs) waits for the freeze review | Short sittings |
| **M3 App Studio** | Persistent projects, conversational editing, a catalogue of generated apps, install management within the slot rules | A person with no agent knowledge describes an app, approves its brief and capabilities, and gets it on the phone; can come back a week later (after re-signing) and change it | As M2 |

Sequencing: this page now. M0 only when Reuben asks, and not before the device-validation baseline from the Nov 18 review is done. M1 onward only after the 2026-12-15 freeze review decides on agent tooling.

## Open decisions

| | Question | Options | Recommendation |
|---|---|---|---|
| **D1** | Where do generated apps run on the phone? | (a) One sandbox slot (`org.forgemobile.agentlab`), borrowed from the VPN app during studio sittings; each generated app replaces the previous one. (b) Remove WebDriverAgent instead (loses UI tests: no). (c) Host generated apps inside Device Lab under a Studio shelf (four hand edits per app, joins the combined build, and Device Lab would depend on outside folders) | **(a)**: it is what Tipster and Murmur did, spends no new App ID, and keeps generated code out of Device Lab. Reinstall the VPN app by cable after the sitting |
| **D2** | Where does App Studio's code live? | A new repo, or a folder in forge-lab | **forge-lab folder first** (`studio/`), its own repo when it has a release of its own |
| **D3** | May the agent edit the studio workspace on the laptop? | Yes (fast SwiftUI fixes), or cloud only (35-minute macOS CI per Swift error) | **Yes, studio workspace only**; ixOS code stays cloud-PR only |
| **D4** | Does the person approve the design (stage 2) every time? | Always, or "auto" for small apps | **Always for M1**; revisit after three runs |
| **D5** | Which app for M0? | Tip calculator again (comparable with 2026-11-07), or something new | **Something new and small** (for example a habit tracker with persistence) so the run tests today's starter, not a remembered answer |

## Sources

Checked 2026-11-19.

- Craft Anything: [repository](https://github.com/storytold/craft-anything), [README](https://github.com/storytold/craft-anything/blob/main/README.md)
- REA: [repository and README](https://github.com/morluto/rea)
- OpenHands: [OpenHands CLI](https://github.com/OpenHands/OpenHands-CLI), [PyPI](https://pypi.org/project/openhands), [product page](https://hub.openhands.dev/product/cli)
- Aider: [repository](https://github.com/Aider-AI/aider), [scripting](https://aider.chat/docs/scripting.html), [release history](https://aider.chat/HISTORY.html)
- forge-lab: [README](https://github.com/GeneralKnowledge/forge-lab), [forge-mcp.md](https://github.com/GeneralKnowledge/forge-lab/blob/claude/project-thread-jt3hrz/docs/forge-mcp.md) (Tipster and Murmur runs), `crates/forge-mcp/src/tools.rs`
- ixOS: [roadmap](../roadmap.md) (tooling freeze, P6, future ideas), [AGENTS.md](../../AGENTS.md), [human-workflow.md](../human-workflow.md), [Device Lab README](../../examples/device-lab/README.md)
