# Showcase friction log: eight apps in one

Started 2026-11-11. Device Lab became the showcase: an **Apps** button on its
dashboard opens Weather, Checklist, Pocket Library, Good Dog, Commonplace,
Chat and the ForgeUI Showcase as full apps, and **Open at Launch** on that screen makes Device Lab start in one
of them. The point was to see how hard it is to put apps together on ixOS, and
what that shows about the framework. This log is that answer. Each entry says
what happened, how the showcase got round it, and what the framework should do
instead.

The apps still build and run on their own, and their Swift is unchanged.
Device Lab depends on each app's own Rust crate and links its Swift from its
own folder through symlinks. The changes to the other apps are their library
names (F1, fixed in the framework) and one line in each `lib.rs` (F2).

## Summary

| # | Friction | Severity | Workaround in the showcase | Framework fix |
|---|---|---|---|---|
| F1 | Every app crate's library is called `forge_ffi`, so two app crates can't be linked into one library | Blocker | **Fixed in this PR**: the tools read the name from Cargo; a linkable app names its library after itself | Make it the template's default |
| F2 | The UniFFI namespace is hard-coded as `"forge_ffi"` in every app | Blocker | `setup_scaffolding!()` with no argument (the namespace follows the library name) | Same, in the template |
| F3 | Bindings generation knew only one app module (plus ForgeKit's) | Blocker | **Fixed**: each linked crate gets a complete ForgeFFI of its own (`Generated/Linked/<crate>/`) | `forge` could also write the small packages (F4) |
| F4 | An app's Swift can't be used by another package | Blocker | One small package per hosted app (`Apps/<App>/`, symlinks), imported with `moduleAliases` | Ship each app's Swift as a package |
| F5 | Five apps call their main target `AppHost` | Annoyance | Module aliases | Name targets after the app in the template |
| F6 | `ForgeLifecycle` could report to one host only | Limit | **Fixed in ForgeKit**: one attachment per host | — |
| F7 | Five state models in four apps | Inconsistency | None needed; they coexist | Move Weather, Device Lab and Pocket Library onto ForgeHost |
| F8 | Three test-data conventions | Inconsistency | `test_data_args` lists two arguments | One argument, honoured by ForgeKit for every app |
| F9 | Info.plist settings belong to the app, not the feature | Limit | Merged by hand in `forge.toml` | Let a linked app declare its permissions and modes |
| F10 | Two location stacks | Duplication | None; both work | Move Weather onto the location native service (an open follow-up since #59) |
| F11 | Duplicated public helpers (`forgeLaunchAppearance`, `ContentView`) | Broke the first build | Device Lab applies the appearance in its ContentView | Ship `forgeLaunchAppearance` once, in ForgeUI |
| F12 | Dependencies listed twice | Drift risk | Gone with the F1 fix | — |
| F13 | The Swift can't be compiled in a cloud session | Process | The laptop build and macOS CI check it | Known (AGENTS.md); no change |
| F14 | The new `import ForgeApp` in Device Lab's bindings | Side effect | Device Lab's ForgeFFI now depends on ForgeApp | None needed |
| F15 | Native services are per crate; Keychain, notifications, deep links and files are per process | Limit | Each app installs its own; its own Keychain service | Install services once for every crate |
| F16 | Natural library names already taken (`chat_core`) | Annoyance | `chat_app`, `good_dog_app` | — |
| F17 | Counter shares its crate with the frozen Android app | Left out | Not hosted | Retire Counter, as planned |

What went well is at the end.

## F1. Every app's library is called `forge_ffi`

**What happened.** The first try was the obvious one: a crate that depends on
the four app crates. Cargo failed with `extern location for weather does not
exist: .../libforge_ffi.rlib`. Every Forge app sets `[lib] name =
"forge_ffi"`, so Swift always sees `ForgeFFI` and the scripts always find
`libforge_ffi.a`. With `staticlib` and `cdylib` in `crate-type`, Cargo
doesn't add a hash to the file name, so the four libraries overwrite each
other. The error doesn't say that.

Renaming the libraries got past Cargo. The linker then failed on a duplicate
symbol, `UNIFFI_META_NAMESPACE_FORGE_FFI` (F2). Once both were fixed, all four
cores linked into one library.

**First workaround.** A second manifest per app (`rust/embedded/<app>/`)
compiled each app's `rust/src/lib.rs` again under another library name. It
worked, but every new app would have needed one, with its dependencies
copied (F12).

**Framework fix, done.** A crate's library may now carry its own name.
`scripts/forge-crate.sh` reads it from `Cargo.toml` (`LIB_NAME`), and
`forge check swift` reads it from `cargo metadata`. Whatever the name, the
library is staged as `libforge_ffi.a`, and `generate-bindings.sh` asks UniFFI
for that crate's Swift as the `ForgeFFI` module over `forge_ffiFFI` (a
per-crate config override; Kotlin likewise). So each standalone app's Swift
package, Xcode project and committed bindings stay as they were: only
internal symbol names and checksums differ. Weather, Checklist and Pocket
Library now name their libraries `weather_core`, `checklist_core` and
`pocket_library_core`, and Device Lab depends on their crates directly.

Linkable crates also drop `cdylib` from `crate-type`: as a dependency Cargo
would link it for iOS, which needs Apple's linker (the same trap forge-app
fell into in #143). `generate-bindings.sh` builds the host shared library
it needs with `cargo rustc --crate-type cdylib`.

Still to do: make an own name the template's default, so every new app can
be linked without a rename. Android apps keep `forge_ffi`.

## F2. The namespace is hard-coded

Every app calls `uniffi::setup_scaffolding!("forge_ffi")`. With no argument,
UniFFI uses the library name, which is still `forge_ffi` for the standalone
apps and becomes `weather_core` and so on in the showcase. That's the one
line changed in Weather, Checklist and Pocket Library. The template should
use the no-argument form too.

## F3. Bindings generation assumed one module

`generate-bindings.sh` kept `forge_ffi.swift` and threw away everything else
(it expected only `forge_app.swift`, which ForgeKit ships).

**First fix (four apps).** Each linked crate became a Swift module of its
own, re-exported by Device Lab's ForgeFFI (`@_exported import weather_core`),
so the apps' `import ForgeFFI` found their types. That held for Weather,
Checklist and Pocket Library. It would not hold for Chat, Good Dog and
Commonplace. Every crate that uses `forge_runtime::app_shell!`,
`native_services!` or `log_export!` exports the same Swift names
(`Cancellation`, `NativePermission`, `PermissionStatus`, `forgeLog`), and its
native services need their own Swift adapters, which declare the same public
types again (`ForgeServices`, `ForgePermissions`, `ForgeShareLink`, ...).
Re-exported into one namespace, all of those become ambiguous.

**Second fix (seven apps).** Each linked crate now gets a complete ForgeFFI
module of its own, as if it were its own app, in
`Generated/Linked/<crate>/`: its bindings, its C module, the Swift helpers,
its native services adapters and ForgeLog. The function that writes the main
app's ForgeFFI writes these too. `check-app-crates.sh` checks them for
drift. See F4 for how the host app uses them.

## F4. An app's Swift can't be reused

Each app's package has its own `ForgeFFI` target, which links its own Rust
library. Depending on `../weather` from Device Lab would bring a second Rust
library, and a second `ForgeFFI` and `AppHost`, names SwiftPM won't allow
twice.

**Fix.** Each hosted app is a small package of its own,
`examples/device-lab/Apps/<App>/Package.swift`, about 40 lines. Its
`Sources/` are symlinks to the app's own Swift folders, and `Generated` is a
symlink to `Generated/Linked/<crate>`. It links nothing: the Rust is in
Device Lab's library. Device Lab depends on each package with SwiftPM
`moduleAliases` (`["ForgeFFI": "ChatFFI", "AppHost": "ChatApp"]`), so every
app compiles exactly as it does on its own, under its own names, and Device
Lab sees `ChatApp.ContentView` and `ChatFFI.ForgeServices`.

The ForgeUI Showcase shows the ideal: with no Rust, it is an ordinary
package, and Device Lab depends on it with one line. If each app shipped its
Swift as a package like the ones under `Apps/`, the host would need no
symlinks.

## F5. Every app calls its main target `AppHost`

Device Lab, Pocket Library, Good Dog, Commonplace and Chat all call their
main target `AppHost` (and their product `AppHostXtool`). The module aliases
(F4) make this harmless, but the template should name targets after the app
(Weather and Checklist already do).

## F6. One lifecycle host per process

`ForgeLifecycle.attach` replaced the previous attachment, so only one
ForgeKit host could get the lifecycle. With Checklist and Commonplace both on
ForgeKit that lost Checklist's. **Fixed in ForgeKit:** `attach` keeps one
attachment per host (`detach(host)` and `detach()` remove them). For a
single-host app nothing changes. It ships with the next ForgeKit release.

## F7. Five state models in four apps

| App | State model | Owner of the Rust core |
|---|---|---|
| Device Lab | `@Observable` model polling a snapshot ten times a second | The root view's `@State` |
| Weather | `ObservableObject` / `@StateObject` | The view model, made per view |
| Checklist | `ForgeHost` + `@Observable` model, revision-driven | The App, for the process |
| Pocket Library | `@Observable` model with its own event loop | The root view's `@State` |
| ForgeUI Showcase | `@Observable` models, no Rust | — |

They coexist fine. What it means in practice: closing Weather's sheet throws
its model away, and opening it again starts from the cache, while
Checklist's core stays open the whole time. That is the application model
review's finding (#141), seen from the user's side.

## F8. Three test-data conventions

Checklist (ForgeKit) uses `-ForgeTemporaryData`, Pocket Library uses
`-ForgeUITestData`, and Weather has `-WeatherOfflineDemo` (no network, which
is a different thing). The showcase's `[ui_test] test_data_args` lists the
first two. ForgeKit's argument should be the only one, which happens when
Pocket Library moves onto ForgeHost.

## F9. App settings are per app

Location text, background modes and the like live in one `forge.toml`. The
showcase merged them by hand: the location text now mentions Weather, and
`background_modes = ["audio"]` is there for Pocket Library's read-aloud. A
linked app can't bring its own requirements. Fine for one showcase; it would
matter if apps were put together often.

## F10. Two location stacks

Weather has its own `CLLocationManager` wrapper (`LocationProvider`, with its
own `LocationFix` type); Device Lab uses the location native service. Both
run in one process and both work. Moving Weather onto the service has been an
open follow-up since #59.

## F11. Duplicated public helpers

`forgeLaunchAppearance()` is a public `View` extension in seven apps, copied
from the template. The first laptop build failed on it: Device Lab's App
calls `.forgeLaunchAppearance()`, and the Weather and Pocket Library copies
are visible there too (Swift sees extension members of every module in the
build, not only the ones a file imports), so the call was ambiguous. The only
Swift error in the build. Device Lab now applies the appearance inside its
own ContentView. The helper belongs in ForgeUI, once.

Public `ContentView` is the same kind of clash, in Device Lab and Pocket
Library; the Apps screen writes `PocketLibraryApp.ContentView()`.

## F12. Dependencies listed twice

The first workaround for F1 repeated each app's dependencies in a second
manifest. Gone with the framework fix.

## F13. No Swift compiler in the cloud session

The Rust side, bindings and their checks were all run on Linux in the cloud.
The Swift (symlinked targets, re-exports, the Apps screen) was first compiled
on the laptop with xtool. SwiftPM and xtool took the symlinked targets and
the per-crate modules without complaint. See the build notes below.

## F14. A new import in Device Lab's bindings

Linking Checklist and Pocket Library brings forge_app into the library, so
`generate-bindings.sh` adds `@preconcurrency import ForgeApp` to
`forge_ffi.swift` and Device Lab's ForgeFFI target now depends on ForgeKit's
ForgeApp product. That's correct, and costs nothing.

## F15. Native services are per crate

`forge_services::native_services!` keeps its installed services in the
crate that calls it. Good Dog, Commonplace and Chat each have their own Rust
side for them, so each needs its Swift adapters installed: Device Lab calls
`GoodDogFFI.ForgeServices.install(...)` and so on at launch
(`EmbeddedApps.install()`).

What is per process, not per app, collides. **Secrets**: the Keychain
service name defaults to the bundle id, so the showcase gives each app its
own (`<bundle>.chat`). **Notifications**: one
`UNUserNotificationCenter` delegate, so the last app to install wins.
**Deep links**: Chat's `forgechat://` links reach only the module whose
`forgeOpenURLs()` is on the root view, which is Device Lab's. **Files**: the
files service hands every app the same app directories. A framework that
expects to share a process would install services once, for every crate.

## F16. Library names already taken

Chat's and Good Dog's natural names, `chat_core` and `good_dog_core`, belong
to their shared crates (`examples/chat-core`, `examples/good-dog-core`). Their
app libraries are `chat_app` and `good_dog_app`. Library names live in one
namespace across the whole build.

## F17. Counter is left out

Counter's Rust crate is shared with the Android Counter app, whose scripts
load `libforge_ffi.so` by name and need a `cdylib`. Renaming it means
changing the Android build, which is frozen (WIP). Counter is also due to be
retired (app model review), so it stays out.

## What went well## What went well

- The Rust side composes once the names differ: four cores, two of them on
  forge-app, one with native services, linked into one library with no other
  change. One `forge_app` and one `forge_runtime` serve all of them.
- No public type in the four bindings clashed with another, apart from
  `Cancellation` (F3).
- The ForgeUI Showcase dropped in as a package dependency with no work.
- Data folders didn't collide: Application Support/`Checklist`,
  `PocketLibrary` and `Weather` were already distinct.
- The standalone apps are untouched: their committed bindings are identical
  after regeneration, and their Rust tests pass.
- One app slot on the phone instead of five.

## Build notes

| Check | Where | Result |
|---|---|---|
| Rust: fmt, clippy, tests of Device Lab (linking the other three), Weather, Checklist, Pocket Library | Cloud, `check-app-crates.sh` | Pass |
| Bindings drift, including the linked crates | Cloud, `check-app-crates.sh` | Pass |
| Swift build with xtool (`forge build ios`), first try | Laptop | One error: F11 |
| Swift build with xtool, after the fix | Laptop, 159 s | Pass; 20 MB app bundle. Warnings only: the six Sendable warnings in `ForgeServices+Selected.swift`, which that file has on main too |
| After the F1 framework fix: Device Lab, Weather, Checklist, Pocket Library each built with xtool | Laptop | All pass; each stages `libforge_ffi.a`; no new warnings |
| Opening each app on the iPhone | Next sitting | Not run |
