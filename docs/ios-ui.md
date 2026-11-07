# Building native iOS interfaces

An ixOS app has two halves. Rust owns the app: its models, rules, persistence, networking and events. Swift owns the iPhone experience: SwiftUI views, navigation, presentation state, native controls, accessibility and platform integrations. The two meet in the UniFFI bindings (`import ForgeFFI`), which give Swift ordinary `async` methods, records and thrown errors.

This guide covers the Swift half. It sets out the conventions that make an ixOS app feel like an iPhone app rather than a demo, and the patterns for showing Rust-backed state in SwiftUI. The starter that `forge init` generates follows all of them, so the quickest way to learn them is to read `Sources/AppHost/` in a new app next to this page. For the Rust half, see [app-crate.md](app-crate.md).

Everything here builds with xtool on Linux. Nothing requires a Mac.

## Principles

- **Use the real component.** SwiftUI already provides navigation, lists, forms, sheets, menus, search, swipe actions and empty states that behave exactly as users expect. Use them as they are. Don't rebuild them from stacks and rounded rectangles, and don't wrap them in a component library.
- **Let the system style it.** System fonts, semantic colours, SF Symbols and the standard list and form styles adapt automatically to light and dark mode, Dynamic Type, accessibility settings and future iOS releases. Every hardcoded colour or point size is one more place where the app can fall out of step.
- **Rules in Rust, presentation in Swift.** If a behaviour should match on Android, or should be tested without a UI, it belongs in Rust: validation, sorting, search, state transitions. How something looks and moves belongs in Swift: titles, dates, layout, animation, haptics.
- **Add abstractions only for real duplication.** The starter has one helper, `errorAlert`, because every screen that can fail needs the same alert. Everything else is plain SwiftUI.

## App structure

```
Sources/AppHost/
  NotesModel.swift        @Observable model: state the screens read, async actions that call Rust
  Note+Display.swift      Identifiable conformance and display helpers for the Rust record
  ContentView.swift       root: owns the model, opens Rust, switches between loading, ready and failed
  NoteListView.swift      screen 1 (NavigationStack, search, swipe actions, empty states)
  NoteDetailView.swift    screen 2 (edit in place, delete with confirmation)
  ComposeNoteView.swift   sheet (Form, Cancel and Add, focus, validation)
  SettingsView.swift      sheet (Form, an async Rust request you can cancel)
  ErrorAlert.swift        the one shared helper
```

Keep each screen in its own file, named after what it shows. Put the model in its own file too, and give it no SwiftUI import, so you can test it without the UI (`Tests/AppHostTests` drives it against the real Rust library).

## Presenting Rust-backed state

### The model

The minimum deployment target is iOS 17, so models use the Observation framework:

```swift
@Observable
@MainActor
public final class NotesModel {
    public private(set) var phase: Phase = .loading   // loading, ready, failed(String)
    public private(set) var notes: [Note] = []        // Rust's list, as last reported
    public var searchText = ""
    public var actionError: String?                   // shown by an alert
    public private(set) var core: AppCore?
    ...
}
```

- Mark the class `@MainActor`, because SwiftUI reads it on the main thread. Rust does its slow work on its own threads, so awaiting a Rust `async` method never blocks the UI.
- Make state `private(set)` unless a view must bind to it (`searchText`, a form field). Views then change state only through the model's methods.
- Use `@ObservationIgnored` for bookkeeping that no view displays, such as tasks, subscriptions and cancellation handles.
- Hold the model in `@State` on the root view, then pass it down. A child that needs bindings declares `@Bindable var model: NotesModel`; one that only reads it declares `let model: NotesModel`. SwiftUI re-renders a view only when a property it actually read changes.

Don't use `ObservableObject` and `@Published` in new code. The older examples (Counter, Weather, Chat) still do, and they keep working.

### Opening Rust and the loading state

Open the Rust core once, from the root view's `.task`, off the main actor:

```swift
.task { await model.open() }   // open() returns at once if already open
```

```swift
let core = try await Task.detached { try AppCore(storageDirectory: directory) }.value
let events = try core.events()   // subscribe before the first read, so no change is missed
notes = core.notes()
phase = .ready
```

The root view switches on `phase`. Show a bare `ProgressView()` while loading, because opening normally takes milliseconds and a label would only flash. If opening fails, show a `ContentUnavailableView` with a Try Again button. Opening fails when the data is unreadable or came from a newer build, and Rust leaves such files untouched, so a retry is safe.

