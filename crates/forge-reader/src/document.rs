//! The reader document: sections of styled text blocks. Formats (an EPUB
//! chapter, a feed article's HTML) convert into it; the host lays it out.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A run of text in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReaderSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub monospace: bool,
    /// Where the text links to (an absolute `https:`, `http:` or `mailto:`
    /// address), if the format's converter kept the link.
    pub link: Option<String>,
}

impl ReaderSpan {
    /// Unstyled text.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            monospace: false,
            link: None,
        }
    }
}

/// What a block is. The host chooses how each looks.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ReaderBlockKind {
    /// `level` 1–6, as `<h1>`–`<h6>`.
    Heading {
        level: u8,
    },
    Paragraph,
    /// Inside a `<blockquote>`.
    Quote,
    /// `marker` is "•" or "3." and so on.
    ListItem {
        marker: String,
    },
    /// Keep line breaks and spacing; show in a fixed-width font.
    Preformatted,
    /// A scene break: `<hr>`, or a paragraph of only "* * *" and the like.
    Separator,
    /// A picture: `source` is an absolute file path, or an `https:`/`http:`
    /// address for content that lives on the web.
    Image {
        source: String,
        alt: String,
    },
}

impl ReaderBlockKind {
    /// A short fixed name, part of a content id.
    fn tag(&self) -> &'static str {
        match self {
            Self::Heading { .. } => "h",
            Self::Paragraph => "p",
            Self::Quote => "q",
            Self::ListItem { .. } => "li",
            Self::Preformatted => "pre",
            Self::Separator => "hr",
            Self::Image { .. } => "img",
        }
    }
}

/// One paragraph, heading, image… in reading order.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReaderBlock {
    /// Stable within its section: the source's own id where it has one,
    /// else derived from the block's content (see [`assign_ids`]). Never
    /// the block's place in the section.
    pub id: String,
    pub kind: ReaderBlockKind,
    /// Empty for separators and images. Line breaks are `\n`.
    pub spans: Vec<ReaderSpan>,
}

impl ReaderBlock {
    /// A block whose id is set by [`assign_ids`].
    pub fn new(kind: ReaderBlockKind, spans: Vec<ReaderSpan>) -> Self {
        Self {
            id: String::new(),
            kind,
            spans,
        }
    }

    /// The block's text, as shown (spans joined).
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    /// How many words it holds.
    pub fn words(&self) -> u32 {
        let words = self
            .spans
            .iter()
            .map(|s| s.text.split_whitespace().count())
            .sum::<usize>();
        u32::try_from(words).unwrap_or(u32::MAX)
    }
}

/// A part of a document read as one scroll: a book's chapter, an article.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReaderSection {
    /// Stable within the document (an EPUB chapter's file path, "article").
    pub id: String,
    pub title: String,
    pub blocks: Vec<ReaderBlock>,
}

impl ReaderSection {
    pub fn words(&self) -> u64 {
        self.blocks.iter().map(|b| u64::from(b.words())).sum()
    }

    /// The index of the block with `id`.
    pub fn position_of(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }
}

/// Lower case, whitespace collapsed: what content ids and anchor matching
/// compare, so a change of spacing or case is not a different block.
pub(crate) fn normalise(text: &str) -> String {
    text.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// FNV-1a, 64 bits: fixed for ever, unlike std's hasher, because ids are
/// saved in positions.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The id a block gets from its content: `c` and 12 hex digits of a hash of
/// its kind and normalised text (an image's source and alt text).
pub(crate) fn content_id(block: &ReaderBlock) -> String {
    let body = match &block.kind {
        ReaderBlockKind::Image { source, alt } => format!("{source}\n{}", normalise(alt)),
        _ => normalise(&block.text()),
    };
    let hash = fnv1a(format!("{}\n{body}", block.kind.tag()).as_bytes());
    format!("c{:012x}", hash & 0xffff_ffff_ffff)
}

/// Gives every block without an id one derived from its content, with an
/// occurrence number (`-2`, `-3`…) when the same content appears again in
/// the section, so repeated "* * *" blocks still differ. A block that
/// already has an id (the source's own) keeps it, unless an earlier block
/// took it, in which case it is treated as having none.
pub fn assign_ids(blocks: &mut [ReaderBlock]) {
    use std::collections::{HashMap, HashSet};
    let mut taken: HashSet<String> = HashSet::new();
    let mut seen: HashMap<String, u32> = HashMap::new();
    for block in blocks.iter_mut() {
        if !block.id.is_empty() && taken.insert(block.id.clone()) {
            continue;
        }
        let base = content_id(block);
        let n = seen.entry(base.clone()).or_insert(0);
        let mut id;
        loop {
            *n += 1;
            id = if *n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            if taken.insert(id.clone()) {
                break;
            }
        }
        block.id = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> ReaderBlock {
        ReaderBlock::new(ReaderBlockKind::Paragraph, vec![ReaderSpan::plain(text)])
    }

    #[test]
    fn content_ids_ignore_place_spacing_and_case() {
        let mut a = vec![para("One"), para("Two  words")];
        let mut b = vec![para("New first"), para("one"), para("two words")];
        assign_ids(&mut a);
        assign_ids(&mut b);
        assert_eq!(a[0].id, b[1].id);
        assert_eq!(a[1].id, b[2].id);
        assert_ne!(a[0].id, a[1].id);
        // Fixed hash: an id saved today matches tomorrow.
        assert_eq!(a[0].id, content_id(&para("ONE")));
        assert!(a[0].id.starts_with('c') && a[0].id.len() == 13);
    }

    #[test]
    fn repeats_and_source_ids() {
        let mut blocks = vec![
            ReaderBlock {
                id: "intro".into(),
                ..para("Hello")
            },
            para("* * *"),
            para("* * *"),
            ReaderBlock {
                id: "intro".into(),
                ..para("Duplicate id")
            },
        ];
        assign_ids(&mut blocks);
        assert_eq!(blocks[0].id, "intro");
        assert_eq!(blocks[2].id, format!("{}-2", blocks[1].id));
        assert_eq!(blocks[3].id, content_id(&para("Duplicate id")));
    }
}
