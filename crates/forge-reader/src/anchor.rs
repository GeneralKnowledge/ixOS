//! Reading positions as block anchors: which block of which section, and
//! how far into it. An anchor survives a change of text size or layout, and
//! an edit of the text around it; when its own block is gone, it falls back
//! to the closest thing that is left (docs/native-capability-roadmap.md,
//! Native reader).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::document::{normalise, ReaderSection};

/// How many words of the block's opening an anchor keeps, to find the
/// block again after it was edited.
const OPENING_WORDS: usize = 8;
/// How many of those must match for an edited block to count as the same.
const MATCHING_WORDS: usize = 4;

/// A saved place in a document. The document itself (a book, an article)
/// is the app's key; the anchor is the place inside it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReaderAnchor {
    pub section_id: String,
    pub block_id: String,
    /// How far into the block, 0.0–1.0.
    pub offset: f64,
    /// The block's first words, normalised: finds it again after an edit
    /// changed its id.
    pub opening: String,
    /// The ids of the blocks either side, for when the block is gone.
    pub before: Option<String>,
    pub after: Option<String>,
}

/// A place in a loaded section: a block index and how far into it.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReaderLocation {
    pub block: u32,
    pub offset: f64,
}

/// How [`ReaderAnchor::resolve`] found the place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorMatch {
    /// The same block.
    Exact,
    /// A block of the section that opens with the same words (edited).
    Text,
    /// The block was gone: next to a surviving neighbour.
    Neighbour,
    /// Nothing matched (or another section): its start.
    Start,
}

impl ReaderAnchor {
    /// The anchor for `location` in `section`. A location past the last
    /// block is the last block's end; an empty section has no anchor.
    pub fn at(section: &ReaderSection, location: ReaderLocation) -> Option<Self> {
        let last = section.blocks.len().checked_sub(1)?;
        let (index, offset) = if location.block as usize > last {
            (last, 1.0)
        } else {
            (location.block as usize, clamp(location.offset))
        };
        let block = &section.blocks[index];
        Some(Self {
            section_id: section.id.clone(),
            block_id: block.id.clone(),
            offset,
            opening: opening(&block.text()),
            before: index.checked_sub(1).map(|i| section.blocks[i].id.clone()),
            after: section.blocks.get(index + 1).map(|b| b.id.clone()),
        })
    }

    /// Where the anchor is in `section` now. Tries, in order: the same
    /// block; a block opening with the same words; the block after the
    /// saved one's earlier neighbour, or the later neighbour itself; the
    /// section's start.
    pub fn resolve(&self, section: &ReaderSection) -> (ReaderLocation, AnchorMatch) {
        let start = (
            ReaderLocation {
                block: 0,
                offset: 0.0,
            },
            AnchorMatch::Start,
        );
        if section.id != self.section_id || section.blocks.is_empty() {
            return start;
        }
        let at = |index: usize, offset: f64, how| {
            (
                ReaderLocation {
                    block: index as u32,
                    offset,
                },
                how,
            )
        };
        if let Some(index) = section.position_of(&self.block_id) {
            return at(index, clamp(self.offset), AnchorMatch::Exact);
        }
        let saved: Vec<&str> = self.opening.split(' ').filter(|w| !w.is_empty()).collect();
        if !saved.is_empty() {
            let need = saved.len().min(MATCHING_WORDS);
            let found = section.blocks.iter().position(|b| {
                let words = opening(&b.text());
                let words: Vec<&str> = words.split(' ').filter(|w| !w.is_empty()).collect();
                words.len() >= need && words[..need] == saved[..need]
            });
            if let Some(index) = found {
                return at(index, clamp(self.offset), AnchorMatch::Text);
            }
        }
        if let Some(index) = self
            .before
            .as_deref()
            .and_then(|id| section.position_of(id))
        {
            let next = (index + 1).min(section.blocks.len() - 1);
            return at(next, 0.0, AnchorMatch::Neighbour);
        }
        if let Some(index) = self.after.as_deref().and_then(|id| section.position_of(id)) {
            return at(index, 0.0, AnchorMatch::Neighbour);
        }
        start
    }
}

/// How far through `section` a location is, 0.0–1.0, by words (by blocks
/// when the section has no words, such as a page of pictures).
pub fn section_progress(section: &ReaderSection, location: ReaderLocation) -> f64 {
    let count = section.blocks.len();
    if count == 0 {
        return 0.0;
    }
    let index = (location.block as usize).min(count - 1);
    let offset = clamp(location.offset);
    let total = section.words();
    if total == 0 {
        return clamp((index as f64 + offset) / count as f64);
    }
    let before: u64 = section.blocks[..index]
        .iter()
        .map(|b| u64::from(b.words()))
        .sum();
    let within = f64::from(section.blocks[index].words()) * offset;
    clamp((before as f64 + within) / total as f64)
}

