//! The library's rules, as plain functions over [`LibraryData`]. No I/O and no
//! clock: callers pass `now`, so every rule is tested directly.

use forge_app::Validator;
use serde::{Deserialize, Serialize};

use crate::error::LibraryError;
use crate::model::{
    ActivityEntry, ActivityKind, Book, BookInput, BookSource, LibraryQuery, LibrarySettings,
    LibrarySort, LibraryStats, ReadingPosition, ReadingStatus,
};

pub const MAX_TITLE_CHARS: usize = 300;
pub const MAX_AUTHOR_CHARS: usize = 200;
pub const MAX_DESCRIPTION_CHARS: usize = 20_000;
pub const MAX_NOTES_CHARS: usize = 20_000;
pub const MAX_PAGES: u32 = 100_000;
/// Oldest history entries are dropped beyond this.
pub const MAX_ACTIVITY: usize = 2_000;
/// Progress updates on the same book closer together than this are one
/// reading session in the history.
pub const SESSION_GAP_MS: i64 = 60 * 60 * 1000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// A book as saved. Covers are file names inside the library's `covers/`
/// folder, never absolute paths: iOS gives the app a new container path
/// when it is reinstalled or updated.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StoredBook {
    pub id: u64,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub status: ReadingStatus,
    pub current_page: u32,
    pub total_pages: Option<u32>,
    pub rating: Option<u8>,
    #[serde(default)]
    pub notes: String,
    pub cover_file: Option<String>,
    pub cover_url: Option<String>,
    pub source: BookSource,
    pub added_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    /// The saved EPUB: a file name inside the library's `books/` folder.
    #[serde(default)]
    pub epub_file: Option<String>,
    #[serde(default)]
    pub reading_position: Option<SavedPosition>,
}

/// Where the reader was left, and the page that put the book on. If the
/// page has changed since (typed in by hand), the reader opens at the new
/// page instead.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SavedPosition {
    pub chapter: u32,
    pub fraction: f64,
    pub page: u32,
}

/// Everything in `library.json`. Fields added later carry `#[serde(default)]`
/// so older files load without a schema bump; a change that cannot be
/// defaulted bumps `STORE_VERSION` and adds a migration (see `store.rs`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LibraryData {
    pub next_id: u64,
    pub books: Vec<StoredBook>,
    #[serde(default)]
    pub next_activity_id: u64,
    #[serde(default)]
    pub activity: Vec<ActivityEntry>,
    #[serde(default)]
    pub settings: LibrarySettings,
}

