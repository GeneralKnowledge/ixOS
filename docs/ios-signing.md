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
forge run ios --project examples/counter
```

## Limitations

- App Store upload and full provisioning matrix are **out of scope** for the initial release.
- Cloud Agent VMs typically lack USB devices and cached SDKs — use a local Linux workstation or self-hosted runner for device validation.