### Events from Rust

Rust publishes changes as events, so the screen updates no matter where a change came from. Keep a single loop that runs for the model's lifetime:

```swift
eventTask = Task { [weak self] in
    while let event = await events.next() {
        guard let self else { return }
        switch event {
        case .notesChanged(let notes): self.notes = notes
        case .eventsDropped: if let core = self.core { self.notes = core.notes() }
        }
    }
}
```

After a command succeeds, also read the state directly (`notes = core.notes()`). The screen then changes in the same frame as the tap, and the event that arrives a moment later carries the same list.

### Records in SwiftUI

UniFFI records are already `Equatable`, `Hashable` and `Sendable`. To use them in `ForEach`, `List(selection:)` and `navigationDestination` without an `id:` argument, add `Identifiable` in Swift:

```swift
extension Note: Identifiable {}   // no @retroactive: ForgeFFI is in the same package
```

Name Rust types so they don't collide with Apple's: a record called `Settings`, `SortOrder`, `Text` or `Image` shadows the SwiftUI or Foundation type in every file that imports ForgeFFI. Prefix them with the app's domain instead (`LibrarySettings`, `LibrarySort`).

Display helpers belong in the same Swift extension: a `Date` built from Rust's milliseconds, or a title taken from the first line. If a helper encodes a rule that Android needs too, move it into Rust.

### Commands, errors and validation

Every Rust command is `async throws`, and the error is the crate's `AppError` enum. Two patterns cover most screens:

- **Fire-and-forget actions** (swipe to delete, save on leaving a screen) catch the error into `actionError`. The list presents it with `.errorAlert("Something Went Wrong", message: $model.actionError)`.
- **Form submissions** let the error propagate (`try await model.add(text:)`), so the sheet stays open with the text intact and presents the error itself. A view covered by a sheet can't present an alert, so every presented screen needs its own `errorAlert`.

`NotesModel.describe(_:)` turns an `AppError` into user-facing text. The `reason` fields are diagnostics from Rust, not finished copy.

Rust is the authority on validation. Swift may still disable a button for an obviously invalid input, such as an empty field, but if Rust refuses, show Rust's error.

### Search

Bind `.searchable(text: $model.searchText)`, and let the model ask Rust (`core.searchNotes(query:)`) whenever `searchText` is set. Matching is a domain rule, and an in-memory Rust read is fast enough to run on every keystroke. Show `ContentUnavailableView.search(text:)` when nothing matches.

### Cancellation

Cancelling a Swift `Task` does not reach Rust on its own (UniFFI 0.32). Commands that can run for a long time take a `Cancellation`, and the model forwards the cancel to it:

```swift
let cancellation = Cancellation()
let status = try await withTaskCancellationHandler {
    try await core.checkUrl(url: url, cancellation: cancellation)
} onCancel: {
    cancellation.cancel()
}
```

Start that work from `.task` or `.task(id:)`. SwiftUI then cancels it when the view disappears, so closing a sheet stops its network request in Rust. `SettingsView` shows the pattern: a Check button changes `checkRequest`, and `.task(id: checkRequest)` runs the check. To also offer an explicit Cancel button, keep the `Cancellation` on the model.

## Navigation

- **One `NavigationStack` per navigation hierarchy.** Push detail screens with value-based links: `NavigationLink(value: note.id)` together with `.navigationDestination(for: UInt64.self)`. Pass identifiers rather than copies of records, so the detail screen always shows the latest state from the model and can handle a record deleted while it was open.
- **Tabs for separate top-level areas.** Once an app has two or more areas of equal weight (for example Library, Activity and Settings in Pocket Library), use a `TabView` with a `NavigationStack` inside each tab, never one stack around the whole `TabView`. Give each tab a `Label` with an SF Symbol. A single-purpose app such as the starter doesn't need tabs.
- **Sheets for self-contained tasks.** Use a sheet for creating something, for settings, or for picking something. Give every sheet its own `NavigationStack`, so it has a title bar and toolbar. Use Cancel in `.cancellationAction` and Add, Save or Done in `.confirmationAction`. Apply `.interactiveDismissDisabled(hasUnsavedChanges)` so a swipe down can't discard typing.
- **Titles.** Use the large title (the default) on root screens and `.navigationBarTitleDisplayMode(.inline)` on pushed screens and sheets. Use title case for navigation titles, buttons and alerts ("New Note", "Delete Note").
- **Toolbars.** Put the primary action in `.primaryAction` (top trailing). Collect secondary actions in a `Menu("More", systemImage: "ellipsis.circle")`. Create icon buttons with `Button("Title", systemImage:)` so VoiceOver and long-press have a name.

