# App friction log: building the remaining demos

Started 2026-11-11. The remaining demo apps (FieldKit, Sketchbook,
Soundscape, Relay, in that order) are built one at a time, and each one
writes down where the framework got in the way. This log continues the
[showcase friction log](showcase-friction-log.md), which covers putting the
existing apps into Device Lab.

How it is used:

1. **Log as you go.** Each entry says what happened, how the app got round
   it, and what the framework should do instead.
2. **Work around in the app**, so the app keeps moving. The framework is not
   changed in the middle of an app.
3. **Review when an app is finished.** Each entry gets a recommendation (fix
   now, fix later, accept) and Reuben picks.
4. **"Fix now" lands before the next app starts**, and the next app is the
   check that the fix worked.
5. **Measure** each app: time from start to first run on the phone, and the
   lines needed to host it in Device Lab.

Severity: **blocker** (had to stop), **limit** (had to work around),
**annoyance** (cost time, nothing more).

## Summary

| # | App | Friction | Severity | Status |
|---|---|---|---|---|
| A1 | FieldKit | The template on main still names every library `forge_ffi`, so a new app can't start in the hostable shape | Limit | Waits for #167, which changes the template |
| A2 | FieldKit | `sha256_file` is public in forge-storage but not exported, and `import_hashed` drops the file extension | Limit | Open |
| A3 | FieldKit | SQLite can't store `u64`, so SQLite apps use `i64` ids and file-based apps use `u64` | Annoyance | Open |
| A4 | FieldKit | Great-circle distance lives in `forge_services::sensors`, so a pure Rust core depends on the native services crate | Annoyance | Open |
| A5 | FieldKit | The runtime has no way to read data the command thread owns without a command, and every command raises the revision | Limit | Open |
| A6 | FieldKit | No date formatting in the framework: an ISO 8601 writer by hand for GPX and GeoJSON | Annoyance | Open |
| A7 | FieldKit | `forge init FieldKit` names the library `field_kit_core`, and the template's folders and test target are still called `AppHost` | Annoyance | Open |
| A8 | FieldKit | `PermissionStatus::allows_use` is Rust only, so each app redefines `allowsUse` in Swift | Annoyance | Open |
| A9 | FieldKit | Feeding a native service's events into commands has no documented pattern: a listener queues a command and drops the future | Limit | Open |
| A10 | FieldKit | The location adapter can't keep recording in the background (no `allowsBackgroundLocationUpdates`, no background mode) | Limit | Open, needed by F3 |

## A1. A new app can't start in the hostable shape yet

**What happened.** FieldKit is meant to start from `forge init` in the shape
the showcase needs: a library named after the app and
`setup_scaffolding!()` with no argument. The template on main still names
every library `forge_ffi`, and the scripts on main only find
`libforge_ffi.a`.

**Workaround.** F1 is the Rust crate alone, written by hand in the new shape
(`[lib] name = "fieldkit_core"`, `crate-type = ["staticlib", "lib"]`). It
needs no scripts, so it builds and tests on main today. The Swift app comes
in F2, from the new template.

