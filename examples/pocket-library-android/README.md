# Pocket Library (Android, stage 0)

Stage 0 of the Android plan ([roadmap](../../docs/roadmap.md#android--work-in-progress)): the
iPhone app's own Rust core, **unchanged**, under a small Compose app on an Android emulator. It
proves that "the Rust core is shared" holds for a real app, not only for Counter.

What it does: the library list, adding a book (title and author), opening it, and deleting it.
Everything else in [Pocket Library](../pocket-library/README.md) (catalogue, EPUB, reader,
progress, settings, backups) is iOS only for now.

```
PocketLibraryApp (Compose, Material 3) → LibraryModel → ForgeHost (host/ForgeHost.kt)
  → Generated Kotlin: uniffi.pocket_library_core + uniffi.forge_app
  → rust/ (links examples/pocket-library/rust into libforge_ffi.so) → forge-app runtime
```

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
./gradlew :app:connectedDebugAndroidTest    # the Compose UI test (add, open, delete)
```

JVM unit tests run the host and model against the real core, with no emulator:

```bash
cargo build --manifest-path examples/pocket-library-android/rust/Cargo.toml
cd examples/pocket-library-android && ./gradlew :app:testDebugUnitTest
```

The UI test starts the app with the `forge.temporaryData` extra, the Android form of iOS's
`-ForgeTemporaryData`: an empty, throwaway library, so your own books are never touched.

CI runs both on every push to main that touches Android or shared Rust (`.github/workflows/android.yml`,
job `pocket-library-emulator`). No physical Android phone has run it.
