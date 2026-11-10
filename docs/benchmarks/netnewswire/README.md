# Benchmark: NetNewsWire

**Reference:** NetNewsWire for iOS, an open-source feed reader (MIT licence), installed from the App Store.
**Our app:** [Commonplace](../../../examples/commonplace/README.md), "a native, local-first reading application built with ixOS", written from scratch with its own name and look.
**Status (2026-11-10):** B0. Commonplace slice 1 is built and tested on Linux; the explorer can walk another app (Linux-tested). Nothing has run on the phone yet: the reference has not been walked, so the [spec](spec.md) is a list of what to confirm.

## Scope

| Area | What we reproduce | Slice |
|---|---|---|
| Feed management | Add a feed by address, unsubscribe, rename | 1 (done) |
| Timeline | Articles newest first, unread state, smart timelines (unread, bookmarks, all) | 1 (done) |
| Interactions | Read and unread, bookmark (NetNewsWire calls it starring), mark all read, refresh | 1 (done) |
| Persistence | SQLite on the device, survives relaunch | 1 (done) |
| Reader | Article text, images and links | 2, on the native reader ([roadmap](../../native-capability-roadmap.md) Tier 1); slice 1 shows plain paragraphs |
| Organise | Folders, OPML import and export | 3 |
| Search | Search saved articles (SQLite FTS5) | 4 |
| Settings | Appearance and reading preferences | 4 |

Left out: accounts and sync services (Feedbin, iCloud and the like), widgets, share and Safari extensions, background refresh until the roadmap's background tasks exist.

## Running the comparison

Everything runs from the laptop with the phone on USB (Linux, no Mac).

1. **Fixture feeds.** `python3 docs/benchmarks/netnewswire/fixtures/serve.py` serves [the fixtures](fixtures/) on the laptop and prints the address. Both apps subscribe to the same feeds, so they show the same content. The phone and the laptop must be on the same network; Commonplace asks for local network access the first time.
2. **The reference.** Install NetNewsWire from the App Store, subscribe it to the fixture feeds by hand, and give it a line in `~/.config/forge/device-control.toml`:

   ```toml
   [apps]
   "com.ranchero.NetNewsWire-iOS" = "operate"   # taps on real data (marks articles read); "full" only if it holds test feeds alone
   ```

   Confirm the bundle id with `forge device apps` first. Then walk it:

   ```bash
   forge ui explore --app com.ranchero.NetNewsWire-iOS --any-app --real-data --actions 40 --seed 1
   ```

   The screens, `graph.json` and the policy journal go to `~/.cache/forge/any-app/`, never into the repository: they are screenshots of an app on the person's phone.
3. **Our app.** `forge run ios --project examples/commonplace`, then the same walk on test data, and the flows:

   ```bash
   forge ui explore --project examples/commonplace --actions 40 --seed 1 --text addFeed.url=http://<laptop>:8765/rss.xml
   forge ui run examples/commonplace/uitests/subscribe-and-read.toml
   ```

4. **Compare** screen by screen and flow by flow as [spec.md](spec.md) lists them, and record what differs in [gaps.toml](gaps.toml) and what was reused in [patterns.toml](patterns.toml).

## Limits

- Forge sees the reference only through its accessibility tree and screenshots: never its database or memory. Persistence is checked by relaunching.
- Swipe actions, context menus and pull to refresh need gestures the explorer does not make; a person checks them, or they are reached through buttons that do the same.
- Screen ids differ between the two apps (they share no identifiers), so the mapping from our screens to theirs is written by hand in the spec. Doing it automatically is deferred.
