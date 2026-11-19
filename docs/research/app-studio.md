# ixOS App Studio: specification and forge-lab audit

Status: **direction approved, implementation paused** (Reuben, 2026-11-19 09:08; Recreate added 09:10). Nothing is built. Decisions D1 to D5 are [answered](#decisions); forge-lab repairs G2 to G5 may go ahead as maintenance; M0 runs after the device-validation baseline; M1 is reconsidered at the 2026-12-15 freeze review ([next steps](#next-steps)). Written 2026-11-19 from Reuben's App Studio message (project thread, 2026-11-19 08:59). Checked against ixOS `main` at `a4b3b4f` and forge-lab `b8d8bca` on its default branch `claude/project-thread-jt3hrz` (it has no `main`; pinned to ixOS `1291083`). Every claim about an outside project was checked on its own page on 2026-11-19 ([sources](#sources)).

App Studio is the idea that a person describes an app, an AI coding agent builds it as a real Forge project, and the app is built, tested and installed on the iPhone through the normal `forge` commands. This page specifies it, says where it collides with standing decisions, and audits what forge-lab already offers for Reuben's first milestone (M0: an agent can create, modify, build and test a Forge app through forge-lab).

## Summary

1. **M0 has already been done once, on 2026-11-07 and 2026-11-08.** A headless Claude Code agent created, built, installed, UI-tested and fixed Tipster through Forge MCP, unattended, and Reuben confirmed it worked. The next day the same agent built Murmur (a local microblogging app with SQLite, migrations and photos) over seven staged sessions, all passing, with about 19 minutes of phone time ([forge-lab docs/forge-mcp.md](https://github.com/GeneralKnowledge/forge-lab/blob/claude/project-thread-jt3hrz/docs/forge-mcp.md#first-tipster-run-on-cati-2026-11-07)). So App Studio's M0 is **re-qualifying** that loop after the 2026-11-15 move to forge-lab, not building it.
2. **The loop has drifted since.** Of the 23 MCP tools, 20 work as they are on today's `forge`; `forge_ui_explore` and `forge_ui_focus` call removed commands (known, parked), and `forge_verify` with `run: true` passes `--run` and `--journal`, which `forge verify` no longer accepts, and `forge_device_status` breaks when the server is started with `--udid`. forge-lab pins an ixOS commit 291 commits behind `main`. Its own tests pass against current ixOS docs (42 of 42, run for this page), but those use a stub `forge`.
3. **The phone is the real constraint, not the tooling.** All three free-team slots are in use (Device Lab, WebDriverAgent, the VPN app). A generated app needs a slot, and joining Device Lab is four hand edits to an app that ships a combined build. Decided: generated apps borrow the VPN app's slot during an authorised studio session, with a preflight and a restore check ([D1](#decisions)). One correction found while writing that up: the VPN app itself uses `org.forgemobile.agentlab`, so generated apps need their own bundle id ([why](#the-phone-slot-and-recovery)).
4. **App Studio collides with four standing decisions** (the tooling freeze, P6, the Nov 18 direction review and "expand existing apps before new ones"). The proposed fit: a **separate companion repository**, no ixOS code changes beyond bug fixes, and **M0 only until the 2026-12-15 freeze review** ([conflicts](#where-this-conflicts-with-standing-decisions)).
5. **Recreate is the new direction, and REA is its integration candidate** (2026-11-19 09:10): the agent investigates an existing app and builds an ixOS equivalent. ixOS has done this by hand three times (general device control, NetNewsWire to Commonplace, the Calculator expedition), and REA adds web and package analysis but has no iOS device layer, so the iOS side is `forge ui --any-app` and the parked explorer ([Recreate](#recreate-investigating-an-app-and-building-an-ixos-equivalent)).
6. **None of the four external tools is the app engine.** Craft Anything is the closest match, but it is early alpha, drives Claude Code as its builder, and doesn't target iOS or SwiftUI. REA investigates software and builds nothing. The OpenHands CLI is unmaintained (its README says so) in favour of Agent Canvas. Aider has no MCP support. The coding agent that already passed M0 (Claude Code, headless) stays the reference backend, behind a small adapter so others can be tried ([assessments](#external-tools-checked)).

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
| **Agent tooling deferred to P6 and moved to forge-lab** (2026-11-15) | App Studio is agent tooling by definition | It lives **outside ixOS**: a new companion repository (working name `app-studio`) that depends on forge-lab, or a folder in forge-lab until it earns its own repo ([D2](#decisions)). No ixOS crate, command or doc beyond this page |
| **Direction review** (2026-11-18): next milestone is making ixOS feel complete: real-device validation, then app parity, then completing existing apps. "Framework work must not substitute for product work" | App Studio is neither validation nor completing an existing app | Sequence it **after** the device-validation baseline. Until then, only this page and, if Reuben wants, one M0 re-qualification sitting, which doubles as a device-validation exercise of `forge run ios`, `forge ui` and `forge diagnose` |
| **Expand existing apps before new ones** (2026-11-16) | App Studio's whole output is new apps | Generated apps are **experiments, not ixOS apps**: they live in the studio workspace, not `examples/`, and never join the parity check or the ledger. A good one becomes an ixOS app only by a normal PR with Reuben's go-ahead |
| **Laptop sessions only run builds; code changes only in cloud PRs** | Fast SwiftUI fixes need the agent on the laptop (xtool there reports errors in a minute or two; macOS CI takes about 35) | The rule is about ixOS code. Generated apps aren't ixOS code, so a laptop agent editing **only the studio workspace** doesn't break it. Needs Reuben's confirmation ([D3](#decisions)) |
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
| Bundle id | `org.forgemobile.studio` | Every generated app shares one studio id, so it spends one App ID once and installing one replaces the previous one. Not `org.forgemobile.agentlab`: the VPN app uses that id ([D1](#the-phone-slot-and-recovery)) |
| Provenance | `studio.toml` in the app folder | The ixOS commit and `forge --version` it was made and last built with, the forge-lab commit, the agent backend and model, and one line per stage (date, request, commit, result). A project can be reopened months later, upgraded to a newer ixOS like any app, and continued |

The studio workspace is a git repository of its own (one folder per app), outside the ixOS checkout, pinned to an ixOS commit like forge-lab is.

## Stages

Each stage has an input, an output, the forge-lab tools it may use and a gate. The agent may loop inside a stage; it may not skip a gate.

| Stage | Input | Output | Tools | Gate |
|---|---|---|---|---|
| **0 Preflight** | Phone and laptop state | A preflight record: which apps fill the three slots, their bundle ids and builds, the VPN app's recovery steps saved, the queue free, the ixOS pin | `forge_device_status`, `forge_doctor` | Mandatory before any phone stage. Fails if Device Lab or WebDriverAgent would be touched, or if the slot to borrow isn't authorised for this session |
| **1 Describe** | The person's brief; optional screenshots; optional reference app; or an existing project to extend ([three ways in](#create-recreate-extend)) | `brief.md`: purpose, users, screens wanted, data, offline needs, **capabilities requested** (camera, location, notifications…) | None | The person confirms the brief and the capability list |
| **2 Design** | Brief | `design.md`: screens and navigation, Rust data model and commands, persistence (with schema version), the ForgeUI components used, `requirements.toml` draft | MCP resources only (forge://docs, conventions, capabilities, limits) | The person approves, always ([D4](#decisions)). A later change to capabilities, architecture or permissions goes back through this gate |
| **3 Generate** | Design | The project: `forge_create_app`, Rust core, SwiftUI, `forge.toml` limited to the approved capabilities, Rust tests, UI tests | `forge_create_app`, `forge_inspect_app`, `forge_configure_app`, `forge_check` | `forge_check` (Rust and Swift twin) passes; `forge_inspect_app` has no convention errors |
| **4 Build and test** | Project | An iOS build, Rust and model tests green, requirements proven on the Linux twin | `forge_build_ios`, `forge_check`, `forge_verify` (twin only) | `forge_build_ios` passes on the laptop |
| **5 Preview** | Build | The app on the phone; screenshots and trees at default and largest text, light and dark; UI tests run; logs and crashes read | `forge_install_device` (needs `--allow install`), `forge_launch_app`, `forge_screenshot`, `forge_inspect_ui`, `forge_ui_audit`, `forge_ui_act`, `forge_run_ui_test`, `forge_read_logs`, `forge_read_crashes`, `forge_diagnose` | The person plugs in only when the build is ready; a phone stage is 15 to 20 minutes at most |
| **6 Refine** | A request ("the timeline feels cramped", "add bookmarks") | Changes to the same project, a migration if the data changed, tests extended | Same as 3 to 5 | Same gates; a schema change needs a migration test with the previous version's data |
| **7 Restore** | Preflight record | The phone back as it was: the studio app removed by the person, the VPN app reinstalled by cable from the VPN repo and its smoke test passing, Device Lab and WebDriverAgent untouched | `forge_device_status`, `forge_run_ui_test` (the VPN app's smoke test) | The session isn't complete until the slots match the preflight record |

This is the Murmur run's shape, which worked: everything without the phone first (`forge-lab mcp --no-device`), then short phone stages resuming the same agent session.

### Evidence decides, not the agent

The agent fixes code; it doesn't judge whether the app works. A stage passes only on results it can't write itself: `forge` exit codes and `--json` results (build, tests, `forge verify`), UI test runs with their screenshots and trees under `.forge/runs/`, the capability diff, and, for anything on the phone, the person's own check. The agent's final message ("all done, it works") is a summary to read, never a pass. The studio record keeps the evidence path for every claim, and stays empty where nothing ran.

### Create, Recreate, Extend

Reuben, 2026-11-19 09:10: rather than an app builder that later learns to understand apps, give the existing ixOS development agent three abilities, built in this order of interest but **Recreate is the new one**:

| Ability | Input | What changes in the stages |
|---|---|---|
| **Create** | A description | The stages above. M0 and M1 |
| **Recreate** | An app (or one feature of it) to investigate | A **stage 1R Investigate** before Design: observe, understand, and write an evidence-backed specification with what is known and what isn't. Design maps it onto ForgeUI and ForgeAppKit; after Preview, a **Compare** step checks the result against the reference and loops on the differences |
| **Extend** | An existing Forge project he owns | Stage 3 skips `forge_create_app`; stage 0 records the project's state; Investigate runs on the project itself (`forge inspect`, its tests, its screens). ixOS's own apps stay on the cloud PR workflow: extending one of them is a normal ixOS PR, not a studio session ([D3](#decisions)) |

It all lives in forge-lab as one capability, not a separate product ([D2](#decisions)).

## Recreate: investigating an app and building an ixOS equivalent

The goal: point the agent at an app or feature ("the library experience of a music app"), have it investigate screens, navigation, interactions, data and visual design, write down what the app actually does and what is still unknown, then build an ixOS equivalent and compare. It learns from observable behaviour; it never copies proprietary code, assets, names or branding, and the result has its own identity.

### ixOS has done this three times already, by hand

This is not a blank page. The same idea has run three times, and the pieces it built are the starting point:

| When | What | What it built or proved |
|---|---|---|
| 2026-11-08 | [General device control](general-device-control.md) (#99, #101, #105, #107) | `forge ui tree`, `screenshot` and `run` reach apps Forge didn't build with `--any-app`, under a person-written per-app policy (observe, operate, full) and a commit gate on labels like send or pay. Tried on the Claude app on Cati |
| 2026-11-10 | NetNewsWire benchmark (#148) | `forge ui explore --app` walked another app's screens into a `graph.json`; the rebuild became **Commonplace**, with a written spec, gap log and pattern inventory |
| 2026-11-17 to 18 | Calculator expedition ([PR #318](https://github.com/GeneralKnowledge/ixOS/pull/318), open) | The investigation format: a profile, evidence (trees, screenshots, 39 recorded sequences and answers), a map, findings tagged **Observed / Inferred / Unknown**, a reconstruction (the Sums app in Device Lab, its Rust engine tested against the recorded answers) and verification |

So Reuben's proposed components map onto things that exist:

| Proposed | What exists | State |
|---|---|---|
| REA MCP | Nothing yet | To assess ([below](#rea-what-it-gives-and-what-it-doesnt)) |
| ixOS Explorer (iOS visual and interaction inspection) | `forge ui tree/screenshot/run --any-app` with the policy (ixOS, frozen but working); `forge ui explore` with `--app` and `graph.json` (parked in forge-lab since 2026-11-15) | Observation works today. Walking needs the parked explorer revived in forge-lab, which the expedition plan already put at Calculator R3 |
| App Mapper (structured screen and behaviour descriptions) | The expedition's six outputs and Observed/Inferred/Unknown tags (#318) | A format, written by hand. Becomes a template the agent fills |
| Forge MCP | forge-lab | Works, with G1b to G5 to fix |
| Agent workflow | This page's stages plus 1R Investigate and Compare | Spec only |

### REA: what it gives and what it doesn't

Checked on 2026-11-19 against its README, `DISCLAIMER.md`, the Apple and Android guides and the installation page ([sources](#sources)).

- **Its model is Decompile, Understand, Recreate**, in its own words: recover code, strings and names; "follow the code … until the agent can explain how a feature actually works"; turn that into "a feature for your own product, adapted to your stack". It "does not claim to recover original source code or automatically clone an application." That matches Recreate's shape well.
- **About 116 MCP tools** by its own catalogue table (families of 39, 14, 7, 5, 7, 9, 5, 2, 7 and 21; the README gives no total). Mutating tools are marked and "never run implicitly", but no allowlist for restricting which tools an agent may call was found.
- **Apple: static only.** It inventories IPA payloads, plists, compiled Interface Builder files and asset catalogs, reads Mach-O metadata, signatures and Swift symbols with its own tools, and decompiles through Hopper or Ghidra. **No iOS device interaction** (no screens, taps or accessibility tree) is documented. Encrypted content is listed as unavailable and not read.
- **App Store apps are FairPlay-encrypted.** The executable in an App Store app is encrypted; decrypting it is circumventing a technical protection measure, which REA's disclaimer says it won't help with and which App Studio must never do. So for an App Store reference (a music app, for example) REA can at most read the unencrypted resources of a legitimately obtained package, not the code. **For iOS references, observation through `forge ui` is the main source, not decompilation.**
- **Android:** device state, logcat, packages, one-frame screenshots and starting apps over adb; tapping, typing and UI automation are "a separate lane", out of scope. Irrelevant while the Android phone is under repair.
- **Web:** passive observation of a page in an existing Chrome-family browser (structure, network metadata, scripts, screenshots), and Playwright scenarios that can interact. This is the most useful part for Recreate: many apps have a **web version** that can be observed lawfully and without a phone.
- **Linux:** Node 22.19+ or 24.11+, Ubuntu 24.04+; Ghidra 12.1.4 with JDK 21 for native code. MIT. npm package `rea-agents`.
- **Responsible use** (its disclaimer): lawful analysis only; no DRM, licence-check or access-control circumvention; the person is responsible for authorisation, licences and terms of service; "authorization to inspect an artifact does not necessarily authorize execution, interception, device changes".

**Verdict: integration candidate, run as a separate MCP server.** REA goes into the agent's MCP configuration next to forge-lab, installed with npm, never vendored, forked or linked into ixOS or forge-lab code. It covers web references and unencrypted packages; the iOS observation layer is ixOS's own (`forge ui --any-app`, the explorer) because REA has none. Before it is used on a real reference, an assessment in the [evaluation framework](external-projects/evaluation-framework.md) format records the version, licence, a Linux install, the tools it exposes and what it found on two targets ([X0](#recreate-milestones)).

### Rules for Recreate

1. **Observable behaviour only.** Screens, navigation, interactions, data shapes and visual design are studied; code, assets, icons, names and branding are not copied. The result gets its own name and look within ForgeUI.
2. **No circumvention.** No decryption of App Store binaries, no bypassing logins, paywalls, DRM or bot protection, no reading another app's data.
3. **The policy decides what the agent may touch.** An iOS reference app is investigated only at the level the person's `forge ui` policy sets for it (observe by default; operate only on his say), with the existing commit gate and never-list (payments, secure fields, Settings, Wallet). An account-based app is observed signed in as the person, read-only.
4. **Uncertainty is written down.** Every finding is Observed, Inferred or Unknown, as in the Calculator expedition; Design may not rely on an Unknown without saying so.
5. **Compare against the specification, not the agent's impression.** Differences come from evidence on both sides (trees, screenshots, recorded sequences), as in Calculator's 39 recorded answers.
6. **Reference apps on the phone don't take sideload slots** (they come from the App Store), but the reconstruction does: it follows the D1 slot rules, or joins Device Lab like Sums when it becomes an ixOS app.

### Recreate milestones

Small first, as Reuben asked: connect REA, investigate one feature, produce one useful implementation; build more inspection tools only when that exposes a real gap.

| | Deliverable | Done when | Phone |
|---|---|---|---|
| **X0 REA assessment** | REA installed in a cloud session (not the shared laptop), its MCP server next to forge-lab in one agent session; an assessment record | Recorded: version, licence, tool list, what it reports on (a) an IPA ixOS built itself (Device Lab, unencrypted) and (b) a public web app's page; whether its findings were usable as a specification; what's missing for iOS | None |
| **X1 One feature, end to end** | The agent investigates one feature of a reference with REA (web) and/or `forge ui --any-app` (iOS, at the policy's level), writes the expedition outputs, builds the feature in a studio app through forge-lab, compares | One feature works on the phone and its comparison is written from evidence. Every gap is logged with whether an existing tool already covers it | One sitting after M0, VPN slot borrowed and restored |
| **X2 Fill real gaps** | Only what X1 showed was missing; first candidate is reviving the parked explorer in forge-lab for walking an app's screens | Decided after X1, and after the freeze review where it touches frozen tooling | — |

### X1: Spotify Library

Chosen by Reuben (D6, 2026-11-19 09:16) because it tests both sources on the same feature: REA on the web player, `forge ui --any-app` on the iPhone app, and a comparison of where the two differ.

**Scope: the Library screen only.**

- View saved albums, playlists and artists.
- Search and filter the library.
- Switch between library views.
- Open an item and navigate back.
- Keep sorting and filtering state where the reference does.

**Output:** the expedition outputs (profile, evidence, map, findings tagged Observed / Inferred / Unknown, reconstruction, verification), then a working native Library app on ForgeUI, with its own name and look, Rust owning the library model, search, filter and sort, and local fixture data.

**Boundaries** (Reuben's, plus what follows from them):

1. No proprietary backend, no streaming, no protected audio, no extracted assets (artwork, icons, fonts, the logo). The recreation uses invented fixture data and its own artwork.
2. **Web, through REA:** passive observation of the web player in a browser where Reuben is signed in: page structure, screenshots and network *metadata* (which requests a view makes, their timing and shape), to learn behaviour. No replaying or calling Spotify's API, no scraping, no tools that decompile or patch the player's scripts. Spotify's terms restrict reverse engineering of the service, so the study stays at what a user's own browser shows, and Reuben decides whether network metadata is in bounds before X1 starts ([open point](#open-points-before-x1)).
3. **iPhone, through `forge ui --any-app`:** Spotify is added to the person's policy at **observe** first (read the tree, screenshots, scroll). Opening items and switching views needs **operate**, his call; **full** is never used on a real account. Changing a sort or filter may sync to his account; the run puts each one back as it found it and records that it did.
4. **His library is personal data.** Screenshots and trees show his playlists and listening. They stay out of the ixOS repository: raw evidence goes in project files, and what the repository gets is redacted (titles replaced) or described in words.
5. REA runs where his signed-in browser is: his own computer (decided 09:43). A cloud session has no access to his account; X0 stays cloud-only.

#### Open points before X1

- ~~Is Spotify installed on the phone?~~ **Yes** (Reuben, 2026-11-19 09:43).
- ~~Which machine runs REA?~~ **His computer or the server** (09:43). REA's passive browser observation attaches to an existing, signed-in Chrome-family browser, and the server (the Ubuntu laptop) is headless and runs his GPU job. So the web part runs on **his own computer**, where he signs in to the web player himself; App Studio never handles his Spotify password. The server keeps doing what it does now: builds, `forge ui` on the phone over USB, and the queue.
- **Still open:** is network metadata from the web player in bounds, or only page structure and screenshots? Until he says, X1 plans for structure and screenshots only.

## Safety and permissions

The rule from the brief, "don't let an AI agent inject arbitrary capabilities into the host runtime", made concrete:

1. **Capabilities come only from `forge.toml`.** The agent may edit `forge.toml`, but App Studio diffs its `[ios.permissions]`, entitlements and background modes after every stage against the capability list the person approved in stage 1. Anything extra stops the run and asks. (forge-lab's `forge_configure_app` today regenerates and checks, but doesn't compare against an approved list: **gap G6**.)
2. **No host or framework edits.** The agent's write scope is the app folder. It never edits ixOS, ForgeKit, forge-lab, Device Lab or `Generated/`. Enforced by the agent host's sandbox (Claude Code permission rules), checked after each stage with `git status` outside the app folder.
3. **Installing is the person's** (forge-lab's tier 2): the server starts with `--allow install --bundle-id org.forgemobile.studio` only for a preview sitting the person authorised. Uninstalling apps, pairing, Apple ID, signing and secrets stay out of App Studio's reach (tier 3); the person removes an app on the phone.
4. **Acceptance is the person's.** `forge verify --accept` already refuses inside an MCP server; App Studio never records a phone result it didn't see.
5. **Reference tools are opt-in per brief**, run on software the person owns or may examine, in clean-room mode by default, and their output is study material, never copied code. Recreating a specific existing app's name, look or assets is out of scope.
6. **Secrets** (API keys for a hosted model, Jamendo-style client ids) are never written into a generated app's repository; the app asks for them at run time like Soundscape does.
7. **Write scope on the laptop** ([D3](#decisions)): the agent may edit generated apps and the studio workspace there. It never edits ixOS, Device Lab, or framework dependencies (the ixOS pin moves only by an approved update). ixOS development stays on the cloud PR workflow.

### The phone slot and recovery

All three free-team slots are in use: Device Lab, WebDriverAgent and the VPN app. Decided (D1): a studio session borrows the VPN app's slot, and only that one.

**Correction found while incorporating D1:** the VPN app's `forge.toml` (GeneralKnowledge/VPN `apps/ios`, at `59376ae`) uses `bundle_id = "org.forgemobile.agentlab"`, the sandbox id this page first proposed for generated apps. Installing a generated app under that id would not free a slot cleanly: it would install *over* the VPN app as an upgrade, so the generated code would inherit the VPN app's data container and could reach its Keychain sign-in. Generated apps therefore use their own id, `org.forgemobile.studio` (one new App ID, once, then reused), and the VPN app is removed by the person, not overwritten.

A session:

1. **Before** (stage 0): record the slots; save the VPN app's recovery steps (the VPN repo checkout and commit, `forge run ios --project apps/ios`, sign-in details the person needs, its smoke test); confirm the session is authorised in Reuben's own words.
2. **Swap**: Reuben deletes the VPN app on the phone. App Studio never uninstalls.
3. **Studio stages** 5 and 6 install `org.forgemobile.studio` by cable.
4. **After** (stage 7): Reuben deletes the studio app; the VPN app is reinstalled by cable and its smoke test passes; the slots match the record. Until then the session is not complete.

Never touched: Device Lab and WebDriverAgent. App Studio never signs, never manages the Apple team, never uninstalls.

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
| [REA](https://github.com/morluto/rea) (MIT, Node 22.19+ / 24.11+) | Agent reverse engineering over CLI and MCP: native binaries (via Hopper or Ghidra), JavaScript and Electron, .NET, websites, packages (ZIP, APK, IPA, ASAR). Linux supported | Right that it isn't a builder. No iOS device interaction; App Store binaries are encrypted and out of bounds | **Integration candidate** (revised 2026-11-19 with Reuben): a separate MCP server beside forge-lab for Recreate, after the X0 assessment ([details](#rea-what-it-gives-and-what-it-doesnt)). Never vendored or linked |
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
| **G1** | **No phone slot**: Device Lab, WebDriverAgent and the VPN app fill all three, and the VPN app already uses `org.forgemobile.agentlab` | Decided: borrow the VPN slot with `org.forgemobile.studio`, preflight and restore ([D1](#the-phone-slot-and-recovery)) | — |
| **G1b** | forge-lab's default sandbox id is `org.forgemobile.agentlab` (`SANDBOX_BUNDLE_ID`) | forge-lab: pass `--bundle-id org.forgemobile.studio` for studio runs, or change the default with a note | Small |
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
| **M0 Re-qualify the loop** | G1b to G5 fixed in forge-lab; one headless agent run on the laptop | A headless agent, through forge-lab tools only, creates a **habit tracker** ([D5](#decisions)): several screens, persistent local storage, create, edit and complete actions, a simple history view, at least one UI test. It passes `forge_check` and `forge_build_ios` with `--no-device`; then, after the preflight, one phone stage of at most 20 minutes installs, launches, screenshots at largest text, runs the UI test and fixes one thing found from device evidence; then one requested change, checked again; then the restore. Passing is decided by the evidence and Reuben opening the app, not by the agent's report | One authorised sitting, VPN slot borrowed and restored |
| **M1 App generation** | `studio` CLI with stages 1 to 4 and gates; capability check (G6); studio record (G8); brief and design templates | Three different briefs (a list-and-detail app, one with persistence and a migration, one asking for one permission) each produce a project that passes stage 4 without a person editing code; the permission one stops at the capability gate | None |
| **M2 Visual feedback** | Stages 5 and 6 automated: screenshots and trees compared with the design, ForgeUI audit, refine loop | Two refine requests per app land with a passing UI test each. Anything here that needs frozen tooling (explore, focus, tree diffs) waits for the freeze review | Short sittings |
| **M3 App Studio** | Persistent projects, conversational editing, a catalogue of generated apps, install management within the slot rules | A person with no agent knowledge describes an app, approves its brief and capabilities, and gets it on the phone; can come back a week later (after re-signing) and change it | As M2 |

### Next steps

Agreed with Reuben, 2026-11-19:

1. Merge this specification with D1 to D5 in. It sets the direction; it starts no implementation.
2. Repair forge-lab G1b to G5 as maintenance, in forge-lab PRs. No new `forge` commands.
3. Wait for the device-validation baseline (the 2026-11-18 direction review); App Studio doesn't compete with it.
4. Run M0 again: the habit tracker, headless development, one controlled phone session, real evidence.
5. Reconsider M1 at the 2026-12-15 freeze review: does the repeatable workflow deserve to become an App Studio product?
6. Recreate: X0 (the REA assessment, cloud only, no phone) can run any time as research; X1 after M0; X2 only for gaps X1 shows.

## Decisions

Answered by Reuben on 2026-11-19 (09:08).

| | Question | Decision |
|---|---|---|
| **D1** | Where do generated apps run on the phone? | **Borrow the VPN app's slot, with safeguards**: save the VPN configuration and recovery steps first; remove the VPN app only for an explicitly authorised studio session; reinstall and test it before the session counts as complete; never remove Device Lab or WebDriverAgent; App Studio never manages signing or uninstalls apps. Phone-slot status is a mandatory preflight. Generated apps use `org.forgemobile.studio`, because the VPN app already uses `org.forgemobile.agentlab` ([details](#the-phone-slot-and-recovery)) |
| **D2** | Where does App Studio's code live? | **`forge-lab/studio/`.** It owns the MCP integration and is already separate from ixOS. Its own repository when it has earned an independent release cycle |
| **D3** | May the agent edit on the laptop? | **Yes, within bounds**: generated apps and the studio workspace only. No ixOS edits, no Device Lab edits, no framework dependency changes beyond approved updates. ixOS development stays on the cloud PR workflow |
| **D4** | Does the person approve the design? | **Always, for M1.** No agent decides on its own that an app needs extra capabilities, a different architecture or new permissions. Revisit automatic approval after three genuinely different apps have passed the workflow |
| **D5** | Which app for M0? | **A small habit tracker**: several screens, persistent local storage, create, edit and complete actions, a simple history view, at least one UI test. A better test than another tip calculator because it exercises app structure and persistence |
| **D6** | Which reference for Recreate's first feature (X1)? | **Spotify's Library** (Reuben, 2026-11-19 09:16): the web player through REA, the iPhone app through `forge ui --any-app`, compared, then recreated natively. If Spotify isn't on the phone, another installed music app with a web version. Scope and boundaries in [X1: Spotify Library](#x1-spotify-library) |

## Sources

Checked 2026-11-19.

- Craft Anything: [repository](https://github.com/storytold/craft-anything), [README](https://github.com/storytold/craft-anything/blob/main/README.md)
- REA: [repository and README](https://github.com/morluto/rea), [DISCLAIMER.md](https://github.com/morluto/rea/blob/main/DISCLAIMER.md), [Apple application analysis](https://github.com/morluto/rea/blob/main/docs/guides/apple/apple-application-analysis.md), [Android adb analysis](https://github.com/morluto/rea/blob/main/docs/guides/android/adb-device-analysis.md), [installation](https://github.com/morluto/rea/blob/main/docs/installation.md)
- OpenHands: [OpenHands CLI](https://github.com/OpenHands/OpenHands-CLI), [PyPI](https://pypi.org/project/openhands), [product page](https://hub.openhands.dev/product/cli)
- Aider: [repository](https://github.com/Aider-AI/aider), [scripting](https://aider.chat/docs/scripting.html), [release history](https://aider.chat/HISTORY.html)
- forge-lab: [README](https://github.com/GeneralKnowledge/forge-lab), [forge-mcp.md](https://github.com/GeneralKnowledge/forge-lab/blob/claude/project-thread-jt3hrz/docs/forge-mcp.md) (Tipster and Murmur runs), `crates/forge-mcp/src/tools.rs`
- ixOS: [roadmap](../roadmap.md) (tooling freeze, P6, future ideas), [AGENTS.md](../../AGENTS.md), [human-workflow.md](../human-workflow.md), [Device Lab README](../../examples/device-lab/README.md)
