//! Chapter summaries written by a small on-device language model (Apple
//! Intelligence through Foundation Models, in Swift).
//!
//! The model reads about 4,000 tokens at a time, prompt and answer
//! together, which is far less than a long chapter. So Rust plans the work
//! and the model only answers one short request at a time:
//!
//! 1. The chapter's paragraphs are packed into evenly sized **parts** that
//!    fit the window with room to spare ([`split_into_parts`]).
//! 2. A **reader** request per part lists its facts in the model's own
//!    words, with the facts of the part before as context, so names and
//!    threads carry over. Every chapter goes through this step, however
//!    short.
//! 3. If the facts together are still too long, consecutive notes are
//!    **combined** in groups until they fit.
//! 4. A **writer** request turns the facts into the summary. It never sees
//!    the book's text, so it can't echo the chapter's opening (it did, when
//!    short chapters were summarised straight from the text: phone run,
//!    2026-11-14).
//! 5. Rust checks the summary for runs of seven or more words copied from
//!    the book ([`copied_phrases`]); if it finds any, an **editor** request
//!    rewrites it from the facts, naming the copied phrases.
//!
//! A part the model can't read (too long after all, or declined by its
//! guardrails) is split in two and tried again; a passage still declined
//! is left out and counted, so the reader is told the summary has a gap.
//! No inference happens in Rust: the Swift engine implements
//! [`SummaryModel`].

use std::sync::Arc;

use forge_runtime::CancelHandle;

use crate::error::LibraryError;

/// What the model can read and write in one request (Apple's on-device
/// model: 4,096 tokens).
pub const CONTEXT_TOKENS: usize = 4096;
/// The most text in one part. With the instructions, the notes on the part
/// before and the answer, a request stays near 2,800 tokens, leaving room
/// for the token estimate being low (it is only an estimate; the real
/// tokenizer is Apple's).
pub const PART_TOKENS: usize = 2000;
/// The most notes combined in one request.
pub const NOTES_TOKENS: usize = 2400;
/// Longest answers, in tokens.
const NOTE_MAX_TOKENS: u32 = 300;
const SUMMARY_MAX_TOKENS: u32 = 450;
const OVERVIEW_MAX_TOKENS: u32 = 700;
/// Low, so the model sticks to the text.
const TEMPERATURE: f64 = 0.3;
/// Shorter than this is a title page or a copyright notice.
pub const MIN_WORDS: u32 = 120;
/// Longer than this takes many minutes on a phone; such "chapters" are
/// usually a whole book in one file.
pub const MAX_WORDS: u32 = 60_000;
/// How many times a part is split in two after the model can't read it.
const MAX_SPLITS: u32 = 3;

/// The reader: turns a part of the chapter into facts. It sees the book's
/// text, so it is told to restate, never copy.
const READER_INSTRUCTIONS: &str = "You take notes on a part of a book for a reader who wants to \
remember it. Use only the text you are given. Write each note as a short fact in your own words: who \
does what, what changes, what is revealed. Never copy sentences or phrases from the text. Add no \
opinions, morals or warnings.";

/// The writer: turns the facts into the summary. It never sees the book's
/// own text, so it cannot borrow its opening lines.
const WRITER_INSTRUCTIONS: &str = "You write short chapter summaries for a reader who wants to \
remember what they read, from notes someone else took. Use only the facts in the notes. Write in \
your own words, as plain prose in the language of the notes, with no headings or lists. Add no \
opinions, morals or warnings.";

// MARK: The engine (Swift)

/// One request to the model.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SummaryRequest {
    /// The system instructions.
    pub instructions: String,
    /// The text and what to write.
    pub prompt: String,
    /// The longest answer, in tokens.
    pub max_tokens: u32,
    /// Sampling temperature, 0 to 1.
    pub temperature: f64,
}

/// What the engine says about itself.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SummaryModelInfo {
    /// The name shown under a summary ("Apple Intelligence").
    pub name: String,
    pub available: bool,
    /// Why not, in words the reader can act on, when `available` is false.
    pub unavailable_reason: Option<String>,
}

/// Why the engine gave no answer.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum SummaryModelError {
    /// The model can't run now (Apple Intelligence off, still downloading,
    /// an iPhone that doesn't support it).
    #[error("unavailable: {reason}")]
    Unavailable { reason: String },
    /// The model's safety rules declined the text.
    #[error("refused: {reason}")]
    Refused { reason: String },
    /// The request did not fit the model's context window.
    #[error("too long: {reason}")]
    ContextTooLong { reason: String },
    /// Stopped through the sink.
    #[error("stopped")]
    Stopped,
    /// The model is throttled for now (Foundation Models limits apps in
    /// the background). Nothing is lost: the run pauses and carries on
    /// when the app is back on screen.
    #[error("busy: {reason}")]
    Busy { reason: String },
    #[error("{reason}")]
    Engine { reason: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for SummaryModelError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Engine { reason: err.reason }
    }
}

/// A language model, implemented in Swift. Rust calls it from a thread of
/// its own, one request at a time.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait SummaryModel: Send + Sync {
    /// Whether the model can run now. Cheap.
    fn describe(&self) -> SummaryModelInfo;

    /// Answers `request`. While it writes, it passes the answer so far to
    /// `sink.update`, and stops (returning `Stopped`) as soon as that
    /// returns false.
    async fn generate(
        &self,
        request: SummaryRequest,
        sink: Arc<SummarySink>,
    ) -> Result<String, SummaryModelError>;
}

/// Where the engine reports the answer so far, and learns it should stop.
#[derive(uniffi::Object)]
pub struct SummarySink {
    cancel: CancelHandle,
    /// Hears the final summary as it is written.
    draft: Option<Arc<dyn SummaryProgress>>,
}

#[uniffi::export]
impl SummarySink {
    /// The whole answer so far. Returns false once the summary was
    /// stopped: the engine must end the generation.
    pub fn update(&self, text_so_far: String) -> bool {
        if let Some(progress) = &self.draft {
            progress.changed(SummaryStage::Writing {
                draft: clean(&text_so_far),
            });
        }
        !self.cancel.is_cancelled()
    }