impl StoredBook {
    pub fn progress(&self) -> f64 {
        match (self.status, self.total_pages) {
            (ReadingStatus::Finished, _) => 1.0,
            (_, Some(total)) if total > 0 => {
                (f64::from(self.current_page) / f64::from(total)).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    /// The record the host sees; `covers_dir` turns the saved file name into
    /// a path for this launch.
    pub fn to_book(&self, covers_dir: &std::path::Path) -> Book {
        Book {
            id: self.id,
            title: self.title.clone(),
            author: self.author.clone(),
            description: self.description.clone(),
            status: self.status,
            current_page: self.current_page,
            total_pages: self.total_pages,
            progress: self.progress(),
            rating: self.rating,
            notes: self.notes.clone(),
            cover_file: self
                .cover_file
                .as_ref()
                .map(|name| covers_dir.join(name).to_string_lossy().into_owned()),
            cover_url: self.cover_url.clone(),
            source: self.source.clone(),
            can_read: self.epub_file.is_some(),
            reading_position: self.reading_position.map(|p| ReadingPosition {
                chapter: p.chapter,
                fraction: p.fraction,
            }),
            added_at_ms: self.added_at_ms,
            started_at_ms: self.started_at_ms,
            finished_at_ms: self.finished_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// A form that passed validation: trimmed, within limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidInput {
    pub title: String,
    pub author: String,
    pub description: String,
    pub status: ReadingStatus,
    pub current_page: u32,
    pub total_pages: Option<u32>,
    pub rating: Option<u8>,
    pub notes: String,
    pub cover_url: Option<String>,
}

/// Checks the book form, field by field (`forge_app::Validator`), and
/// tidies it. Field names are `BookInput`'s, so the form can show each
/// problem under its own field.
pub fn validate(input: BookInput) -> Result<ValidInput, LibraryError> {
    let title = input.title.trim().to_string();
    let author = input.author.trim().to_string();
    let description = input.description.trim().to_string();
    let notes = input.notes.trim_end().to_string();
    let total_pages = match input.total_pages {
        Some(0) | None => None,
        Some(pages) => Some(pages),
    };
    let cover_url = input
        .cover_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());

    let mut v = Validator::new();
    v.required("title", &title, "Enter a title.")
        .max_chars(
            "title",
            &title,
            MAX_TITLE_CHARS,
            &format!("Keep the title under {MAX_TITLE_CHARS} characters."),
        )
        .max_chars(
            "author",
            &author,
            MAX_AUTHOR_CHARS,
            &format!("Keep the author under {MAX_AUTHOR_CHARS} characters."),
        )
        .max_chars(
            "description",
            &description,
            MAX_DESCRIPTION_CHARS,
            &format!("Keep the description under {MAX_DESCRIPTION_CHARS} characters."),
        )
        .max_chars(
            "notes",
            &notes,
            MAX_NOTES_CHARS,
            &format!("Keep your notes under {MAX_NOTES_CHARS} characters."),
        )
        .in_range(
            "total_pages",
            total_pages,
            1..=MAX_PAGES,
            &format!("A book can have at most {MAX_PAGES} pages."),
        );
    match total_pages {
        Some(total) => v.check(
            "current_page",
            input.current_page <= total,
            &format!("The current page is past the last page ({total})."),
        ),
        None => v.check(
            "current_page",
            input.current_page <= MAX_PAGES,
            "That page number is too large.",
        ),
    };
    v.in_range("rating", input.rating, 1..=5, "Give 1 to 5 stars.");
    v.check(
        "cover_url",
        cover_url
            .as_deref()
            .is_none_or(|url| url.starts_with("https://") && url.len() > "https://".len()),
        "Cover links must start with https://.",
    );
    v.finish()?;

    Ok(ValidInput {
        title,
        author,
        description,
        status: input.status,
        current_page: input.current_page,
        total_pages,
        rating: input.rating,
        notes,
        cover_url,
    })
}

/// The reading state a request settles on, before history is written.
///
/// - The last page of a book with a page count means Finished.
/// - Finished means the last page.
/// - A page past 0 on a Want to Read book means Reading.
fn settle(
    status: ReadingStatus,
    page: u32,
    total: Option<u32>,
) -> Result<(ReadingStatus, u32), LibraryError> {
    if let Some(total) = total {
        if page > total {
            return Err(LibraryError::invalid(format!(
                "page {page} is past the last page ({total})"
            )));
        }
    }
    Ok(match (status, total) {
        (ReadingStatus::Finished, Some(total)) => (ReadingStatus::Finished, total),
        (ReadingStatus::Finished, None) => (ReadingStatus::Finished, page),
        (_, Some(total)) if page == total && total > 0 => (ReadingStatus::Finished, total),
        (ReadingStatus::WantToRead, _) if page > 0 => (ReadingStatus::Reading, page),
        _ => (status, page),
    })
}

impl LibraryData {
    pub fn book(&self, id: u64) -> Result<&StoredBook, LibraryError> {
        self.books
            .iter()
            .find(|b| b.id == id)
            .ok_or(LibraryError::NotFound { id })
    }

    fn book_mut(&mut self, id: u64) -> Result<&mut StoredBook, LibraryError> {
        self.books
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or(LibraryError::NotFound { id })
    }

    pub fn book_with_source(&self, source: &BookSource) -> Option<&StoredBook> {
        self.books.iter().find(|b| &b.source == source)
    }

    fn record(&mut self, book_id: u64, book_title: &str, at_ms: i64, kind: ActivityKind) {
        // Several progress updates in one sitting are one entry.
        if let ActivityKind::Progress { to_page, .. } = kind {
            if let Some(last) = self.activity.last_mut() {
                if last.book_id == book_id && at_ms - last.at_ms < SESSION_GAP_MS {
                    if let ActivityKind::Progress {
                        to_page: last_to, ..
                    } = &mut last.kind
                    {
                        *last_to = to_page;
                        last.at_ms = at_ms;
                        return;
                    }
                }
            }
        }
        self.next_activity_id += 1;
        self.activity.push(ActivityEntry {
            id: self.next_activity_id,
            book_id,
            book_title: book_title.to_string(),
            at_ms,
            kind,
        });
        if self.activity.len() > MAX_ACTIVITY {
            let excess = self.activity.len() - MAX_ACTIVITY;
            self.activity.drain(..excess);
        }
    }

    /// Adds a book. Its starting status and page follow the same rules as
    /// later changes; the history gets one "Added" entry.
    pub fn add(
        &mut self,
        input: ValidInput,
        source: BookSource,
        cover_file: Option<String>,
        now: i64,
    ) -> Result<u64, LibraryError> {
        let (status, page) = settle(input.status, input.current_page, input.total_pages)?;
        self.next_id += 1;
        let id = self.next_id;
        self.books.push(StoredBook {
            id,
            title: input.title,
            author: input.author,
            description: input.description,
            status,
            current_page: page,
            total_pages: input.total_pages,
            rating: input.rating,
            notes: input.notes,
            cover_file,
            cover_url: input.cover_url,
            source,
            added_at_ms: now,
            started_at_ms: (status != ReadingStatus::WantToRead).then_some(now),
            finished_at_ms: (status == ReadingStatus::Finished).then_some(now),
            updated_at_ms: now,
            epub_file: None,
            reading_position: None,
        });
        let title = self.book(id)?.title.clone();
        self.record(id, &title, now, ActivityKind::Added);
        Ok(id)
    }

    /// Moves a book to `status` at `page`, recording what happened:
    /// starting, pages read, finishing, or moving back.
    pub fn set_reading_state(
        &mut self,
        id: u64,
        status: ReadingStatus,
        page: u32,
        now: i64,
    ) -> Result<(), LibraryError> {
        let book = self.book(id)?;
        let (status, page) = settle(status, page, book.total_pages)?;
        let (old_status, old_page) = (book.status, book.current_page);
        let title = book.title.clone();
        if (status, page) == (old_status, old_page) {
            return Ok(());
        }

        let book = self.book_mut(id)?;
        book.status = status;
        book.current_page = page;
        book.updated_at_ms = now;
        let mut history = Vec::new();
        match status {
            ReadingStatus::WantToRead => {
                book.current_page = 0;
                book.finished_at_ms = None;
                history.push(ActivityKind::StatusChanged { status });
            }
            ReadingStatus::Reading => {
                book.finished_at_ms = None;
                match old_status {
                    ReadingStatus::WantToRead => {
                        book.started_at_ms = Some(now);
                        history.push(ActivityKind::Started);
                    }
                    ReadingStatus::Finished => history.push(ActivityKind::StatusChanged { status }),
                    ReadingStatus::Reading => {}
                }
            }
            ReadingStatus::Finished => {
                if old_status == ReadingStatus::WantToRead {
                    book.started_at_ms = Some(now);
                }
                book.finished_at_ms = Some(now);
            }
        }
        // Pages read forwards count as reading; corrections backwards do not.
        if status != ReadingStatus::WantToRead
            && old_status != ReadingStatus::Finished
            && page > old_page
        {
            history.push(ActivityKind::Progress {
                from_page: old_page,
                to_page: page,
            });
        }
        if status == ReadingStatus::Finished && old_status != ReadingStatus::Finished {
            history.push(ActivityKind::Finished);
        }
        for kind in history {
            self.record(id, &title, now, kind);
        }
        Ok(())
    }

    /// Sets the book's saved EPUB (a file name in `books/`). A new file
    /// starts the reader from the book's current page.
    pub fn attach_epub(&mut self, id: u64, epub_file: Option<String>) -> Result<(), LibraryError> {
        let book = self.book_mut(id)?;
        book.epub_file = epub_file;
        book.reading_position = None;
        Ok(())
    }

    /// Records where the reader is in a book's EPUB and moves its current
    /// page to match, so reading in the app counts as reading (history,
    /// started, pages this week). `chapter_words` are the words in each
    /// chapter. The page stops one short of the last: finishing is the
    /// reader's choice. A finished book keeps its page and status.
    pub fn save_reading_position(
        &mut self,
        id: u64,
        position: ReadingPosition,
        chapter_words: &[u32],
        now: i64,
    ) -> Result<(), LibraryError> {
        if position.chapter as usize >= chapter_words.len() {
            return Err(LibraryError::invalid(format!(
                "the book has no chapter {}",
                position.chapter
            )));
        }
        let position = ReadingPosition {
            chapter: position.chapter,
            fraction: clamp_fraction(position.fraction),
        };
        let book = self.book(id)?;
        if book.status != ReadingStatus::Finished {
            let page = page_at(
                book_fraction(chapter_words, position),
                book.total_pages,
                chapter_words,
            );
            self.set_reading_state(id, book.status, page, now)?;
        }
        let book = self.book_mut(id)?;
        book.reading_position = Some(SavedPosition {
            chapter: position.chapter,
            fraction: position.fraction,
            page: book.current_page,
        });
        Ok(())
    }

    /// Where the reader should open a book: the saved position if the page
    /// has not changed since, otherwise the place that page is at (the start
    /// for a book not begun, or finished without the reader).
    pub fn reader_start(
        &self,
        id: u64,
        chapter_words: &[u32],
    ) -> Result<ReadingPosition, LibraryError> {
        let book = self.book(id)?;
        let start = ReadingPosition {
            chapter: 0,
            fraction: 0.0,
        };
        if let Some(saved) = book.reading_position {
            if saved.page == book.current_page && (saved.chapter as usize) < chapter_words.len() {
                return Ok(ReadingPosition {
                    chapter: saved.chapter,
                    fraction: clamp_fraction(saved.fraction),
                });
            }
        }
        if book.status == ReadingStatus::Finished || book.current_page == 0 {
            return Ok(start);
        }
        let total_words: u64 = chapter_words.iter().map(|&w| u64::from(w)).sum();
        let fraction = match book.total_pages {
            Some(total) if total > 0 => f64::from(book.current_page) / f64::from(total),
            _ if total_words > 0 => {
                f64::from(book.current_page) * f64::from(crate::epub::WORDS_PER_PAGE)
                    / total_words as f64
            }
            _ => 0.0,
        };
        Ok(position_at(chapter_words, fraction))
    }

    /// Applies an edit form: details replace the old ones, and the status
    /// and page go through [`Self::set_reading_state`].
    pub fn update(&mut self, id: u64, input: ValidInput, now: i64) -> Result<(), LibraryError> {
        // Check the reading state against the new page count before
        // changing anything.
        settle(input.status, input.current_page, input.total_pages)?;
        let book = self.book_mut(id)?;
        book.title = input.title;
        book.author = input.author;
        book.description = input.description;
        book.total_pages = input.total_pages;
        book.rating = input.rating;
        book.notes = input.notes;
        if input.cover_url.is_some() || book.cover_file.is_none() {
            book.cover_url = input.cover_url;
        }
        book.updated_at_ms = now;
        self.set_reading_state(id, input.status, input.current_page, now)
    }

    /// Removes a book and returns it (so its cover file can be deleted). Its
    /// history stays, under the title it had.
    pub fn delete(&mut self, id: u64) -> Result<StoredBook, LibraryError> {
        let index = self
            .books
            .iter()
            .position(|b| b.id == id)
            .ok_or(LibraryError::NotFound { id })?;
        Ok(self.books.remove(index))
    }

    pub fn query(&self, query: &LibraryQuery) -> Vec<&StoredBook> {
        let words: Vec<String> = query.text.split_whitespace().map(fold).collect();
        let mut books: Vec<&StoredBook> = self
            .books
            .iter()
            .filter(|b| query.status.is_none_or(|s| b.status == s))
            .filter(|b| {
                if words.is_empty() {
                    return true;
                }
                let haystack = format!("{} {}", fold(&b.title), fold(&b.author));
                words.iter().all(|w| haystack.contains(w.as_str()))
            })
            .collect();
        match query.sort {
            LibrarySort::RecentlyAdded => {
                books.sort_by(|a, b| b.added_at_ms.cmp(&a.added_at_ms).then(b.id.cmp(&a.id)))
            }
            LibrarySort::Title => books.sort_by_cached_key(|b| (sort_title(&b.title), b.id)),
            LibrarySort::Author => books.sort_by_cached_key(|b| {
                (
                    b.author.is_empty(),
                    fold(&b.author),
                    sort_title(&b.title),
                    b.id,
                )
            }),
            LibrarySort::Progress => books.sort_by(|a, b| {
                let known =
                    |x: &StoredBook| x.total_pages.is_some() || x.status == ReadingStatus::Finished;
                known(b)
                    .cmp(&known(a))
                    .then(b.progress().total_cmp(&a.progress()))
                    .then(sort_title(&a.title).cmp(&sort_title(&b.title)))
            }),
        }
        books
    }

    pub fn stats(&self, year_start_ms: i64, now: i64) -> LibraryStats {
        let count = |s| self.books.iter().filter(|b| b.status == s).count() as u32;
        let week_ago = now - 7 * DAY_MS;
        let pages_last_7_days = self
            .activity
            .iter()
            .filter(|e| e.at_ms > week_ago && e.at_ms <= now)
            .map(|e| match e.kind {
                ActivityKind::Progress { from_page, to_page } => to_page.saturating_sub(from_page),
                _ => 0,
            })
            .sum();
        let ratings: Vec<f64> = self
            .books
            .iter()
            .filter_map(|b| b.rating)
            .map(f64::from)
            .collect();
        LibraryStats {
            total: self.books.len() as u32,
            want_to_read: count(ReadingStatus::WantToRead),
            reading: count(ReadingStatus::Reading),
            finished: count(ReadingStatus::Finished),
            finished_this_year: self
                .books
                .iter()
                .filter(|b| b.finished_at_ms.is_some_and(|t| t >= year_start_ms))
                .count() as u32,
            pages_last_7_days,
            average_rating: (!ratings.is_empty())
                .then(|| ratings.iter().sum::<f64>() / ratings.len() as f64),
        }
    }

    /// History, newest first. `book_id` limits it to one book.
    pub fn history(&self, book_id: Option<u64>, limit: usize) -> Vec<ActivityEntry> {
        self.activity
            .iter()
            .rev()
            .filter(|e| book_id.is_none_or(|id| e.book_id == id))
            .take(limit)
            .cloned()
            .collect()
    }
}

/// Lower-case and without common accents, so "Proust" finds "Prōust" and
/// "eco" finds "Éco".
fn clamp_fraction(fraction: f64) -> f64 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// How far through the whole book `position` is, 0.0–1.0, by words.
pub fn book_fraction(chapter_words: &[u32], position: ReadingPosition) -> f64 {
    let total: u64 = chapter_words.iter().map(|&w| u64::from(w)).sum();
    if total == 0 {
        // No text at all (a picture book): go by chapters.
        let chapters = chapter_words.len().max(1) as f64;
        return clamp_fraction((f64::from(position.chapter) + position.fraction) / chapters);
    }
    let before: u64 = chapter_words
        .iter()
        .take(position.chapter as usize)
        .map(|&w| u64::from(w))
        .sum();
    let within = chapter_words
        .get(position.chapter as usize)
        .map_or(0.0, |&w| f64::from(w) * clamp_fraction(position.fraction));
    clamp_fraction((before as f64 + within) / total as f64)
}

/// The position `fraction` of the way through the book, by words.
pub fn position_at(chapter_words: &[u32], fraction: f64) -> ReadingPosition {
    let total: u64 = chapter_words.iter().map(|&w| u64::from(w)).sum();
    let fraction = clamp_fraction(fraction);
    if total == 0 || chapter_words.is_empty() {
        return ReadingPosition {
            chapter: 0,
            fraction: 0.0,
        };
    }
    let target = fraction * total as f64;
    let mut before = 0.0;
    for (index, &words) in chapter_words.iter().enumerate() {
        let words = f64::from(words);
        if words > 0.0 && target < before + words {
            return ReadingPosition {
                chapter: index as u32,
                fraction: (target - before) / words,
            };
        }
        before += words;
    }
    // The very end: the end of the last chapter with text.
    let last = chapter_words.iter().rposition(|&w| w > 0).unwrap_or(0);
    ReadingPosition {
        chapter: last as u32,
        fraction: 1.0,
    }
}

/// The page `fraction` of the way through, never the last page (see
/// [`LibraryData::save_reading_position`]). Without a page count, pages are
/// words read at [`crate::epub::WORDS_PER_PAGE`].
fn page_at(fraction: f64, total_pages: Option<u32>, chapter_words: &[u32]) -> u32 {
    match total_pages {
        Some(total) if total > 0 => {
            let page = (fraction * f64::from(total)).floor() as u32;
            page.min(total - 1)
        }
        _ => {
            let words: u64 = chapter_words.iter().map(|&w| u64::from(w)).sum();
            let read = (fraction * words as f64) as u64;
            u32::try_from(read / u64::from(crate::epub::WORDS_PER_PAGE)).unwrap_or(MAX_PAGES)
        }
    }
}

pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
            'ç' | 'č' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ñ' | 'ń' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
            'ý' | 'ÿ' => 'y',
            'š' | 'ś' => 's',
            'ž' | 'ź' | 'ż' => 'z',
            'ł' => 'l',
            '’' | '‘' => '\'',
            other => other,
        })
        .collect()
}

/// Title order key: folded, without a leading article.
pub fn sort_title(title: &str) -> String {
    let folded = fold(title.trim());
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = folded.strip_prefix(article) {
            return rest.trim_start().to_string();
        }
    }
    folded
}

