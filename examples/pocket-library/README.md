# Pocket Library

An offline-first reading tracker for iPhone, built with Forge from Linux. Keep a list of the books you want to read, are reading and have finished; record the page you're on; see what you've read this week.

It is also Forge's acceptance test for real apps: it was created with `forge init`, owns its Rust crate, talks to SwiftUI only through generated UniFFI bindings, and is built and installed with xtool. [ACCEPTANCE.md](ACCEPTANCE.md) records what has been checked and how.

<!-- Screenshots go here once the app has run on a device: docs/images/pocket-library-*.png -->

## What it does

| Screen | What you can do |
|--------|-----------------|
| **Library** | Search by title or author, filter by status (Want to Read, Reading, Finished), sort by recently added, title, author or progress. Swipe right to start or finish a book, left to delete it (with confirmation); long-press for share and status. |
| **Add a book** | Three ways, from the **+** menu: search the Open Library catalogue and add a result with its cover and description; type the details in yourself; or import an EPUB file from Files (title, author, description, cover and an estimated page count are read from it; the book's text is not kept). |
| **Book** | Cover, progress bar, current page (type it or step it), status, start and finish dates, a 1–5 star rating, notes, description and the book's own reading history. Edit, share, delete. |
| **Activity** | What you're reading now, books finished this year, pages read in the last 7 days, average rating, and your reading history by day. |
| **Settings** | Default sort order; turn online catalogue search off entirely; storage used; export the whole library as JSON through the share sheet; erase everything. |

Everything except catalogue search works offline. Catalogue failures (offline, timeouts, server errors, malformed answers) are shown with what happened and leave the library untouched; an import whose cover or description cannot be fetched still adds the book and says what is missing.

## How it is built

```
examples/pocket-library/
  forge.toml                 [rust] path = "rust"
  rust/src/
    lib.rs                   the UniFFI API: Library, LibraryEvents
    domain.rs                reading rules, search/sort, stats, history (pure, no I/O)
    store.rs                 library.json, covers/, exports/ (forge-storage JsonFile)
    catalogue.rs             BookCatalogue trait, Open Library client (forge-net)
    epub.rs                  EPUB metadata reader (zip + quick-xml, pure Rust)
  Generated/                 Swift bindings (never edited by hand)
  Sources/AppHost/           SwiftUI: LibraryModel + one file per screen
  Tests/AppHostTests/        XCTest against the real Rust library (macOS)
```

**Rust owns** the book model and ids, validation, the reading rules, search, filtering and sorting, persistence and its schema version, the reading history, statistics, catalogue requests and parsing, EPUB parsing, typed errors and events. A few of the rules, all tested in `domain.rs`:

- Reaching the last page finishes a book; marking it Finished jumps to the last page; moving a page past 0 on a Want to Read book starts it.
- Progress updates on one book within an hour are one history entry ("Read 25 pages"), so stepping through pages does not flood the history. Corrections backwards are not counted as reading.
- Titles sort without "The", "A" and "An"; search ignores case and common accents.

**SwiftUI owns** navigation (a `TabView` with a `NavigationStack` per tab), lists, forms, sheets, confirmation dialogs, swipe actions, context menus, search fields, the document picker, the share sheet, remote images (`AsyncImage`), haptics, and all text shown to the user. `LibraryModel` is an `@Observable` class that calls Rust and re-reads its state after each command and on each `LibraryEvent`; it holds no rules of its own.

**Threads.** Every Rust method that touches the disk or network is `async` and runs on its own thread (`forge_runtime::background`), so SwiftUI awaits it from the main actor. Reads (`books`, `stats`, `history`) are plain memory reads.

**Persistence.** Every change is written (atomically) before the call returns, so the app can be suspended or killed at any time. Covers are saved under `covers/` and stored as file names, never absolute paths: iOS moves an app's container when it is reinstalled. A library written by a newer app version is refused and never overwritten; an unreadable one can be set aside (it is kept as `library.unreadable-<time>.json`) and the app started fresh.

**Networking.** HTTPS through `forge-net` (rustls with the iOS trust store), a 15-second timeout, and a User-Agent naming the app, as Open Library asks. The catalogue sits behind the `BookCatalogue` trait, so another service could replace it; the tests run the Open Library client against canned responses.

**Lifecycle.** The app keeps one `Library` for its whole life. When it returns to the foreground it re-reads its state (so "this year" and "last 7 days" are current). Nothing needs saving when it goes to the background. Switching away from a catalogue search drops its result in Swift; the Rust request runs to its timeout. In-flight request cancellation from Swift arrives with Forge's PR #30, and this app should adopt it then.

## Build and run from Linux

You need the Forge prerequisites for iOS on Linux: Rust, Swift 6.2, xtool with the Darwin SDK from Xcode 26, and an iPhone connected over USB ([docs/linux-setup.md](../../docs/linux-setup.md)). `forge doctor` checks all of it and says how to fix what is missing.

```bash
cd examples/pocket-library
cargo test --manifest-path rust/Cargo.toml     # Rust tests: no SDK or device needed
forge run ios                                  # build, install and launch (debug)
forge run ios --release                        # the same, Swift release configuration (PR #31)
```

`forge run ios` rebuilds the Rust library when anything in `rust/` (or the Forge crates it uses) changed, regenerates `Generated/`, then hands over to xtool. To work on the Rust API and see the new Swift API before building:

```bash
forge bindings generate --project .
```

On a Mac you can also open `Xcode/PocketLibrary.xcodeproj` or run the XCTest suite with `forge test --project .`.

The Rust crate's tests cover the reading rules, persistence across reopening, schema guards, corrupt-file recovery, catalogue parsing and failures, EPUB reading, events and shutdown. To try the EPUB reader on a book of your own:

```bash
POCKET_LIBRARY_EPUB=~/Books/some-book.epub cargo test --manifest-path rust/Cargo.toml real_epub -- --nocapture
```

## Not included

No accounts, sync or backend: the library lives on the phone (export it from Settings to keep a copy). No camera or barcode scanning. No iPad-specific layout or App Store packaging.
