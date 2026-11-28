# Business Preview

A local business's own app, made from its website, to show the owner. It is milestone M1 of [from a business website to its own app](../../docs/research/business-to-app.md): BusinessForge's extractor reads a business's website into a profile pack, a person checks it, and this app shows it on the iPhone as the business's app, in their colours, with their services, hours and contacts, and a Book button that opens their own booking page. It runs inside Device Lab (Apps, Business Preview).

Rust owns the rules (the profile format itself is BusinessForge's [`business-profile`](https://github.com/GeneralKnowledge/BusinessForge) crate, pinned by commit):

- **Packs.** A folder, a zip of one, or a lone `profile.json`. The profile must be valid for `business-profile/0`; every image it names must be in the pack, a PNG or JPEG of at most 4 MB and 6000 pixels a side; and when the pack has a `hashes.json`, every file must match it. The extractor's review list and page text are left behind.
- **The copy on the phone.** Each prospect's files are kept in `packs/<id>/` with their SHA-256, checked again every time the preview opens: a changed copy is refused, never shown as the business. A second pack of the same business replaces the first.
- **What the preview shows.** Only prices a person has verified (the others are counted, and the Services screen says some are left out). "Concept preview, not an official app" on every screen while `preview.status` is `concept`. The category's words (Lessons and Book a Lesson for a riding school). The brand colour, darkened or lightened only as far as WCAG AA needs in light and dark mode. Book opens their own page in Safari's in-app view, or calls or emails them; nothing else.

## Using it

1. On the computer, make a pack with BusinessForge's extractor (`extract <url> --out <folder>`), review it, and zip the folder.
2. Get it to the phone: AirDrop or Relay it into Files, or put it at an https address.
3. In Device Lab, open Business Preview, tap **+** and import it **From Files** or **From a Web Address**.
4. Tap the business to open its app. **Present** hides our buttons, so the owner sees only their app; a long press anywhere brings them back. **Done** goes back to the list. Swipe a prospect (or long-press it) to delete it.

## Layout

| Path | What |
|---|---|
| `rust/src/pack.rs` | Reading and checking a pack: folder, zip or lone profile; images; hashes |
| `rust/src/view.rs` | What the screens show: verified prices, the concept label, words, colours, contact URLs |
| `rust/src/lib.rs` | The UniFFI API (`BusinessPreviewCore`): the prospects list, import, delete, open |
| `rust/tests/preview.rs` | The rules end to end, with a made-up riding school in `rust/tests/fixtures` |
| `Sources/AppHost/` | The SwiftUI screens (`ContentView.swift`, `PreviewScreens.swift`) and their models (`ProspectsModel.swift`) |

## Checking it

```bash
cd rust && cargo test
```

On the phone (about 10 minutes): import a pack, every screen in light and dark at the largest text size, Book opens the right page, Present and the long press out of it, delete. `uitests/smoke.toml` checks only the empty list and the import menu, since showing a business needs a pack on the phone.

Not yet: opening a pack straight from the share sheet ("Open in Device Lab") needs a document type in Device Lab's settings, which every hosted app would share; for now packs come in through Files or an https address.