#[cfg(test)]
mod tests {
    use super::*;
    use ReadingStatus::*;

    const HOUR: i64 = 60 * 60 * 1000;

    pub(crate) fn input(title: &str, total: Option<u32>) -> BookInput {
        BookInput {
            title: title.into(),
            author: "Someone".into(),
            description: String::new(),
            status: WantToRead,
            current_page: 0,
            total_pages: total,
            rating: None,
            notes: String::new(),
            cover_url: None,
        }
    }

    fn add(data: &mut LibraryData, input: BookInput, now: i64) -> u64 {
        data.add(validate(input).unwrap(), BookSource::Manual, None, now)
            .unwrap()
    }

    fn kinds(data: &LibraryData) -> Vec<ActivityKind> {
        data.activity.iter().map(|e| e.kind.clone()).collect()
    }

    #[test]
    fn validation_names_each_field_with_its_problem() {
        let err = validate(BookInput {
            current_page: 11,
            rating: Some(9),
            cover_url: Some("http://insecure.test/c.jpg".into()),
            ..input("  ", Some(10))
        })
        .unwrap_err();
        let LibraryError::InvalidFields { fields } = err else {
            panic!("expected InvalidFields, got {err:?}")
        };
        let names: Vec<&str> = fields.iter().map(|f| f.field.as_str()).collect();
        assert_eq!(names, ["title", "current_page", "rating", "cover_url"]);
        assert_eq!(fields[0].message, "Enter a title.");
        assert_eq!(
            fields[1].message,
            "The current page is past the last page (10)."
        );
    }