    pub fn is_stopped(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

// MARK: Progress (Swift)

/// What the summary is doing now.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum SummaryStage {
    /// Getting the chapter's text.
    Opening,
    /// Writing notes on part `part` (1-based) of `parts`.
    ReadingPart { part: u32, parts: u32 },
    /// Bringing long notes together.
    Combining,
    /// Writing the summary; `draft` is the text so far.
    Writing { draft: String },
    /// The summary repeated the book word for word; rewriting it.
    Revising,
    /// Whole book: starting chapter `chapter` (1-based) of `chapters`. The
    /// chapter's own stages follow; `saved` means its summary was already
    /// there and is reused.
    Chapter {
        chapter: u32,
        chapters: u32,
        title: String,
        saved: bool,
    },
    /// Whole book: updating the list of people after a chapter.
    UpdatingCast,
    /// Whole book: writing the overview. `Combining` and `Writing` follow.
    Overview,
}

/// Told about each stage, on Rust's thread.
#[uniffi::export(with_foreign)]
pub trait SummaryProgress: Send + Sync {
    fn changed(&self, stage: SummaryStage);
}

// MARK: The result

/// A summary of one chapter.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChapterSynopsis {
    /// The reader chapter the summary starts at (see [`section`]).
    pub chapter: u32,
    /// The chapter's title.
    pub title: String,
    pub text: String,
    /// How many parts the chapter was read in.
    pub parts: u32,
    /// Passages the model declined to read, which the summary leaves out.
    pub skipped_passages: u32,
    /// Which model wrote it ("Apple Intelligence").
    pub model_name: String,
    pub created_at_ms: i64,
}

/// The reader chapters one summary covers: `index` and the files that
/// continue it. Many EPUBs split a long chapter across files; only the
/// first is in the table of contents, so a summary covers a chapter from
/// one listed file to the next. A book that lists nothing is summarised a
/// file at a time.
pub fn section(chapters: &[crate::ChapterSummary], index: u32) -> std::ops::Range<u32> {
    let index = index.min(chapters.len().saturating_sub(1) as u32);
    if !chapters.iter().any(|c| c.in_contents) {
        return index..index + 1;
    }
    let start = chapters[..=index as usize]
        .iter()
        .rposition(|c| c.in_contents)
        .map_or(index, |i| i as u32);
    let end = chapters[index as usize + 1..]
        .iter()
        .position(|c| c.in_contents)
        .map_or(chapters.len() as u32, |i| index + 1 + i as u32);
    start..end
}

/// One person in a book, as the cast keeper describes them.
#[derive(uniffi::Record, serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CastMember {
    pub name: String,
    pub role: String,
}

/// A summary of a whole book: an overview, the people in it, and every
/// chapter's summary.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct BookSynopsis {
    pub text: String,
    pub cast: Vec<CastMember>,
    /// Every chapter summary saved for the book, in order (the whole-book
    /// run writes the missing ones).
    pub chapters: Vec<ChapterSynopsis>,
    /// Chapters the model read, and chapters it declined or failed.
    pub chapters_read: u32,
    pub chapters_skipped: u32,
    /// How hard the phone worked: model requests, and wall-clock seconds.
    pub model_requests: u32,
    pub seconds: u32,
    pub model_name: String,
    pub created_at_ms: i64,
}

/// A whole-book run that stopped part way (stopped, interrupted, the app
/// closed): the next run carries on from `next_chapter`, with the cast and
/// counts so far.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct BookRunProgress {
    /// Chapters done, so the next is `chapters_done + 1`.
    pub chapters_done: u32,
    pub chapters: u32,
    pub seconds: u32,
    pub model_requests: u32,
}

/// The sections a whole-book summary reads: one per chapter (see
/// [`section`]), leaving out title pages, copyright notices and other
/// sections under [`MIN_WORDS`].
pub fn book_sections(chapters: &[crate::ChapterSummary]) -> Vec<std::ops::Range<u32>> {
    let mut out: Vec<std::ops::Range<u32>> = Vec::new();
    let mut index = 0;
    while (index as usize) < chapters.len() {
        let range = section(chapters, index);
        let words: u32 = chapters[range.start as usize..range.end as usize]
            .iter()
            .map(|c| c.words)
            .sum();
        if words >= MIN_WORDS {
            out.push(range.clone());
        }
        index = range.end.max(index + 1);
    }
    out
}

/// The cast keeper's list, read back: lines like "- Name: who they are"
/// (or "Name — who"), at most [`MAX_CAST`].
pub fn parse_cast(text: &str) -> Vec<CastMember> {
    let mut cast: Vec<CastMember> = Vec::new();
    for line in text.lines() {
        let line = line
            .trim()
            .trim_start_matches(['-', '*', '\u{2022}'])
            .trim();
        let split = line
            .split_once(':')
            .or_else(|| line.split_once(" \u{2014} "))
            .or_else(|| line.split_once(" - "));
        let Some((name, role)) = split else { continue };
        let name = name.trim().trim_matches('*').trim();
        let role = role.trim();
        if name.is_empty() || role.is_empty() || name.split_whitespace().count() > 6 {
            continue;
        }
        if cast.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
            continue;
        }
        cast.push(CastMember {
            name: name.to_string(),
            role: role.to_string(),
        });
        if cast.len() == MAX_CAST {
            break;
        }
    }
    cast
}

/// The most people the cast keeper tracks.
pub const MAX_CAST: usize = 20;

fn cast_text(cast: &[CastMember]) -> String {
    cast.iter()
        .map(|c| format!("- {}: {}", c.name, c.role))
        .collect::<Vec<_>>()
        .join("\n")
}

// MARK: Planning

/// A rough token count that errs high: about four characters a token for
/// ASCII text, and a token for every other character (accented letters,
/// other scripts), whose real cost is between a third and two.
pub fn estimate_tokens(text: &str) -> usize {
    let (ascii, other) = text.chars().fold((0usize, 0usize), |(a, o), c| {
        if c.is_ascii() {
            (a + 1, o)
        } else {
            (a, o + 1)
        }
    });
    ascii.div_ceil(4) + other
}

