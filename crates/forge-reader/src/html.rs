//! HTML and XHTML into reader blocks: headings, paragraphs, quotes, list
//! items, preformatted text, scene breaks and images, with bold, italic,
//! code and links inside them. The source's CSS is not used: the host
//! chooses the look, so text follows the reader's settings, Dynamic Type
//! and dark mode.
//!
//! It reads an EPUB chapter (well-formed XHTML) and a feed article's HTML
//! fragment alike: end tags need not match, void elements need no slash,
//! HTML's common named entities are known, and a syntax error ends the
//! text read so far rather than failing.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::document::{assign_ids, ReaderBlock, ReaderBlockKind, ReaderSpan};

/// What a converter needs from the app: where pictures and links go.
pub trait HtmlResources {
    /// An `<img src>` (or SVG `<image href>`): the source to show it from
    /// (a saved file's absolute path, or an `https:` address), or `None`
    /// to leave it out.
    fn image(&mut self, src: &str) -> Option<String>;
    /// An `<a href>`: the absolute address to open, or `None` to show the
    /// text without a link.
    fn link(&mut self, href: &str) -> Option<String> {
        let _ = href;
        None
    }
}

/// Resources for web content (a feed article): images stay on the web and
/// relative addresses are resolved against the page's own. Only `https:`
/// and `http:` images and `https:`, `http:` and `mailto:` links are kept.
pub struct WebResources {
    base: Option<url::Url>,
}

impl WebResources {
    /// `base` is the address the HTML came from (an article's link), if known.
    pub fn new(base: Option<&str>) -> Self {
        Self {
            base: base.and_then(|b| url::Url::parse(b).ok()),
        }
    }

    fn absolute(&self, address: &str) -> Option<url::Url> {
        let address = address.trim();
        if address.is_empty() {
            return None;
        }
        match url::Url::parse(address) {
            Ok(url) => Some(url),
            Err(url::ParseError::RelativeUrlWithoutBase) => self.base.as_ref()?.join(address).ok(),
            Err(_) => None,
        }
    }
}

impl HtmlResources for WebResources {
    fn image(&mut self, src: &str) -> Option<String> {
        let url = self.absolute(src)?;
        matches!(url.scheme(), "https" | "http").then(|| url.to_string())
    }

    fn link(&mut self, href: &str) -> Option<String> {
        let url = self.absolute(href)?;
        matches!(url.scheme(), "https" | "http" | "mailto").then(|| url.to_string())
    }
}

/// Turns HTML (or a chapter's XHTML) into blocks with stable ids. Text
/// before `<body>` is skipped when there is a body; a fragment without one
/// is read whole.
pub fn blocks_from_html(html: &str, resources: &mut dyn HtmlResources) -> Vec<ReaderBlock> {
    let mut reader = Reader::from_reader(html.as_bytes());
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    let mut b = Builder::default();
    let has_body = html.contains("<body");
    let mut in_body = !has_body;
    // A malformed document still shows what was read before the error.
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(e) => {
                let name = lower(e.local_name().as_ref());
                if name == "body" {
                    in_body = true;
                    continue;
                }
                if in_body {
                    b.start(&name, &e, resources);
                }
            }
            Event::Empty(e) => {
                if in_body {
                    let name = lower(e.local_name().as_ref());
                    b.empty(&name, &e, resources);
                }
            }
            Event::End(e) => {
                let name = lower(e.local_name().as_ref());
                if name == "body" {
                    in_body = false;
                    continue;
                }
                if in_body {
                    b.end(&name);
                }
            }
            Event::Text(t) if in_body && b.skip == 0 => b.text(&t.xml10_content()),
            Event::CData(t) if in_body && b.skip == 0 => b.text(&t.xml10_content()),
            Event::GeneralRef(r) if in_body && b.skip == 0 => b.text(&resolve_entity(
                &r.xml10_content(),
                r.resolve_char_ref().ok().flatten(),
            )),
            Event::Eof => break,
            _ => {}
        }
    }
    b.flush();
    let mut blocks = b.blocks;
    assign_ids(&mut blocks);
    blocks
}

