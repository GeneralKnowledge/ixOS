# Relay design specification

The complete Relay experience this programme works towards. Built on
[examples/relay/docs/relay-design.md](../../examples/relay/docs/relay-design.md) (Liquid Glass, #438) and the
behaviour in the [Relay audit](../../examples/relay/docs/relay-audit-2026-11.md). Tokens:
[DESIGN_TOKENS.md](DESIGN_TOKENS.md#3-relay-tokens-app-local-proposed-relaycolors-relaymetrics). References:
[DESIGN_REFERENCES.md](DESIGN_REFERENCES.md#relay-references).

## Identity

A quiet, sophisticated communication app: minimal, precise, calm, private, dependable. Quiet confidence.
Not playful, not corporate, not futuristic.

- **Silent when well, explicit when not** (Element X): ordinary success is quiet; failure, mailbox waiting,
  and trust changes are said plainly.
- **Trust lives in the conversation** (Signal): when a device is revoked or introduced, or an identity
  changes, the affected conversation shows it as a system row with one action.
- **Calm about the network**: connection loss is information, never an alarm.
- **Colour carries meaning, not mood**: one muted accent; red only for failure.

## What must not change

Identity, pairing, trust, introducer and revocation rules; delivery states and their meaning (Sending, In
the Mailbox, Delivered, Read, Not Delivered); security and trust warnings (never softened into
invisibility); the outbox, SQLite store and migrations; connection modes (Private means no relay); Rust
networking; accessibility identifiers used by `uitests/`. A V4 PR changes views; a change to `RelayModel` or
`ChatModel` beyond presentation needs its own reason in the PR.

## Screens

### A. Chats (priority 1)

| Part | Spec | Today | Work |
|---|---|---|---|
| Row | Avatar, name (`.headline`), time, last message (two lines max), unread count or dot | Dot, title, time, last message; no avatar | V4.1 |
| Avatar | Initials on a colour derived from the device or person's identity key; a device glyph for own devices | None | V4.1 (derive in Swift from the public key bytes Relay already has; no new Rust) |
| Network state | One quiet line at the top of the list when not connected ("Offline. Messages will send when you're connected.") | Per conversation only | V4.1 |
| Organisation | Search (exists), mute (exists), swipe read / unread (exists) | — | Keep; pinning is a feature, not planned unless asked |

States: **no conversations** (first-run: pair a device or join a room, one clear action each);
**offline** (above); **loading** (rows at final size, no spinner over the list); **failed to load store**
(Rust already reports data it set aside; the screen for it is still owed and belongs here).

### B. Conversation (priority 2)

| Part | Spec | Today | Work |
|---|---|---|---|
| Bubbles | Outgoing accent, incoming surface; inner corners tightened by position in a run (first / middle / last) | Same 18 pt corners on every bubble | V4.2 |
| Spacing | 2 pt within a run, 8 pt between runs, day headers centred | 0 and 6 | V4.2 |
| Timestamps | One per run (exists); every message's time on demand (swipe or tap) | Per run | V4.2 |
| Delivery | Under the last outgoing message of a run: Sending (animated), In the Mailbox, Delivered, Read as a quiet caption with a small symbol; Not Delivered in red with Try Again | Text label under each message | V4.2: same states, quieter form; In the Mailbox and Not Delivered stay explicit |
| Unread divider | A thin accent line with "N New Messages" | Two system dividers | V4.2 |
| New message arrival | Inserted with a short rise only when at the bottom; otherwise the New Messages button counts up and the reading position doesn't move | Bottom tracking exists | V4.2 |
| Reply | Quote card above the composer with Cancel | Bar in the composer | V4.2 |
| Reactions | At the top of the message's menu; shown as a small row under the bubble | Exists | Restyle only |
| Trust rows | "Tablet was removed from your devices by Laptop", centred system text with View | Not in the conversation | V4.3 (presentation of events Relay already records; confirm the data is there) |
| Composer | Floating glass card (#438) or iOS 26 `safeAreaBar` (decide on the phone), grows to 6 lines then scrolls | Glass card | V4.2 |

States: **empty conversation** (who it is with and how it is protected, one line); **connecting /
reconnecting / not connected** (the calm notice from #438); **message failed** (red caption with Try
Again, nothing else turns red); **peer revoked** (composer replaced by an explanation and no send);
**very long message**, **only emoji**, **deleted message** (placeholder).

### C. Composer and attachments (priority 3)

- Staged attachments as thumbnails inside the composer card, each removable, with a caption field.
- Voice recording: live level waveform, elapsed time, Cancel and Send as buttons (slide-to-cancel only as
  an extra); playback fills the waveform with progress and shows remaining time.
- Send: light tap, the bubble rises from the composer (fade only under Reduce Motion).

States: **attachment too large** (said before sending), **recording permission denied** (explains with a
Settings link), **staging while offline** (allowed; it queues in the outbox as today).

### D. Assistant (priority 4)

- A distinct identity: a mark and the accent at low saturation for its bubbles, so it never looks like a
  person.
- Responses render Markdown (headings, lists, code, links) with `AttributedString`; long code scrolls.
- Thinking: a quiet animated "Thinking" line in place of the reply, cancelable; tool or activity steps as
  small secondary lines.
- The same composer as conversations.

States: **model unavailable** (why, and what to do), **response failed** (Try Again on that turn),
**empty history**.

### E. Devices (priority 5)

- Device cards (content layer, solid): name, kind glyph, trust state, online or offline, route (Local,
  Tailscale, relay) as a small label, last seen.
- Actions: Send and Receive (glass in the toolbar, #438), per-device Message, Revoke behind confirmation.
- Pairing: the pairing sheet's code card resolves into the device card when trust completes (matched
  geometry inside the sheet; cross-fade under Reduce Motion).

States: **no devices** (pair your first device), **all offline**, **pairing failed** (why and Try
Again), **revoked device** (in its own section, with who revoked it).

### F. Settings (priority 6)

Order: Account and identity · Privacy and security (connection mode, pairing policy, servers) ·
Notifications (link to the system setting, plus muted conversations) · Appearance (only if it does
something: an app-level Light / Dark / System override) · Devices and sessions · Assistant · Delete
Account. Plain grouped list; no glass.

## Signature interactions

| Interaction | Behaviour | Reduce Motion |
|---|---|---|
| Send message | Light tap, restrained rise | Fade |
| New message | Smooth insertion without moving the reading position | Instant |
| Reply | Quote card above the composer | No slide |
| Attach file | Thumbnail staging in the composer | No move |
| Voice message | Live waveform while recording, progress while playing | No pulse |
| Connection loss | Calm notice under the title | Same |
| Device connection | Pairing card resolves into the device card | Cross-fade |
| Unread messages | Floating return-to-bottom with count | Same |

## Milestones (from the programme)

V4.1 Chats · V4.2 Conversation and composer · V4.3 Assistant and Devices · V4.4 Settings and polish. Each
PR carries the evidence in [DESIGN_QA.md](DESIGN_QA.md).