/// Packs paragraphs into parts of at most `budget` estimated tokens, in
/// order and evenly sized, so the last part is not a scrap. A paragraph
/// longer than `budget` is split at sentence ends (or, failing that, at
/// spaces).
pub fn split_into_parts(paragraphs: &[String], budget: usize) -> Vec<String> {
    let pieces: Vec<String> = paragraphs
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .flat_map(|p| split_long(p, budget))
        .collect();
    let total: usize = pieces.iter().map(|p| estimate_tokens(p) + 1).sum();
    if total == 0 {
        return Vec::new();
    }
    let count = total.div_ceil(budget);
    let target = total.div_ceil(count);
    let limit = (target + target / 5).min(budget);
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut size = 0;
    for piece in pieces {
        let cost = estimate_tokens(&piece) + 1;
        if !current.is_empty() && size + cost > limit {
            parts.push(std::mem::take(&mut current));
            size = 0;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(&piece);
        size += cost;
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// A paragraph in pieces of at most `budget` tokens.
fn split_long(text: &str, budget: usize) -> Vec<String> {
    if estimate_tokens(text) <= budget {
        return vec![text.to_string()];
    }
    let mut pieces = Vec::new();
    let mut current = String::new();
    for sentence in sentences(text) {
        let candidate = if current.is_empty() {
            sentence.to_string()
        } else {
            format!("{current} {sentence}")
        };
        if estimate_tokens(&candidate) <= budget {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            pieces.push(std::mem::take(&mut current));
        }
        if estimate_tokens(sentence) <= budget {
            current = sentence.to_string();
        } else {
            // One enormous "sentence": cut at spaces, or anywhere.
            for word in sentence.split_whitespace() {
                let candidate = if current.is_empty() {
                    word.to_string()
                } else {
                    format!("{current} {word}")
                };
                if estimate_tokens(&candidate) <= budget {
                    current = candidate;
                } else {
                    if !current.is_empty() {
                        pieces.push(std::mem::take(&mut current));
                    }
                    let mut chunk = String::new();
                    for c in word.chars() {
                        chunk.push(c);
                        if estimate_tokens(&chunk) >= budget {
                            pieces.push(std::mem::take(&mut chunk));
                        }
                    }
                    current = chunk;
                }
            }
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

/// Sentences, each with its closing punctuation.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let ends = matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '\n');
        let next_is_space = chars.peek().is_none_or(|(_, n)| n.is_whitespace());
        if ends && next_is_space {
            let end = i + c.len_utf8();
            let s = text[start..end].trim();
            if !s.is_empty() {
                out.push(s);
            }
            start = end;
        }
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// How many words in a row a summary may share with the book before it
/// counts as copying. Names and stock phrases ("at the end of the day")
/// stay well under it.
pub const COPY_RUN_WORDS: usize = 7;

fn plain_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase().replace('\u{2019}', "'"))
        .collect()
}

/// The phrases of `summary` that appear word for word in the book's text
/// (runs of at least [`COPY_RUN_WORDS`] words, case and punctuation
/// ignored), longest first, at most three.
pub fn copied_phrases(summary: &str, paragraphs: &[String]) -> Vec<String> {
    let n = COPY_RUN_WORDS;
    let mut seen = std::collections::HashSet::new();
    for paragraph in paragraphs {
        let words = plain_words(paragraph);
        for window in words.windows(n) {
            seen.insert(window.join(" "));
        }
    }
    let words = plain_words(summary);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i + n <= words.len() {
        if seen.contains(&words[i..i + n].join(" ")) {
            let mut end = i + n;
            while end < words.len() && seen.contains(&words[end + 1 - n..=end].join(" ")) {
                end += 1;
            }
            runs.push((i, end));
            i = end;
        } else {
            i += 1;
        }
    }
    runs.sort_by_key(|(a, b)| std::cmp::Reverse(b - a));
    runs.into_iter()
        .take(3)
        .map(|(a, b)| words[a..b].join(" "))
        .collect()
}

/// Halves a part at a paragraph or sentence boundary near its middle.
fn halve(part: &str) -> Option<(String, String)> {
    let paragraphs: Vec<String> = part.split("\n\n").map(str::to_string).collect();
    let pieces = if paragraphs.len() >= 2 {
        paragraphs
    } else {
        sentences(part).into_iter().map(str::to_string).collect()
    };
    if pieces.len() < 2 {
        return None;
    }
    let total: usize = pieces.iter().map(|p| estimate_tokens(p)).sum();
    let mut size = 0;
    let mut cut = 1;
    for (i, piece) in pieces.iter().enumerate() {
        size += estimate_tokens(piece);
        if size * 2 >= total {
            cut = (i + 1).clamp(1, pieces.len() - 1);
            break;
        }
    }
    let join = |p: &[String]| p.join("\n\n");
    Some((join(&pieces[..cut]), join(&pieces[cut..])))
}

/// The model's answer without the things small models add: a "Here is a
/// summary:" line, Markdown emphasis, surrounding quotes, extra blank
/// lines.
pub fn clean(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    if let Some(first) = lines.first() {
        let lower = first.trim().to_lowercase();
        let preamble = lower.ends_with(':')
            && (lower.starts_with("here")
                || lower.starts_with("summary")
                || lower.starts_with("notes")
                || lower.starts_with("sure"));
        if preamble || lower.starts_with('#') {
            lines.remove(0);
        }
    }
    let mut out = String::new();
    let mut blank = false;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            blank = true;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(line);
    }
    let mut out = out.replace("**", "").replace("__", "");
    for prefix in ["Summary:", "Notes:", "summary:"] {
        if let Some(rest) = out.strip_prefix(prefix) {
            out = rest.trim_start().to_string();
        }
    }
    let quoted = out.len() >= 2
        && ((out.starts_with('"') && out.ends_with('"'))
            || (out.starts_with('\u{201C}') && out.ends_with('\u{201D}')));
    if quoted && out.matches(['"', '\u{201C}', '\u{201D}']).count() == 2 {
        let mut chars = out.chars();
        chars.next();
        chars.next_back();
        out = chars.as_str().trim().to_string();
    }
    out
}

// MARK: Prompts

/// What every prompt says about where the text comes from.
#[derive(Clone, Debug)]
pub struct Source {
    pub book_title: String,
    pub author: String,
    pub chapter_title: String,
}

impl Source {
    fn header(&self) -> String {
        let book = if self.author.trim().is_empty() {
            format!("Book: \u{201C}{}\u{201D}", self.book_title)
        } else {
            format!(
                "Book: \u{201C}{}\u{201D} by {}",
                self.book_title, self.author
            )
        };
        format!("{book}\nChapter: {}", self.chapter_title)
    }
}

fn part_prompt(
    source: &Source,
    part: usize,
    parts: usize,
    text: &str,
    earlier: Option<&str>,
) -> String {
    let earlier = earlier
        .map(|n| format!("Notes on the part before (for names and context only):\n{n}\n\n"))
        .unwrap_or_default();
    let which = if parts == 1 {
        "the chapter".to_string()
    } else {
        format!("part {part} of {parts} of the chapter")
    };
    format!(
        "{}\nThe text is {which}.\n\n{earlier}The text:\n<<<\n{text}\n>>>\n\nList the 4 to 8 \
         most important things that happen or are explained in this text, in order, one short fact \
         per line starting with \"- \". Use the characters' names. Restate everything in your own \
         words; do not copy the text's sentences, and do not start with its opening line.",
        source.header()
    )
}

fn notes_list(notes: &[String]) -> String {
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| format!("Part {}: {n}", i + 1))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn combine_prompt(source: &Source, notes: &[String]) -> String {
    format!(
        "{}\n\nNotes on consecutive parts of the chapter, in order:\n\n{}\n\nCombine these notes into \
         one set of notes of 4 to 6 sentences, keeping the events in order and the names.",
        source.header(),
        notes_list(notes)
    )
}

fn final_prompt(source: &Source, notes: &[String]) -> String {
    format!(
        "{}\n\nNotes on the chapter, in order:\n\n{}\n\nFrom these notes, write a summary of the \
         whole chapter in one paragraph of 4 to 6 sentences: what happens or what it explains, who is \
         involved, and how it ends. Open with the chapter's main event or turn, not with its setting \
         or first moment. Keep the events in order after that.",
        source.header(),
        notes_list(notes)
    )
}

fn revise_prompt(source: &Source, notes: &[String], draft: &str, copied: &[String]) -> String {
    let phrases = copied
        .iter()
        .map(|p| format!("- \u{201C}{p}\u{201D}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{}\n\nNotes on the chapter, in order:\n\n{}\n\nA draft summary:\n{draft}\n\nThe draft \
         copies these phrases word for word from the book:\n{phrases}\n\nRewrite the summary as one \
         paragraph of 4 to 6 sentences, keeping its facts but saying everything in your own words. Open \
         with the chapter's main event or turn.",
        source.header(),
        notes_list(notes)
    )
}

/// The cast keeper: follows who is who across the book.
const CAST_INSTRUCTIONS: &str = "You keep a list of the people in a book for a reader, chapter \
by chapter. Use only what the summaries say. Write only the list, one person per line.";

fn cast_prompt(book: &str, cast: &[CastMember], chapter: &str, summary: &str) -> String {
    let current = if cast.is_empty() {
        "(nobody yet)".to_string()
    } else {
        cast_text(cast)
    };
    format!(
        "{book}\n\nThe people so far:\n{current}\n\nThe next chapter, \u{201C}{chapter}\u{201D}:\n\
         {summary}\n\nUpdate the list: keep everyone on it, add anyone new who matters, and update \
         what we know about them from this chapter. One line per person, as \"- Name: who they are \
         and what they have done\", under 20 words each, most important first, at most {MAX_CAST} \
         people."
    )
}

fn stretch_prompt(book: &str, summaries: &[String]) -> String {
    format!(
        "{book}\n\nSummaries of consecutive chapters, in order:\n\n{}\n\nSummarise this stretch of \
         the book in one paragraph of 4 to 6 sentences, keeping the events in order and the names.",
        summaries.join("\n\n")
    )
}

fn overview_prompt(book: &str, cast: &[CastMember], summaries: &[String]) -> String {
    let people = if cast.is_empty() {
        String::new()
    } else {
        format!("The people in the book:\n{}\n\n", cast_text(cast))
    };
    format!(
        "{book}\n\n{people}The whole book, in order:\n\n{}\n\nWrite an overview of the whole book \
         in three short paragraphs: how it begins and who it is about, how the story or argument \
         develops, and how it ends. Use the names. Write in your own words.",
        summaries.join("\n\n")
    )
}

fn request(instructions: &str, prompt: String, max_tokens: u32) -> SummaryRequest {
    SummaryRequest {
        instructions: instructions.to_string(),
        prompt,
        max_tokens,
        temperature: TEMPERATURE,
    }
}

// MARK: Running

/// A finished summary, before it is saved.
#[derive(Debug, PartialEq, Eq)]
pub struct Written {
    pub text: String,
    pub parts: u32,
    pub skipped_passages: u32,
}

/// Runs the plan above on a Rust thread. `cancel` stops it between and
/// during requests.
pub struct Summariser<'a> {
    pub model: &'a dyn SummaryModel,
    pub cancel: &'a CancelHandle,
    pub progress: Option<Arc<dyn SummaryProgress>>,
    /// How many requests the model has answered (or declined).
    pub requests: std::cell::Cell<u32>,
    pub source: Source,
}

impl Summariser<'_> {
    /// Three layers, each a separate request with its own instructions: a
    /// reader turns each part into facts, a writer turns the facts into the
    /// summary without seeing the book's text, and if the summary still
    /// repeats the book word for word (checked here, in Rust) an editor
    /// rewrites it from the facts.
    pub fn run(&self, paragraphs: &[String]) -> Result<Written, LibraryError> {
        let parts = split_into_parts(paragraphs, PART_TOKENS);
        if parts.is_empty() {
            return Err(LibraryError::invalid(
                "this chapter has no text to summarise",
            ));
        }
        let count = parts.len();
        let mut notes: Vec<String> = Vec::new();
        let mut skipped = 0u32;
        for (i, part) in parts.iter().enumerate() {
            self.report(SummaryStage::ReadingPart {
                part: i as u32 + 1,
                parts: count as u32,
            });
            let earlier = notes.last().map(String::as_str);
            let (note, missed) = self.notes_on(part, i + 1, count, earlier, MAX_SPLITS)?;
            skipped += missed;
            if let Some(note) = note {
                notes.push(note);
            }
        }
        if notes.is_empty() {
            return Err(LibraryError::SummaryRefused {
                reason: "the model declined every part of this chapter".into(),
            });
        }
        while notes.len() > 1 && estimate_tokens(&notes_list(&notes)) > NOTES_TOKENS {
            self.report(SummaryStage::Combining);
            notes = self.combine(&notes)?;
        }
        self.report(SummaryStage::Writing {
            draft: String::new(),
        });
        let mut text = self
            .ask(
                request(
                    WRITER_INSTRUCTIONS,
                    final_prompt(&self.source, &notes),
                    SUMMARY_MAX_TOKENS,
                ),
                true,
            )
            .map_err(|e| self.error(e))?;
        let copied = copied_phrases(&text, paragraphs);
        if !copied.is_empty() {
            self.report(SummaryStage::Revising);
            let prompt = revise_prompt(&self.source, &notes, &text, &copied);
            match self.ask(
                request(WRITER_INSTRUCTIONS, prompt, SUMMARY_MAX_TOKENS),
                true,
            ) {
                // Keep the rewrite only if it copies less.
                Ok(rewrite)
                    if !rewrite.is_empty()
                        && copied_phrases(&rewrite, paragraphs).len() < copied.len() =>
                {
                    text = rewrite
                }
                Ok(_)
                | Err(
                    SummaryModelError::Refused { .. } | SummaryModelError::ContextTooLong { .. },
                ) => {
                    // Show the first draft again.
                    self.report(SummaryStage::Writing {
                        draft: text.clone(),
                    });
                }
                Err(err) => return Err(self.error(err)),
            }
        }
        Ok(Written {
            text,
            parts: count as u32,
            skipped_passages: skipped,
        })
    }

    /// Notes on one part, splitting it if the model can't take it whole.
    /// Returns the notes (none if every piece was declined) and how many
    /// passages were left out.
    fn notes_on(
        &self,
        text: &str,
        part: usize,
        parts: usize,
        earlier: Option<&str>,
        splits_left: u32,
    ) -> Result<(Option<String>, u32), LibraryError> {
        let prompt = part_prompt(&self.source, part, parts, text, earlier);
        match self.ask(request(READER_INSTRUCTIONS, prompt, NOTE_MAX_TOKENS), false) {
            Ok(note) if !note.is_empty() => Ok((Some(note), 0)),
            Ok(_) => Ok((None, 1)),
            Err(SummaryModelError::ContextTooLong { .. } | SummaryModelError::Refused { .. })
                if splits_left > 0 =>
            {
                let Some((a, b)) = halve(text) else {
                    return Ok((None, 1));
                };
                let (first, missed_a) = self.notes_on(&a, part, parts, earlier, splits_left - 1)?;
                let earlier_b = first.as_deref().or(earlier);
                let (second, missed_b) =
                    self.notes_on(&b, part, parts, earlier_b, splits_left - 1)?;
                let joined = [first, second]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Ok(((!joined.is_empty()).then_some(joined), missed_a + missed_b))
            }
            Err(SummaryModelError::ContextTooLong { .. } | SummaryModelError::Refused { .. }) => {
                Ok((None, 1))
            }
            Err(err) => Err(self.error(err)),
        }
    }

    /// One round of combining: consecutive notes in groups that fit.
    fn combine(&self, notes: &[String]) -> Result<Vec<String>, LibraryError> {
        let mut groups: Vec<Vec<String>> = vec![Vec::new()];
        for note in notes {
            let group = groups.last_mut().expect("never empty");
            let mut candidate = group.clone();
            candidate.push(note.clone());
            if !group.is_empty() && estimate_tokens(&notes_list(&candidate)) > NOTES_TOKENS {
                groups.push(vec![note.clone()]);
            } else {
                *group = candidate;
            }
        }
        if groups.len() == notes.len() {
            // Every note alone is too long: combining can't shrink them, so
            // keep the first sentences of each.
            return Ok(notes.iter().map(|n| shorten(n, 3)).collect());
        }
        let mut out = Vec::new();
        for group in groups {
            if group.len() == 1 {
                out.extend(group);
                continue;
            }
            let prompt = combine_prompt(&self.source, &group);
            match self.ask(request(WRITER_INSTRUCTIONS, prompt, NOTE_MAX_TOKENS), false) {
                Ok(text) if !text.is_empty() => out.push(text),
                // Declined or empty: keep the group's notes, shortened.
                Ok(_)
                | Err(
                    SummaryModelError::Refused { .. } | SummaryModelError::ContextTooLong { .. },
                ) => out.push(
                    group
                        .iter()
                        .map(|n| shorten(n, 1))
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                Err(err) => return Err(self.error(err)),
            }
        }
        Ok(out)
    }

    /// The cast keeper's turn after a chapter: the updated list, or the
    /// old one if the model declines or answers with no list.
    pub fn update_cast(
        &self,
        cast: &[CastMember],
        chapter: &str,
        summary: &str,
    ) -> Result<Vec<CastMember>, LibraryError> {
        self.report(SummaryStage::UpdatingCast);
        let prompt = cast_prompt(&self.book_line(), cast, chapter, summary);
        match self.ask(request(CAST_INSTRUCTIONS, prompt, 600), false) {
            Ok(text) => {
                let updated = parse_cast(&text);
                Ok(if updated.is_empty() {
                    cast.to_vec()
                } else {
                    updated
                })
            }
            Err(SummaryModelError::Refused { .. } | SummaryModelError::ContextTooLong { .. }) => {
                Ok(cast.to_vec())
            }
            Err(err) => Err(self.error(err)),
        }
    }

    /// The book's overview from its chapter summaries ("Chapter title:
    /// summary", in order): stretches of chapters are summarised until
    /// they fit one request, then the overview is written from them and
    /// the cast.
    pub fn overview(
        &self,
        cast: &[CastMember],
        chapters: &[String],
    ) -> Result<String, LibraryError> {
        self.report(SummaryStage::Overview);
        let book = self.book_line();
        let budget = NOTES_TOKENS
            .saturating_sub(estimate_tokens(&cast_text(cast)) + 200)
            .max(800);
        let mut summaries = chapters.to_vec();
        while summaries.len() > 1 && estimate_tokens(&summaries.join("\n\n")) > budget {
            self.report(SummaryStage::Combining);
            let mut groups: Vec<Vec<String>> = vec![Vec::new()];
            for summary in &summaries {
                let group = groups.last_mut().expect("never empty");
                let size = estimate_tokens(&group.join("\n\n")) + estimate_tokens(summary);
                if !group.is_empty() && size > NOTES_TOKENS {
                    groups.push(vec![summary.clone()]);
                } else {
                    group.push(summary.clone());
                }
            }
            if groups.len() == summaries.len() {
                summaries = summaries.iter().map(|s| shorten(s, 2)).collect();
                continue;
            }
            let mut next = Vec::new();
            for group in groups {
                if group.len() == 1 {
                    next.extend(group);
                    continue;
                }
                let prompt = stretch_prompt(&book, &group);
                match self.ask(request(WRITER_INSTRUCTIONS, prompt, NOTE_MAX_TOKENS), false) {
                    Ok(text) if !text.is_empty() => next.push(text),
                    Ok(_)
                    | Err(
                        SummaryModelError::Refused { .. }
                        | SummaryModelError::ContextTooLong { .. },
                    ) => next.push(
                        group
                            .iter()
                            .map(|s| shorten(s, 1))
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    Err(err) => return Err(self.error(err)),
                }
            }
            summaries = next;
        }
        self.report(SummaryStage::Writing {
            draft: String::new(),
        });
        self.ask(
            request(
                WRITER_INSTRUCTIONS,
                overview_prompt(&book, cast, &summaries),
                OVERVIEW_MAX_TOKENS,
            ),
            true,
        )
        .map_err(|e| self.error(e))
    }

    fn book_line(&self) -> String {
        self.source
            .header()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string()
    }

    fn ask(&self, request: SummaryRequest, last: bool) -> Result<String, SummaryModelError> {
        if self.cancel.is_cancelled() {
            return Err(SummaryModelError::Stopped);
        }
        self.requests.set(self.requests.get() + 1);
        // Only the final summary is shown as it is written.
        let draft = if last { self.progress.clone() } else { None };
        let sink = Arc::new(SummarySink {
            cancel: self.cancel.clone(),
            draft,
        });
        let result = forge_runtime::block_on(self.model.generate(request, sink));
        if self.cancel.is_cancelled() {
            return Err(SummaryModelError::Stopped);
        }
        result.map(|text| clean(&text))
    }

    fn report(&self, stage: SummaryStage) {
        if let Some(progress) = &self.progress {
            progress.changed(stage);
        }
    }

    fn error(&self, err: SummaryModelError) -> LibraryError {
        match err {
            SummaryModelError::Stopped => LibraryError::Cancelled,
            _ if self.cancel.is_cancelled() => LibraryError::Cancelled,
            SummaryModelError::Unavailable { reason } => {
                LibraryError::SummaryUnavailable { reason }
            }
            SummaryModelError::Refused { reason } => LibraryError::SummaryRefused { reason },
            SummaryModelError::Busy { .. } => LibraryError::Interrupted,
            SummaryModelError::ContextTooLong { reason } | SummaryModelError::Engine { reason } => {
                LibraryError::SummaryFailed { reason }
            }
        }
    }
}

/// The first `n` sentences of a note.
fn shorten(note: &str, n: usize) -> String {
    sentences(note)
        .into_iter()
        .take(n)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A model that answers from a script and records what it was asked.
    pub(crate) struct FakeModel {
        pub asked: Mutex<Vec<SummaryRequest>>,
        /// Prompts containing this are declined.
        pub refuse: Option<&'static str>,
        /// Prompts estimated above this many tokens are "too long".
        pub window: usize,
        pub available: bool,
        /// Requests after this many answer `Busy`.
        pub busy_after: Option<usize>,
    }

    impl FakeModel {
        pub(crate) fn new() -> Self {
            Self {
                asked: Mutex::new(Vec::new()),
                refuse: None,
                window: CONTEXT_TOKENS,
                available: true,
                busy_after: None,
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.asked
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.prompt.clone())
                .collect()
        }
    }

    #[async_trait::async_trait]
    impl SummaryModel for FakeModel {
        fn describe(&self) -> SummaryModelInfo {
            SummaryModelInfo {
                name: "Fake Model".into(),
                available: self.available,
                unavailable_reason: (!self.available).then(|| "Turn it on.".into()),
            }
        }

        async fn generate(
            &self,
            request: SummaryRequest,
            sink: Arc<SummarySink>,
        ) -> Result<String, SummaryModelError> {
            let n = {
                let mut asked = self.asked.lock().unwrap();
                asked.push(request.clone());
                asked.len()
            };
            if self.busy_after.is_some_and(|limit| n > limit) {
                return Err(SummaryModelError::Busy {
                    reason: "rate limited".into(),
                });
            }
            if self.refuse.is_some_and(|r| request.prompt.contains(r)) {
                return Err(SummaryModelError::Refused {
                    reason: "no".into(),
                });
            }
            let size = estimate_tokens(&request.instructions)
                + estimate_tokens(&request.prompt)
                + request.max_tokens as usize;
            if size > self.window {
                return Err(SummaryModelError::ContextTooLong {
                    reason: format!("{size} tokens"),
                });
            }
            let answer = format!("Here is a summary:\n**Answer {n}.**");
            if !sink.update("Answer".into()) {
                return Err(SummaryModelError::Stopped);
            }
            Ok(answer)
        }
    }

    struct Stages(Mutex<Vec<SummaryStage>>);

    impl SummaryProgress for Stages {
        fn changed(&self, stage: SummaryStage) {
            self.0.lock().unwrap().push(stage);
        }
    }

    fn source() -> Source {
        Source {
            book_title: "Night Trains".into(),
            author: "A. Writer".into(),
            chapter_title: "The Platform".into(),
        }
    }

    /// `n` paragraphs of `words` words each, numbered so order shows.
    fn paragraphs(n: usize, words: usize) -> Vec<String> {
        (0..n)
            .map(|i| {
                let mut p = format!("Paragraph{i} begins.");
                for _ in 0..words {
                    p.push_str(" word");
                }
                p.push('.');
                p
            })
            .collect()
    }

    fn run(
        model: &FakeModel,
        text: &[String],
        progress: Option<Arc<dyn SummaryProgress>>,
    ) -> Result<Written, LibraryError> {
        let cancel = CancelHandle::new();
        Summariser {
            model,
            cancel: &cancel,
            progress,
            requests: Default::default(),
            source: source(),
        }
        .run(text)
    }

    #[test]
    fn parts_fit_the_budget_keep_order_and_are_even() {
        let text = paragraphs(40, 100); // ~40 × 130 tokens
        let parts = split_into_parts(&text, PART_TOKENS);
        assert!(parts.len() >= 3);
        for part in &parts {
            assert!(estimate_tokens(part) <= PART_TOKENS);
        }
        let joined = parts.join("\n\n");
        assert_eq!(joined, text.join("\n\n"));
        let sizes: Vec<usize> = parts.iter().map(|p| estimate_tokens(p)).collect();
        let (min, max) = (sizes.iter().min().unwrap(), sizes.iter().max().unwrap());
        assert!(*min * 2 > *max, "uneven parts: {sizes:?}");
    }

    #[test]
    fn a_huge_paragraph_is_split_at_sentences_or_spaces() {
        let sentences = vec!["A sentence of several words here. ".repeat(600)];
        for part in split_into_parts(&sentences, 500) {
            assert!(estimate_tokens(&part) <= 500);
            assert!(part.ends_with('.'));
        }
        let words = vec!["word ".repeat(5000)];
        let parts = split_into_parts(&words, 500);
        assert!(parts.len() >= 12);
        assert!(parts.iter().all(|p| estimate_tokens(p) <= 500));
        let unbroken = vec!["x".repeat(5000)];
        assert!(split_into_parts(&unbroken, 500)
            .iter()
            .all(|p| estimate_tokens(p) <= 500));
    }

    #[test]
    fn other_scripts_count_as_more_tokens() {
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens("日本語"), 3);
    }

    #[test]
    fn the_writer_sees_only_the_readers_facts() {
        let model = FakeModel::new();
        let stages = Arc::new(Stages(Mutex::new(Vec::new())));
        let written = run(&model, &paragraphs(5, 100), Some(stages.clone())).unwrap();
        assert_eq!(written.text, "Answer 2.");
        assert_eq!((written.parts, written.skipped_passages), (1, 0));
        let asked = model.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 2);
        // The reader gets the text and is told not to copy it.
        assert_eq!(asked[0].instructions, READER_INSTRUCTIONS);
        assert!(asked[0]
            .prompt
            .contains("Book: \u{201C}Night Trains\u{201D} by A. Writer"));
        assert!(asked[0].prompt.contains("Chapter: The Platform"));
        assert!(
            asked[0].prompt.contains("Paragraph0 begins.")
                && asked[0].prompt.contains("Paragraph4")
        );
        assert!(asked[0].prompt.contains("The text is the chapter."));
        // The writer gets the facts, never the book's text.
        assert_eq!(asked[1].instructions, WRITER_INSTRUCTIONS);
        assert!(asked[1].prompt.contains("Part 1: Answer 1."));
        assert!(!asked[1].prompt.contains("Paragraph0"));
        // The draft is shown as it is written.
        assert_eq!(
            stages.0.lock().unwrap().last(),
            Some(&SummaryStage::Writing {
                draft: "Answer".into()
            })
        );
    }

    #[test]
    fn a_long_chapter_is_read_in_parts_with_the_notes_before() {
        let model = FakeModel::new();
        let stages = Arc::new(Stages(Mutex::new(Vec::new())));
        let text = paragraphs(40, 100);
        let parts = split_into_parts(&text, PART_TOKENS).len();
        let written = run(&model, &text, Some(stages.clone())).unwrap();
        assert_eq!(written.parts as usize, parts);
        let prompts = model.prompts();
        assert_eq!(prompts.len(), parts + 1);
        assert!(prompts[0].contains(&format!("part 1 of {parts}")));
        assert!(!prompts[0].contains("Notes on the part before"));
        assert!(prompts[1]
            .contains("Notes on the part before (for names and context only):\nAnswer 1."));
        let last = prompts.last().unwrap();
        assert!(last.contains("Part 1: Answer 1.") && last.contains("From these notes"));
        assert_eq!(written.text, format!("Answer {}.", parts + 1));
        let stages = stages.0.lock().unwrap();
        assert_eq!(
            stages[0],
            SummaryStage::ReadingPart {
                part: 1,
                parts: parts as u32
            }
        );
        // Only the final summary's draft is shown.
        let drafts = stages
            .iter()
            .filter(|s| matches!(s, SummaryStage::Writing { draft } if !draft.is_empty()))
            .count();
        assert_eq!(drafts, 1);
    }

    #[test]
    fn a_declined_passage_is_left_out_and_counted() {
        let mut model = FakeModel::new();
        model.refuse = Some("Paragraph12 ");
        let text = paragraphs(40, 100);
        let written = run(&model, &text, None).unwrap();
        assert_eq!(written.skipped_passages, 1);
        // The part was split; the half without the passage was still read.
        // The part was split until the passage was small; the rest of it
        // was still read.
        let read: Vec<String> = model
            .prompts()
            .into_iter()
            .filter(|p| !p.contains("Paragraph12 "))
            .collect();
        for i in (0..40).filter(|&i| !(12..=13).contains(&i)) {
            let mark = format!("Paragraph{i} ");
            assert!(read.iter().any(|p| p.contains(&mark)), "{mark}was not read");
        }
    }

    #[test]
    fn a_whole_chapter_declined_is_an_error() {
        let mut model = FakeModel::new();
        model.refuse = Some("Night Trains");
        assert!(matches!(
            run(&model, &paragraphs(40, 100), None),
            Err(LibraryError::SummaryRefused { .. })
        ));
    }

    #[test]
    fn a_smaller_window_than_expected_splits_parts() {
        let mut model = FakeModel::new();
        model.window = 1500;
        let written = run(&model, &paragraphs(12, 100), None).unwrap();
        assert_eq!(written.skipped_passages, 0);
        assert!(model.prompts().len() > 2);
    }

    #[test]
    fn long_notes_are_combined_before_the_summary() {
        let model = FakeModel::new();
        let notes: Vec<String> = (0..60)
            .map(|i| format!("Note {i}. {}", "detail ".repeat(40)))
            .collect();
        let cancel = CancelHandle::new();
        let summariser = Summariser {
            model: &model,
            cancel: &cancel,
            progress: None,
            requests: Default::default(),
            source: source(),
        };
        let combined = summariser.combine(&notes).unwrap();
        assert!(combined.len() < notes.len());
        assert!(model.prompts()[0].contains("Combine these notes"));
    }

    #[test]
    fn cancelling_stops_with_cancelled() {
        let model = FakeModel::new();
        let cancel = CancelHandle::new();
        cancel.cancel();
        let result = Summariser {
            model: &model,
            cancel: &cancel,
            progress: None,
            requests: Default::default(),
            source: source(),
        }
        .run(&paragraphs(40, 100));
        assert_eq!(result, Err(LibraryError::Cancelled));
        assert!(model.prompts().is_empty());
    }

    #[test]
    fn answers_lose_what_small_models_add() {
        assert_eq!(
            clean("Here is a summary:\n\n**Anna** leaves."),
            "Anna leaves."
        );
        assert_eq!(clean("Summary: Anna leaves."), "Anna leaves.");
        assert_eq!(clean("\"Anna leaves.\""), "Anna leaves.");
        assert_eq!(
            clean("\"Go,\" she says. Then \"stay.\""),
            "\"Go,\" she says. Then \"stay.\""
        );
        assert_eq!(clean("One.\n\n\n\nTwo."), "One.\n\nTwo.");
        assert_eq!(clean("# The Platform\nAnna leaves."), "Anna leaves.");
    }

    /// A writer that copies the book's opening, then rewrites when asked.
    struct Copier(Mutex<Vec<String>>);

    #[async_trait::async_trait]
    impl SummaryModel for Copier {
        fn describe(&self) -> SummaryModelInfo {
            FakeModel::new().describe()
        }

        async fn generate(
            &self,
            request: SummaryRequest,
            _: Arc<SummarySink>,
        ) -> Result<String, SummaryModelError> {
            self.0.lock().unwrap().push(request.prompt.clone());
            Ok(if request.prompt.contains("The text:") {
                "- Anna waits for the night train.".into()
            } else if request.prompt.contains("Rewrite the summary") {
                "Anna waits alone for a train that never comes.".into()
            } else {
                "The rain had not stopped for three days when Anna came. She waits.".into()
            })
        }
    }

    #[test]
    fn a_summary_that_repeats_the_book_is_rewritten() {
        let text = vec![
            "The rain had not stopped for three days when Anna came to the station.".to_string(),
            "word ".repeat(200),
        ];
        let model = Copier(Mutex::new(Vec::new()));
        let stages = Arc::new(Stages(Mutex::new(Vec::new())));
        let cancel = CancelHandle::new();
        let written = Summariser {
            model: &model,
            cancel: &cancel,
            progress: Some(stages.clone()),
            requests: Default::default(),
            source: source(),
        }
        .run(&text)
        .unwrap();
        assert_eq!(
            written.text,
            "Anna waits alone for a train that never comes."
        );
        let prompts = model.0.lock().unwrap().clone();
        assert_eq!(prompts.len(), 3);
        assert!(prompts[2]
            .contains("\u{201C}the rain had not stopped for three days when anna came\u{201D}"));
        assert!(stages.0.lock().unwrap().contains(&SummaryStage::Revising));
    }

    #[test]
    fn copying_is_long_runs_of_the_same_words() {
        let book = vec!["\u{201C}Go,\u{201D} said Marcus Brutus, and the senators rose from their seats at once.".to_string()];
        assert!(copied_phrases("Marcus Brutus tells the senators to go.", &book).is_empty());
        assert_eq!(
            copied_phrases(
                "Brutus speaks, and the senators rose from their seats at once.",
                &book
            ),
            vec!["and the senators rose from their seats at once".to_string()]
        );
    }

    #[test]
    fn the_cast_list_is_read_back_from_the_models_lines() {
        let cast = parse_cast(
            "Here is the list:\n- Anna: a teacher who misses her train\n* **Marcus** \u{2014} her brother\n\
             - Anna: again\nNo list here\n- : nobody",
        );
        assert_eq!(
            cast,
            vec![
                CastMember {
                    name: "Anna".into(),
                    role: "a teacher who misses her train".into()
                },
                CastMember {
                    name: "Marcus".into(),
                    role: "her brother".into()
                },
            ]
        );
    }

    #[test]
    fn a_book_leaves_out_title_pages() {
        let mut chapters = vec![
            chapter(0, false),
            chapter(1, true),
            chapter(2, false),
            chapter(3, true),
        ];
        for c in &mut chapters {
            c.words = 500;
        }
        chapters[0].words = 20;
        assert_eq!(book_sections(&chapters), vec![1..3, 3..4]);
    }

    fn chapter(index: u32, listed: bool) -> crate::ChapterSummary {
        crate::ChapterSummary {
            index,
            title: format!("C{index}"),
            in_contents: listed,
            words: 100,
        }
    }

    #[test]
    fn a_chapter_split_across_files_is_one_section() {
        // Front matter, chapter 1 in three files, chapter 2 in one.
        let chapters = vec![
            chapter(0, false),
            chapter(1, true),
            chapter(2, false),
            chapter(3, false),
            chapter(4, true),
        ];
        assert_eq!(section(&chapters, 0), 0..1);
        assert_eq!(section(&chapters, 1), 1..4);
        assert_eq!(section(&chapters, 3), 1..4);
        assert_eq!(section(&chapters, 4), 4..5);
        // No table of contents: a file at a time.
        let unlisted = vec![chapter(0, false), chapter(1, false)];
        assert_eq!(section(&unlisted, 1), 1..2);
    }
}
