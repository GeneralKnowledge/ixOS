# Showcase friction log: eight apps in one

Started 2026-11-11. Device Lab became the showcase: an **Apps** button on its
dashboard opens Weather, Checklist, Pocket Library, Good Dog, Commonplace,
Chat and the ForgeUI Showcase as full apps, and **Open at Launch** on that screen makes Device Lab start in one
of them. The point was to see how hard it is to put apps together on ixOS, and
what that shows about the framework. This log is that answer. Each entry says
what happened, how the showcase got round it, and what the framework should do
instead.

The apps still build and run on their own. Device Lab depends on each app's
own Rust crate and its own Swift package. After a second round of fixes (see
"Second round" below), adding an app takes a package line, a crate line, a
`use` line and a row in the Apps list, with no copies, symlinks or aliases.

## Summary

| # | Friction | Severity | Status |
|---|---|---|---|
| F1 | Every app crate's library is called `forge_ffi`, so two app crates can't be linked into one library | Blocker | **Fixed**: the tools read the name from Cargo; hosted apps and new apps (`forge init`) name it after the app |
| F2 | The UniFFI namespace is hard-coded as `"forge_ffi"` in every app | Blocker | **Fixed**: `setup_scaffolding!()` with no argument, in hosted apps and the template |
| F3 | Bindings generation knew only one app module | Blocker | **Fixed**: each app's bindings come from its own `Generated/`; the host generates only its own |
| F4 | An app's Swift can't be used by another package | Blocker | **Fixed**: Device Lab depends on each app's own package |
| F5 | Five apps call their main target `AppHost` | Blocker once hosted | **Fixed**: forge.toml `[swift]` names the modules (`ChatFFI`, `ChatApp`); `forge init` names them after the app |
| F6 | `ForgeLifecycle` could report to one host only | Limit | **Fixed in ForgeKit**: one attachment per host |
| F7 | Five state models in four apps | Inconsistency | Open: move Weather, Device Lab and Pocket Library onto ForgeHost (roadmap) |
| F8 | Three test-data conventions | Inconsistency | **Fixed**: every app honours both `-ForgeTemporaryData` and `-ForgeUITestData` |
| F9 | Info.plist settings belong to the app, not the feature | Limit | **Fixed**: forge.toml `[host] apps` merges each hosted app's permissions, modes, schemes and test arguments |
| F10 | Two location stacks | Duplication | Open: move Weather onto the location native service (since #59) |
| F11 | Duplicated public helpers (`forgeLaunchAppearance`, `ContentView`) | Broke the first build | **Fixed** by `MemberImportVisibility` (Device Lab and the template); F15's plan removes the copies |
| F12 | Dependencies listed twice | Drift risk | Gone with F1 |
| F13 | The Swift can't be compiled in a cloud session | Process | Known; the laptop build checks it |
| F14 | The new `import ForgeApp` in Device Lab's bindings | Side effect | Gone: Device Lab no longer generates the other apps' bindings |
| F15 | Native services are per crate; Keychain, notifications, deep links and files are per process | Limit | Open: planned below ("Native services once per process") |
| F16 | Natural library names already taken (`chat_core`) | Annoyance | `chat_app`, `good_dog_app` |
| F17 | Counter shares its crate with the frozen Android app | Left out | Retire Counter, as planned |
| F18 | Module aliases reach only the targets a product's own target names | Broke two builds | **Gone**: no aliases since F5's fix |
| F19 | Every app's bindings declare the same public View helpers, and each has its own `ForgeDeepLinks` | Broke the third build | Helpers: **fixed** by `MemberImportVisibility`. Deep links: routed by Device Lab until F15's plan |
| F20 | Two crates with the same library name fail with Cargo's "extern location does not exist" | Confusing | **Fixed**: the build names both crates and says what to change |

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

## F18. Module aliases are hard to reach

Every app calls its bindings module `ForgeFFI`, so Device Lab renames each
one with SwiftPM `moduleAliases`. SwiftPM 6.2 applies an alias only to the
targets a product *depends on*, not to the targets it contains, and in
practice only to the targets the product's own target names: Weather's
`WeatherBridge`, one level further down, kept importing Device Lab's
`ForgeFFI` and failed with "missing required module 'forge_ffiFFI'". So
each `Apps/<App>` package exports one wrapper target, `<App>Embed`, that
re-exports the app and names every target of the app that imports
`ForgeFFI`. It cost two laptop builds to find. The real fix is F4/F5: give
each app's bindings and views a module name of its own, so no aliases are
needed.

## F19. The same View helpers in every app's bindings

The native services' Swift is copied into each app's `ForgeFFI`, so each
one declares `View.forgeOpenURLs()` and `onForgeDeepLink`. With four
bindings modules visible in Device Lab's AppHost, the call is ambiguous
(Swift extension members leak across files of a module), as F11 was for
`forgeLaunchAppearance`. Worse, each module has its own
`ForgeDeepLinks.shared`: a link fed to Device Lab's never reaches Chat.
Device Lab now calls `onOpenURL` itself and sends `forgechat://` links to
Chat's `ForgeDeepLinks`. Its own `receivesDeepLinks()` then clashed in the
App target with the public ones in Chat, Commonplace and Good Dog: a
module's extension members are visible wherever the module is loaded, even
through another module's imports. So the routing moved into ContentView, as
the launch appearance did (F11). Other public helpers declared by more than
one app wait to clash the same way: `forgeLaunchAppearance` (six apps),
`inlineNavigationTitle` and `groupedList` (Device Lab, Pocket Library),
`forgeLifecycle` and `errorAlert` (Chat, Good Dog). Swift 6's
`MemberImportVisibility` feature would scope them to the files that import
their module. Notification taps that carry a deep link go to
whichever app installed its notification adapter last (F15). The fix is
the same as F15's: native services' Swift in ForgeKit, once per process,
with one router that every app's Rust registers with.

## Second round: the fixes (2026-11-11)

Reuben asked to fix what the log found. What changed:

- **Module names** (F3, F4, F5, F18). forge.toml has a `[swift]` section:
  `bindings_module` and `app_module`, default `ForgeFFI` and `AppHost`.
  `forge bindings generate` writes the bindings under that name over the C
  module `<library>FFI`, keeping the file paths, and `forge check swift`
  uses the same names. The six hosted apps use `<App>FFI` and `<App>App`,
  every target in a product carries the app's name (SwiftPM rejects two
  packages with the same target name in a product, even one the host never
  uses), and the Rust library is linked from the app, launcher and test
  targets only, so a host links just its own. Device Lab depends on the
  apps' own packages: `Apps/`, the aliases and `Generated/Linked` are gone.
  Pocket Library's executable is now `PocketLibraryXtool`.
