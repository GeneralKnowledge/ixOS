# Over-the-air installs

`forge publish ios` builds your app, signs it, and adds it to one Install page on your tailnet. On the iPhone you open the page in Safari and tap **Install**. Builds never replace each other on the page: each one is listed under its app with the branch it came from, its commit and when its signature runs out.

**Parked (2026-11-16): with a free Apple team, an install from the page has never been seen to put a new build on the phone.** Install builds over the cable with `forge run ios` instead (it builds and signs before it looks for the phone, so no build waits on it). The page and `forge publish ios` stay, because a paid team may make them work, but don't rely on them for testing.

## What was seen with a free team

All on Cati (iPhone 15 Pro Max, iOS 26.3.1), with builds signed by a free team's development profile that listed the phone:

- **A new app** (one not on the phone): the phone downloads the whole `.ipa`, then shows "Unable to Install … Please try again later" and leaves a grey icon with a cloud badge (2026-11-15).
- **An update** to an app installed by cable: the phone fetches the manifest and the whole `.ipa` (the server log shows every byte served to `appstored`), shows no error, and keeps the old app. Device Lab's **Apps > This Build** still shows the cable build's number after a full relaunch. This happened with a compressed and an uncompressed `.ipa`, both valid (`unzip -t`, same bundle id, profile listing the phone, signature not expired) (2026-11-16).
- **The install does replace the provisioning profile.** `forge device apps` reports the newest profile's expiry, so after an attempt from the page it shows the new build's date even though the app wasn't replaced. Earlier notes (2026-11-14 and 2026-11-15) took that date as proof that updates from the page worked; they hadn't checked the build number, so treat those results as unproven.

The likely cause is the free team itself. A free (personal) team gets only short-lived development profiles, and sideloading tools built on free teams (AltStore, SideStore, xtool) install through the phone's installation service over USB or Wi-Fi sync, never through `itms-services` links. No Apple document found says this outright, and the phone's install log during a failed update hasn't been captured yet, so it isn't proven. Over-the-air pages are the normal route with a paid team's ad hoc or development profiles; that is untested here ([ios-signing.md](ios-signing.md)).

## How it works

