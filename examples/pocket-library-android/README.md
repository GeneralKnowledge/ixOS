# Pocket Library (Android, work in progress)

The iPhone app's own Rust core, **unchanged**, under a Compose app on an Android emulator. It is
being brought to parity with the iPhone app one milestone at a time, as a capped side track
([roadmap](../../docs/roadmap.md#android--work-in-progress)): parity means meeting the same
requirements (PL-1 to PL-10 in [requirements.toml](../pocket-library/requirements.toml)).

Done: stage 0 (the shared core, the Kotlin host, add, open, delete), A1 (typed navigation,
the process lifecycle, Kotlin counterparts of ForgeAppKit's patterns, the iPhone app's error
text) and A2 (ForgeUI's tokens and look). Requirements met on the emulator: **PL-4** and **PL-10**. Everything else in
[Pocket Library](../pocket-library/README.md) (search, progress, catalogue, EPUB, reader, read
aloud, settings, backups) is iOS only for now.

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
  iOS too), so this one links it into the library Android loads. It exports nothing of its own.
- `host/ForgeHost.kt` is the Android counterpart of ForgeKit's `ForgeHost`: it opens the core
  off the main thread, checks the runtime contract version, reports the lifecycle and re-reads
  when Rust announces a change. It stays in this app until a second Android app needs it.
- `uniffi.toml` renames the core's `Library` to `PocketLibrary` in Kotlin (it clashes with
  JNA's `Library`) and leaves out `Library.open_fresh`, which UniFFI 0.32 can't rename.
- Controls carry the iOS app's identifiers as test tags (`library.add.manual`,
  `library.book.<id>`, `bookForm.title`, `bookDetail.delete`, ...), exposed as resource ids.

## Run it (Linux)

You need JDK 17+, the Android SDK with NDK 28 (`ANDROID_HOME`, `ANDROID_NDK_HOME`) and
`cargo install cargo-ndk`; see [android-setup.md](../../docs/android-setup.md).

```bash
# Rust for the emulator (x86_64) and the Kotlin bindings
FORGE_ANDROID_ABIS=x86_64 ./scripts/package-android-library.sh examples/pocket-library-android/Generated

cd examples/pocket-library-android
./gradlew :app:installDebug                 # on a running emulator
./gradlew :app:connectedDebugAndroidTest    # Compose UI tests: PL-4, PL-10, add/open/delete, screenshots
```

JVM unit tests run the host and model against the real core, with no emulator:

```bash
cargo build --manifest-path examples/pocket-library-android/rust/Cargo.toml
cd examples/pocket-library-android && ./gradlew :app:testDebugUnitTest
```

The UI tests start the app with the `forge.temporaryData` extra, the Android form of iOS's
`-ForgeTemporaryData`: each launch gets an empty, throwaway library, so your own books are never
touched. UI tests are named by requirement (`pl4_...`); a requirement counts as met on Android
only when its test passes on the CI emulator. `ThemeScreenshotTest` saves the library in light,
dark and at 200% text to `app/build/outputs/connected_android_test_additional_output/`; CI
uploads them as the `pocket-library-android-screenshots` artifact.

CI runs both on every push to main that touches Android or shared Rust (`.github/workflows/android.yml`,
job `pocket-library-emulator`). No physical Android phone has run it.
