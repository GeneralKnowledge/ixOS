# Good Dog demo: plan

Good Dog is the second demo in the core sequence, after Chat. It replaced Pocket Ledger on 2026-11-08 (Reuben's decision). This page is a plan. GD0 (the Rust engine, [`examples/good-dog-core`](../examples/good-dog-core/README.md)) is built, and GD1 (the app, [`examples/good-dog`](../examples/good-dog/README.md)) is built and passed its journey on an iPhone (2026-11-08). The rest is not built.

Good Dog already exists as a web app ([GeneralKnowledge/Good-Dog](https://github.com/GeneralKnowledge/Good-Dog), Next.js and SQLite). It helps ordinary UK dog owners train their dog through short, reward-based activities: open the app, see what to practise today, follow the steps, say how it went, and get a better plan tomorrow. The native version keeps that product and rebuilds it the ixOS way: Rust owns the plan engine, the content and the data, and SwiftUI presents them.

Like every demo, it is chosen for the capabilities it forces ixOS to build, not for being another app.

## Why it replaced Pocket Ledger

| | Pocket Ledger | Good Dog |
|---|---|---|
| Forms with Rust validation | Heavy | Light (dog profile, onboarding) |
| Relational storage | Yes | Yes (dogs, plans, sessions, skill progress, content versions) |
| Charts | Yes | Yes (progress over time) |
| CSV export | Import and export | Export only (training history for a trainer) |
| Face ID | Yes | No. Moves to the services check app when it is next needed (W3) |
| Large-dataset performance | Yes | No. Chat already covers it |
| Recurring schedules, conditional reminders | No | Yes |
| Notification actions | No | Yes |
| App extensions: widget and Live Activity | No | Yes, the first extension in ixOS |
| Bundled, versioned content with migrations | No | Yes |
| Porting an existing engine into Rust, checked against recorded output | No | Yes (100 golden plan scenarios) |

Pocket Ledger mostly reused what Chat and Pocket Library already exercise. Good Dog covers most of Ledger's list too, and adds scheduling and the first app extension. It also absorbs most of the parked PlantLab idea (widgets and scheduling). Pocket Ledger is parked with the other ideas.

## What the web app has

- **Engine** (`src/lib/coaching`, about 600 lines): a domain-neutral plan generator and skill-progress state machine. It handles prerequisites, easier and harder variations, avoiding recent repeats, variety, a time budget and welfare cooldowns. It never imports the dog domain, and the web app's ESLint rules enforce that.
- **Dog policy** (`src/lib/domains/dog-training`): implements `CoachingPolicy` (eligibility by life stage, daily budget, starter exercises, goal matching, "why today" wording), plus safety rules and assessment.
- **Content**: 36 exercises with versioned snapshots, guides, a glossary, referrals and approved Ask answers, vendored as JSON from GeneralKnowledge/DogResearch.
- **Tests**: engine tests against a made-up domain, and `golden-plans.test.ts`, which compares exact plan output for 100 recorded scenarios.
- **Tabs**: Today, Learn, Shop, My dog, Ask. There are accounts with guest sessions, Web Push reminders sent only if practice is still outstanding, and an optional Ask helper backed by OpenAI.

## The native app

Version one runs on the phone only, like Murmur. There is one local dog profile, no account, no server, no shop and no remote AI.

**Rust** (the engine is in `examples/good-dog-core`, GD0; the app crate will be `examples/good-dog/rust`):

- `coaching`: the engine ported from TypeScript. Keep the same split and the rule that it never mentions dogs. If a second coaching app ever appears, the engine is the candidate to move into the framework ([architecture.md](architecture.md#framework-principles)).
- `dog_training`: the policy, the safety rules and the content loader.
- Content ships inside the app as the same JSON files the web app vendors. The app records each exercise's `contentVersion` when it is used, so a content update never rewrites history.
- Storage on forge-storage SQLite: dogs, daily plans, training sessions, skill progress and term progress, with forge-storage migrations.
- The golden fixture is copied over unchanged. `cargo test` must reproduce all 100 plans exactly, which proves the port matches the web app.
- Everything is logged through `forge_runtime::logging`, never including the owner's notes or the dog's name.

**SwiftUI** (per [ios-ui.md](ios-ui.md)):

- **Today**: the day's 2 or 3 activities with "why today", and swapping an activity.
- **Exercise**: preparation, steps, what to look for, and what to do if it's hard. It has a session timer, a haptic marker tap, and the outcome buttons (easy, getting there, too difficult), plus a welfare concern option.
- **Learn**: exercises by topic, guides, and a searchable glossary with tap-to-define terms.
- **My dog**: profile, plain-English progress with charts, reminder settings, and history export.
- **Ask**: search over the approved answers. Answering on the device is GD5, and only if Reuben approves it.

Accessibility identifiers follow `screen.element` (`today.item.0`, `exercise.outcome.easy`, `dog.reminders.toggle`).

## Capabilities it forces

These are the ledger rows Good Dog would move. Today each is **No** in the [capability ledger](capability-ledger.md).

| Capability (ledger row) | What Good Dog needs | Where it should live | Workstream |
|---|---|---|---|
| Recurring schedules | A daily reminder at the owner's chosen time, skipped when today's practice is done | `forge-services` notifications, with schedule rules owned by Rust | W4 |
| Notification actions and grouping | "Start now" opens today's plan; "Tomorrow" snoozes | `forge-services` (categories and actions; grouping exists) | W4 |
| Background refresh and processing tasks | Roll the plan over at the day boundary and reschedule the reminder without opening the app | `forge-services` | W4 |
| Charts (Swift Charts) | Sessions per week, and skills moving from "introduced" to "consistent" | A SwiftUI helper; the data comes from Rust | W8 |
| Widgets | Today's plan on the Home Screen | First app extension; a Rust store shared through an App Group | W9, spike S2 |
| Live Activities | A running session on the Lock Screen and in the Dynamic Island | Same extension | W9 |
| CSV import and export, Report export | The training history as CSV and a one-page PDF to share with a trainer | forge-storage export plus the share sheet | W2 |
| Haptics | A marker tap during an exercise | SwiftUI, documented | W8 |
| Apple Foundation Models (GD5, optional) | Ask answered on the device, grounded only in approved answers and guides, with the web app's safety filters applied in Rust | `forge-ai` once it exists | Local AI |

## Milestones

Each milestone ends with something the phone can show. Agent runs use the staged pattern from the Murmur run ([forge-mcp.md](forge-mcp.md)).

| Milestone | What | Needs the iPhone? |
|---|---|---|
| GD0 | Rust port of the engine, policy and content; the golden fixture passes 100 out of 100. **Done** (2026-11-08): also matches all 900 scenarios with swaps and 315 simulated weeks | No |
| GD1 | Native app: onboarding, Today, exercise runner, Learn, My dog, all on forge-storage; UI test journey. **Done** (2026-11-08): Rust and model tests pass on Linux; macOS CI and the laptop compiled the SwiftUI. On Cati (iPhone 15 Pro Max, iOS 26.3.1) the `journey.toml` UI test passed 29 of 29 steps, and Reuben checked dark mode | Yes: install, run the journey, and check dark mode |
| GD2 | Daily reminders that skip finished days, notification actions, and the day rollover in the background | Yes: a reminder arrives, "Start now" opens Today, and no reminder comes after practice |
| GD3 | Progress charts, and CSV and PDF export through the share sheet | Yes: charts read with VoiceOver, and the export opens in Files |
| GD4 | Spike S2, then the widget and the Live Activity | Yes: the widget shows today's plan, and the Live Activity follows a session |
| GD5 | On-device Ask (optional, only with Reuben's go-ahead) | Yes: answers stay grounded, and refusals are handled |
| GD6 | iPhone acceptance run and a framework gap report; ledger updated | Yes |

## Where the native app departs from the web app

Reuben asked for the better native or ixOS approach where there is one (2026-11-08). GD1's departures:

| Web app | Good Dog on iOS | Why |
|---|---|---|
| "What would you like help with?" is free text, matched by keywords | A closed list of six goals, each mapped to the phrase the engine matches | The web app's own architecture notes call free text brittle; a picker is also the native control |
| Accounts, guest sessions, sign-in | One dog, on the phone, no account | Phone-only v1; nothing to sync yet |
| Shop and Ask | Left out | Shop needs a server; Ask (on the device) is GD5 and waits for Reuben's go-ahead |
| Glossary terms in text open a tooltip | Terms are links that open the term in a sheet, with related words and exercises to practise | The native pattern, and the glossary becomes a place to learn from |
| SQLite through Drizzle, several tables | One versioned JSON document (forge-storage `JsonFile`) | A dog's data is small (a profile, a few plans, sessions); one document saves atomically and needs no queries. SQLite comes back if history grows or Pocket-Ledger-style reports return |
| The server's time zone setting decides the day | The phone passes the time and its UTC offset (`LocalTime`) | The plan follows the owner's own day, and tests fix the clock |
| A session runner with a marker word | A stopwatch; the marker word stays in the steps | A haptic "marker" tap is useless while you look at the dog; a sound marker can come later |
| Progress copy | Rust writes the after-session message, and it only promises what the planner does (a welfare concern rests the skill for three days and keeps plans gentle) | The web app's improvement roadmap found an over-claim (P0-3) |

Three web exercises use skills with no name in the web content. The app falls back to the exercise's title for them (see `good-dog-core/content/SOURCE.md`).

## Risks and open questions

- **Free team.** The widget extension needs its own App ID, and Good Dog takes one of the three app slots on the phone. Spike S2 must check whether a free team can use an App Group. If it can't, the Live Activity still works because the app pushes its state, but the widget would need another route or the paid team.
- **Two copies of the engine.** The web app stays in TypeScript. The golden fixture is the contract between them. A plan change in one copy means regenerating the fixture and porting the change. Running the Rust engine on the web through WebAssembly is possible later, but it is out of scope.
- **Content source.** Exercises live in `exercises.ts` in the web repo, not in JSON. `examples/good-dog-core/scripts/export-from-web.sh` exports them from a web checkout without changing the web repo. The export found three exercises whose learning objective has no label in the web app (see `content/SOURCE.md`).
- **Welfare and claims.** Keep the web app's wording rules: general guidance, not veterinary advice, and no claims of professional endorsement.
