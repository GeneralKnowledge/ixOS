# App capability audit for agent-composed surfaces (A2UI)

Status: 2026-11-22. Research only, no code. Reuben approved the audit on 2026-11-22 as the milestone after A2UI-1 and A2UI-1b ([A2UI assessment](external-projects/assessments/ai/a2ui.md#a2ui-1-as-built)).

## The question

Reuben, 2026-11-22: identify **the typed actions and data each app could safely expose to A2UI**, so an agent composes interfaces around real app capabilities. "The next substantial improvement should be making the underlying apps more capable of supporting dynamic workflows", not more components.

The rules this audit works inside:

- **ForgeUI owns the look; A2UI only describes what an agent wants shown; the app confirms every change** ([A2UI and ForgeUI](external-projects/assessments/ai/a2ui.md#a2ui-and-forgeui-reuben-2026-11-22)).
- **No new components.** The eight A2UI-1b draws (Text, Column, Row, Card, Divider, Button, DateTimeInput, ChoicePicker) are enough for now, with only the properties the adapter draws.
- **Expand existing apps before adding new ones**; extract into the framework only on a second use.
- A2UI stays in Device Lab (`scripts/check-boundaries.sh`). Models are A2UI-4, not this audit.

## The answer

1. **Five apps have a capability worth exposing, plus one surface in Device Lab, and every one fits the eight components.** Two shapes cover all of them: a **read-only summary** built by Rust from typed data, and a **pick-and-confirm proposal** that only refers to ids the app already owns. No candidate needs a new component or a new property.
2. **Three apps can do it with functions they already have**: Commonplace (organise feeds into folders), Soundscape (a playlist from the library) and Good Dog (choose a goal from your own words). **Two need an app feature first**: Pocket Library has no reading plan or goal to apply A2UI-1's reading plan to, and Checklist has no due dates.
3. **The capability belongs in the app's crate, the surface in Device Lab.** Each app adds a typed proposal, a check that turns it into a change set, and an apply step, in plain Rust with no A2UI in it. Device Lab's A2UI layer turns that into a surface. The app's capability is then useful without A2UI too (App Intents, a Relay agent later), and no app depends on A2UI.
4. **The gate is written for one surface.** Before a second surface, `a2ui_lab/gate.rs` splits into the general catalogue rules and a per-surface registration (surface id, events and their context, data schema, what may be redrawn). The Bonsai **verified-task card** is the smallest second surface: read-only, no events.
5. **Whole areas are never exposed**: security (all of Relay's trust, rights and pairing), safety (Good Dog's gates and safety notes, Weather's alerts), anything destructive, network requests the agent chooses, playback or recording side effects, and teaching content that needs a human reviewer (Urdu, Naturally).

## 1. Method

For each app: its exported Rust API (`examples/<app>/rust/src/lib.rs`, the `uniffi` records and objects), its next features in [app-plan.md](../app-plan.md), and the A2UI-1 pattern as built. main at `c2b6d15`. Nothing was run; the evidence is the code.

A capability counts as **safe to expose** only when all of these hold:

| Test | What it means here |
|---|---|
| Typed | The agent fills a small Rust record of ids, enums, dates and numbers. Free text only for a name or title, length-bounded, shown verbatim in the change set |
| Checked by Rust | Rust validates every field against the app's own data (known ids, real dates, limits), as `Proposal::check` in `a2ui_lab/plan.rs` does for books |
| Applied by existing logic | The confirmed change set is applied through functions the app already uses, never through a new write path |
| Reversible or small | Nothing destructive, nothing that can't be seen in the change set before Confirm |
| No side effect on display | Showing the surface fetches nothing, plays nothing, records nothing, notifies nothing |
| Fits the eight | Drawn with the eight components and only the properties the adapter draws today (`a2ui_lab/surface.rs`, `properties`) |

## 2. The pattern every capability follows

A2UI-1 set the shape ([decision 3](external-projects/assessments/ai/a2ui.md#decisions-reuben-2026-11-22)); this audit only checks which app data fits it.

| Step | Owner | A2UI-1 (Device Lab) | For an app capability |
|---|---|---|---|
| Typed proposal | App crate | `Proposal` (book ids, priorities, dates) | e.g. Commonplace `FolderProposal` (new folder names, feed id to folder) |
| Check | App crate | `Proposal::check`, `check_edited` | the app's own check against its store |
| Surface | Device Lab `a2ui_*` | `surface.rs` builds the same tree every time; data changes only | unchanged: one fixed tree per capability |
| Gate | Device Lab | `gate.rs`, one surface | general rules + the capability's registration (finding 4) |
| Person edits | Swift surface | chips, dates | chips (include, choose), dates |
| Review | App crate | change set from Rust's own data | same: names and titles from the store, never from the surface |
| Confirm | App-owned sheet | `confirm(revision)` | same revision rule: any update or edit after review voids it |
| Apply | App crate | stand-in library | the app's existing functions (`create_folder`, `move_feed`, ...) |

The two shapes:

- **Summary (read-only).** Rust builds the data; the surface has no events. Nothing to confirm. Risk is only privacy (never logged) and accuracy (all figures from Rust, none from a model).
- **Proposal (pick and confirm).** The agent proposes; the person includes, excludes, chooses or redates; Rust reviews; the app's Confirm applies. Every proposal found refers to ids the app owns; none needs the agent to invent an id.

## 3. App by app

"Applied by" names functions in the app's `lib.rs` today. "Missing" is what the app would have to build first.

### Pocket Library: reading plan

| | |
|---|---|
| Agent helps with | "Plan what I read next": priorities and start and finish dates for books in the library. A2UI-1's own scenario |
| Data shown | `Book`: id, title, author, status, progress, page count. No covers (Image is not in the eight, and `cover_url` is remote) |
| Proposal | Book ids, a priority each, start and finish dates (A2UI-1's `Proposal` unchanged) |
| Applied by | **Nothing yet.** `LibrarySettings` holds only the sort and the catalogue switch; a book has a status but no plan or dates. `set_status` could apply "start reading" only |
| Missing | A reading plan or goals in Rust: [app-plan.md](../app-plan.md#pocket-library) item 3 ("reading goals and reminders to resume"). This is the app expanding, and the A2UI surface follows it |
| Fits the eight | Yes, exactly A2UI-1 |
| Verdict | **Right capability, app not ready.** Build reading goals as a Pocket Library feature first; the A2UI surface is then A2UI-1 on real data |

### Commonplace: organise feeds into folders

| | |
|---|---|
| Agent helps with | "Sort my feeds": group subscriptions into folders |
| Data shown | `Feed`: id, title, unread count, current folder. `Folder`: id, name, feed count |
| Proposal | New folder names (bounded text) and, per feed id, a target folder (existing id or a new one) |
| Applied by | `create_folder`, `move_feed`. Undo: moving back is the same call |
| Missing | Nothing |
| Fits the eight | A card per proposed folder, a chip picker per feed (single choice), Review button |
| Leave out | `add_feed` with a URL from the agent (it fetches an address the agent chose), `delete_folder`, `remove_feed`, `mark_all_read`. Adding a feed from the curated `suggested_feeds` list is possible later, as its own capability |
| Also | A read-only **reading brief**: unread `ArticleSummary` titles and feed names per folder. Opening an article from it is an event with an article id, which the general gate would have to register; leave it out of the first step |
| Verdict | **Ready now.** All actions exist and are reversible |

### Soundscape: a playlist from the library

| | |
|---|---|
| Agent helps with | "Make me a calm evening playlist" from songs already in the library |
| Data shown | Library tracks: id, title, artist, duration. No artwork (remote URLs, and no Image) |
| Proposal | A playlist name (bounded text) and an ordered list of library track ids |
| Applied by | `create_playlist`, `add_to_playlist`, `arrange_playlist` |
| Missing | Nothing. The person can include or exclude songs (multi-select chips, which the adapter draws) but not reorder them: the agent's order stands, or is changed later in the playlist screen |
| Leave out | Catalogue search (Jamendo, Internet Archive: network), `queue_tracks` and anything that starts playback, downloads and `set_offline`, `remove_imported`, `clear_history`, artwork. It adds no source, so it respects "player features before more sources" |
| Verdict | **Ready now** |

### Good Dog: choose a goal from your own words

| | |
|---|---|
| Agent helps with | "He pulls on the lead and barks at bikes": which training goal to start |
| Data shown | `PathwaySummary` list from Rust's own `suggest_goals(own_words)`, today's plan (`TodayPlan`, read-only) |
| Proposal | One `pathway_id` from Rust's suggestion list, and the owner's words |
| Applied by | `start_goal(pathway_id, own_words)` |
| Missing | Nothing in code. But the plan puts GD7 ("Ask") behind Reuben's go-ahead, and none of Good Dog's step 0 is passed on the phone yet ([app-plan.md](../app-plan.md#good-dog)) |
| Leave out | Everything safety-related: `answer_gate`, `note_safety`, `step_back`, the welfare flag on `record_session`, lesson answers. A model must never pass a safety gate or word welfare advice |
| Verdict | **Possible, held** behind GD7's go-ahead and step 0 |

### Checklist: a generated checklist

| | |
|---|---|
| Agent helps with | "A packing list for a weekend away" |
| Data shown | `Item`: id, title, done |
| Proposal | New item titles (bounded text, a capped count) |
| Applied by | `add` per item |
| Missing | **Due dates** ([app-plan.md](../app-plan.md#checklist) item 2). Without them a generated list is only titles, which is a thin use of a surface; with them, DateTimeInput carries its weight |
| Fits the eight | Multi-select chips to include items. The person can't edit a title (no text field in the eight); they edit it afterwards in the list |
| Leave out | `remove`, `clear_done`, `import_csv` |
| Verdict | **After due dates** |

### Device Lab: Bonsai verified-task card

| | |
|---|---|
| Agent helps with | Showing what a Bonsai task actually did once the harness has checked it (suggested in the Bonsai thread, 2026-11-22) |
| Data shown | From the harness journal and `Evidence` (`harness/evidence.rs`): task, the criteria met, files written, tests ran and failed. Rust's evidence only, never the model's own summary ("the model's own word never counts") |
| Proposal | None. Read-only |
| Fits the eight | Card, Text (body and caption), Row, Column, Divider. No Button |
| Verdict | **The best second surface.** It exercises the general gate with no events, in Device Lab, on data that is already typed |

### FieldKit: trip summary

Read-only only. `TripSummary` (distance, climb, notes count) and `Profile` (pace splits) could make a summary card, but FieldKit's trip screen already shows them, so an agent adds little. No proposal is worth it now: `rename_trip` is too small, `delete_trip` is destructive, and `start_trip` and `record` are side effects. Planned trips (app-plan item 6, calendar) would be the first real proposal. **Not now.**

### Urdu, Naturally: practice plan

Rust's own scheduler (`schedule.rs`) already decides reviews, and a model choosing what to practise would compete with it. Agent-written Urdu is out: nothing is marked reviewed without a native speaker (M3). A read-only progress card from `Totals` and `Skills` is possible but adds little to the Home screen. **Not now.**

### Never exposed

| App or area | Why |
|---|---|
| **Relay**: trust, revoke, forget, rights (`set_grant`, `set_receives_from`, `set_introduces`, ...), pairing policy, servers, sites, transfers | Security. Relay's permission model decides who may do what; an agent-drawn surface must not sit in that path. Agents over Relay are stage N9, by their own design |
| **Weather** alerts | Safety-critical (safeguard B). An agent never summarises or rewords an alert. Saving a place needs a place search, which is a network request the agent would choose |
| **Sketchbook** | Drawing; no typed data an agent helps with |
| **Sums** | A specification of the iPhone's Calculator; nothing to compose |
| Every app's destructive calls | `delete_*`, `erase_all`, `erase_library`, `restore_library`, `remove_all_downloads`, `clear_history`, `remove_feed`: destructive work goes through the app's own screens and Confirm, never a proposal (as section 6 of [forgeui-0.2.md](experience/forgeui-0.2.md#62-the-catalogue) also says) |

## 4. Findings across apps

### 4.1 The eight are enough

Every candidate draws with Text, Column, Row, Card, Divider, Button, DateTimeInput and ChoicePicker, using properties the adapter draws today (chips single and multi-select, dates only, primary and default buttons, body and caption text). No candidate needs a new component, a new property or a gate relaxation.

**Recorded demand, not a proposal.** Two candidates would be nicer with a text field (editing a proposed folder name in Commonplace, a proposed item title in Checklist), and one with reordering (Soundscape). Both are workable without: the person edits afterwards in the app's own screen. If that proves clumsy in use, the text field has its second use and is in [forgeui-0.2.md section 6.2](experience/forgeui-0.2.md#62-the-catalogue) as `TextField` already. That is Reuben's call after a surface has been used, not now.

### 4.2 The capability lives in the app; the surface in Device Lab

The A2UI dependency is allowed only in Device Lab's `Package.swift`, and Device Lab already links every app's Rust crate (`examples/device-lab/rust/Cargo.toml`). So:

| Part | Where | Depends on A2UI? |
|---|---|---|
| Typed proposal, check, change set, apply | The app's crate (`examples/commonplace/rust`, ...) | No: plain Rust records and functions, tested on Linux |
| Surface builder and its gate registration | Device Lab `a2ui_*` | Yes (the JSON it builds) |
| Rendering | Device Lab's ForgeUI adapter | Yes |
| Confirm sheet and apply | The app's own Rust, called from Device Lab's sheet | No |

The surface must use the same app object the hosted app uses (the one Device Lab's Apps screen opens), never a second handle on the app's store.

This keeps rule 1 (an app's domain logic stays in the app) and rule 13 (dependencies one way). When a capability's typed proposal exists, it is also what an App Intent or a later Relay agent would call; A2UI is one consumer of it, not the reason for it.

### 4.3 The gate is for one surface

`gate.rs` hard-codes A2UI-1's surface: `SURFACE_ID`, one `REVIEW_EVENT` with one context, data only at `/books`, `UPDATABLE` = the intro text. A second surface needs a split:

| Stays general (every surface) | Becomes per surface (a registration) |
|---|---|
| A2UI 1.0 only, size and component limits | Surface id and catalogue id |
| The eight components and their drawn properties | The registered events, each from one component with an exact context |
| No `@call`, `functionCall`, `callRendererFunction`, `deleteSurface` | The data paths and the check for each (books, feeds, tracks) |
| An update can't change an existing component's action | Which components may be redrawn after creation |
| No `userMessage`; all or nothing | |

This is Device Lab code. It moves toward ForgeKit only when a second *app* (not a second surface) uses A2UI, per Reuben's 11:12 review and the apps-first rule.

### 4.4 What not to build

- No shared "capability" trait or registry in the framework yet. Each app writes its own proposal record; if Commonplace and Soundscape end up with the same shape, that is the evidence for extracting it.
- No free composition. Each capability has one fixed tree; the agent fills data only (A2UI-1's rule).
- No agent-callable functions: still none `agentOnly` or `rendererOrAgent`.

## 5. Proposed order

Each step is its own PR and starts only on Reuben's go.

| Step | What | Where | Needs the iPhone |
|---|---|---|---|
| C1 | Split the gate into general rules and a per-surface registration; A2UI-1's surface re-registered unchanged; tests prove the lab's behaviour is the same | Device Lab Rust | No (Rust only, Linux tests) |
| C2 | Bonsai verified-task card as the second surface, read-only | Device Lab, with the Bonsai thread | Short: the card's look, VoiceOver, large text |
| C3 | First app capability on real data: **Commonplace "organise feeds"** (typed proposal and apply in Commonplace's crate, surface in Device Lab), with a fixed proposal standing in for a model, as A2UI-1 did | Commonplace crate + Device Lab | ~10 min: propose, change, review, confirm, folders moved in Commonplace, Undo by moving back |
| C4 | Soundscape playlist, the second app capability. If its proposal shape matches Commonplace's, that is the evidence for 4.4's extraction question | Soundscape crate + Device Lab | ~10 min |
| Later | Pocket Library reading plan (after reading goals are built), Checklist (after due dates), Good Dog (after GD7's go-ahead). A model filling any of these is A2UI-4 | | |

Why Commonplace first rather than Pocket Library, which A2UI-1 imitated: Pocket Library has nothing to apply a reading plan to, so it would mean building reading goals first; Commonplace's folders exist and its two calls are reversible. Pocket Library's reading goals are worth building on their own merit as an app feature (app-plan item 3), and the A2UI surface then follows at little cost.

## 6. Decisions for Reuben

1. **Order.** C1 to C4 as above, or Pocket Library's reading goals first so the A2UI-1 scenario becomes real. Recommended: C1 to C4; reading goals stay in Pocket Library's own queue.
2. **Where the surface lives.** Capability in the app's crate, surface in Device Lab (4.2), so no app depends on A2UI. Recommended as written; the alternative, A2UI inside the app's own package, contradicts "keep it out of production apps".
3. **Good Dog.** Leave the goal chooser behind GD7's go-ahead. Recommended.
4. **Text field.** Not added now; revisit only after C3 or C4 has been used on the phone (4.1).

## Coordination

The [ForgeUI 0.2](experience/forgeui-0.2.md) work (items 0 to 3, its own thread) owns the catalogue contract in its section 6. This audit adds nothing to it and proposes no ForgeUI change; everything here draws with what A2UI-1b already draws.