    #[test]
    fn validation_trims_and_rejects_bad_input() {
        let ok = validate(BookInput {
            title: "  Dune ".into(),
            total_pages: Some(0),
            cover_url: Some("  ".into()),
            ..input("x", None)
        })
        .unwrap();
        assert_eq!(ok.title, "Dune");
        assert_eq!(ok.total_pages, None, "0 pages means unknown");
        assert_eq!(ok.cover_url, None);

        let bad = |i: BookInput| matches!(validate(i), Err(LibraryError::InvalidFields { .. }));
        assert!(bad(input("   ", None)));
        assert!(bad(input(&"x".repeat(MAX_TITLE_CHARS + 1), None)));
        assert!(bad(BookInput {
            current_page: 11,
            ..input("x", Some(10))
        }));
        assert!(bad(BookInput {
            total_pages: Some(MAX_PAGES + 1),
            ..input("x", None)
        }));
        assert!(bad(BookInput {
            rating: Some(0),
            ..input("x", None)
        }));
        assert!(bad(BookInput {
            rating: Some(6),
            ..input("x", None)
        }));
        assert!(bad(BookInput {
            cover_url: Some("http://insecure.test/c.jpg".into()),
            ..input("x", None)
        }));
        assert!(bad(BookInput {
            cover_url: Some("javascript:alert(1)".into()),
            ..input("x", None)
        }));
        // Exactly at the limits is fine.
        assert!(validate(BookInput {
            current_page: 10,
            rating: Some(5),
            ..input(&"x".repeat(MAX_TITLE_CHARS), Some(10))
        })
        .is_ok());
    }

