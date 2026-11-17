//! Lyrics for reading along. A generated song's lyrics carry section tags
//! (`[verse]`, `[chorus]`, `[bridge - soft]`) on their own lines; they
//! become headings, and blank lines between stanzas become one gap. There
//! are no timestamps, so the lyrics don't scroll with the song.

/// What a line of lyrics is.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LyricKind {
    /// A section heading: "Verse", "Chorus 2", "Bridge".
    Section,
    /// A sung line.
    Line,
    /// Space between stanzas.
    Gap,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub kind: LyricKind,
    pub text: String,
}

/// The lyrics laid out to read: headings, lines and gaps, with no gap at
/// the start or end and never two in a row. Empty when there are no words.
#[uniffi::export]
pub fn lyric_lines(lyrics: String) -> Vec<LyricLine> {
    let mut out: Vec<LyricLine> = Vec::new();
    let gap = |out: &mut Vec<LyricLine>| {
        if out.last().is_some_and(|l| l.kind != LyricKind::Gap) {
            out.push(LyricLine {
                kind: LyricKind::Gap,
                text: String::new(),
            });
        }
    };
    for raw in lyrics.lines() {
        let line = raw.trim();
        if line.is_empty() {
            gap(&mut out);
        } else if let Some(tag) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            gap(&mut out);
            out.push(LyricLine {
                kind: LyricKind::Section,
                text: section_name(tag),
            });
        } else {
            out.push(LyricLine {
                kind: LyricKind::Line,
                text: line.to_string(),
            });
        }
    }
    while out.last().is_some_and(|l| l.kind == LyricKind::Gap) {
        out.pop();
    }
    if !out.iter().any(|l| l.kind == LyricKind::Line) {
        return Vec::new();
    }
    out
}

/// "pre-chorus" → "Pre-Chorus", "verse 2 - whispered" → "Verse 2".
fn section_name(tag: &str) -> String {
    let name = tag.split(['-', ':', '(']).next().unwrap_or(tag).trim();
    // "pre-chorus" splits at its hyphen: keep it whole.
    let name = if tag.trim().to_lowercase().starts_with("pre-") {
        tag.split_whitespace().next().unwrap_or(name)
    } else {
        name
    };
    name.split_whitespace()
        .map(|word| {
            word.split('-')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => first
                            .to_uppercase()
                            .chain(chars.flat_map(char::to_lowercase))
                            .collect(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<String>>()
                .join("-")
        })
        .collect::<Vec<_>>()
        .join(" ")
}