**Framework.** Already done on the showcase branch (#167). F2 will show
whether `forge init` plus the Device Lab README's four lines is all it takes.

## A2. Hashed media files lose their extension

**What happened.** Photos and voice memos are stored by content, so the same
file added twice is kept once. `FileStore::import_hashed` names the copy by
its SHA-256 with no extension. AVFoundation uses the extension as a hint for
the file type, so keeping `.m4a` was the plan: hash the file, then import it
as `<hash>.m4a`. But `forge_storage::sha256_file` is `pub` inside a private
module and not re-exported, so an app can't call it.

**Workaround.** `import_hashed`, no extension. F2 checks on the phone that
AVAudioPlayer plays the file anyway (Chat stores voice notes the same way).

**Framework.** Export `sha256_file` (and `is_sha256_hex`), or let
`import_hashed` keep the source's extension.

## A3. Two kinds of id

**What happened.** rusqlite has no `ToSql` for `u64`, so FieldKit's ids are
`i64`, like Commonplace's. Checklist and other file-based apps use `u64`. In
Swift that is `Int64` in some apps and `UInt64` in others.

**Workaround.** `i64` throughout FieldKit.

**Framework.** Say in [persistence.md](../persistence.md) that ids are
`i64`, and use `i64` in the template, so every app matches.

## A4. Distance needs the native services crate

**What happened.** Track totals need the great-circle distance. Forge has
one, `forge_services::sensors::distance_m`, which Device Lab uses. Using it
makes the pure Rust core depend on `forge-services`, the crate of native
service adapters.

**Workaround.** Depend on it. FieldKit needs the location service in F2
anyway, so it costs nothing yet.

**Framework.** Accept for now. If a second app needs geo maths without
native services, move `distance_m` and its friends into a small module or
crate of their own.

## A5. Reading data means a command, and a command announces a change

**What happened.** A trip's notes and its track live in SQLite, which the
command thread owns. The first version read them with `rt.command`, the only
way onto that thread. But every successful command raises the revision and
announces `Changed`, so a screen that reloads on `Changed` and then reads a
trip would announce another change, and loop. Checklist and Commonplace
don't hit this because everything they show fits in the published snapshot;
a long GPS track doesn't belong there.

**Workaround.** A second, read-only SQLite connection (WAL keeps it apart
from the writer), used as on-screen work with `rt.run` off the command
thread. A test checks that reading a trip leaves the revision alone.

**Framework.** Either a read-only call on the command thread that doesn't
raise the revision (`rt.read`), or a reader connection from
`forge_storage::Database` itself, documented in app-model-review.md as the
way to query data too large to publish.

## A6. No date formatting

**What happened.** GPX and GeoJSON need UTC times as ISO 8601. Forge gives
`now_ms()` but nothing to format it, and the app crates avoid date crates.

**Workaround.** A 15-line ISO 8601 writer in `export.rs`, with tests.

**Framework.** Accept for now. If a second app needs dates in Rust, add a
small `forge_runtime::time` with ISO 8601 in and out, or allow the `time`
crate in app crates.

## A7. Template names

**What happened.** The new template names the library from the app's name
split into words, so `FieldKit` becomes `field_kit_core`. FieldKit's crate
was already `fieldkit_core`, so the Package.swift from `forge init` needed
its C module renamed by hand. The template also still puts the app's Swift
in `Sources/AppHost` and calls the test target `AppHostTests`; only the
module names changed.

**Workaround.** Kept `fieldkit_core` and edited one line of Package.swift.

**Framework.** Let `forge init` take a `--library` name, and name the
folders and test target after the app too, so nothing in a new app says
`AppHost`.

## A8. `allowsUse` in Swift

**What happened.** Rust's `PermissionStatus::allows_use` isn't exported, so
Device Lab defines `allowsUse` in Swift, and FieldKit had to again.

**Workaround.** A two-line extension in TripsModel.swift.

**Framework.** Ship it once with the native services' Swift (or export it
from Rust).

## A9. From a native service's events to commands

**What happened.** FieldKit follows the location service while a trip
records. The watch listener runs on the host's thread and can't await, but
each fix has to reach the command thread, which owns the database. The
runtime queues a command when `rt.command` is called, so the listener calls
it and drops the future. That works, but nothing says it does, and the
listener needs a `Weak` to the core (`Arc::new_cyclic`) so it doesn't keep
the core alive.

**Workaround.** Exactly that, with tests on the fake location service
(follows while recording, stops on Stop, resumes after a relaunch, explains
denied location).

**Framework.** A `rt.post(name, command)` that queues without a future, and
a paragraph in app-model-review.md on feeding service events into commands.

## A10. No background location

**What happened.** A walk is recorded with the phone in a pocket, so
recording has to carry on with the screen off. The location adapter only
asks for "when in use" updates and doesn't set
`allowsBackgroundLocationUpdates`, and forge.toml has no way to say a
service needs the `location` background mode.

**Workaround.** None yet: F2 records while FieldKit is on screen, and says
so on the Record sheet.

**Framework.** An option on `LocationOptions` for background updates, which
the iOS adapter honours, plus the background mode in forge.toml. Planned for
F3.

## A11. Absolute checkout path in the Xcode project

**What happened.** `forge init` wrote the checkout's absolute path
(`/home/user/ixOS`) into the Xcode project's "Build Rust FFI" phase, even
though the app sits inside that checkout. The Xcode build on macOS CI then
failed with `cd: /home/user/ixOS: No such file or directory`.

**Workaround.** Set `REPO_ROOT` to `${SRCROOT}/../../..` by hand, as the
template and the other examples have it.

**Framework.** `forge init` should write a path relative to `${SRCROOT}` when
the app is inside the checkout, and keep the absolute path only for an app
outside it.
