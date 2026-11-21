# ForgeUI: the ixOS design system

ForgeUI gives a SwiftUI app a consistent, accessible look without inventing one: theme tokens for colour, spacing and corners in light and dark, styles for SwiftUI's own buttons, fields and group boxes, and a small set of components for the things every app draws (form fields with validation, cards, settings rows, badges, notices, empty and error states, headers).

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

### Notices

When something is wrong but the screen still works (offline, the map can't update, the server refused the account), say so inside the screen with `ForgeNotice`: what is wrong, what still works, and at most one action.

```swift
ForgeNotice("Offline. Messages you send now go out when you're back.",
            systemImage: "wifi.slash",
            action: ForgeStateAction("Reconnect", identifier: "chat.reconnect") { reconnect() })
ForgeNotice("The map can't be updated right now. Recording carries on.", systemImage: "map")
ForgeNotice(model.refreshProblem, systemImage: "exclamationmark.icloud")   // a reason from Rust, as written
```

- The tone defaults to `.warning`. Use `.info` for news that needs nothing from the person ("Connecting…", "2 messages are waiting") and `.danger` only when something was lost.
- Put it where the problem is: at the top of a list, above the cards, or in `.safeAreaInset(edge: .top)` over a conversation. In a `List`, give it a row of its own with `.listRowInsets(EdgeInsets())` and `.listRowBackground(Color.clear)`.
- When the screen has nothing to show at all, it is a failed state (`ForgeStateView.failed`), not a notice. A status line in a toolbar or a row ("Updated 10:42") is neither: use `ForgeStatus` or plain text.
- An app with its own surfaces (Weather's sky cards) hides the tinted background with `.forgeNoticeBackground(.hidden)`; the notice then draws in the colours around it.

The `notices` [parity rule](app-parity.md) catches the hand-built version: a `Label` with a problem symbol made small and grey or toned.

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

Built on `ContentUnavailableView`, so they look like the system's own; `noResults` is the system's own search state, in the system's words and language. Keep the states apart:

- Show `loading` until the model has loaded, never an empty list that is still loading.
- `noResults` is for every search that found nothing (with `advice:` and `action:` when the search has its own way forward); `empty` is for a screen with nothing in it yet, or an item that has gone (`"Book Deleted"`, no action).
- `failed` takes a retry that repeats the operation that failed, and a model test shows it does.

A state can offer a second way forward, and an action can be a menu when there is a choice to make. Titles and messages from the app's data (a place name, a reason from Rust) are shown as written:

```swift
ForgeStateView.failed(failure.title, message: failure.message,
                      retry: ForgeStateAction("Try Again", identifier: "catalogue.retry") { retry() },
                      secondary: ForgeStateAction("Enter Details by Hand", identifier: "catalogue.manual") { manual = true })
ForgeStateView.empty("No Books Yet", systemImage: "books.vertical",
                     action: .menu("Add Book", systemImage: "plus", identifier: "library.empty.add") {
                         Button("Search Online") { … }.accessibilityIdentifier("library.add.catalogue")
                         Button("Enter Details") { … }.accessibilityIdentifier("library.add.manual")
                     })
ForgeStateView.empty("Summaries Unavailable", systemImage: "sparkles", message: info.unavailableReason)
```

The `state-views` parity rule keeps apps that use ForgeUI off `ContentUnavailableView` itself.

### Bars over content

Anything that floats over a scrolling screen, such as a mini-player or a banner, goes on with `.forgeBar`. It sits on the container (the `NavigationStack`, or a tab's content):

```swift
NavigationStack { … }
    .forgeBar { if player.current != nil { MiniPlayer(player: player) } }
```

The bar measures itself and gives every scroll view below it that much room as a content margin. That includes screens pushed onto the stack, so a list's last row can always scroll clear of the bar. Don't use an `.overlay`, which leaves no room at all, or a `.safeAreaInset` on the stack, whose room didn't reach a pushed list on the iPhone. ForgeAppKit's `forgeBanner` is built on it. The reasons are in [screen-space.md](research/screen-space.md).

### Alerts, dialogs and sheets

Use SwiftUI's `.alert`, `.confirmationDialog` and `.sheet` as they are; the theme tints them. For a form in a sheet, `forgeFormToolbar` adds Cancel and a confirm button that becomes a spinner while saving. A swipe down can't close the sheet while it saves, or, with `hasChanges`, while it holds something the person would lose; Cancel still closes it on purpose:

```swift
NavigationStack {
    Form { ... }
        .forgeFormToolbar(id: "taskForm", confirmTitle: "Add", canConfirm: model.canSave,
                          isWorking: model.isSaving, hasChanges: model.hasChanges,
                          cancel: { dismiss() }, confirm: save)
}
```

The buttons get the identifiers `taskForm.cancel` and `taskForm.confirm` (the spinner `taskForm.working`). Leave `hasChanges` out and only saving stops the swipe. Use it instead of writing the Cancel and spinner toolbar yourself.

For any other sheet that must not be swiped away (a drawing canvas, a transfer that is running), use `.forgeSwipeGuard(isActive)`, which the toolbar uses too. Don't use SwiftUI's `interactiveDismissDisabled` on its own: on iOS 26.3 it didn't stop the swipe when applied inside the sheet's `NavigationStack` (2026-11-18), and the `swipe-guard` parity rule flags it.

**Increase Contrast.** With Settings > Accessibility > Display & Text Size > Increase Contrast on, every theme's secondary and tertiary text, tone text and separators move 40% of the way toward primary text (`ForgePalette.increasedContrast`), so quieter text reaches WCAG AAA (7:1). Backgrounds and fills stay, so the app still looks like itself. Nothing to do in the app: the theme's colours switch by themselves.

### Renaming and naming

To rename something (a trip, a playlist, this device), or name a new one (a folder), use `forgeRename`. It shows a small sheet with one field, Cancel and Save. When the app's rules (in Rust) refuse the name, the reason shows under the field and the sheet stays open with what the person typed; an alert with a text field can do neither, so a refused name used to fail after the alert had closed.

```swift
// ForgeUI: save returns nil once saved, otherwise the reason it wasn't.
.forgeRename("Rename", item: $editing, name: \.title, id: "rename") { item, title in
    await model.rename(item, to: title) ? nil : model.takeError()
}

// ForgeAppKit: save throws, and the error's ForgeFailure message is the reason.
.forgeRename("Rename Trip", isPresented: $isRenaming, name: trip.title, id: "renameTrip") { title in
    try await model.rename(trip, to: title)
}
.forgeRename("New Playlist", isPresented: $naming, name: "", id: "playlistName", confirmTitle: "Create") { name in
    _ = try await model.createPlaylist(name)
}
```

- `item:` renames the item it holds (the sheet closes by setting it to nil); `isPresented:` renames one thing.
- `message:` adds a line under the field ("Other devices see this name.", "Leave it empty to use the feed's own name.").
- Save stays enabled: the app's rules decide what an empty or unchanged name means, not the sheet. A changed name keeps the sheet from being swiped away.
- Identifiers: `<id>.name` for the field, `<id>.cancel` and `<id>.confirm` for the buttons, `<id>.error` for the reason.
- A model alert on the same screen is held back while the sheet is up (the `alerts-over-sheets` rule counts `forgeRename` as a sheet).

The `rename` [parity rule](app-parity.md) catches a hand-built prompt: an `.alert` about a name with a `TextField` in it, or an app's own `…NameSheet`/`…RenameSheet`.

### Sharing a file the app wrote

When the person exports something (a trip as GPX, a checklist as a spreadsheet, a backup), show the system share sheet as soon as the file is written, with `forgeShareFile`. AirDrop, Save to Files, Mail and Messages are all in it, so it is also how a file is saved.

```swift
@State private var exported: URL?

Button("Export Subscriptions") { Task { exported = await model.exportSubscriptions() } }
    ...
    .forgeShareFile($exported)
```

- Set the binding only once the file is written; the sheet sets it back to nil when it closes. Show a failed export as any failed action.
- There is no "your file is ready" sheet or "Share Backup" row in between: the person asked for the export, so the share sheet is what they see next.
- A `ShareLink` stays right for something already there: a link, a book's details, a log on screen.
- A model alert on the same screen is held back while the share sheet is up (the `alerts-over-sheets` rule counts it as a sheet).

The `share-file` [parity rule](app-parity.md) catches a `ShareLink` whose item is named like a written file: `<name>.url` or an identifier containing `URL` (`exportURL`), unless the name says it is a link (`websiteURL`, `linkURL`). It can't see types, so a file under another name passes.

### Row actions

Give a row its actions (Delete, Rename, Mark as Read) with `forgeRowActions`. The same buttons become swipe actions and the menu a long press opens, so someone who doesn't know to swipe, or uses Voice Control or a pointer, still finds them.

```swift
TripRow(trip: trip)
    .forgeRowActions {
        Button("Delete", systemImage: "trash", role: .destructive) { model.askToDelete(trip) }
            .accessibilityIdentifier("trips.delete")
    }

ArticleRow(article: article)
    .forgeRowActions {
        Button("Mark as Read", systemImage: "checkmark.circle") { … }   // swipe from the left
    } trailing: {
        Button("Bookmark", systemImage: "bookmark") { … }               // swipe from the right
    }
```

The `row-actions` [parity rule](app-parity.md) fails a file with `.swipeActions` and no `.contextMenu`.

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