    #[test]
    fn reading_moves_through_the_statuses_with_history() {
        let mut data = LibraryData::default();
        let id = add(&mut data, input("Dune", Some(400)), 0);
        assert_eq!(data.book(id).unwrap().started_at_ms, None);

        // Reading page 50 of a Want to Read book starts it.
        data.set_reading_state(id, WantToRead, 50, HOUR).unwrap();
        let book = data.book(id).unwrap();
        assert_eq!(
            (book.status, book.current_page, book.started_at_ms),
            (Reading, 50, Some(HOUR))
        );
        assert_eq!(book.progress(), 0.125);

        // The last page finishes it.
        data.set_reading_state(id, Reading, 400, 10 * HOUR).unwrap();
        let book = data.book(id).unwrap();
        assert_eq!(
            (book.status, book.finished_at_ms),
            (Finished, Some(10 * HOUR))
        );
        assert_eq!(book.progress(), 1.0);

        // Rereading clears the finish date; Want to Read resets the page.
        data.set_reading_state(id, Reading, 10, 20 * HOUR).unwrap();
        assert_eq!(data.book(id).unwrap().finished_at_ms, None);
        data.set_reading_state(id, WantToRead, 0, 21 * HOUR)
            .unwrap();
        assert_eq!(data.book(id).unwrap().current_page, 0);

        assert_eq!(
            kinds(&data),
            vec![
                ActivityKind::Added,
                ActivityKind::Started,
                ActivityKind::Progress {
                    from_page: 0,
                    to_page: 50
                },
                ActivityKind::Progress {
                    from_page: 50,
                    to_page: 400
                },
                ActivityKind::Finished,
                ActivityKind::StatusChanged { status: Reading },
                ActivityKind::StatusChanged { status: WantToRead },
            ]
        );
    }

