# Pocket Library (Android, work in progress)

The iPhone app's own Rust core, **unchanged**, under a Compose app on an Android emulator. It is
being brought to parity with the iPhone app one milestone at a time, as a capped side track
([roadmap](../../docs/roadmap.md#android--work-in-progress)): parity means meeting the same
requirements (PL-1 to PL-10 in [requirements.toml](../pocket-library/requirements.toml)).

Done: stage 0 (the shared core, the Kotlin host, add, open, delete), A1 (typed navigation,
the process lifecycle, Kotlin counterparts of ForgeAppKit's patterns, the iPhone app's error
text), A2 (ForgeUI's tokens and look), A3 (the complete library: the full book form and
editing, progress, status and rating on a book's screen, search, filter and sort, and starting
an unreadable library fresh) and A4 (the online catalogue through Android's own HTTP stack,
with its on/off setting). Requirements met on the emulator: **PL-1** to **PL-8** and
**PL-10**. Everything else in [Pocket Library](../pocket-library/README.md) (EPUB, reader, read
aloud, the rest of Settings, backups, statistics) is iOS only for now.

```
PocketLibraryApp (Compose, Material 3, Navigation) → LibraryModel → ForgeHost (host/ForgeHost.kt)
  → Generated Kotlin: uniffi.pocket_library_core + uniffi.forge_app
  → rust/ (links examples/pocket-library/rust into libforge_ffi.so) → forge-app runtime
```

- `PocketLibraryApplication` owns the host and the model for the life of the process, and
  reports the process lifecycle (`ProcessLifecycleOwner`) to the runtime, as ForgeKit does on iOS.
- `kit/` holds the Android counterparts of ForgeAppKit (`ForgeFailure`, `ForgeAlert`,
  `ForgeLoad`, `ForgeSubmission`, `ForgeFieldErrors`) and their Compose views. They stay in this
  app until a second Android app needs them.
- `ErrorText.kt` has the iPhone app's words for every Rust error; failures, alerts and failed
  loads all use it.
- `ui/theme/` is the Forge look: `ForgeTokens.kt` is **generated** from ForgeUI's Swift tokens
  (`forgekit/Sources/ForgeUI/Tokens`) by `scripts/android-tokens.py`, so the palette, accents,
  text roles, spacing and radii match the iPhone app; `ForgeTheme.kt` turns them into a
  Material 3 colour scheme and type. Don't edit `ForgeTokens.kt`: change the Swift and run the
  script (`make check-android-tokens` fails when they differ). The app uses the orange accent,
  as on iOS.

- `rust/` is a thin crate: Pocket Library's crate has no `cdylib` (it would then be linked for
  iOS too), so this one links it into the library Android loads. Its one export,
  `setAsideUnreadableLibrary` (`uniffi.forge_ffi`), stands in for `Library.open_fresh` (below):
  `LibraryOpener` calls it and then opens the library as usual.
- `host/ForgeHttpTransport.kt` is Rust's HTTP transport on Android (the counterpart of the
  `URLSession` one, `ForgeServices+Http.swift`): `HttpURLConnection`, so requests get Android's
  proxy settings, trust store and network security config. The app installs it at launch with
  `installNativeHttp`; before that, and in JVM tests that don't install it, Rust uses its own
  `ureq` client. Not like iOS yet: no HTTP/3, and `PATCH` only where the platform allows it.
- `host/ForgeHost.kt` is the Android counterpart of ForgeKit's `ForgeHost`: it opens the core
  off the main thread, checks the runtime contract version, reports the lifecycle and re-reads
  when Rust announces a change. It stays in this app until a second Android app needs it.
- `uniffi.toml` renames the core's `Library` to `PocketLibrary` in Kotlin (it clashes with
  JNA's `Library`) and leaves out `Library.open_fresh`, which UniFFI 0.32 can't rename.
- Controls carry the iOS app's identifiers as test tags (`library.add.manual`,
  `library.book.<id>`, `library.filter`, `bookForm.title`, `bookDetail.page`,
  `bookDetail.markFinished`, `open.startFresh`, ...), exposed as resource ids. Menu choices
  add the choice: `library.filter.finished`, `library.sort.title`, `bookDetail.status.reading`.

## Run it (Linux)

You need JDK 17+, the Android SDK with NDK 28 (`ANDROID_HOME`, `ANDROID_NDK_HOME`) and
`cargo install cargo-ndk`; see [android-setup.md](../../docs/android-setup.md).

```bash
# Rust for the emulator (x86_64) and the Kotlin bindings
FORGE_ANDROID_ABIS=x86_64 ./scripts/package-android-library.sh examples/pocket-library-android/Generated

cd examples/pocket-library-android
./gradlew :app:installDebug                 # on a running emulator
./gradlew :app:connectedDebugAndroidTest    # Compose UI tests: PL-1 to PL-8, PL-10, screenshots
```

JVM unit tests run the host and model against the real core, with no emulator:

```bash
cargo build --manifest-path examples/pocket-library-android/rust/Cargo.toml
cd examples/pocket-library-android && ./gradlew :app:testDebugUnitTest
```

The UI tests start the app with the `forge.temporaryData` extra, the Android form of iOS's
`-ForgeTemporaryData`: each launch gets an empty, throwaway library, so your own books are never
touched. The catalogue tests install a stand-in transport in place of `ForgeHttpTransport`, so
they need no network and see every request Rust sends and every one it stops. `forge.keepTestData` (iOS `-ForgeKeepTestData`) keeps the last launch's throwaway
library, for a relaunch that checks something was saved, and `pocketLibrary.unreadableTestData`
(iOS `-PocketLibraryUnreadableTestData`) starts it as a file that isn't a library, to reach
Start Fresh. UI tests are named by requirement (`pl4_...`); a requirement counts as met on Android
only when its test passes on the CI emulator. `ThemeScreenshotTest` saves the library in light,
dark and at 200% text, and the requirement tests save the screens they check (`pl2-reading`,
`pl3-search`, `pl5-cannot-open`, ...), to `app/build/outputs/connected_android_test_additional_output/`; CI
uploads them as the `pocket-library-android-screenshots` artifact.

CI runs both on every push to main that touches Android or shared Rust (`.github/workflows/android.yml`,
job `pocket-library-emulator`). No physical Android phone has run it.
