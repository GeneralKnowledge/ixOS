# Native services: iPhone check

This runbook checks the native services ([native-services.md](native-services.md)) on a real iPhone, from Linux with xtool. It takes about ten minutes. It uses a throwaway `forge init` app whose screen calls each service through the real Rust → Swift path.

**You need** the Linux iOS setup that already builds Counter, Weather and Chat: xtool, the Xcode 26 SDK, usbmuxd, and an iPhone plugged in and unlocked. `forge doctor` confirms it.

## 1. Create and install the check app

```bash
cd ~/ixOS            # your Forge checkout, on the branch under test
git pull
scripts/make-services-check-app.sh ~/forge-services-check
forge run ios --project ~/forge-services-check
```

The script runs `forge init`. It then adds the Rust checks to `rust/src/lib.rs` (`services_self_check`) and puts the check screen in place of the starter screen. It also registers the `forgecheck://` URL scheme in `forge.toml`. Finally it runs the app's Rust tests.

If the install hangs, unplug the iPhone, plug it back in and unlock it.

## 2. Run the checks

On the iPhone, open **Services check** and tap **Run checks**. Allow notifications when iOS asks. Expected lines:

| Line | Proves |
|------|--------|
| `PASS installed: ".../Library/Application Support"` | `ForgeServices.install()` ran at launch and Rust sees the app's directories |
| `PASS keychain write/read` and `PASS keychain delete` | Rust stored, read and removed a secret through the Swift Keychain adapter |
| `PASS notification permission: NotDetermined -> Granted` | The system prompt appeared and the answer reached Rust (on later runs: `Granted -> Granted`) |
| `PASS schedule notification` and `PASS pending notifications` | A notification was scheduled through `UNUserNotificationCenter` and is pending |
| `PASS rust rejects invalid triggers` | Validation happens in Rust before the platform is involved |
| `PASS write a file to share` | Rust wrote into Documents |

## 3. Notification and deep link

About five seconds after the run, a **Forge services check** banner appears. It shows whether the app is open or the phone is locked. Tap it.

- Section 2 should show `PASS forgecheck://check/notification?from=tap → route ["check", "notification"]`.
- Open Safari, type `forgecheck://check/url` and allow the switch to the app. A second `PASS` line appears.

## 4. Share and import

- Tap **Share the file Rust wrote**, then **Save to Files**. The file `services-check.txt` should open in the Files app with Rust's line of text.
- Tap **Import a file** and pick any file. A `PASS Rust read the imported file: N bytes` line appears in section 1.

## 5. Denied permission

In Settings → Services check → Notifications, turn notifications off. Go back to the app and tap **Run checks** again. Expected:

- `FAIL notification permission: Denied -> Denied`
- `FAIL schedule notification: permission denied: notifications are turned off ...`

This is the correct behaviour: Rust receives a typed `PermissionDenied`. **Open Settings** goes straight to the app's settings page.

## Report back

Send the screen contents (a screenshot is fine), plus anything that differed, the xtool version (`xtool --version`), the iOS version, and the result of each of steps 3 to 5.

To remove the app, delete it from the home screen, then run `rm -rf ~/forge-services-check`.
