# dom_smoothie: finding the article in a web page

Status: 2026-11-20. Part of the [External Projects Atlas](../../README.md). Adopted in Commonplace for slice C4.

## Question

Commonplace's C4 ([ports plan](../../../android-ports.md)) fetches the full article from its web page for feeds that send only a summary. That needs a readability extractor: something that finds the main text of an arbitrary, often malformed HTML page and leaves out navigation, sidebars and footers. It must be pure Rust (it runs in the app's core and cross-compiles for iOS from Linux), permissively licensed, and must not fetch anything itself.

## Evidence

| Field | Value |
|---|---|
| Date | 2026-11-20 |
| Inspected | `dom_smoothie` 0.19.0 from crates.io (source in the cargo registry) |
| Licence | MIT. Its dependency tree is MIT, Apache-2.0, MPL-2.0 (cssparser/selectors, file-level), Zlib and Unlicense: all in `deny.toml`'s allow list |
| Activity | Regular releases through 2026 |
| Read or run | Read `src/readability.rs` and `src/config.rs`; run in Commonplace's tests against hand-written blog pages |
| Build evidence | Builds and its tests pass on Linux in `examples/commonplace/rust`. Its tree has no C code (`cc` and `*-sys` come only from SQLite), so it adds nothing to the iOS cross-compile beyond Rust. Not yet built for the iPhone |
| Platform evidence | None on the phone yet; rides the next combined Device Lab build |
| ixOS baseline | ForgeReader turns HTML into reader blocks with quick-xml, but has no way to tell an article from the page around it |

## What it does

A port of Mozilla's Readability.js on `dom_query` (html5ever's HTML5 parser). `Readability::new(html, Some(url), Some(Config))` then `parse()` returns an `Article` with the content as HTML (relative links and pictures resolved against `url`), its plain text, title, byline and excerpt. `Config::max_elements_to_parse` bounds the work on huge pages. It does no networking and runs no scripts.

## Compared with ixOS

Nothing in ixOS does this. Writing our own scorer would repeat Readability's long tail of heuristics. The alternative crate `readability` (MIT) pulls in `reqwest` by default and is older. Only Commonplace uses it, so it stays an app dependency (rule: extract on a second use). Pocket Library has no need for it; a podcast's show notes in Soundscape might, later.

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | Good | Readability's algorithm, with the article's pictures and links |
| Product value | High | Feeds that send a summary become readable in the app |
| Architecture | Good | A pure function from page to HTML, called from Rust work off the command thread |
| Platform | Good | Pure Rust |
| Development | Good | Small API |
| Maintenance | Fair | One main maintainer |
| Licence | Good | MIT, dependencies all allowed |
| Security | Fair | Parses untrusted HTML; html5ever is widely used; no scripts run; page size and element count are bounded in Commonplace |
| Integration cost | Fair | About 60 crates (html5ever and its kin): build time and some binary size |
| Reusability | Fair | Any app reading web pages |

## Verdict

**Adopt** in Commonplace only.

Reopen when: a second app needs article extraction, or the binary size it adds to Device Lab matters.

## Next step, if any

None beyond C4.
