# Reference deep audit 1: LocalSend, for Relay

Status: 2026-11-17. Phase 1b of [ixOS Experience Research](../README.md). Research only: no change to Relay.

**Reference:** [LocalSend](https://github.com/localsend/localsend) at `d1e6198` (2026-11-17), Apache-2.0, Flutter. It is a workflow reference, not a SwiftUI one: we study what it decides, not how it draws. Paths below are under its `app/lib/`; its words come from `app/assets/i18n/en.json`.
**ixOS app:** Relay on main (`5011810`) and on #268 (Send and Receive screens, branch `claude/project-thread-ebenpi`, not merged). Relay's own plan is [relay-m1-m2-brief.md](../../../../examples/relay/docs/relay-m1-m2-brief.md); its decisions (word codes, pairing policies, iroh, three screens Nearby, Connections and Activity in PR 5) are taken as given. This audit suggests how those decisions should feel, not different decisions.

**Evidence:** everything here is **S** (source verified) on both sides: LocalSend was read, not run, and Relay was read, not run. No screenshots of LocalSend are reproduced; the observations are described in words.

## LocalSend in one paragraph

Three tabs: **Receive**, **Send**, **Settings**. Receive is passive: it shows this device's name very large (a random friendly alias such as "Clever Avocado", `aliasGenerator` in the strings) and a short `#` id, and it waits. Send is a single page: pick what to send (File, Folder, Media, Text, Paste), then tap one of the **Nearby devices** found automatically on the Wi-Fi. The receiver gets a full screen: who is sending (device icon and name), what ("wants to send you 3 files"), **Accept** or **Decline**, Options (where to save), and Verify (compare a fingerprint as icons or text: "Does it look the same on the other device?"). Both sides then see a progress page: every file with its own status and progress, totals for count, size, speed and time left, retry for a failed file, and Cancel, which becomes Done when finished. No account, no pairing: trust is per request, plus Favorites and an optional **Quick Save** that accepts automatically from everyone or only from favorites.

## What LocalSend gets right, and why

### 1. The top level is the person's intent, not the app's model

**LocalSend:** the tabs are Receive, Send, Settings (`pages/home_page.dart`, `HomeTab`). Devices are not a destination of their own; they appear inside Send as targets.
**Relay:** on main the root is **Paired Devices** with History under it (`DevicesView`), and sending with a code hides in a `+` menu called Pair a Device. #268 adds Send and Receive to the bottom bar, but the root is still the device list, and there are two ways to send: open a paired device (`PeerView`: Send Note, Send a File) or Send in the bottom bar (word code).
**Why LocalSend's is better:** someone opens a transfer app with a verb in mind. Starting from the verb means one path to learn, with the device chosen as part of it.
**Where:** App (Relay).
**Proposal (P1):** when PR 5 builds Nearby, Connections and Activity, make **Send** one flow whatever the target: choose what, then choose who, where "who" lists trusted devices nearby first and "anyone, with a code" last. Receive is the other half. Devices stay a list you can manage, not the front door. This fits the brief's three screens; it decides what the first one is for.

### 2. Nearby devices appear before you ask

**LocalSend:** Send lists **Nearby devices** found by multicast as soon as the tab opens, with a faint placeholder tile while it looks (`pages/tabs/send_tab.dart`, `DevicePlaceholderListTile` at 30% opacity) and a rescan button. Manual address entry and Favorites are small buttons beside the heading, for when discovery fails.
**Relay:** Bonjour already knows which paired devices are nearby (`PeerRow`: "Nearby" or "Not seen on this network"), but only on the device list. A code send shows a code and waits; the receiver must type it ("7-acid-tidy-rug").
**Why it's better:** the common case (both devices on one Wi-Fi) needs no typing, and the rare case still has a manual way in.
**Where:** App, with Rust (discovery is PR 5).
**Proposal (P2):** in Send, show trusted devices that are nearby as tappable targets at the top. For code transfers keep the code (it is the security, SPAKE2), but consider letting Receive list senders advertising a code nearby, so the receiver picks one and types only to confirm. Whether that can keep the code's protection is a question for the Relay thread, not this audit.

### 3. The receiver sees what's coming and decides

**LocalSend:** the receive screen (`pages/receive_page.dart`) shows the sender's device-type icon and name, "wants to send you {n} files", the files, and Accept or Decline; Options sets the destination for this transfer; Verify compares a fingerprint. If the sender cancels, it says so: "The sender has canceled the request."
**Relay:** a paired device's note or file simply arrives (trust was given at pairing). A code receive starts downloading once the code is entered and shows "Receiving 3.2 MB" with a bar; the name of what arrives is shown at the end ("Received …").
**Why it's better:** consent and context before bytes move. Relay's paired path is a deliberate, valid choice (the brief's pairing policies are the same idea as LocalSend's Quick Save); the code path could show the offer first at no cost to security.
**Where:** App + Rust.
**Proposal (P3):** after the code is checked, show "From Reuben's iPhone: Trip, 3 items, 24 MB" with Receive and Cancel before the download, at least for large transfers.

### 4. Progress says what, how much, how fast, and what to do about a failure

**LocalSend:** the progress page (`pages/progress_page.dart`) lists every file with its own bar and status (finished, skipped, failed, "Saved in Photos"), a retry button on a failed file, totals ("Files: 2 / 5", "Size: …", "Speed: …/s") and time left, and **Cancel** that turns into **Done** with an automatic close countdown when Auto Finish is on. End states are named for whose doing they were: "Canceled by sender", "Canceled by receiver", "Finished with error".
**Relay:** one bar for the whole transfer (#268's Receive), elapsed seconds, "Resuming: … already here" (an advantage: we found no transfer resume in LocalSend's source), and speed only after finishing ("… in 3.1 s, 7.8 MB/s"). Failures in History are "Not Sent" with a reason drawn from Rust (friction audit F10).
**Why it's better:** a person waiting on a big transfer wants to know it is moving and roughly when it ends; a person whose transfer failed wants to know whose side failed and what to do.
**Where:** App (words and layout); the cause classification is Rust (better-by-default item 6, `ErrorKind`).
**Proposal (P2):** add time left and live speed to the bar; name the end state by cause ("The other device stopped it", "Lost the connection: Try Again"); keep resume as Relay's advantage and say it ("Picks up where it stopped").

### 5. Help for the failure everyone hits

**LocalSend:** a **Troubleshoot** link sits under the nearby list (`pages/troubleshoot_page.dart`). It lists symptoms in the person's terms ("This device cannot discover other devices") with a solution under each, including the one nobody guesses: the Wi-Fi may isolate devices from each other (AP isolation).
**Relay:** a failure is an alert or a "Not Sent" row; the only hint about the network is a list footer ("open to paired devices on this Wi-Fi while Relay is on screen").
**Why it's better:** "they can't see each other" is the most common failure of local transfer, and its causes are outside the app. Saying them in plain words saves a support conversation.
**Where:** App.
**Proposal (P2):** a short "Can't see the other device?" sheet, linked from Send and from a failed transfer: same Wi-Fi, Relay open on both (until background reach exists), guest or hotel Wi-Fi may block devices, Local Network allowed in Settings.

### 6. Technical detail is there, but folded away

**LocalSend:** the Receive tab hides IP and port behind an info button (`_InfoBox`, `AnimatedCrossFade`); the progress page hides details behind **Advanced**; the device page has Logs. Settings has a Network section for port and multicast address, with a warning when a custom value may make the device undiscoverable.
**Relay:** #268 shows a **Memory** section ("Memory …, peak …") in both the Send and Receive sheets (`SendCodeView`, `ReceiveCodeView`), there for the M1 phone gate. On main, the scan screen's paste field shows `forgerelay://pair?…` as its placeholder.
**Why it's better:** progressive disclosure: the person sees the transfer, the developer can still find the numbers.
**Where:** App.
**Proposal (P2):** fold Memory into a Details disclosure, or a test-build-only row, once the M1 gate is passed; give the paste field a human placeholder ("Paste the code").

### 7. A device you can recognise

**LocalSend:** every device gets a friendly random name by default, shown large on the Receive tab, plus a short id from its address (`util/ip_helper.dart`, `visualId`) so two "iPhone"s can be told apart; device rows have a type icon (phone, desktop, web).
**Relay:** the device's own name is renamed through a bare pencil at the top left (friction audit V7). Every peer row uses the `iphone.gen3` symbol (`PeerRow`), including a Linux computer running `relay`.
**Why it's better:** the person on the other side has to find *this* device in a list; a distinct name and the right icon do that.
**Where:** App + Rust (the device's kind travels with its status in PR 5).
**Proposal (P3):** show this device's name where the person receives ("Others see you as …", tap to rename); use a symbol for the device's kind.

### 8. Permissions explained where they bite

**LocalSend:** a dedicated dialog for the iOS Local Network permission: "LocalSend can't find other devices without having the permission to scan the local network", with a Settings button (`dialogs.localNetworkUnauthorized`).
**Relay:** the Local Network prompt text is set (`forge.toml`), but nothing in the app says when it was refused; nearby devices just never appear. The camera case is the same (audit X18).
**Where:** App; `ForgePermissions.openSettings` exists in ForgeNative.
**Proposal (P2):** when discovery finds nothing and Local Network is denied, say so with Open Settings; the same for the camera on the scan screen.

## What Relay already does better

- **Resume** of an interrupted transfer, and saying so.
- **Security without settings:** codes use SPAKE2 and paired traffic is authenticated by key. LocalSend lets the person turn encryption off (`encryptionDisabledNotice`), a footgun Relay should never copy.
- **Pairing policies** (codes only, ask to pair, pair automatically) are LocalSend's Quick Save idea with clearer meaning; the brief's revocation rule has no LocalSend equivalent.
- **The Files app**: #268 puts received items in On My iPhone, Relay, and opens a received folder there.

## What LocalSend gets wrong, so we don't import it

- **It isn't an iOS app in its look.** Flutter's Material widgets, its own tab bar and snack bars: we take the flow, not the components.
- **Network settings in front of people:** port, multicast address, encryption on/off, discovery timeout. Relay should keep these out of the app.
- **Rotating help text** (`OpacitySlideshow` under the device list) changes every six seconds: hard to read, and VoiceOver can't rely on it.
- **A passive Receive tab** spends a whole tab on a logo and a name. On a phone, receiving is better as a state the app is in (and a notification when something arrives) than a screen you must keep open; Relay's "keep Relay open" constraint is a limitation to remove (M2 reach), not a pattern to copy.

## Where each proposal belongs

| # | Proposal | Priority | Where | Depends on |
|---|---|---|---|---|
| 1 | One Send flow (what, then who); devices are targets, not the front door | P1 | Relay app | PR 5 (Nearby, Connections, Activity) |
| 2 | Nearby trusted devices as one-tap targets; nearby code offers on Receive (security question for the Relay thread) | P2 | Relay app + Rust | PR 5 |
| 3 | Show the offer before a code download | P3 | Relay app + Rust | |
| 4 | Time left, live speed, end states named by cause | P2 | Relay app; cause from Rust (`ErrorKind`) | better-by-default item 6 |
| 5 | "Can't see the other device?" help sheet | P2 | Relay app | |
| 6 | Memory behind Details; human paste placeholder | P2 | Relay app | M1 gate passed |
| 7 | This device's name where you receive; device-kind symbols | P3 | Relay app + Rust | PR 5 status |
| 8 | Local Network and camera denial explained | P2 | Relay app | |

Nothing here is a shared ForgeUI or ForgeAppKit piece yet: transfer progress and nearby lists have one user. Two candidates to watch for a second use: a **progress row with time left and named end states** (Soundscape's offline downloads may be the second), and a **permission-denied notice** with Open Settings (FieldKit, Weather and Chat each word their own; this would be the inline notice from the [audit](../audit.md#shared-gap-analysis-what-goes-where)).

## Suggested next step

Hand proposals 1, 2 and 7 to the Relay thread as input to PR 5's screens, and 5, 6 and 8 as small app changes after #268 merges. Nothing should change Relay while #268 is open.
