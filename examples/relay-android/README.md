# Relay (Android, work in progress)

Relay's own Rust core (`examples/relay/rust`), **unchanged**, under a Compose app on Android.
It is being brought to parity with the iPhone app in stages; the gap list and the order are in
[relay-android.md](../relay/docs/relay-android.md). Results come from Linux tests and the CI
emulator: nothing has run on a physical Android phone yet.

Done: stage 1, the Devices experience. Send files or photos with a word code, receive with one,
trust the other device afterwards (or have Relay ask, or never), then send to it without a code;
trusted and revoked devices with their rights (Can Send to This Device, This Is a Person,
Introduces Devices, Can See My Devices, Can Open My Site), presence, Ask Again and Check Its
Devices Now; Revoke, Forget and Trust Again; the pairing policy, the Connections mode and the
servers; the history with open, copy, cancel and remove. Chats, rooms, notifications, the
Keystore key, app lock, QR codes, sites and voice messages come in later stages.

```
RelayApp (Compose, Material 3) → RelayModel → ForgeHost (host/ForgeHost.kt)
  → Generated Kotlin: uniffi.relay_core + uniffi.forge_app
  → rust/ (links examples/relay/rust into libforge_ffi.so) → forge-app runtime, iroh
```

- `RelayApplication` owns the host, the model and the network for the life of the process and
  reports the process lifecycle to the runtime, as ForgeKit does on iOS.
- `RelayModel.kt` is the iPhone app's `RelayModel.swift` in Kotlin: it holds no rules, only asks
  Rust and shows what it says. Plain Kotlin, so the JVM tests drive it.
- `RelayNetwork.kt` finds code senders and trusted devices with `NsdManager`, using the same
  service type and TXT keys as the iPhone (Bonjour) and the `relay` command (`RelayAdverts.kt`),
  so all three find each other. It only finds addresses: every connection is Rust's.
- `PickedFiles.kt` copies what the person picks (content URIs) into the cache, because Rust reads
  paths. Received files stay in Relay's private folder and open in other apps through a
  `FileProvider` (`ui/ReceivedFiles.kt`).
- The device key is `device.key` in Relay's private files, with backups turned off in the
  manifest, until stage 4 moves it into the Android Keystore.
- `RelayText.kt` has the iPhone app's words for every Rust error and every status.
- `host/`, `kit/` and `ui/theme/` are copied from Pocket Library's Android app; they move into a
  shared Android ForgeKit after this stage. `ForgeTokens.kt` is **generated** by
  `scripts/android-tokens.py` from ForgeUI's Swift tokens: don't edit it.
- Test tags are the iPhone app's accessibility identifiers (`relay.send`, `trustOffer.trust`,
  `trusted.revoke`, ...), exposed as resource ids.

## Building and testing

```bash
./scripts/package-android-library.sh examples/relay-android/Generated   # Rust for Android, Kotlin bindings
cargo build --manifest-path examples/relay-android/rust/Cargo.toml       # host build for JVM tests
cd examples/relay-android
./gradlew :app:testDebugUnitTest          # the model against the real Rust core
./gradlew :app:connectedDebugAndroidTest  # Compose tests on an emulator or phone
```

The JVM tests open two Relay devices in one process and send between them over loopback: a word
code transfer, trusting each other, then a file without a code. The emulator tests walk the
screens with throwaway data (`forge.temporaryData`) and never reach Relay's real servers.
