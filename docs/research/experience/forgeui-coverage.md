# ForgeUI coverage audit

Status: 2026-11-18, on main at `61c9e13`. Part of [ixOS Experience Research](README.md). Research only: no app, ForgeUI or ForgeAppKit code changes here. Evidence is the source (S) unless a row says otherwise; nothing here was checked on the phone.

The question this audit answers:

> If we built every ixOS app again today using the existing ForgeUI and ForgeAppKit, how much of each app could be built using the shared foundation, and where would we still have to reinvent things?

It separates two failure modes, because they need opposite fixes:

- **Apps don't use what ForgeUI has** (adoption). The fix is moving app code onto ForgeUI, plus a check so it doesn't drift back.
- **ForgeUI doesn't have what apps need** (coverage). The fix is a small addition to ForgeUI or ForgeAppKit, but only where two apps share a real problem.

## The answer

**Foundations are mostly in place; the apps mostly don't use them, and the checks can't see that.** Rebuilding the apps today, ForgeUI and ForgeAppKit could carry the colour, tones, text hierarchy, state screens, alerts and save flows of every list, form and settings screen. What would still be built by hand is mostly what *should* be built by hand: Weather's sky, Relay's chat bubbles, Soundscape's player, Sketchbook's canvas, cover-led headers. Four things would still be reinvented because the framework is missing a small piece:

1. **A form sheet that guards unsaved changes.** `forgeFormToolbar` exists but has no users, not even the starter, because it only stops a swipe-away *while saving*. Six apps and the starter hand-roll the same toolbar instead, most of them to also stop a swipe-away while the form has changes (finding C1).
2. **A rename prompt.** Seven hand-rolled rename alerts in five apps, each with its own identifiers and none refusing an empty name except by accident (C2).
3. **An inline notice** for offline, stale or reconnecting. Three apps, three looks (C3).
4. **Icon tiles in a category colour.** `ForgeRow`'s icon tile only takes meaning tones, so Soundscape's "Your Music" rows and Pocket Library's Activity rows rebuild it (C4).

Item 1 is a parameter. Items 2 and 3 have their second user already. Item 4 is a decision about the visual language before it is code.

The bigger gap is adoption, not coverage:

| What ForgeUI has | Apps using it | What apps do instead |
|---|---|---|
| Text roles (`forgeText`, `Font.forge`), added in #292 | **0 of 10** | About 360 hand-picked `.font(.headline)`, `.font(.subheadline)` + `.secondary`, `.title3.monospacedDigit().bold()` |
| `ForgeRow` | 1 (Commonplace) | Own row views; several are the same icon-tile row |
| `ForgeBadge` / `ForgeStatus` | 2 (Good Dog, Pocket Library) | Relay's `TrustBadge` is `ForgeStatus` in raw green |
| `forgeFormToolbar` | 0 | Hand-rolled Cancel / spinner / Save in 8 sheets (C1) |
| ForgeUI button styles | 1 use (Pocket Library) | SwiftUI's `.borderedProminent` (19) and `.bordered` (16): see decision D2 |
| `ForgeStateView` | 9 of 10 | Checklist uses `ContentUnavailableView`; three root loading screens are a bare `ProgressView()` |
| A theme with the app's own accent | 5 of 10 | Relay and Soundscape keep `.standard`; Checklist, Weather and Device Lab set none |

So the work this audit points to is mostly migration and enforcement, with three small framework additions. It does not point to a bigger component library.

## What #285 proved, and what has changed since

