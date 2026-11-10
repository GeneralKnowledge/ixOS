# ForgeAppKit: application patterns

ForgeAppKit is the layer between ForgeUI (how things look) and your app's model (what it does). It gives the presentation state every app writes the same few shapes: an error alert, "are you sure?", which sheet is open, a busy Save button, a list that is loading or failed, and which form fields are wrong. Each is a plain value the view model owns, so a test on Linux can check it, and a SwiftUI modifier presents it.

It is optional. An app can use ForgeUI alone, or plain SwiftUI, and adopt ForgeAppKit one screen at a time. It depends only on ForgeUI: no Rust core, no generated bindings, no ForgeKit runtime. Why it exists, with counts of how often the apps rewrote each pattern, is in the [ForgeUI proposal](research/forgeui-proposal.md#what-the-apps-reinvent).

ForgeAppKit 0.1 is new (2026-11). Its model values are tested on Linux; its SwiftUI side is compiled in CI and has not been run on an iPhone yet (see the [capability ledger](capability-ledger.md)).

## Add it

```swift
// Package.swift
.target(name: "AppHost", dependencies: [
    .product(name: "ForgeAppKit", package: "forgekit"),   // brings ForgeUI with it
])
```

Model files `import ForgeAppKit` with Foundation and Observation; it never imports SwiftUI there, so the model still compiles and tests on Linux ([AGENTS.md](../AGENTS.md) rule 3). View files import it with SwiftUI and get the modifiers.

## Errors: `ForgeFailure`

Describe your app's errors once, and every alert, failed load and failed save uses the same words:

```swift
extension LibraryError: ForgeDescribedError {
    public var failure: ForgeFailure {
        switch self {
        case .Offline: ForgeFailure(title: "You're Offline",
                                    message: "Your library still works offline. Try again when you're connected.",
                                    isRetryable: true)
        case .NotFound: ForgeFailure(title: "Book Not Found", message: "It may have been deleted on another screen.")
        // ...
        }
    }
    public var isCancellation: Bool { if case .Cancelled = self { true } else { false } }
}
```

`ForgeFailure(error)` uses that for your errors and a generic title for anything else. Cancellation (Swift's `CancellationError`, or an error whose `isCancellation` is true) is never a failure: ForgeAppKit shows nothing for it, because the person stopped the work on purpose.

## Alerts and confirmations: `ForgeAlert`

The model sets an alert; the view presents it; a tapped button's action comes back to the model.

```swift
// Model (Foundation + Observation)
public enum LibraryAction: Equatable { case delete(UInt64) }
public var alert: ForgeAlert<LibraryAction>?

public func askToDelete(_ book: Book) {
    alert = .confirm("Delete “\(book.title)”?", message: "This can't be undone.",
                     confirm: "Delete Book", action: .delete(book.id),
                     identifier: "book.delete", destructive: true)
}

public func handle(_ action: LibraryAction) async {
    switch action {
    case .delete(let id):
        do { try await library.deleteBook(id: id) } catch { alert = .failure(error) }
    }
}

// View
.forgeAlert($model.alert) { action in Task { await model.handle(action) } }
```

- `.failure(error)` is the error alert: the failure's title and message, and OK with the identifier `alert.ok`. It is nil for a cancellation.
- `.confirm(...)` is the confirming button (`<identifier>.confirm`) and Cancel (`<identifier>.cancel`). A destructive confirmation is a dialog from the bottom with a red button, as iOS does it; otherwise it is an alert.
- `ForgeAlert(title:message:buttons:style:)` builds anything else.
- An alert that only informs is `ForgeAlert<Never>`, presented with `.forgeAlert($model.alert)`.

A Linux test drives the whole exchange without SwiftUI:

```swift
model.askToDelete(book)
XCTAssertEqual(model.alert?.style, .confirmationDialog)
await model.handle(try XCTUnwrap(model.alert?.action(for: "Delete Book")))
XCTAssertTrue(model.books.isEmpty)
```

Present one alert per presented screen: a sheet needs its own `.forgeAlert`, because a view under a sheet cannot present.

## Where the screen is going: destinations

Instead of a Bool per sheet, the model keeps one optional enum, so two sheets can never be open at once and a test can see where the screen went:

```swift
public enum Destination: Equatable {
    case add
    case edit(Book)
    public var edit: Book? { if case .edit(let book) = self { book } else { nil } }
}
public var destination: Destination?
```

```swift
.forgeSheet($model.destination, equals: .add) { BookFormView(mode: .add) }
.forgeSheet($model.destination, \.edit) { book in BookFormView(mode: .edit(book)) }
```

Closing the sheet (Cancel, a swipe down, `dismiss()`) sets the destination back to nil. `.forgeFullScreenCover` and `.forgeNavigationDestination` work the same way; `$model.destination.forgeCase(\.edit)` and `.forgeIs(.add)` are the bindings underneath, for SwiftUI modifiers ForgeAppKit doesn't wrap. A case's computed property (`edit` above) is the one line per case this costs; there are no macros, so it builds with xtool on Linux.

## Loading: `ForgeLoad`

One value instead of a result, a busy flag and an error that can disagree:

```swift
public private(set) var results: ForgeLoad<[CatalogueBook]> = .idle

public func search(_ text: String) async {
    results = .loading
    results = await .capture { try await library.searchCatalogue(query: text) }
}
```

`capture` returns `.loaded`, `.failed` with the error described, or `.idle` when the work was cancelled. `ForgeLoadView` draws it with ForgeUI's state views, and offers Try Again only when the failure is retryable:

```swift
ForgeLoadView(model.results, loading: "Searching",
              retry: ForgeStateAction("Try Again", identifier: "search.retry") { Task { await model.search(text) } }) { books in
    List(books) { BookRow(book: $0) }
}
```

## Saving: `ForgeSubmission`

The busy Save button and its error, kept by the model:

```swift
public let saving = ForgeSubmission()

public func save() async -> Bool {
    await saving.run { try await library.updateBook(id: id, input: input) }
}
```

`run` ignores a second tap while it is working, sets `failure` if the work throws (never for a cancellation) and returns whether it succeeded. In a sheet, the form toolbar takes it whole, spinner and alert included:

```swift
.forgeFormToolbar(id: "bookForm", confirmTitle: "Add", canConfirm: model.canSave,
                  submission: model.saving, cancel: { dismiss() }) {
    Task { if await model.save() { dismiss() } }
}
```

Outside a sheet, use ForgeUI's `ForgeButton("Save", isWorking: model.saving.isWorking) { ... }` with `.forgeAlert(failure: $model.saving.failure)`.

## Form errors: `ForgeFieldErrors`

Which fields are wrong, keyed by the field names the Rust side uses (`forge_app::validation`), shown under each field by ForgeUI's `ForgeField`:

```swift
errors = ForgeFieldErrors(fieldErrors.map { ($0.field, $0.message) })

ForgeField("Title", message: model.errors["title"]) {
    TextField("Title", text: $model.title).accessibilityIdentifier("bookForm.title")
}
ForgeField("Pages", message: model.errors.message(for: "total_pages", otherwise: .help("Optional"))) { ... }
```

`fields` keeps the order they were reported in, so the first problem can be focused or announced; `summary` is every problem as one line each.

## What's not in 0.1

Navigation paths and tab selection as model state, toasts, undo, settings screens as data, and anything that needs a dependency: ForgeAppKit is about 600 lines of plain Swift, borrowing ideas (alerts as data, destination enums) from Point-Free's swift-navigation without depending on it, because its macros are slow and fragile to build with xtool on Linux.

## Building on Linux

`Model/` imports Foundation and Observation only; `Views/` is wrapped in `#if canImport(SwiftUI)`. So `swift test --filter ForgeAppKitTests` in `forgekit/` runs on Linux, and the SwiftUI side compiles wherever the iOS SDK is: with xtool on a Linux laptop, and on macOS CI (`.github/workflows/forgeui.yml`, which builds the `ForgeAppKit` scheme for an iPhone).
