# N5: Capabilities and permissions

Status: plan and first build, 2026-11-22. Stage N5 in the
[network architecture](relay-network-architecture.md). Reuben, 2026-11-22:
"I think we should start building N5 and N6 (a small site)." N6, the
first capability, is [Sites over Relay](relay-web.md); the two are built
together, because a permission model is only proven by something that
uses it.

## The goal

A device can offer a service to its trusted devices, safely: each service
behind its own permission, which the person gives one device at a time.
New services come from the same model, not from a new transport, identity
or permission scheme of their own (the constraint of 2026-11-21).

## The rules

1. **Trust admits; it grants nothing.** Trusting a device lets it connect
   and say hello. Every capability is off for every device until the
   person turns it on for that device. A device trusted again (after it
   was revoked) starts with none.
2. **One grant per capability, per device.** Granting Sites to the laptop
   doesn't grant it to the iPad, and doesn't grant the laptop anything
   else. A trusted person's device can be granted a capability too: the
   person decides, device by device.
3. **Checked where it is used.** The service checks the grant when a
   connection opens and again on every request. An open connection is
   closed within a quarter of a second when the grant goes, the device is
   revoked, or the service stops, even in the middle of a download.
   Changing what the service offers (sharing another folder in place of
   the first) is not one of these: a request already running finishes
   from what it started on, and every later request uses the new one.
4. **Seen only by who may use it.** A device's status answer
   (`relay status <device>`) names a capability among its offers only
   when the asker may use it. A device without the grant can't tell that
   the service exists.
5. **Each capability has its own protocol** (ALPN) on the device's one
   iroh endpoint. Nothing is multiplexed through the device protocol, so
   a capability can be refused before a byte is read.
6. **Revoking or forgetting a device clears its grants.** Trusting it
   again does not bring them back.

## How it is stored

`SavedDevice::grants` is a list of capability names (`["sites"]`), so a
new capability needs no new field and no migration. A name from a newer
version is kept and grants nothing. The rights from before N5 keep their
own fields: may send here (on when trusted), may ask the AI, introduces
devices and sees the devices (all off when trusted). They follow the same
rules and are shown beside the new ones; folding them into the list is a
later clean-up, not part of N5.

The pure rules are in `trust.rs` (`set_grant`, `granted`) and
`capability.rs` (the `Capability` enum and its names), with unit tests,
as for trust and the device list.

## In the app and the CLI

- The device's screen gets one switch per capability, under the existing
  ones: **Can Open My Site**.
- `relay grant <device> sites on|off`; `relay devices` says
  "may open your site".
- The FFI: `set_grant(endpoint_id, capability, on)`, and `Device.grants`.

## What N5 doesn't do yet

- No time-limited or one-off grants. They fit the same list (a grant with
  an expiry) when something needs them.
- No grants for groups of devices. My Devices' signed list (N1) and
  people (N7) come first.
- No audit history of who used what. Relay logs counts only (rule 5,
  never user content); a visible "who opened your site" list can follow
  if the phone run shows it is wanted.

## Steps

1. **Grants, the first capability, Linux** (this PR, with N6's step 2):
   the model above, `relay/web/1`, the CLI, tests over localhost.
2. **iPhone, opening a site** (with step 1, PR #384): the switch on the
   device screen and the in-app site viewer. Swift compiled on the laptop.
   Sharing a folder from the phone is N6's step 4, a later PR.
3. **The real check:** a small page on the laptop opened from the phone
   over Relay only, plus the refusals (no grant, grant taken away,
   revoked device). On Hetzner only with Reuben's go-ahead.

## Decisions for Reuben

These go ahead as recommended unless he says otherwise:

1. **A person's device may be granted a capability** (rule 2). The
   alternative is own devices only until N7. Recommended: allow it, since
   sharing a site with someone you trust is a natural first use.
2. **Grants are off when a device is trusted,** including for your own
   devices. Recommended: keep it so, as for asking the AI.
