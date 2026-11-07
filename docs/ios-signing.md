# iOS signing and devices (xtool)

Forge Mobile does **not** store Apple credentials or signing material in `forge.toml`.

## Authentication

xtool signs the app with your Apple account. Sign in once:

```bash
xtool auth login    # or `xtool setup`, which also installs the Darwin SDK
xtool auth status   # "Logged in." or "Logged out"
```

Two modes:

- **Password**: any Apple ID, including a free one. xtool uses Apple's private APIs for this; consider a secondary Apple ID.
- **API key**: an App Store Connect API key, which needs a paid Apple Developer Program membership.

Credentials go to Apple through xtool and are stored by xtool, never in `forge.toml` or this repository. `forge doctor` and `forge run ios` read `xtool auth status` and tell you when you are logged out.

## Free or paid team

xtool signs with the Apple account you sign in with. A free Apple ID can install development builds on your own devices (Apple limits these to 7 days and a few app IDs per week). Some capabilities are only provisioned for paid Apple Developer Program teams: push notifications, associated domains (universal links), Sign in with Apple, network extensions, personal VPN, multipath, SiriKit and Wi-Fi Aware.

Say which you use in `forge.toml`, and Forge checks entitlements before anything is built:

```toml
[ios.signing]
team_type = "free"        # or "paid"
team_id = "ABCDE12345"    # optional; checked against the Xcode project's DEVELOPMENT_TEAM
```

With `team_type = "free"`, asking for a paid-only entitlement fails with the entitlement's name. Left unset, Forge prints a warning instead. See [app-config.md](app-config.md#iossigning).

### Free Apple accounts

A free account works for development, with Apple's limits:

| Limit | What you see | What to do |
|-------|--------------|------------|
| 3 sideloaded apps per device at a time | "maximum number of apps for free development profiles" | Remove one of your other development apps from the iPhone |
| 10 new App IDs (bundle identifiers) per 7 days | "App ID limit" | Reuse a bundle id you already registered while experimenting |
| Apps stop opening after 7 days | The app crashes at launch or says it is no longer available | Run `forge run ios` again to re-sign |

A paid membership lifts the first two and extends the last to a year.

## The iPhone, the first time

1. **Connect and trust.** USB cable, unlock the phone, tap **Trust** on "Trust This Computer?". `forge devices` lists it (it uses `xtool devices --no-wait`, so it never hangs waiting).
2. **Developer Mode** (iOS 16 and later): Settings > Privacy & Security > Developer Mode, on, restart, confirm. Installs fail until it is on.
3. **Trust the developer profile** after the first install: Settings > General > VPN & Device Management, tap your Apple ID, **Trust**. Until then the app installs but does not open.

`usbmuxd` (Linux) must be running. If `forge devices` shows nothing while the phone is plugged in and unlocked: `sudo systemctl restart usbmuxd` and replug.

## Build, install and launch

```bash
forge doctor                  # "Run on an iPhone (Apple sign-in, connected device): ready"
forge run ios                 # build, sign, install (xtool dev run), from the app directory
forge build ios               # build only (xtool dev build)
forge run ios --release       # release configuration (xtool dev run -c release)
forge install ios --ipa App.ipa
```

Before building, `forge run ios` checks xtool, the iOS Rust target, the Darwin SDK and its iOS version, your sign-in and the connected iPhone, so these problems show up in seconds rather than after the build. When a signing or install step fails, the error ends with a "What went wrong" section naming the problem and the fix ([troubleshooting.md](troubleshooting.md#signing-and-iphone-problems)).

## Limitations

- App Store upload and full provisioning matrix are **out of scope** for the initial release.
- Cloud Agent VMs typically lack USB devices and cached SDKs — use a local Linux workstation or self-hosted runner for device validation.