- **Swift setting** (F11, F19). `MemberImportVisibility` on Device Lab's
  targets and the template's: a file sees the members of only the modules
  it imports, so the same helper in two apps no longer clashes.
- **Template** (F1, F2, F5). `forge init MyApp` names the library
  `my_app_core`, calls `setup_scaffolding!()` with no argument, and names
  the Swift modules `MyAppFFI` and `MyAppApp`, so any new app can be hosted
  as it is.
- **Settings** (F8, F9). `[host] apps` in the host's forge.toml; both test
  data arguments honoured everywhere.
- **Library name check** (F20). `forge-crate.sh` compares the library
  names of the app's crate and its path dependencies before Cargo runs.

## Native services once per process (plan for F15, F19)

Today `generate-bindings.sh` copies Forge's Swift adapters (Keychain,
notifications, deep links, files, sharing, the optional services) into each
app's bindings module, because they implement protocols UniFFI generates in
that crate (`native_services!()` expands in the app's crate). In one process
that gives several copies: each app installs its own, the last one to
install gets notification taps, and each has its own `ForgeDeepLinks`.

The runtime already shows the way: `forge_app`'s contract types are a UniFFI
crate of their own whose Swift ships once, in ForgeKit's `ForgeApp` module.
Native services can do the same:

1. **forge-services exports its own interface.** The callback protocols and
   installers move from the macro into `forge-services` (namespace
   `forge_services`); the macro keeps only selecting optional services.
2. **ForgeKit ships the Swift once.** A `ForgeServices` module (its bindings
   and the core adapters) plus one small target per optional service
   (`ForgeServicesLocation`, ...), so an app still links only the frameworks
   it selects. `generate-bindings.sh` stops copying adapters.
3. **One install per process.** `ForgeServices.install()` once, from the
   app (or the host). One `ForgeDeepLinks`, one notification delegate: each
   app's views filter the links and notifications they handle.
4. **Apps move over** one at a time; the standalone apps behave the same.

That is two or three PRs and a ForgeKit release (which needs the laptop to
push), so it should follow this one. It also removes most of what is
copied into `Generated/` today.

## Third round: FieldKit (2026-11-13)

FieldKit was the first app written in the hostable shape from the start
(after #167), so hosting it took exactly the four steps in Device Lab's
README and no change to FieldKit: the Rust dependency and `use fieldkit_core
as _;`, the package and its `FieldKitApp` product, `"../fieldkit"` in
`[host] apps`, and an `EmbeddedApp` case plus its services install. Nothing
new for this log. F15 still applies: FieldKit installs its own copy of the
native services, which is fine for location (each crate has its own Rust
side) but means FieldKit's `ForgeDeepLinks` gets no links inside Device Lab;
it has no URL scheme or notifications today, so nothing is lost yet.

Every Forge example app is now hosted except Counter (F17) and Kataster,
which is a Bevy trial rather than a Forge app.

## What went well

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
| Seven apps: first build | Laptop | SwiftPM refused the package: conflicting `ForgeFFI` and `AppHost` targets (F18) |
| Seven apps: with wrapper targets | Laptop | Weather's `WeatherBridge` missed its alias (F18) |
| Seven apps: aliases fixed | Laptop | Every app compiled; Device Lab's AppHost failed on an ambiguous `forgeOpenURLs()` (F19) |
| Seven apps: deep links routed in AppHost | Laptop | The App target failed on an ambiguous `receivesDeepLinks()` (F19) |
| Seven apps: after the F19 fixes | Laptop, 39 s (incremental) | Pass; 36 MB app bundle (20 MB with four). Warnings only: the same Sendable warnings, now also in Chat's and Commonplace's `ForgeServices+Selected.swift` |
| Second round: `MemberImportVisibility` on Device Lab | Laptop, 34 s | Pass; with it, the original `forgeLaunchAppearance()` clash compiles |
| Second round: own module names, apps' own packages | Laptop | Pass: Device Lab (48 s, 36 MB) and all six apps on their own |
| Second round: `[host]` merge, template defaults | Laptop | Pass: Device Lab (Info.plist unchanged), Pocket Library, Good Dog, and fresh `forge init` and `forge init --minimal` apps (no warnings) |
| Fresh apps on Linux (`check-fresh-app.sh`) | Cloud | Pass |
| Opening each app on the iPhone | Next sitting | Not run |

On the iPhone, check first what Linux can't show: several apps' Rust cores
and logging starting in one process; the shared Keychain (each app's own
service name); notification taps (the last app to install its adapter gets
them, F15); and each app's data still there after switching apps and
relaunching.
