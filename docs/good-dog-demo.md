# Good Dog demo: plan

Good Dog is the second demo in the core sequence, after Chat. It replaced Pocket Ledger on 2026-11-08 (Reuben's decision). This page is a plan. GD0 (the Rust engine, [`examples/good-dog-core`](../examples/good-dog-core/README.md)) is built, and GD1 (the app, [`examples/good-dog`](../examples/good-dog/README.md)) is built and passed its journey on an iPhone (2026-11-08). On 2026-11-08 Reuben set a product direction: Good Dog becomes a goal-led training companion rather than an exercise list. The milestones after GD1 now follow that direction (see [Product direction](#product-direction)). None of them is built.

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
- **Ask**: search over the approved answers. Answering on the device is GD7, and only if Reuben approves it.

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
| Apple Foundation Models (GD7, optional) | Ask answered on the device, grounded only in approved answers and guides, with the web app's safety filters applied in Rust | `forge-ai` once it exists | Local AI |

## Product direction

Reuben's brief (2026-11-08) asks for Good Dog to change from an exercise library with a daily planner into a personalised training companion. Owners should open it because they want help with something in their dog's life, not to browse exercises. The brief has ten ideas in four phases:

1. Goals become central.
2. Training becomes adaptive.
3. Good Dog understands the individual dog.
4. Beyond formal exercises.

It also sets seven principles. Welfare comes first. Difficulty means making the next step easier. Teach why. Reward real-life progress. AI stays inside welfare-conscious bounds. No forms, streaks or noise. Build on what works.

Its test for every feature: *does this help an owner understand their dog, make a better decision, improve everyday life or recognise meaningful progress?*

### What GD1 already gives us

- **An engine that already responds to evidence.** Each skill moves through `NotIntroduced`, `Introduced`, `Practising`, `BecomingConsistent` and `ReadyToIncrease`, and drops to `NeedsEasier` on a "too difficult" or a welfare concern. One easy result never moves a skill on. Exercises have prerequisites and easier and harder variations. A welfare concern rests that skill for three days.
- **A domain-neutral planner.** It takes a `CoachingPolicy`, so a new policy can steer it without touching the engine. The web app's 100 golden plans keep passing.
- **Goals that are only a keyword.** GD1's six goals are a closed list that boosts one exercise category. Nothing is remembered per goal, and there are no stages, no "why this matters", and no way to pause.
- **Content that is the real limit.** There are 28 exercises across 20 skills. Recall (5 exercises), loose-lead walking (3 plus check-ins), settling (3) and greeting visitors (sit, wait, door and greeting) can each become a pathway from what exists. Handling has 1 exercise, and confidence has 2, both for puppies only. A pathway for either needs new content first.

### How the brief maps onto the app

| Brief | Design | Owned by |
|---|---|---|
| Goals as the starting point; several at once; pause, change or abandon without losing history | A `Goal` is an owner's instance of a pathway: active, paused, finished or set aside, with its own history. At most three are active, so Today stays calm. Setting a goal aside keeps its sessions and stages | Rust (`goals.rs` in the app crate) |
| Natural-language goals | Suggested challenges first, plus "in your own words". The words are kept and matched against DogResearch's owner-questions taxonomy (`oq-recall`, `oq-alone-time` and so on), which points at the pathways. The owner confirms the match, so nothing is guessed silently. Matching the words with Ask comes later, in GD7 | Rust |
| Structured pathways: stages, explanations, exercises, real-world practice | Pathways are versioned content written in DogResearch and exported into `content/pathways.json`, like exercises. Each stage has a plain explanation, "why it matters", the exercises it uses, one real-life practice idea and a "ready when" rule. Sessions record the pathway's `contentVersion` too | Content, loaded by `good-dog-core` |
| Respond to evidence of learning, not completion counts | "Ready when" is a structured rule over the skill states and recent outcomes, for example "becoming consistent, no welfare concern in 7 days, and easy on 2 different days". From GD3 it also counts different contexts. The owner can always say "we're there" or "let's go back a step": progress doesn't have to be linear | Rust |
| Easier alternatives; maintenance after a skill is learned | The easier variations and `NeedsEasier` already exist. A passed stage becomes *maintenance* and appears now and then, never as a chore | Rust |
| Welfare before pathways; AI kept inside the rules | Welfare rules stay structured Rust that no text can override: no stage moves forward during a welfare cooldown, signs of stress step the pathway back and rest it, and repeated fear or reactivity signs show the referral guidance (vet or qualified behaviourist). Ask (GD7) may explain these decisions but never change them | Rust |
| Today: one next step, everyday idea, "not today", no failure | Today leads with the most relevant goal's next step and why. It offers one small everyday idea and a different activity if this one isn't practical. "Not today" is recorded as a choice, never as a missed day. There are no streaks. The planner's plan stays as "more practice" | Rust decides, SwiftUI shows it |
| Keep the library and planner | Learn and the planner stay. A new `GoalPolicy` steers the existing planner toward the active goals' current stages through `affinity_score`. The engine and the golden tests don't change | `good-dog-core` |

**One deliberate split from the web app.** Goals and pathways exist only in the native app at first. The golden fixture keeps proving the shared planner. The goal layer sits on top and has its own tests. Because pathways are written in DogResearch, the web app can adopt them later from the same source.

## Milestones

The product phases form the spine. The ixOS capabilities from the original plan (reminders, charts, export, widget, Ask) attach to the phase where the product needs them. Old numbers are shown in brackets.

| Milestone | What | Needs the iPhone? |
|---|---|---|
| GD0 | Rust port of the engine, policy and content; the golden fixture passes 100 out of 100. **Done** (2026-11-08): also matches all 900 scenarios with swaps and 315 simulated weeks | No |
| GD1 | Native app: onboarding, Today, exercise runner, Learn, My dog, all on forge-storage; UI test journey. **Done** (2026-11-08): Rust and model tests pass on Linux; macOS CI and the laptop compiled the SwiftUI. On Cati (iPhone 15 Pro Max, iOS 26.3.1) the `journey.toml` UI test passed 29 of 29 steps, and Reuben checked dark mode | Yes: install, run the journey, and check dark mode |
| GD2a | **Goals in Rust** (phase 1). Goal and pathway model; the pathway format agreed for DogResearch, with the model and rules tested against a fixture, then recall, loose-lead walking, settling and greeting visitors once DogResearch has them; stage rules; `GoalPolicy`; goal management (start, pause, resume, set aside, "we're there", "go back"). The data file moves from version 1 to 2 with a forge-storage migration, so GD1's data survives. Tests: every pathway references real exercises and life stages, stage rules, welfare holds, history kept across pause and set aside, and the migration. **Built** (2026-11-08): the format, loader and checks in `good-dog-core` (`dog_training::pathways`), `GoalFocusPolicy` (with no goal it plans exactly as GD1; a goal's current-stage exercises get a boost), and the goal API in the app crate (`start_goal`, `pause_goal`, `resume_goal`, `set_goal_aside`, `answer_gate`, `step_back`, `suggest_goals` from the owner's own words), with at most three active goals. Store version 2 migrates GD1 data. Tested on Linux against `tests/fixtures/pathways-sample.json`; `content/pathways.json` stays empty until DogResearch publishes, so the shipped app offers no goals yet. Safety rules are carried but not yet enforced (see below) | No |
| GD2b | **Goals on the phone** (phase 1). Onboarding picks one to three challenges. A goal screen shows the journey: where the dog is, what this stage means and why, what "ready" looks like in words, and what comes next. Today leads with a goal's next step, adds an everyday idea, and has "not today". Pause and set aside are on the goal screen | Yes: the journey test grows to cover goals; data from GD1 survives the update |
| GD3 | **Reflections and adaptive steps** (phase 2). After a session there's a quick record (one tap still works) plus an optional reflection: where it happened (home, garden, street, park, with other dogs or visitors), signs of stress or switching off chosen from a list, and the owner's own words. At most one follow-up question, and only when the answer changes the next step. "What this changes" explains the next step. Stage rules count contexts. Owner-reported changes appear on the goal | Yes: reflections recorded; a stress sign steps the pathway back |
| GD4 [GD3] | **Meaningful progress** (phases 2 and 4). Progress for each goal with Swift Charts: the contexts practised, learned compared with reliable, skills in maintenance. A summary to share with a trainer as CSV and PDF through the share sheet | Yes: charts read with VoiceOver; export opens in Files |
| GD5 [GD2] | **Calm reminders** (phase 1 Today). One optional daily nudge at the owner's time. It's skipped when today's practice is done or the owner said "not today". Its actions are "Start now" and "Not today". The day rolls over in the background | Yes: a reminder arrives; "Start now" opens Today; none after practice or "not today" |
| GD6 [GD4] | Spike S2, then the widget (the goal's next step) and the Live Activity (a running session) | Yes |
| GD7 [GD5] | **Good Dog knows the dog** (phase 3). A richer profile built up gradually (motivators, sensitivities, health notes the owner chooses to give, routines), each item asked when it becomes relevant. Contextual troubleshooting from the history. On-device Ask grounded in the dog's goals and history, which the owner can correct, with transparent memory. **Ask needs Reuben's go-ahead** | Yes: answers stay grounded; welfare rules can't be argued past |
| GD8 | **Everyday life** (phase 4). Guides and activities for walks, rest, enrichment, handling, visitors, travel and confidence. Management and giving space count as valid outcomes. Needs new content, and qualified review where it calls for it | Yes |
| GD9 [GD6] | iPhone acceptance against the brief's test, a framework gap report, and the ledger updated | Yes |

### Pathway format (proposed for DogResearch)

DogResearch already has most of what a pathway needs, so the format reuses it rather than inventing a new one. `kb/meta/uk-lite-course.json` (the 10-week Foundation course) already has stages with an aim, lessons, real-life practice, a **gate** made of observable assessment metrics (`metric-recall-long-line-5m`), `opensAfter`, and `ifGateFails` (the easier route back). It also has branches for puppies, rescues, adolescents and hard walks with their own constraints, and safety gates (`safe-002-recall-management` holds off-lead work). A goal pathway is that course shape scoped to one goal:

```json
{
  "id": "path-recall",
  "title": "Coming back when called",
  "summary": "From turning to their name to coming back on a long line.",
  "matchPhrases": ["come back", "recall", "runs off", "won't come"],
  "ownerQuestionCategoryIds": ["oq-recall"],
  "behaviourIds": ["beh-recall-to-handler"],
  "lifeStages": ["older_puppy", "adolescent", "adult", "senior"],
  "contentVersion": 1,
  "stages": [
    {
      "id": "recall-near",
      "title": "Coming back nearby",
      "whatItMeans": "One recall word, and your dog comes to you in a quiet, enclosed space.",
      "whyItMatters": "A recall that always pays off at home is the base for every place after it.",
      "exerciseIds": ["ex-recall-foundation"],
      "lessonIds": ["lesson-recall-basics"],
      "everydayPractice": "Call once when your dog is already heading your way, and make arriving worth it.",
      "opensAfter": ["recall-name"],
      "gate": { "summary": "Returns to arm's reach on a 5 m long line in low distraction.", "metricIds": ["metric-recall-long-line-5m"],
                "ownerCheck": "On a 5 metre long line somewhere quiet, does {dog} come back to arm's reach when called once?" },
      "ifGateFails": "Go back to the name game and shorten the distance.",
      "safetyRuleIds": ["safe-002-recall-management", "safe-003-no-punish-return"],
      "maintenance": "A few rewarded recalls on ordinary walks each week."
    }
  ]
}
```

Two fields were added while building GD2a: `matchPhrases` (everyday words an owner might type, so "he pulls on the lead" suggests loose-lead walking; nothing is guessed when nothing matches) and `gate.ownerCheck` (the gate as the plain question the owner answers, with `{dog}` for the name). `behaviourIds` is optional and not read yet. The test fixture `good-dog-core/tests/fixtures/pathways-sample.json` is the reference example.

How the app uses it:

- **Ready when.** A stage's gate passes when both are true. First, the app's own evidence: its exercises are at least `BecomingConsistent`, with "went well" or "getting there" on two different days, and there is no welfare concern in the cooldown. Second, the owner confirms the gate's observable criteria, asked as a plain question ("Did Moss come back to within arm's reach on the long line?"). Completion counts never pass a gate.
- **Holds.** A welfare rest or a skill being made easier holds the stage: the owner's "yes" is refused until the hold lifts. Without a hold, the owner may say yes ahead of the app's evidence, since they see the dog every day; the goal's history records that it was ahead of the evidence.
- **When it doesn't go well.** `ifGateFails` is shown. "Not yet" is recorded and changes nothing else; "go back a stage" is the owner's choice. GD3's reflections are where a stress sign steps the stage back automatically.
- **Safety gates (not yet enforced).** `safetyRuleIds` are carried through but not enforced: DogResearch's rules are prose, and Rust needs a machine-readable condition for each before it can hold a stage. Agreeing that shape with DogResearch is part of writing the first pathways.
- **Exports.** DogResearch exports pathways as `dist/good-dog-pathways.json` beside its other files, and its QA checks that every exercise, lesson, metric and rule id exists. `export-from-web.sh` copies the file into `good-dog-core/content/`, and a Rust test checks the same ids again.
- **Branches** (puppy, rescue, adolescent) become per-stage notes chosen by the dog's profile. GD7 widens this.

The first four pathways need recall metrics (which exist), loose-lead metrics (one exists), and settling and visitor metrics (not yet written). DogResearch's own `gap-analysis.md` is where those gaps are tracked.

### Where M6 fits

M6.5 (the agent demonstration in [forge-m6.md](forge-m6.md)) builds a **guided exercise session** on the current app (Reuben, 2026-11-08 17:54): an exercise's steps become cards the owner follows one at a time, recorded through the existing outcome choice, as requirement GD-7. It needs no GD2a. Pausing, resuming and setting aside a goal, after GD2a, is a candidate for M6.6, with the goal journey view (part of GD2b) as its fallback.

### Open questions for Reuben

- **Where pathway content is written: DogResearch first** (Reuben, 2026-11-08). Pathways are written in GeneralKnowledge/DogResearch next to the exercises they use, and `export-from-web.sh` exports them into `good-dog-core/content/pathways.json` like the rest of the content. One source serves both apps. GD2a's Rust model and rules are built and tested against a small test fixture in the agreed format, and real pathways ship once DogResearch has them.
- **Handling and confidence pathways** need new exercises (adult confidence and grooming handling) before they can be offered. Who writes them, and whether they need qualified review.
- **GD3's reflection list** (contexts and signs of stress) should be reviewed for welfare wording before it ships.

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
