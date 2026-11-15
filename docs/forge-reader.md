# ForgeReader: the native reader

ForgeReader is the reading experience apps share: a book's chapter or a feed's article becomes a **section of styled blocks**, SwiftUI lays the blocks out natively (so text follows Dynamic Type, dark mode and the reader's own text settings), and a reading position is an **anchor on a block** that survives a change of text size, layout or the text around it. It is not tied to EPUB or feeds: formats convert into it. Its design is in the [native capability roadmap](native-capability-roadmap.md#native-reader).

It was extracted from Pocket Library when Commonplace became its second user. Today:

| App | What it reads | What it uses |
|---|---|---|
| Pocket Library | EPUB chapters (its own EPUB code finds them, ForgeReader turns their XHTML into blocks) | Sections, anchors for the saved place, the section view, the text menu. Read-aloud is still Pocket Library's own |
| Commonplace | Feed articles (their HTML, with links and pictures) | Sections, the section view, the text menu |

## The Rust side: `crates/forge-reader`

```rust
// In the app crate's lib.rs, after uniffi::setup_scaffolding!():
forge_reader::reader_types!();
```

That one line makes `ReaderSection`, `ReaderBlock`, `ReaderBlockKind`, `ReaderSpan`, `ReaderAnchor` and `ReaderLocation` records of the app's own bindings, so the app's records can hold them, and makes `forge bindings generate` copy ForgeReader's SwiftUI views into the app's Swift module. The app crate needs `uniffi` as a direct dependency (it already has it).

| Piece | What it does |
|---|---|
| `ReaderSection { id, title, blocks }` | One scroll: a chapter, an article. Its id is stable within the document (an EPUB chapter's path, `"article"`) |
| `ReaderBlock { id, kind, spans }` | A heading, paragraph, quote, list item, preformatted text, scene break or picture. Spans carry bold, italic, code and links |
| `blocks_from_html(html, &mut resources)` | HTML or XHTML into blocks with ids. Lenient: end tags need not match, void elements need no slash, a syntax error keeps what was read. `HtmlResources` says where pictures and links go; `WebResources::new(Some(page_url))` keeps web pictures and `https`, `http` and `mailto` links, resolved against the page |
| Block ids | The source's own `id` (or an anchor's `name` at the start of the block); otherwise a hash of the block's kind and normalised text, with `-2`, `-3`… for repeats. Never the block's index |
| `ReaderAnchor::at(section, location)` | Saves a place: section id, block id, how far into the block, the block's opening words and the ids either side |
| `anchor.resolve(section)` | Finds it again: the same block; else a block opening with the same words (edited); else next to a surviving neighbour; else the section's start. Says which (`AnchorMatch`) |
| `section_progress`, `location_at` | A place as a fraction of the section by words, and back: for progress bars and page numbers |

The types derive `serde` with the `serde` feature, so an app can keep anchors in its own saved files (Pocket Library keeps them in `library.json`).

## The Swift side

`ForgeReader.swift` compiles into the app's bindings module (empty on Linux). Rust owns the document and what a place means; the views draw blocks and report places.

```swift
@State private var topBlock: Int?          // the block at the top of the screen
@State private var scrollTarget: Int?      // set to restore a saved place

ForgeReaderSectionView(
    blocks: section.blocks,
    topBlock: $topBlock,
    scrollTarget: $scrollTarget,
    header: { Text(title).font(.title2.bold()) },
    footer: { Button("Next Chapter") { … } }
)
.toolbar { ForgeReaderTextMenu(identifier: "reader.textSettings") }
```

- `ForgeReaderSectionView` scrolls one section, keeps `topBlock` current (the footer is `blocks.count`), scrolls to `scrollTarget` once its row exists and then clears it, and highlights `spokenBlock`/`spokenWord` for read-aloud.
- `ForgeReaderBlockView` draws one block; links open through the environment's `openURL`, so the app decides where they go (Commonplace sends them to its web sheet or Safari, as its settings say).
- `ForgeReaderImage` shows a saved file (ImageIO, downscaled) or a picture on the web (`AsyncImage`).
- `ForgeReaderTextMenu` is the Larger, Smaller, System Text Size and Font menu. Its settings (`ForgeReaderSettings.textStepKey`, `.serifKey`) are shared by every reader in the app.

The app saves a place by sending the block index to Rust, which makes the anchor from the section it handed out (Pocket Library keeps the chapter it loaded last for that), and restores one by asking Rust where the anchor is now.

## Not yet

- Read-aloud moves into ForgeReader when a second app reads aloud.
- Tables become rows of paragraphs; footnotes are ordinary links; pictures on the web are not cached.
- The Swift views are compiled on the laptop's build queue and tried on the iPhone through Device Lab; until then the [ledger](capability-ledger.md) says "iPhone: No".