The [experience audit](audit.md) (#285) was taken at `15bc21d`, an hour before #292 merged. Its findings were true then; two have since moved, and one was never measured:

| #285 finding | Now | Why |
|---|---|---|
| X2: every themed app wears `.forgeTheme(.standard)` | **Partly fixed.** Commonplace indigo, FieldKit teal, Good Dog green, Pocket Library orange, Sketchbook pink. Relay and Soundscape still standard; Checklist, Weather, Device Lab no theme | #292 (`ForgeAccent`) |
| X3: no type roles beyond system styles | **Fixed in ForgeUI, not in any app.** `ForgeTextRole` has eight roles, documented in [forgeui.md](../../forgeui.md#text-roles) and shown in the showcase; no app calls them | #292 added the roles; nothing moved the apps |
| X4: raw colours for meaning | **Mostly fixed for text.** The `toned-text` parity rule now blocks raw orange/yellow text and red error text. Raw colours remain for status icons and labels the rule doesn't match (findings below) | `toned-text` rule |
| X15: bare root `ProgressView()` | **Still true** in Good Dog (`ContentView.swift:21`), Pocket Library (`ContentView.swift:25`) and Relay's chat (`RelayRoot.swift:97`) | No rule covers it |
| (not measured) whether apps use ForgeUI's *components* | Measured here: mostly not | #285 counted `forgeTheme(` and `ForgeStateView` only |

So the discrepancy Reuben spotted is real but explained: the docs are current, the audit is a snapshot from before #292, and #292 added roles without moving any app onto them. #285 documented the ownership model ("look in ForgeUI, behaviour in ForgeAppKit") and counted two adoption signals; it did not prove adoption of components, and the parity checks still don't.

## 1. Inventory: what the shared layer actually provides

From the public API in `forgekit/Sources`, not from the docs.

### ForgeUI (look; no Rust, no AppKit)

| Area | Public API | Showcase page | Tests |
|---|---|---|---|
| Theme | `ForgeTheme`, `.forgeTheme(_:)`, `ForgeTheme(accent:)`, `\.forgeTheme`, `.forgeGroupedBackground()`, `.forgeLaunchAppearance()` | Colour | Contrast of every accent, light/dark, Increase Contrast |
| Colour | `ForgePalette`, `ForgeColors` (8 roles), `ForgeTone` (accent, neutral, success, warning, danger, info) × solid/onSolid/soft/text, `.forgeForeground`, `.forgeTint`, `ForgeAccent` (8 checked accents) | Colour | Yes (Linux) |
| Spacing, corners | `ForgeSpacing` (xxs–xxl), `ForgeRadius` (s, m, l) | Spacing and Corners | Scales ascend |
| Type | `ForgeTextRole` (screenTitle, sectionTitle, rowTitle, body, metadata, note, figure, figureCaption), `.forgeText`, `Font.forge` | Type | Roles sit on system styles |
| Buttons | `ForgeButtonStyle` (primary, secondary, tertiary, destructive; `expands`), `ForgeIconButtonStyle`, `ForgeButton(isWorking:)` | Buttons, Icon Buttons | None |
| Fields | `ForgeTextFieldStyle`, `ForgeField`, `ForgeFieldMessage`, `ForgeFieldMessageView` | Text Fields, Form Fields | Message → tone |
| Rows, cards | `ForgeRow` (icon tile + title + subtitle/value + trailing), `ForgeCard`, `.forgeCard` group box style | Cards, Rows | None |
| Badges, headers | `ForgeBadge`, `ForgeStatus`, `ForgeSectionHeader`, `ForgeDetailHeader` | Badges, Headers | None |
| States | `ForgeStateView.empty / .noResults / .loading / .failed`, `ForgeStateAction` (incl. menu) | States | None |
| Sheets | `forgeFormToolbar(isWorking:)` (oddly, in `ForgeDetailHeader.swift`) | Dialogs | None |

### ForgeAppKit (behaviour; model values testable on Linux)

`ForgeFailure` / `ForgeDescribedError`, `ForgeAlert` + `.forgeAlert`, destinations (`.forgeSheet`, `.forgeFullScreenCover`, `.forgeNavigationDestination`), `ForgeLoad` + `ForgeLoadView`, `ForgeSubmission` + `forgeFormToolbar(submission:)`, `ForgeFieldErrors`, `ForgePickedFile`, `ForgeBanner` (undo, running work), `ForgeFeedback` (haptics), `forgeSaveOnLeave`. Twenty Linux tests. Not in the ForgeUI showcase; Device Lab's Interaction Patterns page shows `ForgeBanner` and `ForgeFeedback` only.

### Shared Swift outside ForgeKit

Copied into apps from crates, so easy to miss in an audit of ForgeKit: `ForgeRemoteImage` (web images, `crates/forge-services/swift/helpers/ForgeImages.swift`), `ForgeWebScreen` / `ForgeWebSheet`, `ForgeShareLink`, and the reader views in `crates/forge-reader/swift/ForgeReader.swift` (Pocket Library, Commonplace).

### Inventory against Reuben's list

| Asked for | Exists | Gap |
|---|---|---|
| Theme and tokens | Yes | None |
| Button styles | Yes | Barely used; overlap with SwiftUI's own bordered styles (D2) |
| Text and typography roles | Yes (since #292) | Unused (A1) |
| Cards and badges | Yes | Lightly used; fine where content leads |
| Form fields and validation | Yes (`ForgeField`, `ForgeFieldMessageView`, `ForgeFieldErrors`) | Only Pocket Library validates fields; others validate on save, which is fine for their forms |
| Rows and section headers | Yes | Icon tile limited to meaning tones (C4) |
| Empty, loading, error, no-results | Yes | Root loading not enforced (A4) |
| Semantic colours and notices | Tones yes; **notice no** | C3 |
| Accessibility support | Contrast checked in tests; Dynamic Type through system styles; identifiers asked for | No component-level Dynamic Type or VoiceOver test (E5) |

## 2. Component-to-app matrix

Key: **✓** uses it · **·** doesn't need it here · **N** native SwiftUI, appropriate · **D** duplicates something ForgeUI/ForgeAppKit has · **G** needs something the framework lacks · **✗** should use it and doesn't. Relay includes the merged Chat (`Messaging/`). Starter is `templates/ios-app`.

| | Checklist | Commonplace | Device Lab | FieldKit | Good Dog | Pocket Library | Relay | Sketchbook | Soundscape | Weather | Starter |
|---|---|---|---|---|---|---|---|---|---|---|---|
| ForgeUI dependency | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ (1 import) | ✓ |
| Theme with own accent | ✗ none | ✓ indigo | · tool | ✓ teal | ✓ green | ✓ orange | ✗ standard | ✓ pink | ✗ standard | N own sky palette | standard (D3) |
| Tones for meaning | · | D 2 raw | D 15 raw | D 1 raw | ✓ 21 | D 7 raw | D 13 raw | ✓ | D 14 raw | N sky | ✓ |
| Text roles | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | N sky type | ✗ |
| Buttons | N | N | N | N | N | N + 1 ✓ | N | N | N | N | N |
| `ForgeRow` | N | ✓ | N | N | N | D StatRow, G | N | N | D, G `row()` | N | N |
| `ForgeBadge` / `ForgeStatus` | · | · | N | · | ✓ | ✓ | D `TrustBadge` | · | N | · | · |
| `ForgeCard` | · | · | · | · | N | N | · | · | ✓ | N own `Card` | · |
| `ForgeDetailHeader` | · | · | · | · | · | N cover-led | · | · | · | · | · |
| `ForgeStateView` | D `ContentUnavailableView` ×2 | ✓ | ✓ | ✓ | ✓, D root | ✓, D root | ✓, D root | ✓ | ✓ | ✓ | ✓ |
| Form sheet toolbar | N | D AddFeed | · | D AddNote, StartTrip | D DogForm | D BookForm | D new chat | · | D new project | · | D ComposeNote |
| Field validation | · | · | · | · | · | ✓ | · | · | · | · | · |
| `forgeAlert` | N plain | ✓ | · | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | · | ✓ |
| Destinations | N | N | N | N | N | ✓ | N | N | N | N | N |
| `ForgeLoad` / `ForgeSubmission` | · | ✓ | · | ✓ | N | ✓ | N | · | N | N | ✓ |
| `ForgeBanner` (undo) | · | ✓ | ✓ demo | · | · | · | · | · | · | · | · |
| `ForgeFeedback` | · | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | · | · |
| Rename prompt | · | G | · | G | · | · | G | G | G ×3 | · | · |
| Inline notice | · | G status line | · | · | · | · | G `ConnectionBanner` | · | · | G `Notice` | · |

Reading the matrix by column gives the rebuild-today answer per app:

- **Good Dog, Pocket Library, Commonplace, FieldKit, Sketchbook**: almost everything list-, form- and state-shaped is covered; the hand-made part is text hierarchy (no roles), a few raw colours and the form toolbar.
- **Relay and Soundscape**: the largest apps (6,200 and 5,700 lines of Swift) and the furthest behind on foundations: standard accent, 13 and 14 raw colours, no roles. Their custom surfaces (chat, player, mixer) are rightly their own.
- **Checklist**: built on ForgeKit without ForgeUI at all, so every ForgeUI parity rule skips it (E1).
- **Weather**: deliberately its own visual system (a drawn sky with its own palette and type); ForgeUI tokens don't fit a white-on-sky page. Native is right here; its Locations list and states are the only ForgeUI-shaped parts, and they use `ForgeStateView`.
- **Device Lab**: a developer tool; its own screens don't need an identity. Its raw colours are sensor-chart series and device-state colours (findings A3).

## 3. Findings

Each finding: what, where, evidence, priority (P1 next, P2 soon, P3 when convenient), and where the fix belongs.

### Adoption: ForgeUI has it, apps don't use it

**A1. No app uses text roles (P1, apps + check).** `forgeText` and `Font.forge` appear only in the showcase. Every app picks system styles by hand: `.font(.headline)` 48 times, `.font(.subheadline)` 67, `.font(.caption)` 72, and `title3.monospacedDigit().bold()`-style figures in FieldKit's `Stat`, Pocket Library's `StatRow` and Weather's `Tile`. The roles were made for exactly these: `rowTitle` + `metadata` for FieldKit's `TripRow` and the starter's `NoteRow`, `figure` + `figureCaption` for FieldKit's `Stat`. This is the one finding where "the same kind of text looks different in every app" is directly fixable by migration. Move the starter first (so new apps start right), then apps a screen at a time when they are next touched; don't do a mass rewrite.

**A2. Accents: Relay and Soundscape still standard (P2, apps).** Both were left for their redesigns (forgeui.md: "Relay and the rest keep the standard blue for now"). Checklist has no theme because it has no ForgeUI. One line each once the accent is chosen (D1).

**A3. Raw colours outside the `toned-text` rule (P2, apps + check).** The rule catches raw orange/yellow `.foregroundStyle` and red error text. It misses status in raw green and red on icons and labels: Relay `TrustBadge` (`TrustedDeviceViews.swift:20`), received-transfer and code-sent labels (`ReceiveCodeView.swift:124`, `SendCodeView.swift:160`, `ContentView.swift:322`), failed delivery (`ChatView.swift:1000`, `:1188`), Pocket Library's catalogue match (`CatalogueSearchView.swift:197`), FieldKit's "Recording" label (`TripListView.swift:117`), Soundscape's recording text (`ProjectView.swift:181`, in the Studio screens the music-player plan removes, so leave it). These are success/danger tones, contrast-checked in ForgeUI and unchecked as raw colours. Not findings: swipe-action tints (Relay's blue/indigo follow Mail's convention), Device Lab's chart series colours, Soundscape's mute/solo toggle colours (mixer convention), ratings stars, Weather's sky.

**A4. Root loading is a bare `ProgressView()` (P3, apps + check).** Good Dog `ContentView.swift:21`, Pocket Library `ContentView.swift:25`, Relay `RelayRoot.swift:97`. `ForgeStateView.loading` exists and the `state-views` rule doesn't look for this case. Unchanged since #285 (X15).

**A5. Relay's `TrustBadge` is `ForgeStatus` (P3, app).** `Label("Trusted", systemImage: "checkmark.shield.fill").font(.caption).foregroundStyle(.green)` is `ForgeStatus("Trusted", tone: .success)` with an unchecked colour. Small, but it is the one exact duplicate of a ForgeUI component found.

**A6. Checklist is outside ForgeUI (P2, app).** 528 lines of Swift, no ForgeUI or ForgeAppKit dependency, two `ContentUnavailableView`s, plain `.alert`s. It is the smallest app and the cheapest to move: theme, two state views, roles. Its `Package.swift` already uses the checkout's ForgeKit, so adding the ForgeUI product is a Swift-only change.

### Coverage: apps need it, the framework lacks it

**C1. `forgeFormToolbar` can't guard unsaved changes (P1, ForgeUI + ForgeAppKit).** The toolbar disables swipe-to-dismiss only while saving. Every real form also stops it while there are unsaved changes, so nobody uses the helper; each hand-rolls the identical Cancel / spinner-labelled-"Saving" / Add toolbar plus `.interactiveDismissDisabled(...)`:

| Where | Guard while |
|---|---|
| Starter `ComposeNoteView.swift:38` | text entered |
| FieldKit `AddNoteView.swift:35` | text entered |
| Pocket Library `BookFormView.swift:150` | saving, or changed |
| Good Dog `DogFormView.swift:164` | changed |
| Relay `ConversationListView.swift:273` | text entered; its spinner has no VoiceOver label |
| Commonplace `AddFeedView.swift:61` | saving only: this one could use `forgeFormToolbar` today |
| FieldKit `StartTripView.swift:35` | never (could use `forgeFormToolbar` today) |
| Soundscape `ContentView.swift:209` (new project) | no spinner; Create is only disabled while saving |

Fix: one parameter (`hasChanges: Bool = false`) on both overloads, then the starter and these forms adopt it in the same PR (rule 12), with a parity rule that flags a hand-rolled `confirmationAction` spinner. Also move `forgeFormToolbar` out of `ForgeDetailHeader.swift` into its own file. This is the clearest case of "the helper exists but is one behaviour short, so everybody reinvents it".

**C2. Rename prompt (P2, ForgeAppKit).** Seven hand-rolled alerts with a `TextField`: Commonplace feed, FieldKit trip, Relay device, Sketchbook sketch, Soundscape project, track and playlist. Each has its own Bool or optional-binding plumbing and its own identifiers (`feeds.rename.title`, `trip.renameField`, `rename.title`); Rename is never disabled for an empty or unchanged name (Commonplace accepts empty on purpose, to restore the feed's own name). A `ForgeRename` value (current name, whether empty is allowed, the action) and `.forgeRename($model.rename)` modifier, presented as the same system alert, gives consistent identifiers and validation and a Linux test. #285 listed "validated rename" for ForgeAppKit; the count confirms it. Five apps already, so the second-use rule is met; extract after the next phone sitting passes the rename flows in two of them.

**C3. Inline notice (P2, ForgeUI).** Offline, stale and reconnecting states are drawn three ways: Weather's `Notice` card on the sky (`ForecastPage.swift`), Relay's `ConnectionBanner` on `.bar` (`ChatView.swift`), Commonplace's `RefreshStatus` caption in the bottom toolbar (`FeedListView.swift`). A small `ForgeNotice(text, systemImage:, tone:, action:)` that takes the theme's colours would serve Relay and Commonplace; Weather keeps its own look but can share the wording rules. #285 X17 named the same three. Second use is met; extract with the next Relay or Commonplace screen change, not on its own.

**C4. Icon tiles in a category colour (decide first; ForgeUI).** `ForgeRow`'s icon tile takes a `ForgeTone`, which the visual language reserves for meaning. Soundscape's "Your Music" rows (`CollectionView.swift:53`, pink/indigo/orange/green gradient tiles) and Pocket Library's Activity rows (`ActivityView.swift:29`, green/blue/orange/yellow symbols) want the Settings-app look: a different colour per category. Either ForgeRow accepts a `ForgeAccent` for the tile (checked colours, no meaning implied) or the apps follow the one-accent rule. A decision (D1), not a component.

### Not gaps (native is right, or fine as it is)

- **Content rows.** Book, trip, track, article, conversation and lesson rows lead with content (covers, artwork, stats, previews). `ForgeRow` is a settings-style row; these should stay app-owned, built from text roles.
- **Content-led headers.** Pocket Library's book header leads with the cover and switches layout at accessibility sizes; `ForgeDetailHeader` (icon tile only) has no users and needs none yet. Leave it.
- **Destinations.** Only Pocket Library uses `.forgeSheet`; other apps use plain `.sheet` with a Bool. Optional by design; adopt when a screen gains a second sheet.
- **Search.** `.searchable` plus `ForgeStateView.noResults` is used consistently in eight apps. No ForgeAppKit search state is needed yet (#285 listed one; the evidence doesn't support it).
- **Success feedback.** `ForgeFeedback` is in eight apps and has a parity rule. Done.
- **Undo.** `ForgeBanner` has one real user (Commonplace). The undo-vs-confirm decision of 2026-11-17 governs new uses; no gap.
- **Weather's `Card`, Relay's bubbles and reactions, Soundscape's player and mixer, Sketchbook's canvas chrome.** App identity.

## 4. Enforcement

What stops drift today, and what doesn't.

| Lever | Today | Gap |
|---|---|---|
| Starter | ForgeUI, ForgeAppKit, `ForgeStateView`, `forgeAlert`, `ForgeSubmission` | Standard accent with no prompt to choose (#285 X2); rows use raw system styles, not roles; hand-rolls the form toolbar (C1). New apps copy all three |
| Parity rules (`check-app-parity.sh`) | 22 rules; ForgeUI-related: `state-views`, `toned-text`, `forgeui-appearance`, `feedback` | **E1.** Rules gated on the ForgeUI dependency skip Checklist entirely. **E2.** No rule for a theme at the root, for text roles, for a bare root `ProgressView()`, or for a hand-rolled form toolbar. **E3.** `toned-text` misses raw green/red status (A3) |
| Showcase | Every ForgeUI component has a page; three complete screens | **E4.** ForgeAppKit isn't in it; Device Lab's Patterns page covers only `ForgeBanner` and `ForgeFeedback`. Alerts, destinations, load and submission have no live example |
| Visual regression | Device Lab's `catalogue.toml` screenshots each app's first screen, light and dark, on empty data (#286) | **E5.** No baselines are committed for it yet (only Good Dog has baselines, from its own scenario). First screens on empty data are mostly `ForgeStateView.empty`, so the catalogue shows little of an app's real hierarchy |
| Tests | Contrast of every accent and text-role mapping, on Linux | **E6.** No test of a component at accessibility sizes or with VoiceOver. On Linux this can only be the [accessibility report](../../accessibility-report.md) and a showcase scenario at `-ax5` |

The useful automatic checks are cheap and match the existing rule style (a plain fact about an app's files, with lag listed in `app-parity.toml`):

- `forge-theme`: the root view applies `.forgeTheme(...)` (Weather listed as not applicable, with its reason).
- `root-loading`: no bare `ProgressView()` as a whole-screen state.
- `form-toolbar`: no hand-rolled `confirmationAction` spinner, once C1 lands.
- Widen `toned-text` to `.foregroundStyle(.green)` / `.red` on `Label` and `Image` status.
- Apply ForgeUI rules to apps that use ForgeKit but not ForgeUI (Checklist), so leaving ForgeUI out isn't a way round them.

Text roles are deliberately **not** proposed as a hard rule: plain system styles stay correct for one-off text (forgeui.md says so). A count in the parity report, or a rule for the specific repeated cases (row title + metadata), is enough.

## 5. Recommendations, in order

Modest by design: the milestone is "complete, not bigger".

| # | What | Where | Size | Gate |
|---|---|---|---|---|
| 1 | `forgeFormToolbar(hasChanges:)`; every sheet in C1's table (the starter included) adopts it; `form-toolbar` rule | ForgeUI + ForgeAppKit + apps | Small | Linux CI, laptop build, short phone check of one form |
| 2 | Starter: text roles on its rows, a comment prompting an accent choice | Template | Small | Template sync |
| 3 | `forge-theme` and `root-loading` rules; widen `toned-text`; make ForgeUI rules see Checklist | Check | Small | Linux CI |
| 4 | Fix A3–A5 in Relay, Pocket Library, FieldKit, Soundscape (tones, `ForgeStatus`, `ForgeStateView.loading`) as each is next touched | Apps | Small each | Rule 3 forces it |
| 5 | Text roles, app by app, when a screen is next changed: FieldKit and Pocket Library first (most figure/metadata text) | Apps | Medium overall | None extra |
| 6 | Move Checklist onto ForgeUI | App | Small | Laptop build |
| 7 | `ForgeRename` | ForgeAppKit | Small | After rename flows pass the phone in two apps |
| 8 | `ForgeNotice` | ForgeUI | Small | With the next Relay or Commonplace change that touches it |
| 9 | ForgeAppKit examples in Device Lab's Patterns page; record catalogue baselines | Showcase, Device Lab | Small | A phone sitting (~3 min for the catalogue) |

Not recommended: a ForgeUI content-row component, a stats/metric component (roles cover it), a search-state type, migrating every `.sheet` to destinations, making ForgeUI button styles mandatory, or touching Weather's visual system.

## Decisions for Reuben

- **D1. Category colours.** Should icon tiles (and only icon tiles) be allowed to use ForgeUI's checked accent set as category colours, as Settings and Music do? Yes makes C4 a one-parameter change; no means Soundscape and Pocket Library move to one accent. *Recommendation: yes, icon tiles only.*
- **D2. Buttons.** Apps use SwiftUI's `.borderedProminent` / `.bordered` (35 uses), tinted by the theme, and ForgeUI's button styles once. On iOS 26 the system styles get Liquid Glass; ForgeUI's draw their own shape. Bless the system styles for in-content buttons and keep ForgeUI's for full-width calls to action, or move apps to ForgeUI's? *Recommendation: bless the system styles, keep `ForgeButton(isWorking:)` and `.forgeIcon`, say so in forgeui.md. Check the Liquid Glass question on the phone first (#285 sitting step 1).*
- **D3. Accents for Relay and Soundscape.** Pick now (one line each) or with their redesigns? *Recommendation: with the Relay transfer-journey work and the Soundscape player PRs already planned.*

## Method and limits

- Source only, on `61c9e13`. App Swift under each app's `Sources/` (generated bindings, launchers and tests excluded); the starter is `templates/ios-app`. Every file and line named was read; counts come from the script below and are for spotting patterns, not scores.
- Classification is a judgement against [forgeui.md](../../forgeui.md)'s own rules: native first, one accent, roles not sizes, tones only for meaning.
- No screenshots and no phone: nothing here says how a screen *looks*, only how it is built. The catalogue baselines (recommendation 9) would add the visual evidence.
- Android (`pocket-library-android`) and the ForgeUI showcase itself are out of scope.

## Appendix: counts

Uses per app, from source lines with comments stripped. Device Lab counts its own screens, not the apps it hosts.

| Pattern | checklist | commonplace | device-lab | fieldkit | good-dog | pocket-library | relay | sketchbook | soundscape | weather | starter |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Swift lines | 528 | 1351 | 3947 | 1080 | 3265 | 4412 | 6164 | 1336 | 5652 | 2153 | 864 |
| `.forgeTheme(` | 0 | 1 | 0 | 1 | 1 | 1 | 1 | 1 | 1 | 0 | 1 |
| `ForgeTheme(accent:` | 0 | 1 | 0 | 1 | 1 | 1 | 0 | 1 | 0 | 0 | 0 |
| `.forgeText(` / `Font.forge` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `.font(.<system style>` | 2 | 8 | 38 | 13 | 77 | 45 | 62 | 4 | 85 | 27 | 2 |
| `.system(size:` | 0 | 0 | 2 | 0 | 1 | 0 | 0 | 0 | 5 | 5 | 0 |
| `.forgeForeground` / `.forgeTint` | 0 | 0 | 7 | 3 | 21 | 1 | 4 | 1 | 8 | 0 | 0 |
| Raw named colours (incl. accent choice) | 0 | 2 | 15 | 4 | 1 | 7 | 13 | 1 | 14 | 0 | 0 |
| `Color(red:` etc. | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | 2 | 8 | 0 |
| `.buttonStyle(.forge…` | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 |
| `.buttonStyle(.bordered…` | 1 | 1 | 1 | 3 | 14 | 8 | 1 | 0 | 6 | 0 | 0 |
| `ForgeRow` | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `ForgeCard` / `.forgeCard` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| `ForgeBadge` / `ForgeStatus` | 0 | 0 | 0 | 0 | 3 | 1 | 0 | 0 | 0 | 0 | 0 |
| `ForgeField` / `ForgeSectionHeader` / `ForgeDetailHeader` / `ForgeButton` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `ForgeStateView.` | 0 | 5 | 5 | 1 | 7 | 12 | 11 | 2 | 15 | 4 | 3 |
| `ContentUnavailableView` | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `forgeFormToolbar` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `.forgeAlert(` | 0 | 3 | 0 | 4 | 8 | 7 | 6 | 3 | 4 | 0 | 3 |
| `.forgeSheet` etc. | 0 | 0 | 0 | 0 | 0 | 4 | 0 | 0 | 0 | 0 | 0 |
| Plain `.sheet` / `.fullScreenCover` | 2 | 2 | 1 | 4 | 9 | 5 | 8 | 2 | 18 | 1 | 2 |
| `ForgeSubmission` / `ForgeLoad<` | 0 | 2 | 0 | 3 | 0 | 2 | 0 | 0 | 0 | 0 | 1 |
| `ForgeBanner` | 0 | 2 | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `ForgeFeedback` | 0 | 2 | 5 | 2 | 4 | 4 | 6 | 5 | 11 | 0 | 0 |
| Literal `spacing:` | 1 | 5 | 22 | 7 | 38 | 30 | 42 | 4 | 47 | 19 | 0 |
| Literal `.padding(` | 0 | 1 | 5 | 0 | 12 | 7 | 35 | 3 | 12 | 25 | 0 |

Literal spacing and padding are listed but not counted as findings: SwiftUI's own spacing is often right, and Relay's and Weather's are mostly inside their custom surfaces.
