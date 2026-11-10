# ForgeUI and Forge AppKit: audit and implementation proposal

Done 2026-11-09 for Reuben's new product direction: ixOS is a native application framework with a Rust application core and a SwiftUI design system. This is Phase 0 of that direction ("stop and consolidate"): what ixOS already has, what seven outside projects do well, what to borrow, and the smallest ForgeUI 0.1 worth building. It is a proposal backed by the code, not a new architecture. The package decision is recorded separately in [ADR 002](../adr/002-forgeui-in-forgekit.md).

Method: the ixOS side was read at `f2e7ea9` (main after #144). The outside projects were cloned at their default branch on 2026-11-09 and read, not built or run: [CofficLab/LumiUI](https://github.com/CofficLab/LumiUI), [frankgumeta/DaVinci](https://github.com/frankgumeta/DaVinci), [isamercan/ThemeKit](https://github.com/isamercan/ThemeKit), [pointfreeco/swift-navigation](https://github.com/pointfreeco/swift-navigation) (including the Inventory example), [pointfreeco/swift-snapshot-testing](https://github.com/pointfreeco/swift-snapshot-testing), [nalexn/ViewInspector](https://github.com/nalexn/ViewInspector) and [ianthetechie/uniffi-starter](https://github.com/ianthetechie/uniffi-starter). All are MIT except uniffi-starter (BSD 3-Clause), so code may be adapted with its notice kept; this proposal borrows ideas and small patterns, and adds none of them as a dependency.

## Summary

- **ForgeUI goes into the existing ForgeKit package** as a third library product, `ForgeUI`, with no dependency on the Rust side. No new repository. Forge AppKit later becomes a fourth product in the same package. See [ADR 002](../adr/002-forgeui-in-forgekit.md).
- **ForgeUI styles SwiftUI's own controls; it does not replace them.** Buttons are `ButtonStyle`s, fields are a `TextFieldStyle` plus a labelled container, cards and rows are small views on top of `GroupBox`, `LabeledContent` and `Label`, empty and error states build on `ContentUnavailableView`. A developer can always drop to plain SwiftUI, and ForgeUI pieces mix with it.
- **Theme = plain values in the SwiftUI environment.** Light and dark are two palettes resolved from `\.colorScheme`, as DaVinci does. No singleton, no asset catalog, no custom fonts, no JSON theme engine. Text uses the system text styles, so Dynamic Type works with no extra code.
- **Tokens and contrast maths are Foundation-only**, so they compile and are tested on Linux. SwiftUI files are wrapped in `#if canImport(SwiftUI)`, as ForgeKit's are today, and compile on the laptop with xtool or on macOS CI.
- **The showcase is a plain SwiftUI app** (`examples/forgeui-showcase`, built with xtool, no Rust core): a gallery of every component with live controls, and three example screens (settings, list, create/edit form) that are the 0.1 acceptance test. #Preview is not used anywhere: it needs Xcode's macro plugin and breaks xtool on Linux.
- **No new test dependencies.** Linux XCTest for tokens and contrast, macOS CI to compile, and the existing Forge screenshot baselines for visual regressions. SnapshotTesting and ViewInspector are deferred, with reasons below.
- **Revised [roadmap](../roadmap.md#product-direction-a-native-application-framework)**: ForgeUI 0.1 next, then AppKit, then Rust application foundations, the reference app and developer experience. Agent tooling expansion is paused; existing device, MCP and diagnostic work is kept as supporting infrastructure.

## What ixOS already has

| Area | What exists | Where | Status for the new direction |
|---|---|---|---|
| Rust application runtime | Serial command executor, published state with revision, lifecycle, cancellation, shutdown, error kinds, data-directory lock | `crates/forge-app` (800 lines) | **Foundation.** Phase 3 builds on it |
| Older Rust runtime | `app_shell!`, `background()`, `EventHub`, logging | `crates/forge-runtime` | Logging and primitives stay; `app_shell!` apps move to `forge-app` app by app (app model phase 5) |
| Capabilities | JSON/SQLite storage, HTTPS client, Keychain, files, notifications, permissions, share, deep links | `forge-storage`, `forge-net`, `forge-services` | Foundation; unchanged |
| Swift framework | `ForgeHost` (owns the core for the process), `ForgeLifecycle`, `ForgeRoot` (loading / ready / retry), contract bindings | `forgekit/` → released as `GeneralKnowledge/ForgeKit` 0.3.x | **The package ForgeUI joins** |
| Swift copied per app | `ErrorAlert`, `LaunchAppearance`, `AppServices`, `AppLifecycle` in the template and apps; `ForgeLog`, `ForgeImages`, service adapters copied into `Generated/` | `templates/ios-app`, `crates/forge-services/swift` | App model phase 2 moves these into ForgeKit; `errorAlert` is replaced by AppKit's alert value (below) |
| UI standard | Native components, system styling, `@Observable` models with no SwiftUI import, identifiers on every control | `docs/ios-ui.md` | Kept. One sentence changes: "don't wrap them in a component library" becomes "style them through ForgeUI; don't rebuild them" |
| Testing | Device screenshot baselines with tolerance and masks, accessibility-tree diffs (`forge ui run --visual`), on-demand simulator screenshots in CI | `crates/forge-uitest/src/visual.rs`, `scripts/ios-screenshots.sh`, `.github/workflows/screenshots.yml` | Reused for ForgeUI's visual checks |
| Packaging | Device-only static library staged and linked with `-L`; framework bindings as a SwiftPM system library; apps built outside the checkout from a git tag | ADR 001, `scripts/forgekit.sh`, `spikes/phase0/RESULTS.md` | Works; nothing in this direction needs it changed |

### What the apps reinvent

Each app has drawn its own interface from raw SwiftUI, and has rewritten the same presentation plumbing. Counted across the template, Pocket Library, Good Dog, Chat, Checklist and Weather:

- **Visual.** No shared tokens: every app picks its own paddings, corner radii and accent use. Private one-off components with the same job: `Card` (Good Dog), `PlaceCard`/`HourlyCard`/`DailyCard` (Weather), `StatRow`/`ProgressRow` (Good Dog, Device Lab), `ConnectionBanner` (Chat), eight different `*Row` views. Empty states are consistent only because all 25 use `ContentUnavailableView` directly.
- **Error alerts: four implementations.** `errorAlert(_:message:)` copied byte for byte in the template, Good Dog and Chat; Pocket Library's own `errorAlert(PresentedError?)` (`Presentation.swift`); Checklist's inline `.alert`. 23 + 5 call sites. Only two put an identifier on OK; titles differ in case; error state lives in the view in 12 places and in the model in 2.
- **Confirmations: 8**, each a view-local Bool (`confirmDelete`, `confirmErase`, `isConfirmingDelete`, ...), none testable from the model.
- **Submission in progress: 5** copies of busy flag + spinner instead of the button + catch into an error, under four names (`isSaving`, `saving`, `isWorking`, Chat's sending state).
- **Hand-written `Binding(get: { x != nil }, set: ...)`: 6**, plus the 4 helper copies.
- **Load phases: 4** enums for "loading / loaded / failed", plus three apps that fork `ForgeHost.Status` to add "needs onboarding" or "signed out".
- **Sheets: 14** `sheet(isPresented:)` with separate Bools (Pocket Library's library screen has three); no app models its destinations as one enum.

All of these have two or more uses, so they qualify for extraction under AGENTS.md rule 1. The visual ones go to ForgeUI 0.1; the presentation-state ones to AppKit (phase 2).

## What the outside projects do well, and what to take

### SwiftUI design systems

**DaVinci** is the closest match and the best engineered of the three.

- Take: the package layering (tokens → components → gallery → a thin app target, `Package.swift`); light and dark as two plain value palettes resolved from `\.colorScheme` by one modifier (`DSPalette.swift`, `DSTheme.swift:166-187`), so components never branch on appearance and no `UIColor` providers or assets are needed; text styles built with `relativeTo:` and `@ScaledMetric` line spacing (`DSTypography.swift`); pure style resolvers that unit tests can call (`DSButtonStyleResolver.swift`) and a foreground picked by contrast on coloured fills (`DSColorContrast`); a theme validator that returns WCAG contrast issues (`DSThemeValidator.swift`); `ACCESSIBILITY.md`'s honest split between automated checks and the manual VoiceOver pass.
- Leave: iOS 18 and Swift 6.4 floors (ixOS is iOS 17 / Swift 6.0); a private system font name; wrapper views with long parameter lists (`DSButton(_:appearance:size:icon:isLoading:isDisabled:accessibilityLabel:accessibilityHint:action:)`), where `isDisabled` duplicates SwiftUI's `.disabled()`; no accessibility identifiers.

**ThemeKit** is far larger than needed (about 98k lines). Inspected for tokens and themes only.

- Take: style protocols for things SwiftUI has no style for, mirroring `ButtonStyle` (`CardStyle.swift`, `ListRowStyle.swift`: `makeBody(configuration:)`, an environment key, `.cardStyle(_:)`); a semantic tone resolving to solid, soft and border colours (`SemanticColor.swift`), which is exactly what badges and status indicators need; the gallery's registry of entries with a shared stage that switches appearance and shows a usage snippet (`Demo/Demo/Gallery/ComponentRegistry.swift`); a tokens-only product.
- Leave: the `Theme.shared` singleton (the project is itself deprecating it in favour of `@Environment(\.theme)`, which is the lesson); imperative dark mode (`setColorScheme(dark:)`); bundled fonts, JSON and CSS themes; an Xcode-project demo with an asset catalog; 267 files with `#Preview`.

**LumiUI** was suggested as the first to study, but its code is the least relevant: macOS-first (menu bar, hover, status bar scaffolds), fixed-point fonts with no Dynamic Type, a singleton theme store, no gallery app (only `#Preview` blocks in 74 files) and a String Catalog that xtool cannot compile.

- Take: merging the system's reduce-motion setting with an app override (`Support/MotionPreference.swift`); skeleton and shimmer that respect it (`AppSkeleton.swift`); the empty-state shape of icon, title, description and one optional action (`AppEmptyState.swift`).
- Leave: the rest.

**None of the three sets accessibility identifiers.** ForgeUI must, because Forge's UI tests and visual baselines find elements by identifier (AGENTS.md rule 4).

### Application composition (for AppKit, phase 2)

**swift-navigation**'s core idea fits ixOS's model rule (models import Foundation only, and are tested on Linux) very well: navigation and alerts are values the model owns.

- Take: one optional `Destination` enum per screen model instead of several Bools, driving `.sheet(item:)` and `.navigationDestination(item:)` (`Examples/Inventory/Inventory.swift`); alerts and confirmation dialogs as plain data with typed actions (`Sources/SwiftNavigation/AlertState.swift` imports only Foundation), so a Linux test can assert "tapping Delete asks for confirmation"; a small `Binding` helper that unwraps an optional or one enum case (`Sources/SwiftUINavigation/Binding.swift`, about 5 lines).
- Leave: the dependency itself. It needs swift-syntax and macro plugins (`Package.swift:68`, `@CasePathable`, `@CaseBindable`), which are slow and fragile to build with xtool on Linux, and its UIKit bindings, `observe {}` and `UINavigationPath` add up to a second state framework, which the direction rules out. The ideas above take about 100 lines of plain Swift with no macros.
- The **Inventory example** (four files, about 480 lines: list, add sheet, pushed edit screen, delete confirmation, a form) is the right shape for the Phase 4 reference app, with Rust owning the items. It has no tests of its own; ixOS would add Linux model tests for every destination.

### Testing and the Rust boundary

- **swift-snapshot-testing**: renders SwiftUI only on iOS and tvOS, which means the Simulator; macOS hosting renders AppKit-styled controls, a poor proxy for an iOS design system. Baselines are tied to one simulator, OS and Xcode version. ixOS already compares screenshots with tolerances and masks (`crates/forge-uitest/src/visual.rs`), driven by `forge` on a real iPhone. **Defer**; borrow the idea of rendering every component through a fixed light/dark × text-size matrix, and feed the showcase into the existing comparison.
- **ViewInspector**: macOS/iOS only (every file imports SwiftUI) and reads SwiftUI's private structure by reflection, so each iOS release needs patches (it already carries iOS 18 and iOS 26 branches). **Defer.** Keep component logic in plain values and functions that XCTest can check on Linux; check behaviour through identifiers and `forge ui run`.
- **uniffi-starter**: builds an XCFramework with `xcodebuild` and simulator slices, consumed as a `binaryTarget`. ixOS deliberately does not (ADR 001: no `xcodebuild` on Linux), and Phase 0 already proved apps build outside the checkout from source. Worth borrowing later: a simulator slice built only on macOS CI (`screenshots.yml` already builds one), and its checksum release pattern if Forge ever ships prebuilt Rust binaries. Irrelevant to ForgeUI, which has no Rust.

## ForgeUI 0.1: the proposal

### Package

`forgekit/Package.swift` gains one product and one target:

```swift
.library(name: "ForgeUI", targets: ["ForgeUI"]),
...
.target(name: "ForgeUI", path: "Sources/ForgeUI"),   // no dependencies
.testTarget(name: "ForgeUITests", dependencies: ["ForgeUI"], path: "Tests/ForgeUITests"),
```

An app adds `.product(name: "ForgeUI", package: "forgekit")` (or `ForgeKit` for the release URL) to its app target and writes `import ForgeUI`. Importing ForgeUI pulls in no Rust and no UniFFI.

Inside the target:

```
Sources/ForgeUI/
  Tokens/      ForgeColors, ForgePalette, ForgeSpacing, ForgeRadius, ForgeTone, Contrast   (Foundation only)
  Theme/       ForgeTheme, EnvironmentValues.forgeTheme, .forgeTheme(_:)                   (SwiftUI)
  Styles/      button styles, field style, card and row styles                              (SwiftUI)
  Components/  ForgeField, ForgeCard, ForgeRow, ForgeBadge, ForgeStatus, ForgeSectionHeader,
               ForgeStateView, ForgeDetailHeader                                            (SwiftUI)
```

### Tokens and theme

- **Colour roles (about 14):** `background`, `groupedBackground`, `surface`, `surfaceRaised`, `textPrimary`, `textSecondary`, `textTertiary`, `separator`, `accent`, `onAccent`, plus four tones, `success`, `warning`, `danger`, `info`, each with solid, soft and on-solid variants. Stored as RGBA values, light and dark, in a `ForgePalette`. The default palette follows Apple's system colours so a ForgeUI app looks native before anyone changes anything.
- **Spacing:** `xxs 2, xs 4, s 8, m 12, l 16, xl 24, xxl 32`; **radius:** `s 6, m 10, l 16, full`; **elevation:** two levels (`raised`, `overlay`), off in dark mode where SwiftUI conventions use colour instead.
- **Typography:** no custom ramp. Components use system text styles (`.body`, `.headline`, `.footnote`...), so Dynamic Type, Bold Text and future iOS styles come free. Paddings and icon sizes that should grow with text use `@ScaledMetric`.
- **Delivery:** `struct ForgeTheme: Sendable` (palette, spacing, radius, a corner style) in `EnvironmentValues` through `@Entry`, set with `.forgeTheme(_:)` at the root. The modifier resolves the palette for the current `\.colorScheme` and sets SwiftUI's `.tint` to the accent, so plain SwiftUI controls inside a ForgeUI app pick up the brand colour too. Changing an app's look means editing one `ForgeTheme` value.

### Components (0.1)

| Need from the direction | ForgeUI 0.1 | Built on |
|---|---|---|
| Buttons and icon buttons | `.buttonStyle(.forgePrimary)`, `.forgeSecondary`, `.forgeTertiary`, `.forgeDestructive`; `.forgeIcon` for icon-only; a `.controlSize` adjusts them; 44 pt minimum hit area; pressed and disabled states from the environment | `ButtonStyle` |
| Busy buttons | `ForgeButton("Save", isWorking: saving) { ... }`: the one wrapper, because a spinner in place of the label needs layout | `Button` + `ProgressView` |
| Text, secure and search fields | `.textFieldStyle(.forge)` for `TextField` and `SecureField`; search stays `.searchable` (native) | `TextFieldStyle` |
| Form fields with labels and validation | `ForgeField("Title", message: .error("Required")) { TextField(...) }`: label, the field, a help or validation line read by VoiceOver | container view |
| Cards and grouped surfaces | `ForgeCard { ... }` and `.groupBoxStyle(.forgeCard)` | `GroupBox` |
| Lists and list rows | `ForgeRow(title, subtitle:, icon:, value:)` for the settings and list shape (icon tile, text, trailing value or accessory); works in `List` and `Form` | `LabeledContent`, `Label` |
| Dividers and section headers | `ForgeSectionHeader("Account", action:)` | `Section` header |
| Badges and status | `ForgeBadge("Overdue", tone: .warning)`, `ForgeStatus(.success, "Synced")` (dot + text) | `ForgeTone` |
| Progress | native `ProgressView` and `Gauge`, tinted by the theme | system |
| Empty, loading and error | `ForgeStateView(.empty / .loading / .failed(message, retry:))` | `ContentUnavailableView`, `ProgressView` |
| Alerts, dialogs and sheets | native `.alert`, `.confirmationDialog`, `.sheet` with the theme applied; a `forgeSheetToolbar(cancel:confirm:)` helper for the Cancel / Save pattern | system |
| Detail headers | `ForgeDetailHeader(title, subtitle:, icon:, badges:)` | composition |

Rules every component follows:

- Takes `LocalizedStringKey` labels, one initialiser per shape; no `isDisabled:` (use `.disabled()`), no colour or font parameters (use the theme).
- Sets no accessibility identifier itself: the caller passes the screen's identifier (`.accessibilityIdentifier("settings.notifications")`), because only the app knows the screen name. The showcase and docs show it on every example.
- Combines its parts into one accessibility element where that is what VoiceOver should read (a row reads "Notifications, Enabled, button").
- Respects reduce motion (no decorative animation) and increases contrast where `colorSchemeContrast` is `.increased`.

### The showcase app (`examples/forgeui-showcase`)

- **Gallery tab:** components grouped as Foundations (colours, spacing, type), Controls, Content, Feedback. Each page shows the component's variants, live controls to change them (style, size, tone, disabled, working), and the code to use it. A registry list (name, group, view, usage string) drives the index, as in ThemeKit's demo.
- **Screens tab:** the three acceptance screens, built only from ForgeUI and plain SwiftUI with no custom styling: **Settings** (the exact layout in Reuben's message: Account, Appearance, About), **Tasks** (searchable list with badges, empty state, swipe to delete with confirmation, loading and error states), **New / Edit task** (a form with labelled fields, validation messages, a busy Save button). The form's rule ("title is required and at most 80 characters") turns into `ForgeFieldMessage` values in the view model, which is how a Rust validation result reaches the field in a full ixOS app.
- **Appearance toolbar** on every page: light/dark, a Dynamic Type size, and two themes (default and an alternative accent) to show that one value changes the whole app.
- The app is a plain SwiftUI package that depends only on ForgeUI: no Rust core, no generated bindings, no `forge.toml`, built with `xtool dev`. That proves ForgeUI works in any SwiftUI app (Reuben's package strategy, [ADR 002](../adr/002-forgeui-in-forgekit.md#package-strategy-reuben-2026-11-09)); the Rust-owned rule moves to Pocket Library, the reference app. Bundle id `org.forgemobile.agentlab`.
- *Changed during the build:* this section first proposed a small Rust crate for the form's title rule. It was dropped so the showcase proves the components and nothing else; `forge` building apps without a Rust crate is left to the minimal-app work.

### Tests (proportionate)

- **Linux, every PR (`swift test` in `forgekit/`):** every text and tone pair in both default palettes passes WCAG contrast (4.5:1 for text, 3:1 for large text and non-text); the palettes define every role in both appearances; spacing and radius scales ascend; the contrast maths matches known values. About ten tests.
- **CI (`forgeui.yml`):** on Linux and macOS, the token tests and the showcase's model tests; on macOS, the showcase built for iOS (unsigned) when `forgekit/` or the showcase changes.
- **Laptop:** `xtool dev build` in `examples/forgeui-showcase`, before asking for the phone.
- **Phone (next sitting, about 5 minutes):** install the showcase, look through both tabs in light and dark and at a large text size. It proves the components render and read well on a real iPhone, which the CI build cannot. Recorded only if Reuben sees it.
- **Visual baselines:** added to `screenshots.yml` once the screens settle, not in 0.1.

### Docs

- `docs/forgeui.md`: what ForgeUI is, tokens and theming, each component with its code, and how to drop to plain SwiftUI.
- `docs/ios-ui.md`: the principle changes from "don't wrap them in a component library" to "use ForgeUI's styles on the real components; don't rebuild them", and links to `forgeui.md`.
- Capability ledger: one record for ForgeUI (has an API, Linux tests, macOS build; iPhone only once seen).

### Not in 0.1

Forge AppKit's state values (alerts as data, destinations, load state, submission: phase 2, with the counts above as the case for each); moving the template and the demos onto ForgeUI (after 0.1 merges, one app at a time, starting with `forge init`); custom fonts; a theme file format; snapshot or ViewInspector tests; Android.

## Size and order

One PR for ForgeUI 0.1 and the showcase: about 2,000 lines of Swift, docs. Then a ForgeKit release (0.4.0, because forge-app's version is the framework version and the release script refuses to reuse a tag), so apps outside the checkout get it. Then the template adopts it, so every `forge init` app starts on ForgeUI.
