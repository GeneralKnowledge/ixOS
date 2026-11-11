# Showcase friction log: five apps in one

Started 2026-11-11. Device Lab became the showcase: an **Apps** button on its
dashboard opens Weather, Checklist, Pocket Library and the ForgeUI Showcase as
full apps, and **Open at Launch** on that screen makes Device Lab start in one
of them. The point was to see how hard it is to put apps together on ixOS, and
what that shows about the framework. This log is that answer. Each entry says
what happened, how the showcase got round it, and what the framework should do
instead.

The apps' own code is unchanged. Each app's Swift is linked into Device Lab
from its own folder through symlinks, and each app's Rust is compiled from
its own folder. The one change to the other apps is a single line in each of
their `lib.rs` files (F2).

## Summary

| # | Friction | Severity | Workaround in the showcase | Framework fix |
|---|---|---|---|---|
| F1 | Every app crate's library is called `forge_ffi`, so two app crates can't be linked into one library | Blocker | `rust/embedded/`: a second manifest for each app's source, with its own library name | Let the library name be the app's own; tools read it from Cargo |
| F2 | The UniFFI namespace is hard-coded as `"forge_ffi"` in every app | Blocker | `setup_scaffolding!()` with no argument (the namespace follows the library name) | Same, in the template |
| F3 | Bindings generation knew only one app module (plus ForgeKit's) | Blocker | `generate-bindings.sh` now makes a Swift module per linked crate and re-exports it from ForgeFFI | Keep it, and generate the Package.swift entries too |
| F4 | An app's Swift can't be used by another package | Blocker | Symlinks under `Sources/` | Split each app's Swift into a package product that doesn't contain the bindings |
| F5 | Two apps call their main target `AppHost` | Annoyance | Pocket Library's becomes `PocketLibraryApp` | Name targets after the app in the template |
| F6 | `ForgeLifecycle` can report to one host only | Limit | Only Checklist uses a ForgeKit host, so it's attached | Let ForgeLifecycle take several hosts |
| F7 | Five state models in four apps | Inconsistency | None needed; they coexist | Move Weather, Device Lab and Pocket Library onto ForgeHost |
| F8 | Three test-data conventions | Inconsistency | `test_data_args` lists two arguments | One argument, honoured by ForgeKit for every app |
| F9 | Info.plist settings belong to the app, not the feature | Limit | Merged by hand in `forge.toml` | Let a linked app declare its permissions and modes |
| F10 | Two location stacks | Duplication | None; both work | Move Weather onto the location native service (an open follow-up since #59) |
| F11 | Duplicated public helpers (`forgeLaunchAppearance`, `ContentView`) | Broke the first build | Device Lab applies the appearance in its ContentView | Ship `forgeLaunchAppearance` once, in ForgeUI |
| F12 | Dependencies listed twice | Drift risk | Comments; a missing one fails the build | Goes away with F1 |
| F13 | The Swift can't be compiled in a cloud session | Process | The laptop build and macOS CI check it | Known (AGENTS.md); no change |
| F14 | The new `import ForgeApp` in Device Lab's bindings | Side effect | Device Lab's ForgeFFI now depends on ForgeApp | None needed |

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

**Workaround.** `examples/device-lab/rust/embedded/<app>/Cargo.toml` compiles
each app's `rust/src/lib.rs` again as a library with its own name
(`weather_core`, `checklist_core`, `pocket_library_core`). The standalone apps
keep their manifests and their `forge_ffi` name, so they build exactly as
before (their committed bindings are unchanged).

**Framework fix.** The fixed name made sense when an app was one crate. It
means an app crate can never be reused. The template should name the library
after the app, and the scripts and `forge` should read the name from `cargo
metadata` instead of assuming `libforge_ffi`. That touches about 20 places in
`scripts/` and `crates/forge-cli` (`swift_check.rs` most of all), so it's a
change of its own, not part of this one.

## F2. The namespace is hard-coded

Every app calls `uniffi::setup_scaffolding!("forge_ffi")`. With no argument,
UniFFI uses the library name, which is still `forge_ffi` for the standalone
apps and becomes `weather_core` and so on in the showcase. That's the one
line changed in Weather, Checklist and Pocket Library. The template should
use the no-argument form too.

## F3. Bindings generation assumed one module

`generate-bindings.sh` kept `forge_ffi.swift` and threw away everything else
(it expected only `forge_app.swift`, which ForgeKit ships). Now each other
crate in the library gets:

- `Generated/<crate>/<crate>.swift`, a Swift module of its own, as forge_app
  is in ForgeKit;
- `Generated/<crate>FFI/`, its C module;
- a line in `Generated/ForgeFFI/ForgeFFI+Linked.swift`
  (`@_exported import weather_core`), so code that says `import ForgeFFI`
  sees every app's types and the apps' views needed no change.

`check-app-crates.sh` checks these files for drift as well.

Separate modules, rather than one big ForgeFFI, because each generated file
brings its own C types (`RustBuffer` and friends) and the one name clash found
is a real one: Pocket Library's `Cancellation` (from
`forge_runtime::app_shell!`) and ForgeKit's `Cancellation` (forge_app).

What's still manual: Package.swift needs a system library and a target for
each linked crate. `forge` could write those.

## F4. An app's Swift can't be reused

Each app's package has its own `ForgeFFI` target, which links its own Rust
library. Depending on `../weather` from Device Lab would bring a second
`ForgeFFI` and a second Rust library, and SwiftPM rejects the duplicate target
names anyway. So the showcase lists the other apps' folders as its own
targets, through symlinks in `examples/device-lab/Sources/`.

The ForgeUI Showcase shows how it should work: it has no Rust, so it is an
ordinary package with a library product, and Device Lab depends on it with one
line. An app's views and models should be a product like that, depending on a
bindings module the host app provides, not containing one.

## F5. Two targets called `AppHost`

Device Lab and Pocket Library both call their main target `AppHost` (and
their product `AppHostXtool`, and their tests `AppHostTests`). The showcase
compiles Pocket Library's folder as `PocketLibraryApp`. Nothing in Pocket
Library refers to its own module name, so this worked, but the template should
name targets after the app (Weather and Checklist already do).

## F6. One lifecycle host per process

`ForgeLifecycle.attach` replaces the previous attachment. Checklist is the
only embedded app on ForgeKit's host, so it gets the lifecycle and nothing is
lost today. When Pocket Library or Weather moves to `ForgeHost` (lifecycle
step 2 of the architecture assessment), a showcase needs several hosts
attached at once.

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

Each `embedded/<app>/Cargo.toml` repeats that app's dependencies. A missing
one fails the build at once; a different version is resolved by Cargo. It
goes away with the F1 fix.

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
| Rust: fmt, clippy, tests of Device Lab (with the embedded crates), Weather, Checklist, Pocket Library | Cloud, `check-app-crates.sh` | Pass |
| Bindings drift, including the linked crates | Cloud, `check-app-crates.sh` | Pass |
| Swift build with xtool (`forge build ios`), first try | Laptop | One error: F11 |
| Swift build with xtool, after the fix | Laptop, 159 s | Pass; 20 MB app bundle. Warnings only: the six Sendable warnings in `ForgeServices+Selected.swift`, which that file has on main too |
| Opening each app on the iPhone | Next sitting | Not run |
