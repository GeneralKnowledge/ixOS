# N2: Away from home

Status: plan, 2026-11-21. Reuben chose N2 as the first stage after
Relay 1.0, with "Hybrid with Relay's servers on by default, and setup
that needs no networking knowledge" (decision card, 16:09). Stage N2 in
the [network architecture](relay-network-architecture.md).

## The goal

Your devices reach each other wherever they are, with nothing to set up
beyond pairing them once. You never type an address, open a port, run a
VPN or know what a relay is. What the servers can and can't see is said
plainly, and turning them off is one switch that takes effect at once.

## What exists today

The acceptance run on 2026-11-21 proved the pieces work:

- **Hybrid** (the default mode) tries the local network, then private
  networks such as Tailscale, then direct over the internet, then Relay's
  relay. On 5G with Tailscale off, messages reached the laptop through
  relay.avedeus.ovh, once each.
- **Private** never uses the servers: messages waited until a local or
  Tailscale route came back.
- **The lookup server** publishes only each device's relay address, signed
  by its key, so a trusted device is found by key alone.

What's missing is the default and the setup:

- "Use Relay's Servers" is **off** until the person finds it in
  Settings. On Reuben's phone it was turned on by hand.
- Changing it **takes effect the next time Relay opens**, which nobody
  would guess.
- Connections (Local / Private / Hybrid) and the servers switch are two
  separate settings, explained in networking terms.
- When a device can't be reached, nothing says whether that's because the
  servers are off, down, or the other device is closed.

## The plan

### 1. The servers on by default

- New installs: Relay's servers on, mode Hybrid.
- Upgrades: on as well, unless the person chose otherwise before. A
  person who turned the servers off, or picked Private or This Network
  Only, keeps that choice. To know who chose, the record gains
  `servers_chosen`; anyone who never touched the switch is treated as
  not having chosen.
- An upgrade that turns them on says so once, in the same way as the saved-data
  notice: "Relay now reaches your devices away from home through its
  servers. What you send stays encrypted end to end. Settings →
  Connections to change this."

### 2. Changes take effect at once

- Turning the servers on or off, or changing mode, applies to the running
  endpoint: the relay is added or removed (as mode changes already do),
  and the lookup is published or stops being published. No "next time
  Relay opens".
- If the lookup service can't be swapped on a running iroh endpoint,
  Relay rebinds the endpoint (same key) and says nothing to the person.

### 3. One Connections choice, in plain words

One setting replaces both, with three choices:

| Choice | Was | Says |
|---|---|---|
| **Everywhere** (default) | Hybrid + Relay's servers | Your devices reach each other from anywhere. When there's no direct way, an encrypted connection passes through Relay's server, which can't read it but sees that two devices are connected, when and how much. |
| **Private networks** | Private | Only the same Wi-Fi or a private network such as Tailscale. Nothing goes through Relay's servers. |
| **This network only** | Local | Only the same Wi-Fi or wired network. |

Own servers (self-hosting) move to an "Advanced" row under Everywhere:
the relay and lookup URLs, with `relay servers <relay> <lookup>` on the
command line as today. The wording comes from the security claims table
in the architecture doc; nothing claims more than it does.

### 4. Saying why a device can't be reached

The device screen already gives a reason. N2 makes it specific:

- "Relay's servers can't be reached right now" (a health check of the
  relay and lookup, cached for a minute), with local and Tailscale
  routes still tried.
- "Connections is set to Private networks, and laptop isn't on one with
  you." with a link to the setting.
- "laptop hasn't been online since 14:02" when the lookup has no fresh
  record for it.
- When it is reached: "Reached through Relay's server" or "Direct" in the
  device's route line (exists today).

### 5. The laptop and the command line

- `relay listen` uses the same default.
- `relay status` prints the mode, whether the servers answer, and this
  device's lookup record.
- Two fixes found in the acceptance run: a word-code transfer, and
  `sees-devices`, work while `relay listen` runs on the same data (they go
  through the listener, as `revoke` already does).

## Out of scope

- The mailbox (N3): a device that is closed still can't receive until it
  opens. N2 is about reaching a device that is open somewhere else.
- More relay regions, accounts, or paying for the servers.
- Background delivery while Relay is closed on the iPhone.

## Acceptance

On the iPhone and the laptop, from a fresh install of both:

1. Pair them once with a word code on the same Wi-Fi. Touch no settings.
2. Phone on 5G, Tailscale off: a message and a photo both ways, through
   the relay, once each.
3. An upgrade from today's build with the servers never touched: they are
   on, and the notice shows once. One with Private chosen: still Private,
   no notice.
4. Switch to Private networks: the next message waits; switch back to
   Everywhere: it goes within seconds, without reopening Relay.
5. Stop the relay server for a minute: the device screen says the servers
   can't be reached, and the same Wi-Fi still works.

Each check is recorded on its own, like the 1.0 run.

## Order of work

1. Record and default (`servers_chosen`, upgrade rule, notice), with
   tests for the three upgrade cases.
2. Applying changes at once, with a test that a mode switch reaches a
   device without a restart.
3. The Connections screen and its wording.
4. Reasons on the device screen and the server health check.
5. Command-line fixes and `relay status`.
6. The phone run.
