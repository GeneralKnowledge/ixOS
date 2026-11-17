# AI Lab, VPN and eSIM: one plan

Status: **proposed, waiting on Reuben's decisions** (2026-11-17). Planning only: no code.

Reuben (2026-11-17): "Could we plan out the AI Lab, and the VPN and eSIM apps? And put it into a plan?"

This page brings two earlier plans together and moves each to its next small, phone-proven step:

- the Forge AI plan ([forge-ai-plan.md](../forge-ai-plan.md), approved 2026-11-14), where step A2, the AI Lab, is next;
- the VPN and eSIM plan ([VPN and eSIM apps plan](https://claude.ai/artifact/HqXN9pQXSVi3BYZhYAJgYR), 2026-11-15), which is still waiting on his answers. Those open questions are carried forward below, each with a recommendation.

## Summary

| | Where it lives | Free team? | First step | Waits on |
|---|---|---|---|---|
| **AI Lab** | A screen inside Device Lab, not a new app | Yes | L1: Apple's model only, in Device Lab | Nothing |
| **eSIM app** | The VPN repo (`GeneralKnowledge/VPN`), as a product app built on ixOS | Yes, to develop and test | E0: a half-day spike of Apple's eSIM setup link | A test eSIM to install |
| **VPN app** | The VPN repo, next to the eSIM app | **No**: needs a paid team enrolled as an organisation | V0: company and Apple account (paperwork) | Company, D-U-N-S number, paid organisation team |

The three don't block each other. The AI Lab is ixOS work that can start now. The eSIM app is the smaller of the two products and builds the shared core the VPN app reuses. The VPN app's first real step is a business one, not code.

## Expand an existing app first?

Reuben's rule (2026-11-16): a missing category doesn't justify a new app; first ask whether an existing app should grow into it.

| Proposal | Could an existing app grow into it? | Answer |
|---|---|---|
| AI Lab | Chat already has the assistant; Pocket Library has summaries; Relay carries AI asks | **Not a new app.** It is the test bench the Forge AI plan already put inside Device Lab (section 2, "The playground"). Products keep their AI features: Chat's assistant, Pocket Library's summaries, Good Dog's planned Ask (GD7). A capability is tried in the Lab first, then shipped in a product app |
| VPN | **Relay** is the closest: it already makes encrypted device-to-device connections over iroh | **No, keep them apart.** Relay connects your own devices peer to peer; a consumer VPN routes all of a phone's traffic to an exit server it sells. Relay's own plan rules this out ("Not another ... VPN", [relay-platform-plan.md](../../examples/relay/docs/relay-platform-plan.md)). Folding a tunnel into Relay would also make Relay a VPN app under App Review rule 5.4, so Relay itself would need the paid organisation team. Tailscale and iroh stay what they are: the dev network and Relay's transport |
| eSIM | No existing app sells or manages mobile data | **A product app**, not a demo. It belongs to the Northstar business in the VPN repo, not to `examples/`, so it does not add to the "no new demo apps" list |

So: no new app in ixOS. The two product apps live in the VPN repo and depend on ixOS by git, as forge-lab does. Device Lab cannot host them, because ixOS must never depend on the VPN repo ([areas.md](../areas.md)).

## What needs a paid Apple team

Checked against [ios-signing.md](../ios-signing.md) (Forge refuses paid-only entitlements before building when `team_type = "free"`) and Apple's forums and review rules.

| Need | Entitlement | Free team? |
|---|---|---|
| Apple's on-device model (Foundation Models) | None | **Yes**: proven on the iPhone (L0, 2026-11-07; Chat L1, 2026-11-08) |
| Talking to a laptop model over Relay or HTTP on the tailnet | None (an ATS exception for plain http, as Soundscape uses) | Yes |
| Downloaded models (L2) in the background | Background Assets | No, paid only |
| Bigger models in memory | Extended virtual addressing | No, paid only. The increased memory limit entitlement is unverified |
| Opening Apple's eSIM setup link | None: it is a URL | Yes |
| Installing an eSIM directly from the app | Carrier entitlement (`CTCellularPlanProvisioning`) | **Never** for a non-carrier app, paid or not |
| Stripe card checkout in a web sheet, returning by custom URL scheme | None | Yes |
| Returning from checkout by universal link | Associated domains | No, paid only (use the URL scheme instead) |
| Apple Pay | Merchant ID | No, paid only |
| VPN tunnel | Network Extension (packet tunnel) or Personal VPN | **No**: paid only, no iOS workaround. App Review 5.4 also requires the team to be an **organisation** |
| Sharing data between an app and its extension (widget or tunnel) | App Groups | Unverified on a free team: spike S2 in the [native capability roadmap](../native-capability-roadmap.md) answers it, and V2 below runs it |

Everything here is Linux-first: the AI Lab and the eSIM app build with xtool like every Device Lab app. The VPN tunnel needs app-extension support in Forge, which xtool can build but Forge has no template for ([roadmap](../roadmap.md)). The App Store upload may still need a Mac at the end ([first-app-store-release.md](../first-app-store-release.md)).

## AI Lab

### Where things stand

- **L1** (Chat's assistant on Apple's on-device model) passed on the iPhone. **L2** (downloadable Qwen models through CoreML-LLM) was spiked in L0 (Qwen3.5 0.8B at ~36 tokens a second in release) and parked.
- **Pocket Library** summarises chapters and whole books on Apple's model (#181).
- **A0** (merged, #196): one `ai` feature in `forge-services` (`ChatRequest` in OpenAI chat-completions shape, `LanguageModel`, `TextSink`, the answer checker, `OpenAiServer`, a scripted model for tests) and one Swift adapter, `ForgeAppleModel`. Chat and Pocket Library both use it.
- **A1** (merged, #196): Relay protocol 2 (`Ask`, `Delta`, `Answered`, `Stop`), `relay serve-ai` and `relay ask`. Tested on Linux against a fake server, never against a real Ollama or llama.cpp.
- **Relay is moving to iroh.** Its M1 to M2 brief moves today's AI asks onto iroh "as they are" in its PR 3 ([relay-m1-m2-brief.md](../../examples/relay/docs/relay-m1-m2-brief.md)). Building the Lab's laptop link on the old TCP + Noise wire now would be thrown away.

### What the AI Lab is

One screen in Device Lab's Apps, with its Rust in Device Lab's crate:

- pick where the request runs (this iPhone, or a nearby model once that phase lands);
- write a prompt or paste a long text, watch the answer stream, press Stop;
- see what the checker removed, time to first text, tokens per second, and why a model is unavailable.

It is a bench, not a product: nothing in it is meant for an end user. When a feature works in the Lab, it moves to the product app that wants it (Chat, Pocket Library, Good Dog).

### Phases

Each is one PR with a short phone check, batched into a sitting with other checks.

| Phase | What | Done when | iPhone |
|---|---|---|---|
| **L1 Lab on the phone** | The AI Lab screen on Apple's model only: streaming, Stop, the checker's removals, timings, availability reasons | In Device Lab, a prompt and a pasted chapter both answer, Stop ends the answer at once, and an unavailable model says why | ~5 min |
| **L2 A bigger model nearby** | The Lab sends asks to a model on another machine over Relay, once Relay's PR 3 has moved asks onto iroh. Before that, `relay ask` against a real Ollama or llama.cpp on Linux, which A1 never ran | On the phone, the Lab answers from Apple's model and from the nearby model, with timings side by side | ~5 min |
| **L3 Router** | Forge AI plan A3: on the device by default, large jobs to the nearby model, fall back to the device mid-job; Pocket Library sizes its parts from the model's window and says which model answered | A whole book summarised from the phone on the nearby model, and the same book finishing on the phone when the link drops | ~10 min |
| **Later** | The phone answering other devices (A4); Good Dog's Ask (A5); downloaded Qwen in the Lab (old L2), only if offline use beyond Apple's model is wanted | | |

Extraction stays as approved: the whole-book pipeline stays in Pocket Library until Commonplace needs it; the pairing code leaves Relay only when a standalone app needs it.

### Risks

- **Which machine serves the bigger model.** The laptop is shared, runs Reuben's GPU job and the build queue, and must not be touched by a model server without his say. Hetzner is now on the tailnet but is CPU only, so an 8B model writes a few tokens a second there. L2 measures before L3 depends on it.
- **Foundation Models throttles background use**, so long book runs still pause when the phone is locked, as today.

## eSIM app

The backend already sells ResellPortal travel eSIMs and shows the install QR code on the web dashboard. The app does the same natively: browse packages by country, buy, install, see my eSIMs and data left (if ResellPortal reports usage), top up.

- **Install:** a non-carrier app can't install an eSIM itself. Travel eSIM apps open Apple's eSIM setup link (`esimsetup.apple.com/...?carddata=LPA:1$...`, iOS 17.4 and later), which hands the activation code to Settings. Apple doesn't document the link, so a spike proves it first; the QR code and the typed activation code are the fallback that always works.
- **Payment:** Stripe checkout in a web sheet. Rule 3.1.3(e) says services consumed outside the app must not use in-app purchase, and mobile data fits; that is a reading of the rule, not Apple's wording for eSIMs.
- **Shape:** Rust owns the account, the API client and the order state; SwiftUI and ForgeUI present them. A shared Rust crate for account, session and the Northstar API serves both the eSIM and the VPN app, so it passes the second-use rule from the start. Each app keeps its own identity and screens.
- **Testing on the phone:** it can't go inside Device Lab (ixOS can't depend on the VPN repo), so during its phases it takes one of the free team's three app slots.

| Phase | What | Done when | iPhone |
|---|---|---|---|
| **E0 Setup link spike** (half a day) | A throwaway screen that opens Apple's setup link with a real activation code | The code reaches Settings' eSIM install sheet in one tap (or we know it doesn't, and the QR code becomes the main path) | ~5 min, needs a real test eSIM |
| **E1 Shared core** (VPN repo) | A token API in the web service for apps; the shared Rust crate, tested on Linux against staging | The crate signs in to staging and lists eSIM packages and VPN locations in its tests | No |
| **E2 eSIM app** | Packages, Stripe checkout, install by setup link with QR fallback, my eSIMs | On the phone, buy a package on staging and get it into Settings in one tap | ~10 min |

## VPN app

Unchanged from the earlier plan except for the order: it follows the eSIM app.

- **Pieces:** a SwiftUI app (sign in, location picker, one connect switch, devices, account) that drives `NETunnelProviderManager`; the shared Rust core; a thin `NEPacketTunnelProvider` extension handing packets to BoringTun (Rust WireGuard), with WireGuardKit (Go) as the fallback; a token API in the web service.
- **Servers:** VPNresellers first (its API already hands out WireGuard configs), own servers on Hetzner or Oracle only when margin or control earns the work.
- **Framework gap:** app-extension support in Forge. Good Dog's widget needs the same, so it passes the second-use rule. It goes to ixOS as its own PR, proven first with the widget, then reused by the tunnel. A widget reads the app's data through an App Group, and whether a free team can sign one is still the open spike S2; if it can't, V2 waits for the paid team too.

| Phase | What | Done when | iPhone |
|---|---|---|---|
| **V0 Company and Apple account** (paperwork, alongside E1 and E2) | A company, a D-U-N-S number, the Apple Developer Program as an organisation (US$99 a year), xtool signed in with that team | `forge doctor` shows the paid organisation team | No |
| **V1 App without the tunnel** | Sign-in, locations, devices, account on the shared core; connect hands off to the official WireGuard app | On the phone, sign in and import a config into the WireGuard app | ~5 min |
| **V2 Extensions in Forge** (ixOS PR) | Spike S2 (App Group on a free team), then an app-extension template and build support, proven by Good Dog's widget | The widget shows on the home screen from a Linux build | ~5 min |
| **V3 The tunnel** (paid team) | Packet tunnel with BoringTun in the extension | Connect to a VPNresellers server, check the exit IP, survive Wi-Fi to mobile data and a locked phone for 10 minutes, stay under the extension memory cap (about 50 MB) | ~10 min |
| **V4 App Store** | In-app purchase for the card plans, the data-use screen rule 5.4 requires, TestFlight, review; eSIM app submitted first | Both approved | |

## Order of work

1. **AI Lab L1**, now, in ixOS. Small, free team, no dependencies.
2. **eSIM E0 spike**, whenever a test eSIM is available; it decides the eSIM app's main install path.
3. **AI Lab L2** after Relay's PR 3 moves asks onto iroh.
4. **E1 and E2** in the VPN repo, with **V0** paperwork running alongside.
5. **V2** extensions in ixOS with Good Dog's widget, so the framework part is proven before any paid-team work.
6. **V1, V3, V4** once the organisation team exists.

One thing at a time on the laptop: each phone step waits for the single combined build through the build queue.

## Decisions for Reuben

Each has a recommendation; "yes to all" takes them as written.

1. **AI Lab home:** a screen inside Device Lab, not a new app. *Recommended.*
2. **AI Lab first step:** L1 on Apple's model now, and the laptop link only after Relay's iroh move. *Recommended*, so nothing is built on the TCP wire Relay is retiring.
3. **Which machine serves the bigger model:** the laptop (fastest, but shared with your GPU job and builds), Hetzner (CPU only, always on), or the iMac? *Recommended: Hetzner for the first measurement*, leaving the laptop alone; move only if it is too slow.
4. **Downloaded Qwen (old L2):** stay parked. *Recommended.*
5. **Web launch before the apps:** yes, prove paying customers first; it costs nothing on Apple's side. *Recommended.*
6. **First product app:** eSIM, then VPN. *Recommended*: no paid or organisation block, and it builds the shared core.
7. **VPN inside Relay?** No: separate app; Relay stays account-free and off the 5.4 rules. *Recommended.*
8. **Where the product apps live:** the VPN repo, depending on ixOS by git; framework gaps go to ixOS as their own PRs. *Recommended.*
9. **A test eSIM for the spike:** buy one small real ResellPortal package (or any travel eSIM with an activation code). *Recommended*; the spike can't be proven without a real code.
10. **Company and Apple account:** a UK Ltd you control, enrolled as an organisation, started alongside the eSIM app. *Recommended*; it is needed anyway for Stripe and rule 5.4. This replaces any personal paid team for these apps.
11. **Tunnel library:** BoringTun, with a one-day spike that falls back to WireGuardKit. *Recommended.*
12. **VPN payments:** in-app purchase for card plans (required for review) plus sign-in for web and crypto customers. *Recommended.*
13. **Servers:** VPNresellers until margin or control justifies running your own. *Recommended.*
14. **Still open from 2026-11-15, needing your facts rather than a recommendation:** who the customers are (UK consumers, travellers, businesses); whether "Northstar" is the real name for both apps; whether you already have a company and VPNresellers, ResellPortal or Stripe accounts. Android stays on the web plus the official WireGuard app. *Recommended.*