    #[test]
    fn marking_finished_jumps_to_the_last_page() {
        let mut data = LibraryData::default();
        let id = add(&mut data, input("Short", Some(120)), 0);
        data.set_reading_state(id, Finished, 0, HOUR).unwrap();
        let book = data.book(id).unwrap();
        assert_eq!((book.current_page, book.started_at_ms), (120, Some(HOUR)));
        // Without a page count, Finished keeps the page.
        let unknown = add(&mut data, input("Unknown length", None), 0);
        data.set_reading_state(unknown, Finished, 0, HOUR).unwrap();
        assert_eq!(data.book(unknown).unwrap().progress(), 1.0);
        assert!(data.set_reading_state(id, Reading, 121, HOUR).is_err());
    }

    #[test]
    fn progress_in_one_sitting_is_one_history_entry() {
        let mut data = LibraryData::default();
        let id = add(&mut data, input("Long", Some(1000)), 0);
        data.set_reading_state(id, Reading, 10, HOUR).unwrap();
        data.set_reading_state(id, Reading, 20, HOUR + 10 * 60_000)
            .unwrap();
        data.set_reading_state(id, Reading, 35, HOUR + 50 * 60_000)
            .unwrap();
        // A correction backwards changes the page but is not "reading".
        data.set_reading_state(id, Reading, 30, HOUR + 55 * 60_000)
            .unwrap();
        // The next day is a new session.
        data.set_reading_state(id, Reading, 60, 30 * HOUR).unwrap();
        assert_eq!(
            kinds(&data)[2..],
            [
                ActivityKind::Progress {
                    from_page: 0,
                    to_page: 35
                },
                ActivityKind::Progress {
                    from_page: 30,
                    to_page: 60
                },
            ]
        );
        // The same page again records nothing.
        let before = data.activity.len();
        data.set_reading_state(id, Reading, 60, 31 * HOUR).unwrap();
        assert_eq!(data.activity.len(), before);
    }

