//! Chapter summaries written by a small on-device language model (Apple
//! Intelligence through Foundation Models, in Swift).
//!
//! The model reads about 4,000 tokens at a time, prompt and answer
//! together, which is far less than a long chapter. So Rust plans the work
//! and the model only answers one short request at a time:
//!
//! 1. The chapter's paragraphs are packed into evenly sized **parts** that
//!    fit the window with room to spare ([`split_into_parts`]).
//! 2. The model writes **notes** on each part in turn, with the notes on
//!    the part before as context, so names and threads carry over.
//! 3. If the notes together are still too long, consecutive notes are
//!    **combined** in groups until they fit.
//! 4. The model writes the **summary** from the notes (or, for a chapter
//!    that fits in one part, straight from the text).
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
/// Low, so the model sticks to the text.
const TEMPERATURE: f64 = 0.3;
/// Shorter than this is a title page or a copyright notice.
pub const MIN_WORDS: u32 = 120;
/// Longer than this takes many minutes on a phone; such "chapters" are
/// usually a whole book in one file.
pub const MAX_WORDS: u32 = 60_000;
/// How many times a part is split in two after the model can't read it.
const MAX_SPLITS: u32 = 3;

const INSTRUCTIONS: &str = "You summarise chapters of books for a reader who wants to remember what \
they read. Use only the text you are given. Name the people, places and events that matter, and keep \
them in order. Do not add opinions, morals, warnings or anything the text does not say. Write plain \
prose in the language of the text, with no headings, lists or quotation of these instructions.";

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

fn whole_chapter_prompt(source: &Source, text: &str) -> String {
    format!(
        "{}\n\nThe chapter:\n<<<\n{text}\n>>>\n\nSummarise this chapter in one paragraph of 4 to 6 \
         sentences: what happens or what it explains, who is involved, and how it ends.",
        source.header()
    )
}

fn part_prompt(
    source: &Source,
    part: usize,
    parts: usize,
    text: &str,
    earlier: Option<&str>,
) -> String {
    let earlier = earlier
        .map(|n| format!("Notes on the part before:\n{n}\n\n"))
        .unwrap_or_default();
    format!(
        "{}\nThis is part {part} of {parts} of the chapter.\n\n{earlier}The text of part {part}:\n<<<\n\
         {text}\n>>>\n\nWrite 3 to 5 sentences of notes on part {part} only: what happens or what it \
         explains, who is involved, and anything revealed. Use the names from the text.",
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
        "{}\n\nNotes on each part of the chapter, in order:\n\n{}\n\nUsing these notes, summarise the \
         whole chapter in one paragraph of 4 to 6 sentences: what happens or what it explains, who is \
         involved, and how it ends. Keep the events in order.",
        source.header(),
        notes_list(notes)
    )
}

fn request(prompt: String, max_tokens: u32) -> SummaryRequest {
    SummaryRequest {
        instructions: INSTRUCTIONS.to_string(),
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
    pub source: Source,
}

impl Summariser<'_> {
    pub fn run(&self, paragraphs: &[String]) -> Result<Written, LibraryError> {
        let parts = split_into_parts(paragraphs, PART_TOKENS);
        if parts.is_empty() {
            return Err(LibraryError::invalid(
                "this chapter has no text to summarise",
            ));
        }
        if parts.len() == 1 {
            self.report(SummaryStage::Writing {
                draft: String::new(),
            });
            let prompt = whole_chapter_prompt(&self.source, &parts[0]);
            return match self.ask(request(prompt, SUMMARY_MAX_TOKENS), true) {
                Ok(text) => Ok(Written {
                    text,
                    parts: 1,
                    skipped_passages: 0,
                }),
                // Too long after all, or declined: read it in pieces.
                Err(
                    SummaryModelError::ContextTooLong { .. } | SummaryModelError::Refused { .. },
                ) => self.by_parts(parts),
                Err(err) => Err(self.error(err)),
            };
        }
        self.by_parts(parts)
    }

    fn by_parts(&self, mut parts: Vec<String>) -> Result<Written, LibraryError> {
        if parts.len() == 1 {
            // A single part the model wouldn't take whole.
            match halve(&parts[0]) {
                Some((a, b)) => parts = vec![a, b],
                None => {
                    return Err(LibraryError::SummaryRefused {
                        reason: "the model declined this chapter".into(),
                    })
                }
            }
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
        let text = self
            .ask(
                request(final_prompt(&self.source, &notes), SUMMARY_MAX_TOKENS),
                true,
            )
            .map_err(|e| self.error(e))?;
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
        match self.ask(request(prompt, NOTE_MAX_TOKENS), false) {
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
            match self.ask(request(prompt, NOTE_MAX_TOKENS), false) {
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

    fn ask(&self, request: SummaryRequest, last: bool) -> Result<String, SummaryModelError> {
        if self.cancel.is_cancelled() {
            return Err(SummaryModelError::Stopped);
        }
        // The final summary is shown as it is written.
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
    }

    impl FakeModel {
        pub(crate) fn new() -> Self {
            Self {
                asked: Mutex::new(Vec::new()),
                refuse: None,
                window: CONTEXT_TOKENS,
                available: true,
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
    fn a_short_chapter_is_summarised_in_one_request() {
        let model = FakeModel::new();
        let stages = Arc::new(Stages(Mutex::new(Vec::new())));
        let written = run(&model, &paragraphs(5, 100), Some(stages.clone())).unwrap();
        assert_eq!(written.text, "Answer 1.");
        assert_eq!((written.parts, written.skipped_passages), (1, 0));
        let prompts = model.prompts();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("Book: \u{201C}Night Trains\u{201D} by A. Writer"));
        assert!(prompts[0].contains("Chapter: The Platform"));
        assert!(prompts[0].contains("Paragraph0 begins.") && prompts[0].contains("Paragraph4"));
        assert!(prompts[0].contains("Summarise this chapter"));
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
        assert!(prompts[1].contains("Notes on the part before:\nAnswer 1."));
        let last = prompts.last().unwrap();
        assert!(last.contains("Part 1: Answer 1.") && last.contains("summarise the whole chapter"));
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
