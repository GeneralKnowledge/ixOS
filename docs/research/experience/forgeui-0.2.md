# ForgeUI 0.2: component-demand audit and specification

Status: proposal, 2026-11-22, on main at `8ca208c`. Part of [ixOS Experience Research](README.md). Asked for by Reuben (2026-11-22): "a proper ForgeUI coverage audit and 0.2 specification, using the existing apps as evidence rather than guessing what components we need", with a Forge A2UI component contract designed alongside it. No component is written here; the only code in this PR is the ForgeButton fix in section 4. Evidence is the source on `main` unless a row says otherwise; nothing here was checked on the phone.

It builds on the [ForgeUI coverage audit](forgeui-coverage.md) (#325, 2026-11-18) and the [app UI review](app-ui-review-2026-11.md) (2026-11-21), and doesn't redo them. Decisions from them still stand: coloured icon tiles from `ForgeAccent` only (D1), SwiftUI's `.bordered`/`.borderedProminent` are first-class (D2), no mass typography migration, and every feature checks ForgeUI and ForgeAppKit before building its own.

## The answer

1. **#325's finding still holds, with a sharper shape.** Adoption follows parity rules. Every shared piece that came with a rule is now in nearly every app that needs it; every piece that came without one is still almost unused. So the first job of 0.2 is still adoption, and the way to do it is a rule per piece, not a migration drive.
2. **The apps now show five real component gaps**, each built two to five times by hand, differently each time, and usually with an accessibility difference between the copies: a **rating** control, a **media thumbnail** frame, a **selectable choice** (chips and tiles), a **stat** (a figure with its label), and a **progress summary**. Plus one decided-but-unbuilt parameter (accent icon tiles, D1) and one piece the A2UI adapter can't reach (ForgeUI's button look outside a `Button`).
3. **Most of Reuben's "missing" list is native SwiftUI and should stay native**: toggles, pickers (segmented, menu, inline), sliders, steppers, date pickers, `LabeledContent` for structured data, `TabView`, `NavigationStack`. The apps use them, correctly, and nothing is gained by wrapping them. ForgeUI documents them; it doesn't wrap them.
4. **No app evidence yet** for toasts, filter chips or multi-select, split layouts, adaptive card grids, elevation or motion tokens. They stay out of 0.2.
5. **The ForgeButton gap is real** and is fixed in this PR: the button ignored taps while working but stayed enabled for VoiceOver, Voice Control and the keyboard. No app uses `ForgeButton`, so nothing in the apps changes.
6. **For A2UI**, the useful contract is a small closed catalogue whose every entry names its ForgeUI (or native) target, typed properties drawn from ForgeUI's tokens, the accessibility it must carry and the events it may send. Section 6 proposes it. Four of the five new components are also what an agent-composed surface most needs, which is a good sign the two plans line up.

So 0.2 is: one fix, one parameter, one public look, five components, three adoption rules, and a showcase that shows states. That is smaller than Reuben's stage list for 0.2, and in one place (thumbnails) it pulls forward something he had in 0.3, because the apps say so.

## 1. Method

Every Swift file in the twelve apps under `examples/` (Generated code left out), counted by pattern, then every app-defined `View` type read for what it is. Apps: Soundscape, Sketchbook, Commonplace, Checklist, FieldKit, Relay, Good Dog, Weather, Device Lab, Pocket Library, Urdu Naturally, Sums. Device Lab's own screens count as one app; it also hosts the others, which are counted once, in their own folders. The A2UI spike is PR #377 (`claude/project-thread-hzo4mj`), read at its head.

Each candidate was judged on four questions, Reuben's four:

| Criterion | What counts as evidence |
|---|---|
| **Built independently** by several apps | Two or more app-defined views doing the same job |
| **Built from raw SwiftUI** where ForgeUI has something | Counted uses of the raw pattern against the ForgeUI piece |
| **Awkward or impossible** to build consistently | Copies that differ in look or, worse, in accessibility |
| **Useful for AI-generated interfaces** | Would an A2UI surface (a reading plan, a task flow, a queue) need it, and can it be described by typed properties alone |

