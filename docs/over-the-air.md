# Over-the-air installs

`forge publish ios` builds your app, signs it, and publishes it on your tailnet. On the iPhone you open a page in Safari and tap **Install**, and iOS installs the new build over the old one, keeping its data. No cable is needed, and the phone can be anywhere it can reach your tailnet: on your home Wi-Fi, on mobile data, or across town.

It works with a free Apple team. On 2026-11-14 a free-team build of Device Lab, signed on the laptop with no phone connected, installed this way on Cati (iPhone 15 Pro Max, iOS 26.3.1).

## How it works

1. Forge builds the Rust library and runs `xtool dev build --sign --ipa`. xtool signs with your team's development profile, which lists the devices registered on the team, so the phone does not need to be connected.
2. Forge copies the `.ipa` into its publish folder (`~/.local/share/forge/publish`, or `FORGE_PUBLISH_DIR`) and writes the install manifest iOS reads, `apps.json`, and a page listing every published app, newest first.
3. A small file server (`forge publish serve`, started in the background) serves the page, `apps.json` and each app's manifest and `.ipa` on `127.0.0.1:47411` only, and nothing else in the folder.
4. Unless that port already serves something else, `tailscale serve` puts it on HTTPS at `https://<this computer>.<tailnet>.ts.net:8443/`, with a certificate the iPhone trusts. Only devices on your tailnet can reach it. Forge never uses Tailscale Funnel, which would put it on the public internet.
5. The page's **Install** button is an `itms-services://` link to the manifest. iOS downloads the `.ipa` and installs it.

## One-time setup

On the computer that builds (the laptop):

```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
sudo tailscale set --operator=$USER    # lets forge run `tailscale serve` without sudo
```

In the [Tailscale admin console](https://login.tailscale.com/admin/dns), turn on **MagicDNS** and **HTTPS Certificates**.

On the iPhone, install **Tailscale** from the App Store and sign in to the same tailnet. The phone must already trust your developer certificate. If you've opened an app from `forge run ios` before, it does. If not, go to Settings > General > VPN & Device Management and trust it once.

## Publishing a build

```bash
forge publish ios --project examples/device-lab
```

It prints the page's address, for example `https://create.taila09abe.ts.net:8443/`. Then:

1. On the iPhone, turn Tailscale on.
2. Open the page in Safari and tap **Install**, then **Install** again in the prompt.
3. Go to the Home Screen. The app's icon shows progress, and then the app opens with the new build.

Bookmark the page or add it to the Home Screen. Each publish updates it.

| Command | What it does |
|---|---|
| `forge publish ios` | Build, sign and publish the app in the current folder (`--project` for another) |
| `forge publish ios --release` | The same in the release configuration |
| `forge publish ios --ipa App.ipa` | Publish an `.ipa` that is already signed |
| `forge publish ios --port 9443` | Use another tailnet port (8443 by default; 443 is often taken by another `tailscale serve`) |
| `forge publish ios --url https://…` | Serve through your own HTTPS address instead of Tailscale. It must forward to `http://127.0.0.1:47411`, and its certificate must be trusted by the iPhone |
| `forge publish serve` | Run the file server in the foreground (for a systemd unit, say) |
| `forge publish stop` | Take the page off the tailnet and stop the server. The builds stay on disk. Pass the same `--port` you published on; a port that serves something else is left alone |

With `--json`, `data` has `page_url`, `install_url`, `bundle_id`, `version`, `signed_until`, `devices` and `commit`.

Each app keeps its latest build. Publishing an app again replaces it, and other apps stay listed.

## Limits

- **Same signing rules as a cable install.** The build installs only on the devices in its profile, a free team's signature lasts 7 days (the page says when, and says when it has run out), and the 3-app limit still applies. Publishing again renews the 7 days.
- **Install only.** Logs, crash reports, `forge ui` tests and the live connection to a `--devtools` build still use the cable. iOS 17 and later don't offer the phone's device services over a VPN, so `forge device` can't reach a phone over Tailscale.
- **The laptop must be on**, with the server running. After a reboot, run `forge publish ios` again or `forge publish serve`.
- The page and the builds are visible to every device on your tailnet. Don't share the tailnet with people who shouldn't have your development builds.

## Troubleshooting

| Problem | Fix |
|---|---|
| `Access denied` or `must be root` from `tailscale serve` | `sudo tailscale set --operator=$USER` once |
| `port … already serves …, and Forge won't replace it` or `listener already exists for port …` | Another `tailscale serve` uses that port. Forge never takes it over: `forge publish ios --port 9443` (and `forge publish stop --port 9443` later) |
| `tailscale serve` waits, or says HTTPS isn't enabled | Turn on HTTPS Certificates in the admin console (DNS page) |
| Safari can't open the page | Tailscale isn't on on the phone, or the laptop is asleep |
| "Unable to install" | The profile doesn't list this iPhone (`forge publish` prints the devices it does list), or the app's 3-slot limit is full. Build once with `forge run ios` over the cable to register a new phone |
| The app won't open after a week | The free team's signature expired: publish again and reinstall |
