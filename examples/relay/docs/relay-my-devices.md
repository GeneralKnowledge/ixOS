# My Devices, step 1: an introducer

Status: built 2026-11-21, tested on Linux with three devices (laptop,
phone, tablet in one process over localhost). Swift not yet tried on a
phone. This is step 1 of P5 in the [audit](relay-audit-2026-11.md#6-p5-my-devices)
and of N1 in the network architecture: pair once, revoke once.

## What it does

You mark one of your trusted devices (usually the laptop) as an
introducer. From then on:

- A device the introducer trusts is **offered** here, with "Trust
  “tablet”? laptop trusts it." You still decide; nothing is trusted
  for you.
- A device the introducer **revoked** is revoked here too, and the list
  says "Revoked by laptop". You can trust it again yourself, which clears
  that.

It follows the review's authority model (refinement 1): recommending,
authorising and confirming stay separate. An introducer only recommends
additions; it can only take trust away, which is the safe direction, and
that is shown and can be undone.

## Two rights, both off by default

Trust admits and grants nothing else, so this is two new rights on the
trusted-device list, named like the others:

| Right | Set on | Means |
|---|---|---|
| Introduces Devices (`introduces`) | the phone, for the laptop | What the laptop lists is offered here, and what it revoked is revoked here |
| Can See My Devices (`sees_devices`) | the laptop, for the phone | The phone may ask the laptop which devices it trusts and revoked |

Both need to be on, one on each side. Turning either off stops it.

## On the wire

Two items on `relay/device/1`, the existing device protocol:

- `DevicesAsk`: "which devices do you trust?"
- `DeviceList { trusted: [(endpoint id, name)], revoked: [endpoint id] }`,
  or `Refused` when the asker lacks Can See My Devices. The list leaves
  out the asker. Ids are checked, names cleaned, at most 256 entries.

The phone asks when it opens its device service, when the introducer is
reached, when Introduces is turned on, and when the person taps "Check
Its Devices Now". No new transport, queue or identity.

## What it never does

- Trust a device for you.
- Revoke the introducer itself, or this device.
- Re-offer a device you revoked here: only you trust it again.
- Pass rights on: a device introduced here gets plain trust, with
  "Can Send to This Device" as usual and no other right.

## Command line

```
relay introduces <device> on|off
relay sees-devices <device> on|off
relay sync <device> [--accept]
```

`relay devices` shows "introduces devices", "sees your devices",
"introduced by" and "revoked by".

## Next

Step 2 is a signed device list (one identity for a person, the device
list signed by it) so a device learns of the others without one
introducer being reachable. The phone check for step 1: pair the tablet
with the laptop only, turn on both switches, and see the tablet offered
on the phone; revoke it on the laptop, and see it revoked on the phone.
