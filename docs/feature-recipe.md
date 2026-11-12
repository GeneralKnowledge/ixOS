# Feature recipe: how a Pocket Library feature is built

Status: 2026-11-11. Written from Pocket Library at main `1b1e01a`, before it moves onto ForgeAppKit and the `forge-app` `Runtime` (roadmap P2 and P3). Update this page as part of that move: it is the specification for the feature template planned in P5 ([roadmap](roadmap.md#future-ideas-with-no-ai-needed)), so the template copies the patterns Pocket Library ends up with, not the ones it has today.

[first-app.md](first-app.md) adds one button to a starter app. This page takes a whole feature, **adding a book**, and lists every piece it touches, layer by layer, in the order you would write them. Each step names the file in `examples/pocket-library/` and the rule it follows.

## The pieces, in order

| # | Layer | File | What it holds |
|---|---|---|---|
| 1 | Rust record | `rust/src/model.rs` | `BookInput`, `Book`, `ReadingStatus`: plain `uniffi::Record`/`uniffi::Enum` data Swift sees, no rules |
| 2 | Rust rules | `rust/src/domain.rs` | `validate(BookInput) -> ValidInput` with `forge_app::Validator`, and `LibraryData::add`: the change, applied to a copy |
| 3 | Rust errors | `rust/src/error.rs` | `LibraryError`, including `InvalidFields` built from `forge_app::Invalid`, so each field's problem reaches Swift by name |
| 4 | Rust persistence | `rust/src/store.rs` | `Store::load`/`save` on `forge-storage`, `STORE_VERSION` and `migrate` for saved-version upgrades, fixtures in `rust/fixtures/` |
| 5 | Rust command | `rust/src/lib.rs` | `Library::add_book`: validate, then `commit_book("add-book", …)`, a `forge-app` runtime command that runs off the main thread, saves the copy and only then swaps it in, publishes the snapshot and sends `Changed` |
| 6 | Rust tests | `rust/src/tests.rs` | The rule, the refusal (nothing changes on an invalid input) and the round trip through a saved file |
| 7 | Bindings | `Generated/` | `forge bindings generate --project examples/pocket-library` (also run by every `forge build ios`); never edited by hand |
| 8 | View model | `Sources/AppHost/LibraryModel.swift` | `add(_:)` calls `addBook` through one `run` helper; Foundation and Observation only, so it compiles on Linux |
| 9 | Screen | `Sources/AppHost/BookFormView.swift` | A ForgeUI form; ForgeAppKit's `ForgeSubmission` (save in progress) and `ForgeFieldErrors` show `InvalidFields` under each field; every control has a `bookForm.*` identifier |
| 10 | Entry point | `Sources/AppHost/LibraryView.swift` | The `library.add` toolbar button and the destination that presents the form |
| 11 | Model tests | `Tests/AppHostTests/LibraryModelTests.swift` | The real model against the real Rust core; `forge check swift` runs them on Linux in seconds |
| 12 | UI scenario | `uitests/m0.toml` | `forge ui run` on the iPhone: open the form by identifier, type, check the state, screenshot, cancel |
| 13 | Requirements | `requirements.toml` | One sentence per behaviour (PL-4: invalid details are refused and change nothing), naming the tests that prove it |

## What each step must get right

1. **Records are data.** `model.rs` holds what crosses the boundary and nothing else. Optional fields are `Option`, enums are closed. ([ffi-policy.md](ffi-policy.md))
2. **Validation returns every field's problem at once.** `Validator` collects them; the form shows them all after one save, in plain language. Text that isn't a number is caught in Swift before it reaches Rust, because it never becomes a `u32`.
3. **A change is all or nothing.** The command works on a clone of the data, saves it, and replaces the live state only after the save succeeded. A failed save leaves the app exactly as it was. ([persistence.md](persistence.md))
4. **Work happens off the main thread.** Commands run one at a time on the runtime's command thread (`Runtime::command`), so the Swift call is `async` and a slow disk never blocks the UI; reads take the published snapshot without waiting. ([ffi-policy.md](ffi-policy.md))
5. **Other screens hear about it through events,** not by being told by the form. The library list reloads when ForgeKit's host says the revision moved (`host.changes`, from the runtime's `Changed` event).
6. **Errors are typed.** Swift matches `LibraryError.InvalidFields` and shows it under the fields; anything else becomes an alert through ForgeAppKit.
7. **Tests prove behaviour on Linux first.** Rust tests and model tests run without a Mac or phone; the UI scenario is the one step that needs the iPhone, and it proves the screen is reachable and reacts.

## What the template will generate, and what stays hand-written

The feature template (`forge add feature`, P5) will write steps 1 to 13 for a record with simple fields (text, whole number, date, choice, yes/no): one list, one detail and one create/edit screen, with validation stubs, tests and a scenario that pass as generated. It will then regenerate bindings.

It will not write domain rules beyond "required" and "at most N characters", relationships between records, network calls, or a screen's identity. Those stay in the app (AGENTS.md rule 1).

## Open before the template is written

- **Repositories and queries.** `LibraryData::query` is app code today. P3 plans repositories and queries in `forge-app`; the template's list screen waits for them.
- **A second app.** Extraction needs a second use (AGENTS.md rule 1). Check this recipe against a feature in a second app (Commonplace is the likely one) before writing the template.
