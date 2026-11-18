//! Lyrics for reading along. A generated song's lyrics carry section tags
//! (`[verse]`, `[chorus]`, `[bridge - soft]`) on their own lines; they
//! become headings, and blank lines between stanzas become one gap.
//!
//! Timed lyrics are LRC: each line starts with when it is sung,
//! `[01:23.40] One more night` (a line may carry several stamps when it
//! repeats, and a section tag may follow a stamp). With timings the lines
//! are in time order and the screen follows the song ([`lyric_line_at`]).
//!
//! Enhanced LRC adds a time before each word,
//! `[00:12.30] <00:12.30>One <00:12.61>more <00:12.90>night`, so the word
//! being sung can light up ([`lyric_word_at`]).

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

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LyricLine {
    pub kind: LyricKind,
    pub text: String,
    /// When it is sung, in seconds from the start; timed lyrics only.
    pub at_seconds: Option<f64>,
    /// Its words with when each is sung, for enhanced LRC; empty otherwise.
    /// Joined, they are `text`.
    pub words: Vec<LyricWord>,
}

/// One sung word, with the space after it, and when it starts.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LyricWord {
    pub text: String,
    pub at_seconds: f64,
}

/// The lyrics laid out to read: headings, lines and gaps, with no gap at
/// the start or end and never two in a row. Empty when there are no words.
#[uniffi::export]
pub fn lyric_lines(lyrics: String) -> Vec<LyricLine> {
    let mut stamped: Vec<(Option<f64>, usize, String)> = Vec::new();
    let mut timed = false;
    for (order, raw) in lyrics.lines().enumerate() {
        let (stamps, rest) = stamps(raw.trim());
        if stamps.is_empty() {
            if is_metadata(rest) {
                continue;
            }
            stamped.push((None, order, rest.to_string()));
        } else {
            timed = true;
            for at in stamps {
                stamped.push((Some(at), order, rest.to_string()));
            }
        }
    }
    if timed {
        // Untimed lines (blank lines between stanzas) carry no time, so
        // keep only timed ones, in time order; a gap goes before each
        // section heading.
        stamped.retain(|(at, _, _)| at.is_some());
        stamped.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    }
    let mut out: Vec<LyricLine> = Vec::new();
    for (at, _, line) in stamped {
        if line.is_empty() {
            push_gap(&mut out);
        } else if let Some(tag) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            push_gap(&mut out);
            out.push(LyricLine {
                kind: LyricKind::Section,
                text: section_name(tag),
                at_seconds: at,
                words: Vec::new(),
            });
        } else {
            let (text, words) = words(&line);
            out.push(LyricLine {
                kind: LyricKind::Line,
                text,
                at_seconds: at,
                words,
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

/// The line being sung at `seconds`: the last sung line that has started,
/// as an index into `lines`. `None` before the first line and for lyrics
/// without timings.
#[uniffi::export]
pub fn lyric_line_at(lines: Vec<LyricLine>, seconds: f64) -> Option<u32> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == LyricKind::Line)
        .take_while(|(_, l)| l.at_seconds.is_some_and(|at| at <= seconds))
        .last()
        .map(|(i, _)| i as u32)
}

fn push_gap(out: &mut Vec<LyricLine>) {
    if out.last().is_some_and(|l| l.kind != LyricKind::Gap) {
        out.push(LyricLine {
            kind: LyricKind::Gap,
            text: String::new(),
            at_seconds: None,
            words: Vec::new(),
        });
    }
}

/// The word being sung at `seconds` in `line`: the last word that has
/// started, as an index into its `words`. `None` without word timings or
/// before its first word.
#[uniffi::export]
pub fn lyric_word_at(line: LyricLine, seconds: f64) -> Option<u32> {
    line.words
        .iter()
        .take_while(|w| w.at_seconds <= seconds)
        .count()
        .checked_sub(1)
        .map(|i| i as u32)
}

/// A line's words from `<mm:ss.xx>` stamps: the text without them, and the
/// timed words (empty when the line has no word stamps).
fn words(line: &str) -> (String, Vec<LyricWord>) {
    if !line.contains('<') {
        return (line.to_string(), Vec::new());
    }
    let mut words: Vec<LyricWord> = Vec::new();
    let mut text = String::new();
    let mut rest = line;
    let mut lead = String::new();
    while !rest.is_empty() {
        let at_stamp = rest
            .strip_prefix('<')
            .and_then(|r| r.find('>').map(|end| (r, end)))
            .and_then(|(r, end)| stamp(&r[..end]).map(|at| (at, &r[end + 1..])));
        match at_stamp {
            Some((at, after)) => {
                let end = after.find('<').unwrap_or(after.len());
                let word = &after[..end];
                rest = &after[end..];
                // A trailing stamp with no word marks when the line ends.
                if !word.trim().is_empty() {
                    words.push(LyricWord {
                        text: format!("{lead}{word}"),
                        at_seconds: at,
                    });
                    lead.clear();
                }
            }
            None => {
                // Text before the first stamp, or a stray "<": it belongs
                // to the next word (or the last one).
                let end = rest[1..].find('<').map_or(rest.len(), |i| i + 1);
                match words.last_mut() {
                    Some(last) => last.text.push_str(&rest[..end]),
                    None => lead.push_str(&rest[..end]),
                }
                rest = &rest[end..];
            }
        }
    }
    for w in &words {
        text.push_str(&w.text);
    }
    if words.is_empty() {
        return (lead.trim().to_string(), Vec::new());
    }
    let trimmed = text.trim_end().len();
    if let Some(last) = words.last_mut() {
        let cut = last.text.len() - (text.len() - trimmed);
        last.text.truncate(cut);
    }
    text.truncate(trimmed);
    (text, words)
}

/// The `[mm:ss.xx]` stamps at the start of a line, in seconds, and the rest.
fn stamps(mut line: &str) -> (Vec<f64>, &str) {
    let mut found = Vec::new();
    while let Some(inner) = line.strip_prefix('[') {
        let Some(end) = inner.find(']') else { break };
        let Some(at) = stamp(&inner[..end]) else {
            break;
        };
        found.push(at);
        line = inner[end + 1..].trim_start();
    }
    (found, line.trim())
}

/// "01:23.40", "1:23", "01:23:40" (centiseconds after a colon) in seconds.
fn stamp(text: &str) -> Option<f64> {
    let (minutes, rest) = text.split_once(':')?;
    let minutes: u32 = minutes.trim().parse().ok()?;
    let seconds: f64 = match rest.split_once(':') {
        Some((s, cs)) => format!("{s}.{cs}").parse().ok()?,
        None => rest.parse().ok()?,
    };
    (seconds.is_finite() && (0.0..60.0).contains(&seconds))
        .then(|| f64::from(minutes) * 60.0 + seconds)
}

/// LRC header lines like `[ar:AFTER HOURS]` or `[offset:+120]`.
fn is_metadata(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|l| l.strip_suffix(']'))
        .and_then(|l| l.split_once(':'))
        .is_some_and(|(tag, _)| {
            matches!(
                tag.trim().to_ascii_lowercase().as_str(),
                "ar" | "al" | "ti" | "au" | "by" | "re" | "ve" | "length" | "offset"
            )
        })
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
