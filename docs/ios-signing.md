# iOS signing and devices (xtool)

Forge Mobile does **not** store Apple credentials or signing material in `forge.toml`.

## Authentication

Use xtool’s documented flow:

```bash
xtool setup
# or
xtool auth
```

Supported modes (see xtool docs):

- **API key** — Apple Developer Program (recommended).
- **Password** — any Apple ID (private APIs; use at your own risk).

Credentials are sent to Apple by xtool, not stored in this repository.

## Free or paid team

xtool signs with the Apple account you sign in with. A free Apple ID can install development builds on your own devices (Apple limits these to 7 days and a few app IDs per week). Some capabilities are only provisioned for paid Apple Developer Program teams: push notifications, associated domains (universal links), Sign in with Apple, network extensions, personal VPN, multipath, SiriKit and Wi-Fi Aware.

Say which you use in `forge.toml`, and Forge checks entitlements before anything is built:

```toml
[ios.signing]
team_type = "free"        # or "paid"
team_id = "ABCDE12345"    # optional; checked against the Xcode project's DEVELOPMENT_TEAM
```

With `team_type = "free"`, asking for a paid-only entitlement fails with the entitlement's name. Left unset, Forge prints a warning instead. See [app-config.md](app-config.md#iossigning).

## Devices

```bash
xtool devices
```

Requires `usbmuxd` on Linux and USB access to the iPhone.

## Install and launch

Built artifacts and exact flags depend on the xtool version. Use:

```bash
xtool --help
xtool help install
xtool help launch
```

Forge CLI wrappers:

```bash
forge devices
forge install ios --ipa path/to/app.ipa
forge build ios --project examples/counter   # build only (xtool dev build)
forge run ios --project examples/counter     # build, install and launch (xtool dev run)
forge run ios --project examples/counter --release   # release configuration (xtool dev run -c release)
```

## Limitations

- App Store upload and full provisioning matrix are **out of scope** for the initial release.
- Cloud Agent VMs typically lack USB devices and cached SDKs — use a local Linux workstation or self-hosted runner for device validation.