/// The location `fraction` of the way through `section` by words: the
/// inverse of [`section_progress`].
pub fn location_at(section: &ReaderSection, fraction: f64) -> ReaderLocation {
    let fraction = clamp(fraction);
    let count = section.blocks.len();
    let zero = ReaderLocation {
        block: 0,
        offset: 0.0,
    };
    if count == 0 {
        return zero;
    }
    let total = section.words();
    if total == 0 {
        let at = fraction * count as f64;
        let block = (at.floor() as usize).min(count - 1);
        return ReaderLocation {
            block: block as u32,
            offset: clamp(at - block as f64),
        };
    }
    let target = fraction * total as f64;
    let mut before = 0.0;
    for (index, block) in section.blocks.iter().enumerate() {
        let words = f64::from(block.words());
        if words > 0.0 && target < before + words {
            return ReaderLocation {
                block: index as u32,
                offset: (target - before) / words,
            };
        }
        before += words;
    }
    ReaderLocation {
        block: (count - 1) as u32,
        offset: 1.0,
    }
}

fn opening(text: &str) -> String {
    normalise(text)
        .split(' ')
        .take(OPENING_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn clamp(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{assign_ids, ReaderBlock, ReaderBlockKind, ReaderSpan};

    fn section(texts: &[&str]) -> ReaderSection {
        let mut blocks: Vec<_> = texts
            .iter()
            .map(|t| ReaderBlock::new(ReaderBlockKind::Paragraph, vec![ReaderSpan::plain(*t)]))
            .collect();
        assign_ids(&mut blocks);
        ReaderSection {
            id: "ch1".into(),
            title: "One".into(),
            blocks,
        }
    }

    fn loc(block: u32, offset: f64) -> ReaderLocation {
        ReaderLocation { block, offset }
    }

    const TEXT: [&str; 4] = [
        "It was a dark and stormy night in the old town.",
        "The rain fell in torrents, except at occasional intervals.",
        "* * *",
        "Morning came at last.",
    ];

    #[test]
    fn survives_insertions_before_it() {
        let old = section(&TEXT);
        let anchor = ReaderAnchor::at(&old, loc(1, 0.5)).unwrap();
        let mut texts = vec!["A new foreword.", "And another."];
        texts.extend(TEXT);
        let new = section(&texts);
        assert_eq!(anchor.resolve(&new), (loc(3, 0.5), AnchorMatch::Exact));
    }

    #[test]
    fn finds_an_edited_block_by_its_opening() {
        let anchor = ReaderAnchor::at(&section(&TEXT), loc(1, 0.25)).unwrap();
        let mut texts = TEXT.to_vec();
        texts[1] = "The rain fell in sheets, now and then.";
        assert_eq!(
            anchor.resolve(&section(&texts)),
            (loc(1, 0.25), AnchorMatch::Text)
        );
    }

    #[test]
    fn falls_back_to_neighbours_then_the_start() {
        let anchor = ReaderAnchor::at(&section(&TEXT), loc(1, 0.9)).unwrap();
        let gone = section(&[TEXT[0], "Something else entirely.", TEXT[2], TEXT[3]]);
        assert_eq!(anchor.resolve(&gone), (loc(1, 0.0), AnchorMatch::Neighbour));

        let only_after = section(&["New.", "Different.", TEXT[2]]);
        assert_eq!(
            anchor.resolve(&only_after),
            (loc(2, 0.0), AnchorMatch::Neighbour)
        );

        let nothing = section(&["All new."]);
        assert_eq!(anchor.resolve(&nothing), (loc(0, 0.0), AnchorMatch::Start));

        let mut other = section(&TEXT);
        other.id = "ch2".into();
        assert_eq!(anchor.resolve(&other).1, AnchorMatch::Start);
    }

    #[test]
    fn past_the_end_and_empty() {
        let s = section(&TEXT);
        let anchor = ReaderAnchor::at(&s, loc(9, 0.0)).unwrap();
        assert_eq!(anchor.block_id, s.blocks[3].id);
        assert_eq!(anchor.offset, 1.0);
        assert_eq!(anchor.before.as_deref(), Some(s.blocks[2].id.as_str()));
        assert_eq!(anchor.after, None);
        assert!(ReaderAnchor::at(&section(&[]), loc(0, 0.0)).is_none());
    }

    #[test]
    fn progress_and_location_by_words() {
        let s = section(&TEXT);
        let total = s.words() as f64;
        assert_eq!(section_progress(&s, loc(0, 0.0)), 0.0);
        let p = section_progress(&s, loc(1, 0.0));
        assert!((p - 11.0 / total).abs() < 1e-9, "{p}");
        let back = location_at(&s, p);
        assert_eq!(back.block, 1);
        assert!(back.offset.abs() < 1e-9);
        assert_eq!(location_at(&s, 1.0), loc(3, 1.0));
        assert_eq!(section_progress(&s, loc(3, 1.0)), 1.0);
    }
}
