# Persistence guarantees

What Forge promises about an app's saved data, what each promise rests on, and the three things an app has to do itself. The primitives are in `forge-storage` (`JsonFile`, `Database`, `FileStore`); `forge-app`'s `Document` is a `JsonFile` with the runtime's error kinds. For the history behind this page see [foundation-review.md](foundation-review.md) (S4, S7, E2) and [architecture-assessment.md](architecture-assessment.md#2-persistence-guarantees).

## The promises

| Promise | How | Proved by |
|---|---|---|
| A write is whole or absent, after a crash **and** after a power cut | Write a temporary, sync it, rename it into place, sync the folder (`write_atomic`, `FileStore`); SQLite in WAL mode with transactions | `forge-storage/tests/foundation.rs`: `SIGKILL` mid-write, 30 rounds each for JSON, SQLite and files with records. Power loss cannot be simulated on Linux; the folder sync is what makes the rename durable |
| A saved record never names a missing or half-written file | Stage the file, save the record, commit (below) | `files_and_records_stay_consistent_across_kills` |
| Data from a newer build is never overwritten | `JsonFile` and `Database` refuse a higher schema version | `newer_file_is_reported_and_never_overwritten`, `a_newer_file_is_refused_and_left_alone` |
| Unreadable data is never replaced silently | `JsonFile::save` refuses a corrupt file; the app starts again only on request (`Document::start_fresh`, `set_aside`). SQLite moves a file aside only when SQLite itself says it is damaged | `a_corrupt_file_is_replaced_only_on_request`, `a_locked_database_is_not_recovered` |
| An upgrade can be undone | The first write in a new schema keeps the old file beside it: `library.v1.json`, `app.v3.db` | `the_first_save_after_a_migration_keeps_the_old_file`, `creates_migrates_and_reopens` |
| Recovery copies do not fill the disk | Only the newest three `*.unreadable-*` and `*.corrupt-*` copies of a file are kept | `set_aside_keeps_the_newest_three`, `only_the_newest_damaged_copies_are_kept` |
| Version N's data opens in N+1 | Each app keeps a fixture per schema version it shipped, and a test opens them all | `every_saved_version_still_opens` in Pocket Library and Checklist |

Not promised: two processes writing one store (the runtime's `DataDirectory` lock refuses a second holder), and a migration that loses data on purpose (that is the app's migration function). Backup exclusion and file protection per folder are not built yet (S5): today every file is backed up and gets the app-wide `data_protection` class from `forge.toml`.

## What an app does

### 1. A file and the record that names it: stage, save, commit

```rust
let cover = covers.stage_write("book-17.jpg", &bytes)?; // the file, synced and in place
library.save(&next)?;                                    // the record that names cover.name()
cover.commit();                                          // the file belongs to the record now
```

If the record can't be saved, the `?` returns early and drops `cover`, which deletes the file. A staged file never replaces an existing one (`FileStoreError::Exists`), so a record that is already saved never has its file changed under it. Pick a fresh name for new bytes. The launch sweep also leaves staged files alone. To remove a file, go the other way: save the record without the name, then call `FileStore::remove`.

| The process dies… | What is on disk | At the next launch |
|---|---|---|
| while the file is written | a hidden `.partial-*` temporary | the sweep removes it |
| after staging, before the record is saved | a file no record names | the sweep removes it |
| after the record is saved, before `commit` | the file, named by the record | nothing to do; `commit` only releases the in-memory guard |
| after the record drops a name, before `remove` | a file no record names | the sweep removes it |

Content-named files (`FileStore::import_hashed`, `accept_verified`) are immutable by their names, so they need no staging. Chat's attachments use them.

### 2. Sweep at launch

After loading the records, before any command runs, call `FileStore::sweep(|name| records_name(name))` on each store whose files are named by records. A store whose records are too costly to list (Chat's attachments) calls `sweep_temporaries` instead. Pocket Library does both in `Store::remove_orphan_covers`.

### 3. Keep a fixture per schema version

Put a file in `rust/fixtures/` exactly as each released schema version wrote it (`library-v1.json`), and a test that opens every fixture with today's code and fails when the current version has none. When a schema version goes up, add the new fixture before release and never edit an old one. See `examples/pocket-library/rust/fixtures/` and `examples/checklist/rust/fixtures/`.

Fixtures prove the code reads old files, on Linux. They don't replace a release's update check on the iPhone (install version N, add data, update to N+1, and check the data survives). That stays one Device Lab check per release.

## Schema changes

- **JSON** (`JsonFile`, `Document`): additive changes use `#[serde(default)]` and keep the version. A change that old files can't be read as raises it and adds a step to the migration function. The migrated value is written at the next save, after the `.v<N>` copy is kept.
- **SQLite** (`Database::open(path, migrations)`): append a SQL step. All pending steps run in one transaction after the `.v<N>` copy is made with `VACUUM INTO`, so a failed migration leaves the file as it was.
