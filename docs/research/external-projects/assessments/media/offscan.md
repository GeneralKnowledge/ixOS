# Offscan: how a scan, OCR and PDF pipeline is built on Apple frameworks alone

Status: 2026-11-17. Part of the [External Projects Atlas](../../README.md), P0 investigation 4. Research only: nothing is added to ixOS by this page, and no scanner app is proposed.

## Question

Offscan is an offline document scanner built only on Apple's frameworks. What in its pipeline and architecture should ixOS borrow, and for which existing app, given that Rust owns state in ixOS and apps build with xtool on Linux?

## Evidence

| Field | Value |
|---|---|
| Date | 2026-11-17 |
| Inspected | [bartoszpiech/offscan](https://github.com/bartoszpiech/offscan) at `7b8d682` (2026-11-12) |
| Licence | MIT (`LICENSE`) |
| Activity | **Brand new:** three commits, all on 2026-11-12, one author, one star. The first commit is the whole app (about 21,000 lines of Swift in 221 files), so the history says nothing about how it was developed or will be maintained |
| Read or run | Source and `docs/ARCHITECTURE.md` read. Not built, not run |
| Build evidence | None. It builds with Xcode through XcodeGen (`project.yml`); its CI runs on macOS 26. Its logic package, FlatlyCore, is a plain SwiftPM package for iOS 17 that imports CoreGraphics, Core Image, Vision, ImageIO and PencilKit, so it can't build or test on Linux; whether it builds with xtool was not tried |
| Platform evidence | None from ixOS. Its README says iOS 17 or later and offers an unsigned IPA for sideloading |
| ixOS baseline | main at `98a8446`. Vision (OCR, document scanning) is item 18 of the [native capability roadmap](../../../../native-capability-roadmap.md); Relay already scans QR codes with VisionKit's `DataScannerViewController` (capability ledger, Barcode row). ForgeImages decodes pictures at the size shown. Persistence is Rust (`forge-storage`) |

## What it does

Camera or import (Photos, Files, PDF, TIFF) → document edge detection (Vision segmentation, then rectangles, then the full frame) → corner adjustment with a magnifying loupe → perspective correction → filters (colour, greyscale, an adaptive black and white that removes shadows) → OCR (Vision, accurate) → a searchable library → export as a searchable PDF with an invisible text layer, or JPEG. It has folders, merge, a 30-day Recently Deleted, signatures with PencilKit, and an optional Face ID lock. It has no networking code at all.

Its structure: **FlatlyCore** (a Swift package of `Sendable` structs with synchronous, throwing methods: detector, corrector, filter engine, renderer, text recognizer, exporters, search matcher), **FlatlyApp** (SwiftUI views, a SwiftData store that is the only mutation API, and an actor that runs FlatlyCore off the main actor).

## What ixOS should borrow

| # | Technique | Where in Offscan | Why it's good | Where it belongs in ixOS |
|---|---|---|---|---|
| 1 | **Non-destructive edits: keep the source, store a recipe.** Each page keeps `source.jpg` (the untouched photo), `original.jpg` (corrected), `processed.jpg` (rendered) and a `PageRenderRecipe` (crop, filter, rotation, signature) as JSON. Edits change the recipe and re-render from the original | `Imaging/PageRenderRecipe.swift`, `PageRenderer`, ARCHITECTURE §2 | Edges can be re-adjusted any number of times without compounding loss; changing a filter never damages the page | Any app that edits a photo: FieldKit's photos, Pocket Library's covers, Sketchbook imports. The recipe is state, so it lives in **Rust**; the rendering stays in Swift |
| 2 | **One pending OCR per page, debounced, and stale results dropped.** Each edit cancels the previous recognition and waits 1.5 s; results computed on an image that has since been replaced are discarded by a per-page render counter | ARCHITECTURE §2, `DocumentAssembler`, `ScanProcessor` | Trying five filters recognises text once, and a slow recognition can never overwrite a newer one | The pattern for any background work keyed to something the person is still editing. Rust already owns cancellation (`forge-runtime`); this is the rule for when to cancel |
| 3 | **Search text rebuilt from the pages in order** whenever pages change, matching that ignores case and diacritics and finds words hyphenated across lines | `Library/SearchMatcher.swift`, ARCHITECTURE §2 | Search never finds text that was removed, and finds what people actually type | Rust search in Commonplace and Pocket Library (wave 5 search) |
| 4 | **Recently Deleted for 30 days** for documents, restorable until then | `Library/DeletionRetention.swift`, `DocumentStore` | A third answer to the 2026-11-17 undo-or-confirm rule: costly things (books, trips, sketches) could go to Recently Deleted instead of a confirmation | A proposal for the undo decision, not a change: noted for ForgeAppKit's next review of that rule |
| 5 | **Two image cache budgets** (50 MB thumbnails, 100 MB pages), so opening a page never evicts the library's thumbnails | `Services/ImageCache.swift` | Scrolling back to the library doesn't reload every thumbnail | ForgeImages' memory cache (#298 adds a disk cache, not budgets) |
| 6 | **A demo camera feed** in debug builds: a still desk photo run through real live detection, so screenshots and UI tests show the scanner without a camera | `App/Demo/DemoCameraFeed` | Screens that need hardware can still be tested and captured | Device Lab's sample data (deferred from #286) and `forge ui` baselines |
| 7 | **A failed save isn't rolled back:** changes stay pending, one alert per run of failures, retried when the app goes to the background | `DocumentStore` | The person doesn't lose work to a transient failure, and isn't nagged | Compare with forge-storage's behaviour on a failed write; a question for the foundation review, not a change |

What ixOS should **not** take:

- **SwiftData as the store.** In ixOS state belongs to Rust; FlatlyCore's logic is in Swift and can't be tested on Linux. ixOS would put the recipe, the page list, the search index and the retention rules in Rust, and keep only the Vision, Core Image and PDF calls in Swift adapters.
- **A scanner app.** Following "expand existing apps before new ones", scanning would arrive as a capability in an existing app.
- **Its own camera** (about 1,900 of the app's 16,600 lines outside tests and tools). VisionKit's document camera does the same job with far less code; Offscan itself keeps it as an option. ixOS would start there.

## Compared with ixOS

ixOS has no OCR or document scanning yet. Roadmap item 18 plans Vision as an optional service for still images (Rust gets the text) and a helper for the live scanner. Offscan is a worked example of that item: everything it does uses Apple's frameworks, which fits ixOS's rule to wrap them rather than replace them. Its pipeline order (detect, correct, filter, recognise, export) and its file layout per page are directly reusable as a design. The first ixOS user would be an existing app, for example Commonplace (capture a printed page into a note) or Pocket Library (keep a quote from a paper book); that choice belongs to the roadmap, not to this assessment.

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | Strong | Scanning and OCR are capabilities ixOS doesn't have, and roadmap item 18 wants them |
| Product value | Adequate | Useful in Commonplace or Pocket Library, but no app has asked for it yet |
| Architecture | Weak as code, Strong as design | Swift owns state (SwiftData), which ixOS doesn't allow; the pipeline and recipe design translate cleanly to Rust plus Swift adapters |
| Platform | Unknown | Not run; it targets iOS 17 with sideloading in mind |
| Development | Unknown | Not built with xtool; its project is Xcode-generated. The parts ixOS would use are Apple framework calls that xtool's SDK has |
| Maintenance | Weak | Five days old, one author, three commits |
| Licence | Strong | MIT; snippets could be used with attribution, though the plan is to reimplement |
| Security | Strong | No networking; optional app lock |
| Integration cost | Adequate | Nothing is integrated; a Vision service and one app's scanning flow is a medium-sized feature |
| Reusability | Strong | The recipe, debounce and search techniques apply across several apps |

## Verdict

**Borrow** the techniques above, and **Study** the app as a reference for roadmap item 18. Don't depend on it, and don't build a scanner app.

The reason: it is a clear, well-documented example of a complete pipeline built only on Apple's frameworks, and several of its decisions (non-destructive recipes, debounced and stale-proof background work, search rebuilt from content) solve problems ixOS apps will meet. As a dependency it would be wrong twice over: state in Swift, and a project five days old.

Reopen when: an ixOS app takes on scanning (roadmap item 18), at which point read its `PerspectiveCorrector` and `FilterEngine` again for the image maths.

## Next step, if any

None now. When roadmap item 18 is scheduled, its plan should cite this assessment for the pipeline order, the per-page files and the recipe, and start from VisionKit's document camera.