    #[test]
    fn adding_as_reading_or_finished_sets_dates() {
        let mut data = LibraryData::default();
        let finished = add(
            &mut data,
            BookInput {
                status: Finished,
                ..input("Done", Some(90))
            },
            5,
        );
        let book = data.book(finished).unwrap();
        assert_eq!(
            (book.current_page, book.started_at_ms, book.finished_at_ms),
            (90, Some(5), Some(5))
        );
        let reading = add(
            &mut data,
            BookInput {
                current_page: 3,
                ..input("Started", Some(90))
            },
            6,
        );
        assert_eq!(data.book(reading).unwrap().status, Reading);
    }

    #[test]
    fn edits_replace_details_and_apply_reading_rules() {
        let mut data = LibraryData::default();
        let id = add(&mut data, input("Drafty", Some(100)), 0);
        let edit = BookInput {
            title: "Final Title".into(),
            current_page: 100,
            status: Reading,
            rating: Some(4),
            notes: "Loved it".into(),
            ..input("x", Some(100))
        };
        data.update(id, validate(edit).unwrap(), HOUR).unwrap();
        let book = data.book(id).unwrap();
        assert_eq!(
            (book.title.as_str(), book.status, book.rating),
            ("Final Title", Finished, Some(4))
        );
        assert_eq!(data.activity.last().unwrap().book_title, "Final Title");

        // Shrinking the page count below the current page is refused whole.
        let shrink = BookInput {
            title: "Renamed".into(),
            total_pages: Some(50),
            current_page: 100,
            status: Reading,
            ..input("x", None)
        };
        assert!(validate(shrink).is_err());
        assert_eq!(
            data.update(99, validate(input("x", None)).unwrap(), 0),
            Err(LibraryError::NotFound { id: 99 })
        );
    }

    #[test]
    fn deleting_keeps_history_under_the_old_title() {
        let mut data = LibraryData::default();
        let id = add(&mut data, input("Gone Girl", Some(10)), 0);
        let removed = data.delete(id).unwrap();
        assert_eq!(removed.title, "Gone Girl");
        assert!(data.books.is_empty());
        assert_eq!(data.history(None, 10)[0].book_title, "Gone Girl");
        assert_eq!(data.delete(id), Err(LibraryError::NotFound { id }));
    }

    #[test]
    fn search_filter_and_sort() {
        let mut data = LibraryData::default();
        let mut add_by = |title: &str, author: &str, total, page, now| {
            let id = add(
                &mut data,
                BookInput {
                    author: author.into(),
                    ..input(title, total)
                },
                now,
            );
            if page > 0 {
                data.set_reading_state(id, Reading, page, now).unwrap();
            }
            id
        };
        let hobbit = add_by("The Hobbit", "J.R.R. Tolkien", Some(300), 150, 1);
        let rose = add_by("Il nome della rosa", "Umberto Éco", Some(500), 50, 2);
        let atlas = add_by("An Atlas of Rare Places", "Tolkien Fan", None, 0, 3);
        let zen = add_by("Zen", "", Some(100), 100, 4);

        let q = |text: &str, status, sort| {
            data.query(&LibraryQuery {
                text: text.into(),
                status,
                sort,
            })
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            q("", None, LibrarySort::RecentlyAdded),
            vec![zen, atlas, rose, hobbit]
        );
        assert_eq!(
            q("", None, LibrarySort::Title),
            vec![atlas, hobbit, rose, zen]
        );
        assert_eq!(
            q("", None, LibrarySort::Author),
            vec![hobbit, atlas, rose, zen],
            "J before T before U; no author last"
        );
        assert_eq!(
            q("", None, LibrarySort::Progress),
            vec![zen, hobbit, rose, atlas]
        );
        assert_eq!(q("tolkien", None, LibrarySort::Title), vec![atlas, hobbit]);
        assert_eq!(q("TOLKIEN hob", None, LibrarySort::Title), vec![hobbit]);
        assert_eq!(
            q("eco", None, LibrarySort::Title),
            vec![rose],
            "accents fold"
        );
        assert_eq!(q("", Some(Reading), LibrarySort::Title), vec![hobbit, rose]);
        assert_eq!(q("", Some(Finished), LibrarySort::Title), vec![zen]);
        assert!(q("nothing matches", None, LibrarySort::Title).is_empty());
    }

