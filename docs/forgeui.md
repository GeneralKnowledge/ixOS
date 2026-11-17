# ForgeUI: the ixOS design system

ForgeUI gives a SwiftUI app a consistent, accessible look without inventing one: theme tokens for colour, spacing and corners in light and dark, styles for SwiftUI's own buttons, fields and group boxes, and a small set of components for the things every app draws (form fields with validation, cards, settings rows, badges, empty and error states, headers).

It is not a replacement for SwiftUI. You still write `List`, `Form`, `NavigationStack`, `Toggle`, `.sheet` and `.searchable`; ForgeUI styles them and fills the gaps. Wherever ForgeUI has nothing to offer, plain SwiftUI is the answer, and the two mix freely on the same screen.

ForgeUI 0.1 is new (2026-11). Its SwiftUI side is compiled in CI and built with xtool on Linux, and the showcase has been run on an iPhone in light and dark (2026-11-10; see the [capability ledger](capability-ledger.md)). Why it is shaped this way, and what it borrowed from other design systems, is in the [proposal](https://github.com/GeneralKnowledge/ixOS/blob/8fdd497cc51dfd0a8220f7dc013c3a25515f6c51/docs/research/forgeui-proposal.md).

## Use it in any SwiftUI app

ForgeUI is a library product of the ForgeKit package. It depends on nothing else: no Rust core, no generated bindings, no ForgeKit runtime. An ordinary SwiftUI app can use it.

```swift
// Package.swift
dependencies: [
    .package(url: "https://github.com/GeneralKnowledge/ForgeKit.git", exact: "<version>"),
    // inside the ixOS checkout: .package(path: "../../forgekit")
],
targets: [
    .target(name: "MyApp", dependencies: [.product(name: "ForgeUI", package: "forgekit")]),
]
```

Apply the theme once, at the root:

```swift
import ForgeUI
import SwiftUI

@main
struct MyApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .forgeTheme(.standard)
        }
    }
}
```

`.forgeTheme` puts the theme in the environment and tints plain SwiftUI controls (toggles, pickers, links, swipe actions) with its accent.

The [showcase](../examples/forgeui-showcase/README.md) shows every component live, with the code for each, and three complete screens built only from ForgeUI and SwiftUI.

## Theme and tokens

A theme is one value, `ForgeTheme`: a palette (light and dark colours), a spacing scale, corner radii, and whether cards cast a shadow. Change an app's look by changing that value.

Most apps need only their own accent, picked from ForgeUI's checked set:

```swift
ContentView()
    .forgeTheme(ForgeTheme(accent: .teal))
```

`ForgeAccent` has `blue` (the standard), `teal`, `green`, `indigo`, `purple`, `pink`, `orange` and `graphite`, each a light and a dark tone already checked for contrast on every background, with Increase Contrast too. For any other colour, build the tones yourself and check them:

```swift
let brand = ForgeTheme(
    palette: ForgePalette.standard.withAccent(
        light: ForgeToneColors(solid: ForgeRGBA(hex: 0x1F7A4D), onSolid: ForgeRGBA(hex: 0xFFFFFF),
                               text: ForgeRGBA(hex: 0x17603C), softOpacity: ForgePalette.lightSoft),
        dark: ForgeToneColors(solid: ForgeRGBA(hex: 0x4CD08A), onSolid: ForgeRGBA(hex: 0x000000),
                              text: ForgeRGBA(hex: 0x8BE3B3), softOpacity: ForgePalette.darkSoft)
    ),
    radius: ForgeRadius(s: 9, m: 18, l: 26)
)
```

| Token | What |
|---|---|
| Colour roles | `background`, `groupedBackground`, `surface`, `surfaceRaised`, `textPrimary`, `textSecondary`, `textTertiary`, `separator` |
| Tones | `accent`, `neutral`, `success`, `warning`, `danger`, `info`, each with `solid` (fills), `onSolid` (text on a fill), `soft` (tinted background) and `text` (the tone's readable colour) |
| Spacing | `xxs 2`, `xs 4`, `s 8`, `m 12`, `l 16`, `xl 24`, `xxl 32` |
| Radius | `s 7` (icon tiles), `m 12` (buttons, fields), `l 18` (cards); corners are continuous |
| Type | text roles (below) on top of the system text styles, so Dynamic Type and Bold Text work with no extra code |

Read tokens in your own views from the environment:

```swift
@Environment(\.forgeTheme) private var theme

Text("Due today")
    .foregroundStyle(theme.colors.tone(.warning).text)
    .padding(theme.spacing.m)
```

For the common cases there are two shorthands: `.forgeForeground(.warning)` draws text or a symbol in the tone's readable colour, and `.forgeTint(.success)` tints a control (a progress bar, a swipe action) with its solid colour.

Colours follow light and dark mode wherever they are drawn, sheets and menus included, and switch to stronger text colours when the person turns on Increase Contrast (below).

### Text roles

A text role says what a piece of text is, so the same kind of text looks the same on every screen and in every app. Each role is a system text style underneath, with a weight and a text colour:

```swift
Text(book.title).forgeText(.rowTitle)
Text(book.author).forgeText(.metadata)
Text(trip.distance).forgeText(.figure)
```

| Role | Style | For |
|---|---|---|
| `screenTitle` | title, bold | A big title inside content where the navigation title isn't used |
| `sectionTitle` | title3, semibold | A heading over content that isn't a list section |
| `rowTitle` | headline | The main line of a row or card |
| `body` | body | Reading text |
| `metadata` | subheadline, secondary | Facts about an item: an author, a date, a size |
| `note` | footnote, secondary | A short explanation under a control or a section |
| `figure` | title, semibold, even digits | The number that is the point of a card: a temperature, a distance |
| `figureCaption` | caption, medium, secondary, even digits | A unit, a percentage or a time beside a value |

`Font.forge(.metadata)` gives the font alone. Plain system styles stay fine for one-off text; reach for a role where the same kind of text repeats.

### Contrast

The tokens are plain values (they import only Foundation), so a theme can be checked anywhere, including a Linux test:

```swift
XCTAssertEqual(brand.contrastIssues(), [])
```

`contrastIssues()` checks every pairing ForgeUI's components draw, in light and dark and both again with Increase Contrast, against WCAG AA: 4.5:1 for text, 3:1 for shapes that carry meaning. The standard theme passes; ForgeUI's own tests check it on every change. The default palette is close to Apple's system colours, with text and tone colours darkened (light) or lightened (dark) where Apple's fall short on the backgrounds they are used on.

## Components

Every example below sets an accessibility identifier, because Forge's UI tests and screenshot baselines find controls by it ([AGENTS.md](../AGENTS.md) rule 4). ForgeUI never invents identifiers for your controls; only you know the screen's name. Where a component creates a control itself, it asks for the identifier.

### Buttons

```swift
Button("Save", action: save)
    .buttonStyle(.forgePrimary)              // or .forgeSecondary, .forgeTertiary, .forgeDestructive
    .accessibilityIdentifier("task.save")

Button("Continue", action: next)
    .buttonStyle(.forge(.primary, expands: true))   // full width
    .controlSize(.small)                             // compact

ForgeButton("Save", isWorking: model.isSaving) { Task { await model.save() } }
    .buttonStyle(.forgePrimary)              // a spinner in place of the title while saving

Button { add() } label: { Label("Add", systemImage: "plus") }
    .buttonStyle(.forgeIcon)                 // round, icon only; VoiceOver reads "Add"
```

Disabled and pressed states come from SwiftUI (`.disabled(...)`); there is no `isDisabled` parameter. Buttons are at least 44 points tall, and grow with Dynamic Type.

### Text fields and form fields

```swift
// Outside a Form: a bordered field.
TextField("Email", text: $email)
    .textFieldStyle(.forge)

// A label, the field, and a help or validation line under it.
ForgeField("Title", message: model.titleMessage) {
    TextField("What needs doing?", text: $model.title)
        .accessibilityIdentifier("taskForm.title")
}
```

Inside a `Form`, leave the field's default style: the form row is already its frame. `message` is a `ForgeFieldMessage` (`.help`, `.success`, `.warning`, `.error`). It imports only Foundation, so the view model makes it, for example from a Rust validation result, and a Linux test checks it. VoiceOver reads the message after the field, with "Error:" in front of errors.

For search, use SwiftUI's `.searchable`, and `ForgeStateView.noResults(for:)` when nothing matches.

### Rows

```swift
NavigationLink { ProfileView() } label: {
    ForgeRow("Your profile", subtitle: "Personal information", systemImage: "person.fill")
}
Toggle(isOn: $notifications) {
    ForgeRow("Notifications", systemImage: "bell.badge.fill", tone: .danger)
}
Picker(selection: $appearance) { ... } label: {
    ForgeRow("Dark mode", systemImage: "moon.fill", tone: .neutral)
}
ForgeRow("Version", value: "1.0.0")
ForgeRow(task.title, subtitle: task.notes) { ForgeBadge("High", tone: .danger) }
```

A `ForgeRow` is the label of SwiftUI's own rows, so navigation chevrons, toggles, pickers and swipe actions are the system's. Titles you pass as literals are localised; titles from your data (`task.title`) are shown as written, as with `Text`. The row reads as one element in VoiceOver.

### Cards, badges, status, headers

```swift
ForgeCard {
    Text("This week").font(.headline)
    Text("4 of 6 tasks done")
}
GroupBox("Reading") { ... }.groupBoxStyle(.forgeCard)

ForgeBadge("Overdue", tone: .warning)
ForgeStatus("Synced", tone: .success)

ForgeSectionHeader("Recent", actionTitle: "See All") { showAll() }   // outside a List
ForgeDetailHeader(book.title, subtitle: book.author, systemImage: "book.closed.fill") {
    ForgeBadge("Reading", tone: .accent)
}
```

Inside a `List`, `Section("Account")` already gives the right header; `ForgeSectionHeader` is for screens built from cards.

### Empty, loading and error states

```swift
ForgeStateView.empty("No Tasks", systemImage: "checklist",
                     description: "Tasks you add appear here.",
                     action: ForgeStateAction("Add Task", identifier: "tasks.add") { adding = true })
ForgeStateView.loading("Loading tasks")
ForgeStateView.failed("Couldn't Load Tasks", message: model.loadError,
                      retry: ForgeStateAction("Try Again", identifier: "tasks.retry") { reload() })
ForgeStateView.noResults(for: model.searchText)
```

Built on `ContentUnavailableView`, so they look like the system's own.

### Alerts, dialogs and sheets

Use SwiftUI's `.alert`, `.confirmationDialog` and `.sheet` as they are; the theme tints them. For a form in a sheet, `forgeFormToolbar` adds Cancel and a confirm button that becomes a spinner while saving, and stops the sheet being swiped away meanwhile:

```swift
NavigationStack {
    Form { ... }
        .forgeFormToolbar(id: "taskForm", confirmTitle: "Add", canConfirm: model.canSave,
                          isWorking: model.isSaving, cancel: { dismiss() }, confirm: save)
}
```

The buttons get the identifiers `taskForm.cancel` and `taskForm.confirm`.

**Increase Contrast.** With Settings > Accessibility > Display & Text Size > Increase Contrast on, every theme's secondary and tertiary text, tone text and separators move 40% of the way toward primary text (`ForgePalette.increasedContrast`), so quieter text reaches WCAG AAA (7:1). Backgrounds and fills stay, so the app still looks like itself. Nothing to do in the app: the theme's colours switch by themselves.

### Visual language

ForgeUI keeps the ixOS apps a recognisable family without making them identical:

- **Native first.** System navigation, lists, forms, materials, text styles and SF Symbols do most of the work. A ForgeUI app should look like a good iOS app before it looks like an ixOS app.
- **One accent per app.** The accent is an app's main colour signature; its content (books, maps, artwork, weather) carries the rest of its personality. Don't add a second brand colour; use tones (`success`, `warning`...) only for meaning.
- **Roles, not sizes.** Text by role or system style, never a fixed point size; spacing and corners from the theme.
- **Icons.** SF Symbols at the text style they sit beside (`.imageScale` or the row's font), so they scale with Dynamic Type; filled variants for selected or status symbols, outline for actions.
- **Restraint.** Shadows only on raised cards in light mode, no gradients behind content, motion only to show that something changed.

The current accents: Pocket Library orange, Good Dog green, Commonplace indigo, FieldKit teal, Sketchbook pink; Relay and the rest keep the standard blue for now and get their own in their redesigns.

### Backgrounds

`.forgeGroupedBackground()` on a `List` or `Form` draws the theme's grouped background instead of the system grey. With the standard theme the two match. Row backgrounds stay the system's; with a custom `surface` colour, add `.listRowBackground(theme.colors.surface)`.

## Rules ForgeUI follows

- Styles go on SwiftUI's real controls; nothing is rebuilt from shapes.
- One initialiser per shape, labels as `LocalizedStringKey` (plus a `String` form for app data, like `Text`), no colour or font parameters (use the theme), no `isDisabled` (use `.disabled()`).
- No `#Preview`: it needs Xcode's macro plugin and breaks xtool builds on Linux. The showcase is the preview.
- No asset catalogs, bundled fonts or singletons.
- Reduce motion turns off the press animation; Dynamic Type sizes hit areas and icon tiles.

## What's not in 0.1

Application patterns (alerts and destinations as model values, load and submission state) are [ForgeAppKit](forgeappkit.md), a separate, optional product on top of ForgeUI. Also not in 0.1: moving the template and demos onto ForgeUI, snapshot tests, custom fonts, and Android.

## Building on Linux

ForgeUI's token files import only Foundation; its SwiftUI files are wrapped in `#if canImport(SwiftUI)`. So `swift test` in `forgekit/` runs the token and contrast tests on Linux, and the SwiftUI side compiles wherever the iOS SDK is: with xtool on a Linux laptop (`xtool dev` in `examples/forgeui-showcase`), and on macOS CI (`.github/workflows/forgeui.yml`).