## Lists, forms and actions

- **Lists.** Use the default `List` style (inset grouped on iPhone) with `Section` headers and footers. Don't add backgrounds, borders or custom row cards. A row is usually a `VStack` with a `.headline` line and a `.subheadline` secondary line in `.secondary`.
- **Forms.** For editable settings and records, use `Form`, which is grouped automatically. Use `TextField(..., axis: .vertical)` for multi-line text, `Picker` for a choice from a set, `Toggle` for on/off, `Stepper` or a number-pad `TextField` for numbers, and `LabeledContent` for read-only values.
- **Swipe actions and context menus** offer the same actions. Delete has `role: .destructive`. Add `ShareLink` wherever there's something to share; it needs no permission and no Rust.
- **Confirmation.** Ask with `.confirmationDialog` before destructive actions that are easy to trigger by accident or hard to undo: deleting from a detail screen, or erasing all data. Swipe to delete is deliberate, so it doesn't ask, as in Mail and Notes.
- **Pull to refresh.** Use `.refreshable` only where content comes from somewhere that can change, such as a server. The starter's notes are local, so it has none; Weather has it.

## Loading, empty and error states

| State | Show |
|---|---|
| First load, nothing to show yet | `ProgressView()` |
| Refreshing content already on screen | Nothing extra; `.refreshable` shows its own indicator |
| A long operation with a result | An inline `ProgressView` plus a Cancel button (see `SettingsView`) |
| No data yet | `ContentUnavailableView` with a label, one sentence and the action that fixes it |
| No search results | `ContentUnavailableView.search(text:)` |
| Can't open at all | `ContentUnavailableView` with Try Again |
| An action failed | An alert (`errorAlert`) |
| Content is stale or offline | A `Label` row at the top of the list, in `.secondary` (see Weather) |

Don't show errors as red text inside a list. An alert names what failed and has to be acknowledged; a red row is easy to miss and stays on screen.

## Keyboard and text entry

- Focus the first field when a compose sheet opens (`@FocusState`, set in `.onAppear`).
- Add `.scrollDismissesKeyboard(.interactively)` to scroll views and editors that contain text fields.
- Choose the right keyboard and behaviour for each field: `.keyboardType(.URL)` with `.textInputAutocapitalization(.never)` and `.autocorrectionDisabled()` for URLs, `.textContentType` for names, emails and addresses, and `.submitLabel(.go)` with `.onSubmit` where Return should act.
- When editing in place, offer Done in the toolbar while the keyboard is up, and save when the user leaves the screen as well (see `NoteDetailView`).

## Appearance

- **Typography.** Use text styles only (`.largeTitle` through `.caption2`) and never `.system(size:)`, so Dynamic Type works. Emphasise with `.headline` or `.fontWeight` rather than with size.
- **Colour.** Use `.primary`, `.secondary` and `.tertiary` for text, and `.tint` for interactive elements. When you need a colour for a surface, use the system background colours (`Color(.systemGroupedBackground)`, `Color(.secondarySystemGroupedBackground)`) and materials (`.regularMaterial`). These all adapt to dark mode. Use a fixed colour only when the colour itself carries meaning, such as a weather symbol's palette.
- **Identity.** An app's identity comes from its accent colour (the `AccentColor` asset, or `.tint()` on the root view), its app icon, its symbols and its copy. It doesn't come from restyling system controls. That leaves room for each app to look like itself.
- **Symbols.** Use SF Symbols with their default rendering mode, or `.symbolRenderingMode(.hierarchical)` / `.multicolor` where it adds information.
- **Spacing and shapes.** Rely on the default padding of lists, forms, stacks and `.padding()`. If you need a constant, use a multiple of 4 and name it. For any rounded shape you draw yourself, use `.containerRelativeShape` or `RoundedRectangle(cornerRadius:style: .continuous)`.
- **Safe areas.** Leave them alone. Lists, forms and navigation stacks already respect them. Use `.safeAreaInset(edge:)` to pin content above the home indicator, rather than adding padding.
- **Avoid** gradients behind content, cards with borders or shadows inside lists, oversized custom headings, and custom back buttons or tab bars. They make an app look like a web page.

