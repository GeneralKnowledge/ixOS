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

xtool signs with one team: the one chosen at `xtool auth login`. An Apple ID always has a free personal team, and an Apple Developer Program membership adds a paid team to the same Apple ID. Renewing or joining the program does **not** switch xtool to the paid team; you choose it when you sign in.

| | Free personal team | Paid Developer Program team |
|---|---|---|
| Apps on a phone | 3 sideloaded at a time | No limit |
| New App IDs | 10 per 7 days | No weekly limit |
| How long a signed app opens | 7 days | Up to a year, never past the membership's end |
| Paid-only entitlements (push notifications, associated domains, Sign in with Apple, network extensions, personal VPN, multipath, SiriKit, Wi-Fi Aware) | xtool leaves them out when it signs | xtool turns on the matching App ID capability from the app's entitlements |
| Sign-in | Apple ID and password | Apple ID and password, or an App Store Connect API key |

### Which team is xtool using?

```bash
forge doctor
#   Signing team: Someone Ltd (ABCDE12345), paid Apple Developer Program team
#     Paid team: no 3-app limit, profiles last up to a year (never past the membership)
xtool ds teams list    # every team this Apple ID can sign with
```

Forge reads the team ID from `xtool auth status` and the team's memberships from `xtool ds teams list`, using xtool's own rule: a team with the Apple Developer Program for iOS is paid, any other is free. The listing asks Apple, so offline the kind shows as unknown. An API key sign-in is always a paid team. `forge doctor --json` has the same in `ios_device.signing_team` (`id`, `name`, `kind`, `sign_in`); the Apple ID itself is not shown.

`forge run ios` checks the same thing before it builds:

- it prints `Signing with …`, and `--json` returns it as `data.signing_team`;
- it warns when `forge.toml`'s `team_type` disagrees with the team xtool uses (for example `team_type = "paid"` while xtool is on the personal team), or is unset;
- it stops before the build when the team is free and `forge.toml` asks for a paid-only entitlement, because xtool would leave it out and the feature would silently not work;
- if the install still fails with a free-team limit (3 apps, 10 App IDs) for an app that should be signed by a paid team, the error says that xtool is probably signed in with the wrong team.

### Say which team the app expects

```toml
[ios.signing]
team_type = "free"        # or "paid"
team_id = "ABCDE12345"    # optional; checked against the Xcode project's DEVELOPMENT_TEAM
```

With `team_type = "free"`, asking for a paid-only entitlement fails with the entitlement's name. Left unset, Forge prints a warning instead. See [app-config.md](app-config.md#iossigning).

### Free team limits

| Limit | What you see | What to do |
|-------|--------------|------------|
| 3 sideloaded apps per device at a time | "maximum number of apps for free development profiles" | Remove one of your other development apps from the iPhone |
| 10 new App IDs (bundle identifiers) per 7 days | "App ID limit" | Reuse a bundle id you already registered while experimenting |
| Apps stop opening after 7 days | The app crashes at launch or says it is no longer available | Run `forge run ios` again to re-sign |

### Moving to a paid team

1. `xtool ds teams list` should show the paid team with an `Apple Developer Program (ios)` membership. If it doesn't, the membership is not active for this Apple ID yet.
2. `xtool auth logout`, then `xtool auth login`; it asks which team to use when there are several: pick the paid one.
3. `forge doctor` should now say `paid Apple Developer Program team`.
4. Set `team_type = "paid"` in each app's `[ios.signing]`, so Forge stops refusing paid-only entitlements.
5. Install once with `forge run ios`. Apps already on the phone stay signed by the old team until they are reinstalled; a free-team install still expires after 7 days.

App IDs keep xtool's `XTL-<id>.` prefix on either kind of team, so the bundle id on the phone changes when the team changes: the paid team's copy installs next to the free team's copy as a separate app with its own data.

With a paid team:

- Each connected iPhone is registered with the team. A team can register 100 iPhones a year, and a registration is only freed when the membership renews.
- Development profiles last a year, but never longer than the membership. If it lapses, every app signed by the team stops opening.
- An App Store Connect API key is an alternative to the Apple ID sign-in. Keep the key file outside any project.
- The WebDriverAgent runner for UI tests (`forge ui setup`, #61) is signed the same way, so it no longer takes one of three slots or needs reinstalling weekly. With `--team paid` (or `team_type = "paid"`), `forge ui setup` stops the free-team slot warnings.

Not built: Forge does not make distribution builds, TestFlight uploads or App Store submissions. A paid team makes them allowed by Apple, but nothing in Forge or xtool's development commands produces them.

**Status:** team detection is tested against recorded xtool output only. Signing with a paid team has not yet been run with Forge; the first check is step 3 of the [iPhone session](testing/iphone-session-2026-11-07.md).

## The iPhone, the first time

1. **Connect and trust.** USB cable, unlock the phone, tap **Trust** on "Trust This Computer?". `forge devices` lists it (it uses `xtool devices --no-wait`, so it never hangs waiting).
2. **Developer Mode** (iOS 16 and later): Settings > Privacy & Security > Developer Mode, on, restart, confirm. Installs fail until it is on.
3. **Trust the developer profile** after the first install: Settings > General > VPN & Device Management, tap your Apple ID, **Trust**. Until then the app installs but does not open.

`usbmuxd` (Linux) must be running. If `forge devices` shows nothing while the phone is plugged in and unlocked: `sudo systemctl restart usbmuxd` and replug.

## Build and install

```bash
forge doctor                  # "Run on an iPhone (Apple sign-in, connected device): ready"
forge run ios                 # build, sign, install (xtool dev run), from the app directory
forge build ios               # build only (xtool dev build)
forge run ios --release       # release configuration (xtool dev run -c release)
forge install ios --ipa App.ipa
```

Before building, `forge run ios` checks xtool, the iOS Rust target, the Darwin SDK and its iOS version, your sign-in and the connected iPhone, so these problems show up in seconds rather than after the build. When a signing or install step fails, the error ends with a "What went wrong" section naming the problem and the fix ([troubleshooting.md](troubleshooting.md#signing-and-iphone-problems)).

## Limitations

- App Store upload, TestFlight and distribution signing are **not built**.
- Cloud Agent VMs typically lack USB devices and cached SDKs — use a local Linux workstation or self-hosted runner for device validation.