1. Forge builds the Rust library and runs `xtool dev build --sign --ipa`. xtool signs with your team's development profile, which lists the devices registered on the team, so the phone does not need to be connected.
2. Forge copies the `.ipa` into its publish folder (`~/.local/share/forge/publish`, or `FORGE_PUBLISH_DIR`) as a new build (`<bundle id>/<time>-<commit>/`), compressing its files on the way (xtool stores them uncompressed; Device Lab's download drops from about 100 MB to about 30 MB), writes the install manifest iOS reads for it, and rewrites `apps.json` and the page. The page lists each app, the most recently published first, as a card with its icon (taken from the `.ipa` when the app has one, `[ios] icon` in forge.toml; otherwise its initial on a coloured tile), its newest build's branch, commit, age, size and days left on its signature, and earlier builds folded away under **N earlier builds**. With more than four apps it adds a search box. The icon also shows on the Home Screen while the app downloads. It keeps the newest 5 builds of each app (`--keep`) and deletes older ones.
3. A small file server (`forge publish serve`, started in the background) serves the page, `apps.json` and each build's manifest, `.ipa` and icon on `127.0.0.1:47411` only, and nothing else in the folder.
4. Unless that port already serves something else, `tailscale serve` puts it on HTTPS at `https://<this computer>.<tailnet>.ts.net:8443/`, with a certificate the iPhone trusts. Only devices on your tailnet can reach it. Forge never uses Tailscale Funnel, which would put it on the public internet.
5. The page's **Install** button is an `itms-services://` link to the manifest. iOS downloads the `.ipa` and installs it.

## One-time setup

On the computer that builds (the laptop):

```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
sudo tailscale set --operator=$USER    # lets forge run `tailscale serve` without sudo
forge publish enable                   # keep the page up after a reboot (a systemd user service)
```

`forge publish enable` runs the server as a systemd user service. Run it from an installed `forge` (`cargo install --path crates/forge-cli`), not a build inside a checkout that may go away. If it says so, run `sudo loginctl enable-linger $USER` once, so the service runs when nobody is logged in. `tailscale serve --bg` already survives a reboot by itself.

In the [Tailscale admin console](https://login.tailscale.com/admin/dns), turn on **MagicDNS** and **HTTPS Certificates**.

On the iPhone, install **Tailscale** from the App Store and sign in to the same tailnet. The phone must already trust your developer certificate. If you've opened an app from `forge run ios` before, it does. If not, go to Settings > General > VPN & Device Management and trust it once.

## Which build is on the phone

Every build `forge publish ios` makes, and every build `forge run ios` installs over the cable, gets a new, higher build number (CFBundleVersion: the app's `build_number`, then the minute it was built, such as `1.29911107`), so iOS always treats an install as an update. Its Info.plist also records the commit (`+changes` if the checkout had uncommitted changes), the branch and the time (`ForgeBuildCommit`, `ForgeBuildBranch`, `ForgeBuildDate`). The stamp is only for that build: Forge puts the generated Info.plist back afterwards, so the checkout stays clean.

The page shows each build's version and build number. In the app, ForgeKit's `ForgeBuildInfo.current` reads the same keys (`versionLine`, `sourceLine`), so an app can show which build is running. Device Lab shows it under **This Build** on its Apps screen. This number, not the expiry `forge device apps` shows, tells you which build is running.

## Publishing a build

```bash
forge publish ios --project examples/device-lab
```

It prints the page's address, for example `https://create.taila09abe.ts.net:8443/`. Then:

1. On the iPhone, turn Tailscale on.
2. Open the page in Safari and tap **Install**, then **Install** again in the prompt.
3. Go to the Home Screen. The app's icon shows progress. Check **This Build** afterwards: with a free team the old build stays (see the top of this page).

Bookmark the page or add it to the Home Screen. Each publish adds to it.

To go back to an earlier build, open **Earlier builds** and tap its Install: it replaces the app on the phone the same way. Two builds of the same app can't be on the phone at once, because they share a bundle id.

| Command | What it does |
|---|---|
| `forge publish ios` | Build, sign and publish the app in the current folder (`--project` for another) |
| `forge publish ios --release` | The same in the release configuration |
| `forge publish ios --label "PR 190"` | Name the build on the page (default: the branch it was built from) |
| `forge publish ios --keep 10` | Keep more builds of each app (default 5) |
| `forge publish ios --ipa App.ipa` | Publish an `.ipa` that is already signed |
| `forge publish ios --port 9443` | Use another tailnet port (8443 by default; 443 is often taken by another `tailscale serve`) |
| `forge publish ios --url https://…` | Serve through your own HTTPS address instead of Tailscale. It must forward to `http://127.0.0.1:47411`, and its certificate must be trusted by the iPhone |
| `forge publish enable` | Start the file server with the computer (systemd user service `forge-publish.service`) |
| `forge publish serve` | Run the file server in the foreground |
| `forge publish stop` | Take the page off the tailnet and stop the server. The builds stay on disk. Pass the same `--port` you published on; a port that serves something else is left alone |

With `--json`, `data` has `page_url`, `install_url` (this build's), `build_id`, `label`, `commit`, `bundle_id`, `version`, `signed_until` and `devices`. `apps.json` on the page lists every build, newest first, for tools (or an app) that want to check for updates.

## Limits

- **Same signing rules as a cable install.** The build installs only on the devices in its profile, a free team's signature lasts 7 days (the page says when, and says when it has run out), and the 3-app limit still applies. Publishing again renews the 7 days.
- **No working installs with a free team** ([above](#what-was-seen-with-a-free-team)): a new app fails with "Unable to Install", and an update leaves the old app in place. Install by cable with `forge run ios`.
- **Install only.** Logs, crash reports and `forge ui` tests still use the cable. iOS 17 and later don't offer the phone's device services over a VPN, so `forge device` can't reach a phone over Tailscale.
- **The laptop must be on**, with the server running: `forge publish enable` starts it with the computer, and `forge publish ios` starts it if it isn't running.
- **Each build is signed when it is published,** so an older build on the page expires 7 days after its own publish (the page marks it).
- The page and the builds are visible to every device on your tailnet. Don't share the tailnet with people who shouldn't have your development builds.

## Troubleshooting

| Problem | Fix |
|---|---|
| `Access denied` or `must be root` from `tailscale serve` | `sudo tailscale set --operator=$USER` once |
| `port … already serves …, and Forge won't replace it` or `listener already exists for port …` | Another `tailscale serve` uses that port. Forge never takes it over: `forge publish ios --port 9443` (and `forge publish stop --port 9443` later) |
| `tailscale serve` waits, or says HTTPS isn't enabled | Turn on HTTPS Certificates in the admin console (DNS page) |
| Safari can't open the page | Tailscale isn't on on the phone, or the laptop is asleep |
| "Unable to Install … Please try again later", grey icon with a cloud badge | Seen for every new app with a free team ([above](#what-was-seen-with-a-free-team)). Delete the grey placeholder and install with `forge run ios` over the cable |
| "Unable to install" on an update | The profile doesn't list this iPhone (`forge publish` prints the devices it does list), or the app's 3-slot limit is full. Build once with `forge run ios` over the cable to register a new phone |
| The app still shows the old build after Install | With a free team this is what happens even when the whole download arrives ([above](#what-was-seen-with-a-free-team)): install with `forge run ios` over the cable. A download cut off mid-transfer (seen 2026-11-14) has the same symptom; the server answers range requests, so iOS can resume one |
| The app won't open after a week | The free team's signature expired: publish again and reinstall |