fn lower(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// An attribute's value by local name (`href` matches `xlink:href`).
pub fn attribute(e: &BytesStart<'_>, local: &str) -> Option<String> {
    e.html_attributes().flatten().find_map(|a| {
        a.key
            .local_name()
            .as_ref()
            .eq_ignore_ascii_case(local)
            .then(|| {
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_else(|_| a.value.to_string())
            })
    })
}

/// A named or numeric entity's text. Unknown names become a space rather
/// than vanish, so words either side stay apart.
pub fn resolve_entity(name: &str, char_ref: Option<char>) -> String {
    if let Some(c) = char_ref {
        return c.to_string();
    }
    match name {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "rsquo" => "’",
        "lsquo" => "‘",
        "ldquo" => "“",
        "rdquo" => "”",
        "laquo" => "«",
        "raquo" => "»",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "deg" => "°",
        "middot" => "·",
        "bull" => "•",
        "times" => "×",
        "eacute" => "é",
        "egrave" => "è",
        "aacute" => "á",
        "agrave" => "à",
        "ouml" => "ö",
        "uuml" => "ü",
        "auml" => "ä",
        "szlig" => "ß",
        "ccedil" => "ç",
        "euro" => "€",
        "pound" => "£",
        "shy" => "",
        "zwj" => "\u{200d}",
        "zwnj" => "\u{200c}",
        _ => " ",
    }
    .to_string()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Heading(u8),
    Paragraph,
    ListItem,
    Pre,
    Quote,
    /// `<ul>`/`<ol>`: list items need their marker.
    List,
}

#[derive(Default)]
struct Builder {
    blocks: Vec<ReaderBlock>,
    spans: Vec<ReaderSpan>,
    /// Open block elements, innermost last (with the element's name, so a
    /// stray end tag closes nothing it should not).
    containers: Vec<(String, Container)>,
    /// Counters of the open lists: `Some(n)` for `<ol>`, `None` for `<ul>`.
    lists: Vec<Option<u32>>,
    /// The marker of the list item being read.
    marker: Option<String>,
    /// The source id of the block element opened last, for the next block.
    pending_id: Option<String>,
    bold: u32,
    italic: u32,
    mono: u32,
    /// Open links, innermost last (`None` for one the app didn't keep).
    links: Vec<Option<String>>,
    /// Inside `<script>`, `<style>`, `<head>`…: ignore text.
    skip: u32,
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "aside"
            | "main"
            | "nav"
            | "figure"
            | "figcaption"
            | "blockquote"
            | "li"
            | "ul"
            | "ol"
            | "dl"
            | "dt"
            | "dd"
            | "pre"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "caption"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

/// Elements whose text is never shown.
fn is_hidden(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "head" | "title" | "noscript" | "template" | "iframe" | "object"
    )
}

impl Builder {
    fn start(&mut self, name: &str, e: &BytesStart<'_>, resources: &mut dyn HtmlResources) {
        match name {
            _ if is_hidden(name) => self.skip += 1,
            // HTML void elements, written without a slash.
            "br" | "hr" | "img" | "image" | "source" | "wbr" | "input" | "meta" | "link" => {
                self.empty(name, e, resources)
            }
            "b" | "strong" => self.bold += 1,
            "i" | "em" | "cite" | "dfn" | "var" => self.italic += 1,
            "code" | "tt" | "kbd" | "samp" => self.mono += 1,
            "a" => {
                let link = attribute(e, "href").and_then(|href| resources.link(&href));
                self.links.push(link);
                self.note_id(e);
            }
            _ if is_block(name) => {
                self.flush();
                self.note_id(e);
                let container = match name {
                    "h1" => Container::Heading(1),
                    "h2" => Container::Heading(2),
                    "h3" => Container::Heading(3),
                    "h4" => Container::Heading(4),
                    "h5" => Container::Heading(5),
                    "h6" => Container::Heading(6),
                    "pre" => Container::Pre,
                    "blockquote" => Container::Quote,
                    "ul" => {
                        self.lists.push(None);
                        Container::List
                    }
                    "ol" => {
                        let start = attribute(e, "start")
                            .and_then(|s| s.trim().parse::<u32>().ok())
                            .unwrap_or(1);
                        self.lists.push(Some(start));
                        Container::List
                    }
                    "li" => {
                        self.marker = Some(match self.lists.last_mut() {
                            Some(Some(n)) => {
                                let marker = format!("{n}.");
                                *n += 1;
                                marker
                            }
                            _ => "•".to_string(),
                        });
                        Container::ListItem
                    }
                    _ => Container::Paragraph,
                };
                self.containers.push((name.to_string(), container));
            }
            _ => {}
        }
    }

    /// Remembers an element's `id` for the block it starts (an anchor at
    /// the start of a block names that block too).
    fn note_id(&mut self, e: &BytesStart<'_>) {
        if !self.spans.is_empty() {
            return;
        }
        let id = attribute(e, "id").or_else(|| attribute(e, "name"));
        if let Some(id) = id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
        {
            self.pending_id = Some(id);
        }
    }

    fn empty(&mut self, name: &str, e: &BytesStart<'_>, resources: &mut dyn HtmlResources) {
        match name {
            "br" => self.push_text("\n", true),
            "hr" => {
                self.flush();
                self.push_block(ReaderBlockKind::Separator, Vec::new());
            }
            "img" | "image" => {
                let src = if name == "img" {
                    attribute(e, "src")
                } else {
                    // SVG <image xlink:href> (cover pages).
                    attribute(e, "href")
                };
                if let Some(source) = src.and_then(|src| resources.image(&src)) {
                    self.flush();
                    let alt = attribute(e, "alt").unwrap_or_default();
                    self.push_block(
                        ReaderBlockKind::Image {
                            source,
                            alt: collapse(&alt),
                        },
                        Vec::new(),
                    );
                }
            }
            "a" => self.note_id(e),
            _ if is_block(name) => {
                self.flush();
                self.note_id(e);
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        match name {
            _ if is_hidden(name) => self.skip = self.skip.saturating_sub(1),
            "b" | "strong" => self.bold = self.bold.saturating_sub(1),
            "i" | "em" | "cite" | "dfn" | "var" => self.italic = self.italic.saturating_sub(1),
            "code" | "tt" | "kbd" | "samp" => self.mono = self.mono.saturating_sub(1),
            "a" => {
                self.links.pop();
            }
            _ if is_block(name) => {
                self.flush();
                // Close up to the matching element; ignore a stray end tag.
                if let Some(at) = self.containers.iter().rposition(|(n, _)| n == name) {
                    for (_, container) in self.containers.drain(at..) {
                        if container == Container::List {
                            self.lists.pop();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn in_pre(&self) -> bool {
        self.containers.iter().any(|(_, c)| *c == Container::Pre)
    }

    fn text(&mut self, text: &str) {
        let pre = self.in_pre();
        self.push_text(text, pre);
    }

    /// Adds text in the current style. Outside `<pre>`, runs of whitespace
    /// become one space, and none at the start of a block or line.
    fn push_text(&mut self, text: &str, keep_whitespace: bool) {
        let mut out = String::with_capacity(text.len());
        let mut last = self
            .spans
            .last()
            .and_then(|s| s.text.chars().last())
            .unwrap_or('\n');
        for c in text.chars() {
            if keep_whitespace {
                out.push(c);
                last = c;
            } else if c.is_whitespace() && c != '\u{a0}' {
                // A no-break space is deliberate: kept.
                if last != ' ' && last != '\n' {
                    out.push(' ');
                    last = ' ';
                }
            } else {
                out.push(c);
                last = c;
            }
        }
        if out.is_empty() {
            return;
        }
        let (bold, italic, monospace) = (self.bold > 0, self.italic > 0, self.mono > 0);
        let link = self.links.last().cloned().flatten();
        match self.spans.last_mut() {
            Some(span)
                if (span.bold, span.italic, span.monospace) == (bold, italic, monospace)
                    && span.link == link =>
            {
                span.text.push_str(&out)
            }
            _ => self.spans.push(ReaderSpan {
                text: out,
                bold,
                italic,
                monospace,
                link,
            }),
        }
    }

    /// Ends the block being read, if it has any text.
    fn flush(&mut self) {
        let mut spans = std::mem::take(&mut self.spans);
        let pre = self.in_pre();
        if !pre {
            // A space before a line break, and blank edges, are not text.
            for span in &mut spans {
                while span.text.contains(" \n") {
                    span.text = span.text.replace(" \n", "\n");
                }
            }
            if let Some(first) = spans.first_mut() {
                first.text = first.text.trim_start().to_string();
            }
            if let Some(last) = spans.last_mut() {
                last.text = last.text.trim_end().to_string();
            }
        } else if let Some(last) = spans.last_mut() {
            last.text = last.text.trim_end_matches(['\n', '\r']).to_string();
        }
        spans.retain(|s| !s.text.is_empty());
        if spans.is_empty() {
            return;
        }
        let kind = self.kind();
        let kind = if kind == ReaderBlockKind::Paragraph && is_scene_break(&spans) {
            spans.clear();
            ReaderBlockKind::Separator
        } else {
            kind
        };
        self.push_block(kind, spans);
    }

    fn kind(&mut self) -> ReaderBlockKind {
        let innermost = self
            .containers
            .iter()
            .rev()
            .map(|(_, c)| *c)
            .find(|c| *c != Container::Paragraph && *c != Container::List);
        let quoted = self.containers.iter().any(|(_, c)| *c == Container::Quote);
        match innermost {
            Some(Container::Heading(level)) => ReaderBlockKind::Heading { level },
            Some(Container::Pre) => ReaderBlockKind::Preformatted,
            // The first block of an item carries its marker; later
            // paragraphs of the same item are plain.
            Some(Container::ListItem) => match self.marker.take() {
                Some(marker) => ReaderBlockKind::ListItem { marker },
                None if quoted => ReaderBlockKind::Quote,
                None => ReaderBlockKind::Paragraph,
            },
            _ if quoted => ReaderBlockKind::Quote,
            _ => ReaderBlockKind::Paragraph,
        }
    }

    fn push_block(&mut self, kind: ReaderBlockKind, spans: Vec<ReaderSpan>) {
        // Two scene breaks in a row are one.
        if kind == ReaderBlockKind::Separator
            && self
                .blocks
                .last()
                .is_some_and(|b| b.kind == ReaderBlockKind::Separator)
        {
            return;
        }
        let mut block = ReaderBlock::new(kind, spans);
        if let Some(id) = self.pending_id.take() {
            block.id = id;
        }
        self.blocks.push(block);
    }
}

/// "* * *", "#", "~~~", "⁂" and the like on their own.
fn is_scene_break(spans: &[ReaderSpan]) -> bool {
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
    let marks: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    !marks.is_empty()
        && marks.len() <= 12
        && marks.iter().all(|c| {
            matches!(
                c,
                '*' | '#' | '~' | '•' | '·' | '⁂' | '❧' | '◆' | '♦' | '§' | '_' | '='
            )
        })
}

#[cfg(test)]
mod tests;