## Accessibility

- Every control, row and text field has an `.accessibilityIdentifier("screen.element")`, such as `library.add` or `bookForm.title`, so UI tests can find it ([forge-ui-testing.md](forge-ui-testing.md#accessibility-conventions)). Identifiers are never shown or spoken; they don't replace labels.
- Every control has a text name. Icon-only buttons use `Button("Title", systemImage:)` or a `Label`. Images that carry information have an `accessibilityLabel`; decorative ones are hidden with `.accessibilityHidden(true)`.
- Combine a row's pieces into one VoiceOver element with `.accessibilityElement(children: .combine)`.
- Check every screen at the largest accessibility text size. Rows should wrap rather than truncate their primary text. Swap a horizontal layout for a vertical one under `@Environment(\.dynamicTypeSize)` when `isAccessibilitySize` is true.
- Touch targets are at least 44×44 points. System controls already are; custom tappable views need `.contentShape` and a large enough frame.

## Feedback and motion

- Use `.sensoryFeedback(.success, trigger:)` for a completed save, `.selection` for a changed choice and `.warning` or `.error` for a failure. Use them sparingly; the system already gives feedback for its own controls (toggles, swipe actions, pickers).
- Animate state changes with `withAnimation` or `.animation(_:value:)` using the default curves. Lists already animate inserts and deletes when the array changes.

## Native iOS capabilities

Not every native feature should go through Rust.

| Capability | Where it lives |
|---|---|
| Share sheet (`ShareLink`), haptics, clipboard (`UIPasteboard`), `PhotosPicker`, file import and export (`.fileImporter` / `.fileExporter`) | Swift only. Pass the resulting data or file URL to Rust if Rust should store it. |
| Keychain, local notifications, permissions, deep links | Swift adapters exposed to Rust where Rust needs them. These are being built in the native iOS services work (M3). |
| Location | Swift (CoreLocation), passing coordinates to Rust (see Weather). |
| Camera | Not provided yet. Add it when an app needs it. |

The rule: if Rust needs the result in order to apply a rule or store it, pass the result to Rust. If the feature is purely presentation or system interaction, keep it in Swift.

## Building with xtool on Linux

- The UI compiles with xtool's Swift toolchain against the iOS SDK from Xcode 26. SwiftUI APIs up to iOS 17 work, matching the template's deployment target. Wrap anything newer in `if #available`.
- Don't use `#Preview`. Its macro plugin ships only with Xcode, not with the Swift toolchain on Linux. `@Observable` is fine, because the Observation macros ship with every Swift toolchain.
- Keep iOS-only modifiers (`navigationBarTitleDisplayMode`, `keyboardType`, `textInputAutocapitalization`) inside `#if os(iOS)`, because the same sources also build for macOS for `swift test` and `swift run`.

## Checking the result

- **Logic:** `forge test --project .` runs the app's Rust tests and, on a Mac, the XCTest suite against the real Rust library.
- **Look:** The Screenshots workflow (`.github/workflows/screenshots.yml`, run by hand from the Actions tab: Run workflow, pick the branch and `starter`) renders a freshly generated app in the iOS Simulator in light and dark mode and at the largest text size, and uploads the screenshots as an artifact (`scripts/ios-screenshots.sh`). This is a review aid; the simulator is not part of the supported workflow.
- **Device:** `forge run ios --project .` builds with xtool and installs the app on a connected iPhone. Before calling a screen finished, check the back-swipe gesture, keyboard behaviour, sheets and dark mode on the phone (Settings › Display & Brightness).

## Examples

- **The starter** (`forge init`): one navigation stack, search, swipe actions, a compose sheet, editing in place, a settings sheet with a cancellable request.
- **[Pocket Library](../examples/pocket-library)**: the full showcase. Tabs, a library with search, filters and covers, detail and edit forms, catalogue import, reading history and settings.
- **Weather**: pull to refresh, toolbar actions, a search sheet, stale-data notices.
- **Chat**: a live event stream, settings in a sheet, local notifications.
