# Over-the-air installs

`forge publish ios` builds your app, signs it, and adds it to one Install page on your tailnet. On the iPhone you open the page in Safari and tap **Install**, and iOS installs that build over the one on the phone, keeping its data. The first install of each app goes over the cable (see [below](#first-install-by-cable)); after that no cable is needed, and the phone can be anywhere it can reach your tailnet: on your home Wi-Fi, on mobile data, or across town.

This is the default way to get a build onto the phone: publish every build you want to try. Builds never replace each other on the page. Each one is listed under its app with the branch it came from, its commit and when its signature runs out, so builds from different branches (or different agents) sit side by side and you pick which to install.

It works with a free Apple team. On 2026-11-14 a free-team build of Device Lab, signed on the laptop with no phone connected, installed this way on Cati (iPhone 15 Pro Max, iOS 26.3.1), as an update to a copy installed by cable.

## First install by cable

With a free Apple team, the page **updates** apps; it can't put a new app on the phone. Install each app once with `forge run ios` over the cable and trust the developer (Settings > General > VPN & Device Management). From then on, every build of that app installs from the page.

This was found on 2026-11-15 on Cati (iOS 26.3.1). After every free-team app had been deleted, each install from the page downloaded in full and then failed with "Unable to Install … Please try again later", leaving a grey icon with a cloud badge. That included the exact file that had installed the day before. The signing certificate was valid and not revoked, and the profile listed the phone. Installing Device Lab by cable fixed Device Lab's updates from the page, but Weather, never installed by cable, still failed with the developer trusted. So the cable install is needed per app, not once per team. If you delete an app to make room under the 3-app limit, it goes back on by cable.

## How it works

1. Forge builds the Rust library and runs `xtool dev build --sign --ipa`. xtool signs with your team's development profile, which lists the devices registered on the team, so the phone does not need to be connected.
2. Forge copies the `.ipa` into its publish folder (`~/.local/share/forge/publish`, or `FORGE_PUBLISH_DIR`) as a new build (`<bundle id>/<time>-<commit>/`), writes the install manifest iOS reads for it, and rewrites `apps.json` and the page. The page lists each app, the most recently published first, as a card with its icon (taken from the `.ipa` when the app has one, `[ios] icon` in forge.toml; otherwise its initial on a coloured tile), its newest build's branch, commit, age, size and days left on its signature, and earlier builds folded away under **N earlier builds**. With more than four apps it adds a search box. The icon also shows on the Home Screen while the app downloads. It keeps the newest 5 builds of each app (`--keep`) and deletes older ones.
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

The page shows each build's version and build number. In the app, ForgeKit's `ForgeBuildInfo.current` reads the same keys (`versionLine`, `sourceLine`), so an app can show which build is running. Device Lab shows it under **This Build** on its Apps screen. If the numbers match the page's newest build, the update arrived.

## Publishing a build

```bash
forge publish ios --project examples/device-lab
```

It prints the page's address, for example `https://create.taila09abe.ts.net:8443/`. Then:

1. On the iPhone, turn Tailscale on.
2. Open the page in Safari and tap **Install**, then **Install** again in the prompt.
3. Go to the Home Screen. The app's icon shows progress, and then the app opens with the new build.

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
- **Updates only.** Each app's first install is by cable ([above](#first-install-by-cable)).
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
| "Unable to Install … Please try again later", grey icon with a cloud badge | The app isn't on the phone yet: its first install has to go over the cable (`forge run ios`, see [First install by cable](#first-install-by-cable)). Delete the grey placeholder first |
| "Unable to install" on an update | The profile doesn't list this iPhone (`forge publish` prints the devices it does list), or the app's 3-slot limit is full. Build once with `forge run ios` over the cable to register a new phone |
| The app still shows the old build after Install | The download was cut off and iOS kept the old app (seen 2026-11-14 while the laptop was busy with several builds: the 46 MB download dropped mid-transfer). Compare Device Lab's **Apps > This Build** with the page, and tap Install again. The server answers range requests, so iOS can resume a download |
| The app won't open after a week | The free team's signature expired: publish again and reinstall |
