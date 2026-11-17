# Reference deep audit 2: Ice Cubes, for the ixOS design language

Status: 2026-11-17. Phase 1b of [ixOS Experience Research](../README.md). Research only: no change to an app, ForgeUI or ForgeAppKit.

**Reference:** [Ice Cubes](https://github.com/Dimillian/IceCubesApp) at `98098b9` (2026-10-13), a SwiftUI Mastodon client. **AGPL-3.0:** we read it for decisions and structure and copy no code; this page describes, it doesn't quote. Paths below are relative to its repository.
**ixOS side:** not one app but the shared layer: ForgeUI, ForgeAppKit, and the patterns every app repeats ([audit](../audit.md), cross-app findings X1 to X26). Ice Cubes is the reference because it is a large, mature SwiftUI app that has had to make the same choices ten times over: rows, actions, loading, feedback, settings.

**Evidence:** **S** (source verified) on both sides. Ice Cubes was read, not run; no screenshots of it are reproduced.

## Ice Cubes in one paragraph

A social reader built from feature packages: `DesignSystem` (theme, fonts, colours, a few shared views), `Env` (router, preferences, haptics, toasts, sounds), and one package per feature (`Timeline`, `StatusKit`, `Notifications`, `Conversations`, `Explore`, `Account`, `Lists`, `MediaUI`). One row design, the status row, appears in the timeline, notifications, search and profiles. Its frequent actions (reply, boost, favourite, bookmark, share) are visible under every post; the rest are in a context menu; swipe actions are there too and the person chooses which. It has a lot of settings, and much of its polish is in small, consistent feedback: haptics with a vocabulary, toasts for work that continues in the background, skeleton rows while loading.

## Lessons, and where each belongs

### 1. Named text roles over system styles

**Ice Cubes:** every screen uses the same handful of roles (`scaledTitle`, `scaledHeadline`, `scaledBody`, `scaledSubheadline`, `scaledCallout`, `scaledFootnote`, `scaledCaption`) defined once (`Packages/DesignSystem/Sources/DesignSystem/Font.swift`), each relative to a system text style so Dynamic Type still works.
**ixOS:** system text styles chosen per screen, so the same role (row title, metadata, section note) gets `.headline` in one app and `.subheadline` in another (audit X3).
**Why it's better:** a role carries intent ("this is metadata"), so changing the look of metadata is one edit and every screen agrees.
**Where:** ForgeUI.
**Proposal (P2):** a few named roles in ForgeUI, each a system text style underneath. Leave out Ice Cubes' custom fonts and its own font-size slider: Dynamic Type already does that job.

### 2. Haptics as a small vocabulary

**Ice Cubes:** one place (`Packages/Env/Sources/Env/HapticManager.swift`) names five kinds of haptic: success or error notification, button press, data refresh (with intensity), tab selection, timeline. They're used 47 times across the app, most often for success (24), and Settings can turn each group off (`HapticSettingsView`).
**ixOS:** `sensoryFeedback` 19 times in seven apps, chosen ad hoc; four apps hand-roll a `saveCount` trigger for success (audit X14).
**Why it's better:** a vocabulary makes feedback consistent and proportional (success feels the same everywhere, a refresh is lighter than a save), and being deliberate stops it becoming noise.
**Where:** Docs for the vocabulary (which moments get which feedback); ForgeAppKit for the success trigger the apps already write by hand. SwiftUI's `sensoryFeedback` stays the mechanism.
**Proposal (P2):** write the vocabulary into ios-ui.md (success, failure, selection, refresh, nothing for routine taps). No per-category settings: iOS's own System Haptics switch is enough for apps this size.

### 3. A small banner for work that continues, and for undo

**Ice Cubes:** a toast centre (`Packages/Env/Sources/Env/ToastCenter.swift`) shows a short message, or a progress banner that updates. Posting is the clearest use: the composer closes at once, and a progress toast follows the upload (`StatusKit/Editor/EditorStore.swift`), so the person goes back to reading.
**ixOS:** work either blocks a sheet (Relay's send sheets, Chat's attachment staging) or happens silently; there is no undo after a destructive action (audit X10).
**Why it's better:** the person isn't held hostage by a slow task, and still knows it is happening and when it ended.
**Where:** ForgeAppKit for the model value (message, progress, optional action such as Undo); ForgeUI for its look. It is the same surface the audit's undo proposal needs, so one piece answers two findings.
**Proposal (P2):** one banner value with an optional action and progress, announced to VoiceOver, never used for errors that need a decision (those stay alerts). iOS has no system toast, so keep it rare and small.

### 4. Loading that looks like the content

**Ice Cubes:** lists load as placeholder rows of the real layout, greyed with SwiftUI's own `.redacted(reason: .placeholder)` (18 uses; the models have `placeholder()` values, `Packages/Models/Sources/Models/Account.swift`). Full-screen spinners are rare.
**ixOS:** a centred `ProgressView` or `ForgeStateView.loading` (audit X15).
**Why it's better:** the screen keeps its shape, so nothing jumps when the data arrives, and the wait feels shorter.
**Where:** App (the placeholder values are domain data); a note in ForgeUI's docs that `.redacted` is the native tool for lists.
**Proposal (P3):** use redacted placeholder rows where a list loads over the network (Commonplace refresh of a new feed, Soundscape's library, Pocket Library's catalogue search); keep the spinner for actions.

### 5. One row, actions in plain sight

**Ice Cubes:** the status row (`Packages/StatusKit/Sources/StatusKit/Row/`) has the same anatomy everywhere: header (who, when), content, media, action bar. Frequent actions are buttons in the row; occasional ones are in its context menu (`StatusRowContextMenu.swift`); swipe actions repeat them for speed (`StatusRowSwipeView.swift`).
**ixOS:** frequent actions are often swipe-only: Commonplace's bookmark and read state, Checklist's rename and delete, FieldKit's delete (audit X11).
**Why it's better:** discoverable for a newcomer, fast for an experienced user, and the three ways in say the same thing.
**Where:** Docs (when an action belongs in the row, the menu or a swipe); App.
**Proposal (P2):** a rule of thumb in ios-ui.md: an action used on most visits is visible; an occasional one is in the context menu; swipe is a shortcut for either, never the only way.

### 6. Dense rows that work with VoiceOver

**Ice Cubes:** each row is one VoiceOver element with a composed label, its actions offered as custom actions (`accessibilityActions`), and the row's main action on activate; conversation rows also answer the Magic Tap (`Packages/Conversations/Sources/Conversations/List/ConversationsListRow.swift`). The label is built only while VoiceOver is on.
**ixOS:** rows combine their children (`accessibilityElement(children: .combine)`) and swipe actions reach VoiceOver through the list, but custom actions are used twice in all the apps.
**Why it's better:** a VoiceOver user hears one sentence per row and reaches every action without hunting for buttons inside it.
**Where:** Docs; App (Chat's message bubbles and conversation rows, Commonplace's article rows first).
**Proposal (P2):** for rows with more than one action, add them as custom actions. Confirm on the phone with VoiceOver (phase 5) before making it a check.

### 7. New content never moves what you're reading

**Ice Cubes:** new posts arriving above the reading position add to a counter pill (`Packages/Timeline/Sources/Timeline/TimelineUnreadStatusesObserver.swift`) instead of shifting the list; tapping it goes up.
**ixOS:** Chat now does this with its New Messages button (friction audit F3, fixed in #259). Commonplace's refresh inserts new articles in place.
**Where:** App.
**Proposal (P3):** when Commonplace becomes a full reader (unified feed, background refresh), keep the reading position on refresh and offer "N new" instead. A second use would make it a ForgeAppKit candidate.

### 8. Choosing a server without knowing a URL

**Ice Cubes:** adding an account shows **suggested instances** with what each is about and how big it is, filtered as you type; you can still type any address (`IceCubesApp/App/Tabs/Settings/AddAccountsView.swift`). It loads them as redacted placeholder rows.
**ixOS:** Chat's sign-up is a blank server URL field with a `make chat-server` footer (audit C1, X24).
**Why it's better:** the person picks from something they can understand; the expert path is still there.
**Where:** App (Chat).
**Proposal:** an input to Chat's first-run decision (audit C1, option c): an invitation link or a short list of known servers (for a private group, probably one), with "Use another server" for self-hosting.

## What ixOS already does as well or better

- **Typed destinations.** Ice Cubes' router (`Packages/Env/Sources/Env/Router.swift`: `RouterDestination`, `SheetDestination`) is the same idea as ForgeAppKit's destination helpers (`forgeSheet`, `forgeNavigationDestination`). No change needed.
- **Testability.** Ice Cubes sets no accessibility identifiers at all; every ixOS control has one, so UI tests and baselines can find them.
- **Reduce Motion and accessibility text sizes.** Ice Cubes never reads Reduce Motion and checks accessibility sizes in three places; Good Dog and Pocket Library already relayout at accessibility sizes.
- **Package boundaries.** Ice Cubes' DesignSystem and Env packages match ForgeUI (look) and ForgeAppKit (behaviour); its feature packages are what ixOS keeps inside each app.

## What Ice Cubes gets wrong, so we don't import it

- **Settings as the answer.** More than a dozen settings screens: display, haptics, swipe actions, tab bar entries, fonts, translation and more. Customisation suits a power-user social client used for hours a day; for ixOS apps each setting is a decision we didn't make. Default to deciding.
- **Its own type scale on top of Dynamic Type** (a font-size multiplier and custom fonts). It duplicates a system feature and breaks the "text styles only" rule in ios-ui.md.
- **Fixed sizes in places** (`.system(size:)` five times). Its colour themes pick a readable text colour for the tint (`Theme.swift`, `computeContrastingTintColor`), but we found no check of a whole palette against a contrast standard; ForgeUI's `contrastIssues()` should stay the gate.
- **No identifiers, no Reduce Motion:** see above.

## Where each proposal belongs

| # | Proposal | Priority | Where | Second use |
|---|---|---|---|---|
| 1 | Named text roles over system styles | P2 | ForgeUI | All apps (X3) |
| 2 | Feedback vocabulary; name the success trigger | P2 | Docs; ForgeAppKit | Four apps hand-roll it |
| 3 | One banner for continuing work and undo | P2 | ForgeAppKit (value), ForgeUI (look) | Undo (Checklist, Commonplace), background sends (Relay, Chat) |
| 4 | Redacted placeholder rows for network lists | P3 | App; ForgeUI docs | Commonplace, Soundscape, Pocket Library |
| 5 | Visible, menu, swipe: where an action goes | P2 | Docs; App | X11 apps |
| 6 | Custom VoiceOver actions on multi-action rows | P2 | Docs; App | Chat, Commonplace |
| 7 | Keep the reading position; "N new" | P3 | App | Chat done; Commonplace next |
| 8 | Suggested servers or an invitation at first run | input to C1 | Chat | Soundscape's take server |

Proposals 1 to 3 are the first concrete candidates for phase 3's foundations; each has the second use the extraction rule asks for. They don't add a component library: two are a few named values and one is a single banner.

## Suggested next step

Use proposals 1, 2, 3 and 5 as inputs to the phase 2 design-direction doc. The NetNewsWire audit (Commonplace and the reading view) is the last of the three.