And two filters from the standing rules: a second real use before extraction (apps first), and wrap Apple's frameworks, never replace them (no `ForgeToggle`).

## 2. Adoption: what changed since #325

| ForgeUI / ForgeAppKit piece | Parity rule | Apps using it (of 12) | Then (#325) |
|---|---|---|---|
| `ForgeStateView` | `state-views`, `root-loading` | 11 (all but Sums, which has no lists) | 9 of 10 |
| `ForgeNotice` | `notices` | 5 | not built |
| `forgeRowActions` | `row-actions` | 6 | not built |
| `forgeRename` | `rename` | 6 | not built |
| `forgeShareFile` | `share-file` | 4 | not built |
| `forgeSwipeGuard` | `swipe-guard` | 10 uses | not built |
| `forgeTheme` | `forge-theme` | 10 | 7 of 10 (5 with their own accent) |
| `ForgeAlert` (AppKit) | `alerts-over-sheets` | 7 | not counted |
| Text roles (`forgeText`, `Font.forge`) | none (by decision) | 2 (Checklist 1, Device Lab 13); 462 raw `.font(` | 0 |
| `ForgeRow` | none | 3 (Commonplace 4, Good Dog 1, Urdu 14) | 1 |
| `ForgeBadge` / `ForgeStatus` | none | 5 | 2 |
| ForgeUI button styles | none (D2 made native styles first-class) | 3 (5 uses); `.bordered*` 32 | 1 |
| `ForgeButton(isWorking:)` | none | **0** | 0 |
| `forgeFormToolbar` | none | **0**; at least 5 forms and the starter hand-build it | 0 |
| `ForgeCard`, `ForgeSectionHeader`, `ForgeField`, `ForgeDetailHeader` | none | **0, 0, 0, 1** | 0, 0, 0, 0 |

The rule-backed half of the table moved from almost nothing to almost everything in four days. The other half hasn't moved. Two conclusions:

- **Card, section header and field have no users because the apps don't build card screens or bare fields.** Every app lays out with `List` and `Form`. That is evidence *against* more layout components, not for them (section 5).
- **`forgeFormToolbar` has no users because the move was held** (the starter waits for a ForgeKit release, and the app sheets moved onto `forgeSwipeGuard` only). The hand-built toolbars in Pocket Library (`BookFormView.swift:147`), Good Dog (`DogFormView.swift:162`), FieldKit (`StartTripView.swift:32`, `AddNoteView.swift:33`), Commonplace (`AddFeedView.swift:59`) and the starter (`ComposeNoteView.swift:35`) are correct, labelled spinners included. They are six copies of one toolbar, each deciding its own Cancel-while-saving and swipe rules, which is what a parity rule is for. A consistency item, not a bug.

## 3. Demand: what the apps build by hand

### 3.1 Built independently, two or more apps

| Candidate | Hand-built copies | How they differ | A2UI use |
|---|---|---|---|
| **Rating** | Soundscape `Stars` (`ListenView.swift:400`), Pocket Library `StarRating` (`BookDetailView.swift:308`) | Orange vs yellow raw colours; Soundscape exposes five separate buttons, Pocket Library one adjustable element ("3 of 5 stars", swipe up/down) and tap-again-to-clear | Medium: rate a book or a take in a review surface |
| **Media thumbnail** (image in a fixed frame with placeholder, loading and failed states) | Soundscape `Artwork` (`ListenView.swift:653`), Pocket Library `CoverImage` (`CoverImage.swift:6`), Relay `PhotoThumbnail` (`Attachments.swift:257`), Device Lab `ArtifactThumbnail` (`ArtifactsView.swift:49`), Sketchbook `SketchTile` (`ContentView.swift:145`) | Four corner rules (size × 0.12 in Soundscape, width ÷ 12 capped at 6 in Pocket Library, 12 pt in Relay and Sketchbook, 8 pt at the call site in Device Lab), none from the theme; three placeholder styles; only Pocket Library and Relay show loading; only Relay and Device Lab show failure differently from "no image"; a fixed 220 pt frame in Relay | High: a book, an album or a place in an agent's plan, but only from images the app supplies (section 6) |
| **Selectable choice** (chip or tile with a selected state) | Relay `ReactionsRow` (`ChatView.swift:1063`), Soundscape A/B compare tiles (`ListenView.swift:505`) and A/B switch (`:875`), Device Lab phone-check results (`PhoneChecksView.swift:70`) | Relay's reactions have no `.isSelected` trait and no identifiers (rule 4); the others add the trait by hand; three different selected looks, two in raw `Color.accentColor` | High: A2UI's `ChoicePicker` has no ForgeUI target today; #377 still draws it in A2UI's basic look |
| **Stat** (a figure with its label, optional unit) | FieldKit `Stat` (`RecordingView.swift:96`, used on two screens), Weather `MeasureRow` (`ForecastPage.swift:722`) and `DetailTile` (`:656`), Pocket Library `StatRow` (`ActivityView.swift:77`) | Each picks its own fonts (`title3.monospacedDigit().bold()`, `title2.weight(.medium)`, `body.monospacedDigit()`), though ForgeUI's `figure` and `figureCaption` roles exist for exactly this; only Weather gives VoiceOver a spoken value ("29 kilometres per hour") | High: a summary card ("4 books, 1,200 pages, done by March") is the most common thing an agent shows |
| **Progress summary** (title, bar, "n of m") | Good Dog `ProgressRow` (`TodayView.swift:133`) and two more bars, Urdu `SkillBars` (`LearnScreens.swift:243`), Pocket Library (book detail, library rows, reader, summaries), Relay receive, Device Lab three | All use SwiftUI's `ProgressView` (good), but lay out the label and caption by hand, and VoiceOver varies: labelled, hidden, or the bare percentage | Medium |
| **Category icon tile** (D1, decided 2026-11-18, not built) | Soundscape `CollectionView.swift:63` (gradient tiles), Pocket Library `StatRow` tints | Raw `.green`, `.blue`, `.orange`, `.yellow` | Low |

### 3.2 Awkward or impossible with ForgeUI today

| Gap | Evidence |
|---|---|
| **ForgeUI's button look can't be put on anything but a `Button`.** `ForgeButtonChrome` is internal | `ForgeStateView` already needs it for menus (`ForgeStateView.swift:222`). The A2UI adapter in #377 can't reach it, so `ForgeA2uiButtonStyle` re-implements it and drifts: 0.5/0.8 opacity instead of 0.4/0.7, a fixed 44 pt height that doesn't grow with Dynamic Type, no compact size |
| **ForgeButton wasn't disabled while working** | Section 4 |
| **A `Stat` on a non-standard surface** | Weather draws on its sky; like `ForgeNotice`, a stat needs a way to drop its own background and take its colours from around it (`.forgeNoticeBackground(.hidden)` is the precedent) |

### 3.3 Raw SwiftUI that should stay raw

| Control | Uses (apps) | Verdict |
|---|---|---|
| `Toggle` | 17 (7) | Native. Label with `ForgeRow` where a row has an icon. No `ForgeToggle` |
| `Picker` (default, `.segmented`, `.menu`, `.inline`, `.navigationLink`) | 32 (9) | Native. One finding: Good Dog's `OutcomeChoice` (`ExerciseView.swift:241`) hand-builds a checkmark list that `Picker(...).pickerStyle(.inline)` draws for free, with the selected trait. Fix in the app when touched |
| `Slider`, `Stepper`, `DatePicker` | 2, 1, 1 | Native. Checklist due dates (the [app plan](../../app-plan.md)) will use `DatePicker` |
| `LabeledContent` | 139 (7) | Native, and the right answer to "structured data display". Device Lab's dashboard is built from it |
| `TabView`, `NavigationStack`, `.navigationDestination` | 6, all, 24 | Native; `ForgeDestination` covers model-driven navigation |
| `.searchable`, `.refreshable`, `.contextMenu` | 9, 6, 6 apps | Native; `ForgeStateView.noResults` and `forgeRowActions` already cover the shared part |
| `ProgressView`, Swift Charts (`Chart`, FieldKit) | 6 and 1 apps | Native controls; 0.2 adds a *style* for the bar (3.1), not a new control. Charts wait for Good Dog's GD4 as the second user (the [app plan](../../app-plan.md) names the extraction) |

### 3.4 Reuben's list, checked against the apps

| Area (Reuben) | What the apps show | 0.2? |
|---|---|---|
| Form controls: toggles, checkboxes, radio groups, selectors, sliders, date/time | Native controls, used correctly; checkbox and radio are iOS `Toggle` and `Picker` | No components. A "Controls" showcase page with the right native recipe for each |
| Layout: sections, adaptive grids, split layouts, spacing | Lists and forms everywhere; two grids (Weather fixed two columns, Sketchbook adaptive); no split layout (iPhone only); `ForgeCard` and `ForgeSectionHeader` unused | No. Check Weather's fixed two columns at accessibility text sizes when Weather is next touched |
| Navigation | Native, consistent | No |
| Data display: progress, statistics, timelines, structured data, visualisation | Stat ×4, progress ×5 apps; `LabeledContent` covers structured data; one timeline-like view (Good Dog's goal path) | **Stat, progress style.** Charts at 0.3 |
| Media: images, thumbnails, audio controls | Thumbnails ×5 apps. Audio controls ×4 apps (Soundscape scrubber and transport, Relay voice messages, Urdu play buttons, Read Aloud), but each is different in kind (waveform, scrubber, phrase replay) | **Thumbnail.** Audio controls stay app-owned until two match |
| Feedback: toasts, transient banners, inline progress | `ForgeBanner` (Undo, running work) and `ForgeNotice` cover what apps do; no app builds a toast | No. Inline progress is the ForgeButton fix and `forgeFormToolbar` adoption |
| Selection: multi-select, filter chips, segmented, groups | Segmented is native (Soundscape ×2); choice tiles/chips ×3 apps; no filters or multi-select anywhere | **Choice style** only |
| Accessibility: component-level VoiceOver and Dynamic Type testing | Most copies in 3.1 differ in accessibility | Each 0.2 component ships with a behaviour spec and a showcase state matrix (section 5.4); the full test matrix is 0.4 |
| Theming: semantic tokens, motion, elevation | Zero uses of `.contentTransition`, `.symbolEffect` or `.matchedGeometryEffect`; one shadow (raised cards) | No tokens. Restraint is the visual language; tokens without users would be decoration |

## 4. The ForgeButton gap (verified, fixed here)

**Real.** `ForgeButton` guarded its action with `if !isWorking { action() }` and showed a spinner, but never disabled the SwiftUI `Button`. So while working:

- VoiceOver read it as an ordinary, enabled button (with the value "In progress"), and double-tapping it did nothing, silently;
- Voice Control ("Tap Save") and Full Keyboard Access offered it as pressable;
- ForgeUI's styles couldn't tell it was busy.

**Fix (this PR, its own commit):** `ForgeButton` is disabled while `isWorking`, so VoiceOver reads "dimmed, In progress" and nothing can press it twice. ForgeUI's button styles keep it at full colour while it works, because a spinner faded to 40% is hard to see; a button the app disables itself still fades. The rules are a plain `ForgeButtonState` with Linux tests, the same pattern as `ForgeFormToolbarState`. The Swift is not compiled yet: it goes into the next combined Device Lab build, and the showcase's Buttons page shows it (VoiceOver check on the phone: "Save Changes, dimmed, In progress" while working).

Nothing in the apps changes, because no app uses `ForgeButton`. The hand-built form toolbars of section 2 don't have this flaw: they swap the button for a labelled spinner, which can't be pressed.

## 5. ForgeUI 0.2 specification

### 5.1 Principles (unchanged from 0.1, restated because 0.2 tests them)

- Style SwiftUI's real controls; build a component only where SwiftUI has none.
- A component exists because two apps built it, or one app and the A2UI adapter need the same thing and the app copy is already there.
- No colour, font or size parameters: tones, accents, roles and the theme's spacing.
- Every component that makes a control asks for its identifier, and has one VoiceOver behaviour written down before it is built.
- Each component lands in one PR that moves every app using a copy, with a parity rule (rule 12).

### 5.2 Scope

Ordered by evidence and size. Each item is one PR.

| # | Item | Kind | Moves | Parity rule | A2UI entry |
|---|---|---|---|---|---|
| 0 | **ForgeButton disabled while working** | Fix | none | none | Button |
| 1 | **`forgeFormToolbar` adoption** | Adoption | Pocket Library book form, Good Dog dog form, FieldKit start trip and add note, Commonplace add feed, and the starter (with the next ForgeKit release) | `form-toolbar`: a `.confirmationAction` toolbar item holding a `ProgressView` | none |
| 2 | **Accent icon tiles** (D1): `ForgeRow(..., systemImage:, accent: ForgeAccent)` | Parameter | Soundscape Your Music rows, Pocket Library Summary rows (which become `ForgeRow(value:)`) | `icon-tile`: a symbol on a `RoundedRectangle` background in a raw colour | ListItem |
| 3 | **Public button look**: `.forgeButtonLook(_ kind:, expands:, isPressed:)` on any view. It is the one implementation: `ForgeButtonStyle` and `ForgeStateView`'s menus are rebuilt on it, so there is no second copy. It covers every state the style has: pressed, disabled (from `\.isEnabled`), working (full colour, from `ForgeButton`), compact (`.controlSize(.small)`), full width, Reduce Motion, and the Dynamic Type minimum height. A Linux-testable state value says which of them apply, like `ForgeButtonState` | Expose | `ForgeStateView` menus (internal today), the A2UI adapter's `ForgeA2uiButtonStyle` (Device Lab) | none | Button |
| 4 | **`ForgeRating`**: display and input; one adjustable element for VoiceOver ("3 of 5 stars"), tap the current star to clear when `allowsClearing`, star colour from a fixed rating tone, identifier `<id>` with `<id>.star.<n>` | Component | Soundscape `Stars`, Pocket Library `StarRating` | `rating`: a `ForEach` over `star`/`star.fill` symbols | Rating |
| 5 | **`ForgeThumbnail`**: a frame for any image content, with `shape` (`.rounded`, `.circle`, `.square`), `aspectRatio`, and placeholder, loading and failed states drawn the same everywhere; hidden from VoiceOver unless given a label. It takes a view or a phase, so it works with `ForgeRemoteImage`, a local file, or a `CGImage`, and depends on none of them | Component | Soundscape `Artwork`, Pocket Library `CoverImage`, Relay `PhotoThumbnail`, Device Lab `ArtifactThumbnail`, Sketchbook `SketchTile` frame | `thumbnail`: a `clipShape(RoundedRectangle(cornerRadius: <number>))` around an `Image` | Image (app-supplied only) |
| 6 | **Choice style**: `.toggleStyle(.forgeChoice)` for chips that turn on and off, and `.buttonStyle(.forgeChoice(isSelected:))` for one-of-several tiles; both add the `.isSelected` trait, use the accent's `soft`/`solid`, and are at least 44 pt | Style | Relay reactions (gaining identifiers), Soundscape A/B tiles and switch, Device Lab phone-check results | `choice`: `.accessibilityAddTraits(... .isSelected ...)` outside ForgeUI | ChoicePicker |
| 7 | **`ForgeStat`**: value, label, optional unit and `spokenValue`, optional tone; drawn with the `figure` and `figureCaption` roles; one VoiceOver element ("Distance, 4.2 kilometres"); `.forgeStatBackground(.hidden)` for apps with their own surfaces | Component | FieldKit `Stat` (two screens), Weather `MeasureRow`; Good Dog GD4 uses it from the start. Pocket Library's `StatRow` is a row, not a stat: it moves to `ForgeRow(value:)` in item 2. Weather's `DetailTile` stays (a tile with its own graphic) | `stat`: a `VStack` of a `monospacedDigit` figure over a secondary caption | Stat |
| 8 | **Progress style**: `.progressViewStyle(.forge)` (and `.forge(tone:)`) on SwiftUI's own `ProgressView`, laying out its label and `currentValueLabel` with text roles and giving VoiceOver "n of m" | Style | Good Dog progress row and goal focus, Urdu skill bars, Pocket Library book progress, reader and summaries, Relay receive, Device Lab transfers, downloads and storage | none, by exception: a `ProgressView` without the style is still a correct native control, so there is nothing to forbid | Progress |

**Not in 0.2:** charts (0.3, after Good Dog GD4 as the second user), audio transport (0.3 at the earliest, when two apps' controls match), grids and adaptive layouts (0.3, with a second card-led screen), toasts, filters, multi-select, split views, motion and elevation tokens (no users), custom fonts (no request), and snapshot tests (0.4, see 5.4).

### 5.3 Behaviour specs

Each component gets a short spec in `docs/forgeui.md` when it lands, in this shape. Rating, as the example:

| Aspect | ForgeRating |
|---|---|
| States | unrated, 1 to 5, disabled, read-only |
| Input | tap a star sets it; tap the current star clears it (only with `allowsClearing`) |
| VoiceOver | one element: label (the caller's, default "Rating"), value "3 of 5 stars" or "Not rated", adjustable up and down; read-only drops the adjustable action |
| Dynamic Type | stars scale with the surrounding text style; at accessibility sizes the hit area stays at least 44 pt per star |
| Identifiers | `<id>` on the element, `<id>.star.<n>` on each star for UI tests |
| Colour | the rating tone (a fixed warm tone checked by `contrastIssues()`), never the accent |
| Model | the value is an `Int?` the view model owns; no rating rules in Swift |

### 5.4 The showcase as a component laboratory

The showcase today shows each component once, in the system's appearance. 0.2 turns each page into a matrix, without a new tool:

- **A state bar on every page**: light/dark, Increase Contrast, Dynamic Type (default, xxxLarge, AX3), Reduce Motion, accent, and the component's own states (enabled, disabled, working, empty, failed). Driven by `.environment` overrides, so it is plain SwiftUI and needs no `#Preview`.
- **Each state has an identifier**, so `forge ui run` can step through the matrix on the phone and the existing [visual regression](../../visual-regression.md) baselines record each one. That is the screenshot testing 0.1 left out, done with the tool ixOS already has (rule 11) instead of a macOS-only snapshot library. Building the full baseline set is 0.4; 0.2 adds the state bar and identifiers so the components arrive testable.
- **A Controls page** showing the native recipe for every control ForgeUI deliberately doesn't wrap (Toggle with `ForgeRow`, Picker styles, Slider, Stepper, DatePicker, `LabeledContent`), so "use native" has an example to copy.

### 5.5 Order and gates

1. Items 0 to 3 first: a fix, an adoption, a parameter, an exposure. Small, and 3 unblocks A2UI-1b.
2. Items 4 to 8, one PR each, in table order. Each moves its apps and adds its rule, except item 8 (a style on a native control, see its row).
3. 0.2 ships as a ForgeKit release when items 0 to 8 have merged and the showcase has been looked at on the phone once (light and dark, VoiceOver on rating, choice and stat). Large text sizes are checked from the matrix's screenshots, not by hand on the phone. The starter's `forgeFormToolbar` move goes in with that release.

Phone time: about 15 minutes for the whole of 0.2, in the combined Device Lab sitting: VoiceOver on ForgeButton working, rating, a choice chip and a stat; one form sheet's spinner; the showcase in dark mode.

## 6. Forge A2UI catalogue contract

Designed with 0.2, owned outside ForgeUI. The layering is Reuben's (2026-11-22, in the A2UI assessment, `docs/research/external-projects/assessments/ai/a2ui.md` on [#377](https://github.com/GeneralKnowledge/ixOS/pull/377), not merged yet): **A2UI decides which interface to compose; ForgeUI decides how it looks and behaves.** ForgeUI never imports A2UI. The adapter lives in Device Lab until a second app uses it.

### 6.1 What the contract is

One closed list of components an agent may use in any ixOS surface. Each entry fixes:

1. **The A2UI component name** (A2UI v1.0, the basic catalogue's definition where one exists).
2. **Its Forge target**: the ForgeUI component or style, or the native SwiftUI control on ForgeUI tokens.
3. **Its properties**, each typed and closed: text, a ForgeUI tone, accent, text role or spacing token, a number with bounds, a data-model path. Never a colour, font, size, URL or free-form style.
4. **The accessibility it must carry**: which property becomes the VoiceOver label, and that the renderer refuses the component without it. This applies to components a person reads or operates. Containers (Column, Row, Card) and Divider carry no label: they are structure, and Divider is hidden from VoiceOver. Rating has a default label ("Rating") from `ForgeRating`. Image without a label is decorative and hidden from VoiceOver.
5. **Its identifier**: `a2ui.<surface>.<componentId>`, which satisfies rule 4's `screen.element` shape.
6. **The events it may send**: none, or named events registered by the app (A2UI-1's gate already refuses everything else).

The list is used in three places: the Rust gate's allow-list, the Swift adapter's catalogue, and later a Compose adapter. It starts as the table below. When the adapter moves out of Device Lab (its second user), it becomes a **tested projection of ForgeUI** rather than a second description kept by hand: one file lists each entry's ForgeUI target, and checks fail when the gate's allow-list or the adapter's catalogue disagree with it, when an entry names a ForgeUI piece or property that no longer exists, or when the entry's accessibility requirement is weaker than the ForgeUI component's own behaviour spec (5.3). So a ForgeUI change that breaks the projection fails a check instead of drifting silently. Until then, the adapter's PRs check the table by hand.

### 6.2 The catalogue

| A2UI component | Forge target | Typed properties | Accessibility | Events | Status |
|---|---|---|---|---|---|
| Text | `Text` with a ForgeUI text role | `text`, `role` ∈ {body, metadata, note, rowTitle, sectionTitle, figure} | is its own label | none | Drawn by ForgeUI in #377 (body, caption→metadata); add roles |
| Column, Row | `VStack`, `HStack` | `spacing` ∈ {xs, s, m, l, xl}, `alignment` | container | none | Basic look in #377 → A2UI-1b |
| Divider | `Divider` in `separator` colour | none | hidden | none | → A2UI-1b |
| Card | `ForgeCard` | children | container | none | Done in #377 |
| Button | `Button` with `.forgeButtonLook` (0.2 item 3) | `label`, `kind` ∈ {primary, secondary, tertiary} | label required | one registered event | Re-implemented in #377 → item 3 |
| ListItem | `ForgeRow` | `title`, `subtitle`, `value`, `systemImage` (SF Symbol from an allow-list), `accent` | title | optional registered event | New |
| Badge, Status | `ForgeBadge`, `ForgeStatus` | `text`, `tone` | text | none | New |
| Notice | `ForgeNotice` | `message`, `systemImage`, `tone` ∈ {info, warning} | message | optional registered event | New |
| EmptyState | `ForgeStateView.empty` | `title`, `systemImage`, `message` | title + message | none | New |
| ChoicePicker | Native `Picker` (≤ 4 options: segmented; more: menu) or chips with `.forgeChoice` (0.2 item 6) | `label`, `options` (labels + values), `selection` path, `variant` ∈ {segmented, menu, chips} | label required; selected trait | data-model edit | Basic look in #377 → item 6 |
| DateTimeInput | Native `DatePicker` | `label`, `value` path, `mode` ∈ {date, time, dateTime}, `min`, `max` | label required | data-model edit | Basic look in #377 → A2UI-1b |
| TextField | `TextField` in `ForgeField`, message from Rust's validation | `label`, `value` path, `message` path | label; error read after the field | data-model edit | New |
| Toggle | Native `Toggle`, `ForgeRow` label | `label`, `value` path | label | data-model edit | New |
| Stat | `ForgeStat` (item 7) | `value`, `label`, `unit`, `spokenValue`, `tone` | combined; spoken value if given | none | New |
| Progress | `ProgressView` with the forge style (item 8) | `value`, `total`, `label` | "n of m" | none | New |
| Rating | `ForgeRating` (item 4), read-only by default | `value` path, `editable` | adjustable "n of 5" | data-model edit | New |
| Image | `ForgeThumbnail` (item 5) | `imageId` naming an image **the app supplied** to the surface, `shape`, `aspectRatio`, `label` | hidden unless labelled | none | New |

**Left out on purpose:** Video and AudioPlayer (media side effects), Modal and Tabs (navigation belongs to the app), `openUrl` and every other function (A2UI-1's gate refuses all calls), remote image URLs (an agent must not make the app fetch arbitrary addresses; only app-supplied image ids), colours, fonts and sizes (A2UI v1.0 removed themes, and the contract keeps it that way), and destructive buttons (anything destructive goes through the app's own Confirm, as A2UI-1's change set does).

### 6.3 What this means for ForgeUI

Nothing A2UI-shaped enters ForgeUI. The contract only asks ForgeUI for things the apps already asked for: a public button look (item 3), chips (6), stat (7), progress style (8), rating (4) and thumbnail (5). If an A2UI component ever needs something no app needs, it is drawn in the adapter from native SwiftUI on ForgeUI's tokens, and doesn't become a ForgeUI component until an app wants it too.

### 6.4 Android and Compose

The contract is a component *specification*, not shared code: SwiftUI components don't run on Android. When Compose A2UI reaches 1.0 and Android's A8 has passed (Reuben's gates for Android A2UI), a Compose adapter implements the same table with Material components on ForgeUI's tokens (the tokens already reach Kotlin: `scripts/android-tokens.py` generates Pocket Library Android's palette, accents, text roles, spacing and radii from ForgeUI's Swift tokens, checked by `make validate`). Until then the table carries no Compose column, and Android stays a capped side track.

## 7. Decisions for Reuben

1. **Scope.** Items 0 to 8 as 0.2, with thumbnails pulled forward from 0.3 because five apps built one. Or keep thumbnails in 0.3 and ship 0.2 with seven items. Recommended: pull it forward.
2. **Choice style API.** A toggle style for chips plus a button style for one-of-several tiles (two small APIs on real controls), rather than a `ForgeChoices` component that would duplicate `Picker`. Recommended as written.
3. **Rating colour.** A fixed warm rating tone (checked for contrast) instead of the accent, so stars look like stars in every app. Recommended.
4. **Catalogue file.** Keep the contract as this table until the adapter leaves Device Lab, then make it a tested projection of ForgeUI (6.1). Agreed by Reuben 2026-11-22.
