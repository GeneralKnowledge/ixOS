# Rust application foundations

Two small modules in `forge-app` for the rules every app's Rust side writes, and gets subtly wrong when it writes them alone: saving its state, and refusing bad input in a way the screen can show field by field. Both are plain Rust, tested on Linux with `cargo test`, and optional: an app with a database, or with nothing to save, ignores them.

They sit beside the runtime ([app-model-review.md](app-model-review.md)), not inside it: `Runtime<S>` still runs commands over whatever state the app keeps.

## `forge_app::document`: saved state as one file

```rust
use forge_app::{DataDirectory, Document, DocumentError, Runtime};

let directory = DataDirectory::open(&data_directory)?;
let notes = Document::<Notes>::open(directory.path().join("notes.json"), NOTES_VERSION)?;
let rt = Runtime::with_directory("notes", directory, notes);

// A command: edit a copy, save it, keep it.
rt.command("add", move |doc| doc.change(|notes| notes.add(text))).await
```

What it guarantees:

| Rule | How |
|---|---|
| A failed command changes nothing | `change` edits a copy and keeps it only after the save succeeded, as `Runtime::command` requires |
| Saves are atomic | `forge_storage::JsonFile` (write to a temporary, sync, rename) |
| A file from a newer app version is never touched | Opening fails with `DocumentError::Newer`; it is never saved over or set aside |
| An unreadable file is never destroyed | Opening fails with `DocumentError::Unreadable`; only `Document::start_fresh`, after the person agrees, renames it to `<name>.unreadable-<time>.json` and starts empty |
| Old files keep opening | New fields use `#[serde(default)]`; a change old files can't be read as raises the version and adds a step to `open_with_migration` |

`DocumentError::kind()` gives the framework `ErrorKind` (`Corrupt`, `NewerData`, `Internal`), and `can_start_fresh()` says whether the app may offer "Start a New Library". Map it into your own error with one `From` impl.

The type `T` is your app's saved state: `Serialize + Deserialize + Clone + Default`. If the saved form differs from the in-memory one, keep a separate saved struct and use `#[serde(from = "Saved", into = "Saved")]` (Checklist does).

## `forge_app::validation`: field-by-field errors

```rust
use forge_app::{FieldError, Invalid, Validator};

fn validate(input: &BookInput) -> Result<(), Invalid> {
    let mut v = Validator::new();
    v.required("title", &input.title, "Enter a title.")
        .max_chars("title", &input.title, 200, "Keep the title under 200 characters.")
        .in_range("rating", input.rating, 1..=5, "Give 1 to 5 stars.");
    v.check("current_page", input.total_pages.is_none_or(|t| input.current_page <= t),
            "The current page is past the last page.");
    v.finish()
}
```

- Each field reports its first problem only, in the order found, so the person sees the first thing to fix.
- Field names are yours; use the input record's field names so Swift can match them.
- Messages are sentences for the person, written in Rust, because Rust owns the rules.

`FieldError { field, message }` is a UniFFI record in the shared contract, so it can appear in your app's API (`InvalidInput { fields: Vec<FieldError> }`) and its Swift ships in ForgeKit. On the Swift side, ForgeAppKit turns the list into messages under each field:

```swift
catch LibraryError.InvalidInput(let fields) {
    errors = ForgeFieldErrors(fields.map { ($0.field, $0.message) })
}
// ...
ForgeField("Title", message: model.errors["title"]) { TextField(...) }
```

See [forgeappkit.md](forgeappkit.md#form-errors-forgefielderrors).

## Not yet

A repository or query layer, SQLite documents, and storage classes (backup, protection): phase 4 of the [app model plan](app-model-review.md#part-3-phased-plan). They are extracted when a second app needs the same shape.