    #[test]
    fn stats_count_statuses_pages_and_this_year() {
        let mut data = LibraryData::default();
        let now = 400 * 24 * HOUR;
        let year_start = now - 30 * 24 * HOUR;
        let old = add(&mut data, input("Last year", Some(100)), 0);
        data.set_reading_state(old, Finished, 0, 10 * HOUR).unwrap();
        let recent = add(
            &mut data,
            BookInput {
                rating: Some(4),
                ..input("This week", Some(300))
            },
            now - 48 * HOUR,
        );
        data.set_reading_state(recent, Reading, 40, now - 47 * HOUR)
            .unwrap();
        data.set_reading_state(recent, Reading, 300, now - 2 * HOUR)
            .unwrap();
        let _want = add(
            &mut data,
            BookInput {
                rating: Some(5),
                ..input("Someday", None)
            },
            now,
        );

        let stats = data.stats(year_start, now);
        assert_eq!(
            (
                stats.total,
                stats.want_to_read,
                stats.reading,
                stats.finished
            ),
            (3, 1, 0, 2)
        );
        assert_eq!(stats.finished_this_year, 1);
        assert_eq!(stats.pages_last_7_days, 300);
        assert_eq!(stats.average_rating, Some(4.5));
        assert_eq!(LibraryData::default().stats(0, 0).average_rating, None);
    }

    #[test]
    fn history_is_capped_newest_first_and_filterable() {
        let mut data = LibraryData::default();
        let a = add(&mut data, input("A", None), 0);
        let b = add(&mut data, input("B", None), 1);
        for i in 0..MAX_ACTIVITY as i64 {
            data.record(a, "A", 2 + i * SESSION_GAP_MS, ActivityKind::Started);
        }
        assert_eq!(data.activity.len(), MAX_ACTIVITY);
        assert!(
            data.history(Some(b), 10).is_empty(),
            "B's entry was the oldest"
        );
        assert_eq!(data.history(None, 3).len(), 3);
        assert!(data.history(None, 2)[0].at_ms > data.history(None, 2)[1].at_ms);
    }

    #[test]
    fn titles_sort_without_articles_and_accents() {
        assert_eq!(sort_title("The Hobbit"), "hobbit");
        assert_eq!(sort_title("An Atlas"), "atlas");
        assert_eq!(sort_title("Theory"), "theory");
        assert_eq!(fold("Élan Vital"), "elan vital");
    }

    #[test]
    fn positions_map_to_book_fractions_and_back() {
        let words = [0, 100, 300, 0, 600];
        let at = |chapter, fraction| ReadingPosition { chapter, fraction };
        assert_eq!(book_fraction(&words, at(0, 0.7)), 0.0);
        assert!((book_fraction(&words, at(2, 0.5)) - 0.25).abs() < 1e-9);
        assert_eq!(book_fraction(&words, at(4, 1.0)), 1.0);
        assert_eq!(book_fraction(&words, at(1, f64::NAN)), 0.0);
        assert_eq!(position_at(&words, 0.0), at(1, 0.0));
        let p = position_at(&words, 0.25);
        assert_eq!(p.chapter, 2);
        assert!((p.fraction - 0.5).abs() < 1e-9);
        assert_eq!(position_at(&words, 1.0), at(4, 1.0));
        assert_eq!(position_at(&[], 0.5), at(0, 0.0));
        // Picture books go by chapter.
        assert_eq!(book_fraction(&[0, 0], at(1, 0.0)), 0.5);
        // Pages: never the last one; by words without a page count.
        assert_eq!(page_at(1.0, Some(10), &words), 9);
        assert_eq!(page_at(0.5, Some(10), &words), 5);
        assert_eq!(page_at(1.0, None, &[550]), 2);
    }
}
