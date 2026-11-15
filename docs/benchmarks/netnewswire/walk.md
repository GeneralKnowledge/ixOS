# The NetNewsWire comparison walk

A person's side-by-side walk of NetNewsWire and Commonplace, about ten minutes. Do each step in NetNewsWire, then in Commonplace (Device Lab > Apps > Commonplace), and write **Same**, **Different: …** (one line) or **Missing** beside it. Nothing here needs the cable or the laptop beyond the fixture server.

**Before:** the fixture server is running on the laptop (`python3 docs/benchmarks/netnewswire/fixtures/serve.py`, which prints the address), and both apps are subscribed to `http://<address>:8765/rss.xml`. Subscribing is step 1 if they aren't.

| # | Step | Spec | NetNewsWire | Commonplace |
|---|---|---|---|---|
| 1 | Add the fixture feed by its address | F1, S4 | | |
| 2 | Look at the feeds list: smart lists, the feed, unread counts, toolbar | S1 | | |
| 3 | Open the feed: order, unread markers, summaries | S2 | | |
| 4 | Open "Hello from the fixture": bold text, the link, two paragraphs; back in the list it's read | S3, F2 | | |
| 5 | Tap the article's link: where does it open? | F10 | | |
| 6 | Open "A long article for scrolling": headings, the list, the quotation; make the text larger, then smaller | S3, F11 | | |
| 7 | Open "An article with an image": the picture (the fixture's picture address may not load; note what each app shows instead) and its caption | S3 | | |
| 8 | Star (bookmark) an article; find it under Starred (Bookmarks) | F3 | | |
| 9 | Search for "quotation": the long article is found and opens | S7, F8 | | |
| 10 | Open Settings: what can be changed | S5 | | |
| 11 | Turn on the oldest-first order (Commonplace: Oldest First) and hiding read articles; look at the feed again | F9 | | |
| 12 | Mark all as read in the feed | F4 | | |
| 13 | Close each app from the app switcher and open it again: feeds, read state, bookmarks and settings are kept | F7 | | |

**After:** send the table back (a photo is fine). Each row's result goes into [spec.md](spec.md) (Expected becomes Seen, with the date), and each Different or Missing that says something about ixOS becomes a record in [gaps.toml](gaps.toml).
