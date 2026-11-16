# Benchmark: NetNewsWire

**Reference:** NetNewsWire for iOS, an open-source feed reader (MIT licence), installed from the App Store.
**Our app:** [Commonplace](../../README.md), "a native, local-first reading application built with ixOS", written from scratch with its own name and look.
**Status (2026-11-15):** Commonplace slice 2 (the reader on ForgeReader, search, settings, links in the web view) is built and tested on Linux; slice 1's flows passed on an iPhone on 2026-11-10. The reference has not been walked yet, so the [spec](spec.md) is a list of what to confirm. The comparison is now walked by a person ([walk.md](walk.md)), because the explorer is frozen in forge-lab.

## Scope

| Area | What we reproduce | Slice |
|---|---|---|
| Feed management | Add a feed by address, unsubscribe, rename | 1 (done) |
| Timeline | Articles newest first, unread state, smart timelines (unread, bookmarks, all) | 1 (done) |
| Interactions | Read and unread, bookmark (NetNewsWire calls it starring), mark all read, refresh | 1 (done) |
| Persistence | SQLite on the device, survives relaunch | 1 (done) |
| Reader | Article text, images and links, text size and font | 2 (done), on [ForgeReader](../../../../docs/forge-reader.md) |
| Organise | Folders, OPML import and export | 3 |
| Search | Search saved articles (SQLite FTS5) | 2 (done) |
| Settings | Timeline order, hide read, summaries, where links open, font | 2 (done) |
| Web view | The original page and links inside the app | 2 (done; Show Original Page was in slice 1) |

Left out: accounts and sync services (Feedbin, iCloud and the like), widgets, share and Safari extensions, background refresh until the roadmap's background tasks exist.

## Running the comparison

A person walks both apps side by side with [walk.md](walk.md): about ten minutes, no explorer. Commonplace is in Device Lab (Apps > Commonplace), installed by cable with `forge run ios`; NetNewsWire comes from the App Store.

1. **Fixture feeds.** On the laptop, `python3 examples/commonplace/docs/benchmark/fixtures/serve.py` serves [the fixtures](fixtures/) and prints the address. Both apps subscribe to the same feeds, so they show the same content. The phone must reach the laptop (same Wi-Fi, or the tailnet address); Commonplace asks for local network access the first time.
2. **Walk** the steps in [walk.md](walk.md), doing each in NetNewsWire and then in Commonplace, and mark each Same, Different (one line on how) or Missing.
3. **Record** the result: rows of [spec.md](spec.md) move from Expected to Seen with the date, differences become records in [gaps.toml](gaps.toml) (a finding about ixOS) or accepted differences in the spec, and what was reused goes in [patterns.toml](patterns.toml).

Commonplace's own flows still run as UI tests on the phone when the cable is in (`forge ui run examples/commonplace/uitests/subscribe-and-read.toml`). Walking NetNewsWire with the explorer (`forge-lab explore --app com.ranchero.NetNewsWire-iOS --any-app`) still works, but it is frozen and not needed for this comparison.

## Limits

- Forge sees the reference only through its accessibility tree and screenshots: never its database or memory. Persistence is checked by relaunching.
- Swipe actions, context menus and pull to refresh need gestures the explorer does not make; a person checks them, or they are reached through buttons that do the same.
- Screen ids differ between the two apps (they share no identifiers), so the mapping from our screens to theirs is written by hand in the spec. Doing it automatically is deferred.
