//! The library's rules, as plain functions over [`LibraryData`]. No I/O and no
//! clock: callers pass `now`, so every rule is tested directly.

use serde::{Deserialize, Serialize};

use crate::error::LibraryError;
use crate::model::{
    ActivityEntry, ActivityKind, Book, BookInput, BookSource, LibraryQuery, LibraryStats,
    ReadingStatus, Settings, SortOrder,
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
    pub settings: Settings,
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

fn too_long(field: &str, text: &str, max: usize) -> Result<(), LibraryError> {
    if text.chars().count() > max {
        Err(LibraryError::invalid(format!(
            "{field} is too long (at most {max} characters)"
        )))
    } else {
        Ok(())
    }
}

pub fn validate(input: BookInput) -> Result<ValidInput, LibraryError> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(LibraryError::invalid("a book needs a title"));
    }
    too_long("the title", &title, MAX_TITLE_CHARS)?;
    let author = input.author.trim().to_string();
    too_long("the author", &author, MAX_AUTHOR_CHARS)?;
    let description = input.description.trim().to_string();
    too_long("the description", &description, MAX_DESCRIPTION_CHARS)?;
    let notes = input.notes.trim_end().to_string();
    too_long("the notes", &notes, MAX_NOTES_CHARS)?;
    let total_pages = match input.total_pages {
        Some(0) | None => None,
        Some(pages) if pages > MAX_PAGES => {
            return Err(LibraryError::invalid(format!(
                "the page count must be at most {MAX_PAGES}"
            )))
        }
        Some(pages) => Some(pages),
    };
    if let Some(total) = total_pages {
        if input.current_page > total {
            return Err(LibraryError::invalid(format!(
                "the current page ({}) is past the last page ({total})",
                input.current_page
            )));
        }
    } else if input.current_page > MAX_PAGES {
        return Err(LibraryError::invalid("the current page is too large"));
    }
    if let Some(rating) = input.rating {
        if !(1..=5).contains(&rating) {
            return Err(LibraryError::invalid("ratings are 1 to 5 stars"));
        }
    }
    let cover_url = match input.cover_url.map(|u| u.trim().to_string()) {
        Some(url) if url.is_empty() => None,
        Some(url) if !url.starts_with("https://") || url.len() <= "https://".len() => {
            return Err(LibraryError::invalid(
                "cover links must start with https://",
            ))
        }
        other => other,
    };
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
            SortOrder::RecentlyAdded => {
                books.sort_by(|a, b| b.added_at_ms.cmp(&a.added_at_ms).then(b.id.cmp(&a.id)))
            }
            SortOrder::Title => books.sort_by_cached_key(|b| (sort_title(&b.title), b.id)),
            SortOrder::Author => books.sort_by_cached_key(|b| {
                (
                    b.author.is_empty(),
                    fold(&b.author),
                    sort_title(&b.title),
                    b.id,
                )
            }),
            SortOrder::Progress => books.sort_by(|a, b| {
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

        let bad = |i: BookInput| matches!(validate(i), Err(LibraryError::InvalidInput { .. }));
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
            q("", None, SortOrder::RecentlyAdded),
            vec![zen, atlas, rose, hobbit]
        );
        assert_eq!(
            q("", None, SortOrder::Title),
            vec![atlas, hobbit, rose, zen]
        );
        assert_eq!(
            q("", None, SortOrder::Author),
            vec![hobbit, atlas, rose, zen],
            "J before T before U; no author last"
        );
        assert_eq!(
            q("", None, SortOrder::Progress),
            vec![zen, hobbit, rose, atlas]
        );
        assert_eq!(q("tolkien", None, SortOrder::Title), vec![atlas, hobbit]);
        assert_eq!(q("TOLKIEN hob", None, SortOrder::Title), vec![hobbit]);
        assert_eq!(q("eco", None, SortOrder::Title), vec![rose], "accents fold");
        assert_eq!(q("", Some(Reading), SortOrder::Title), vec![hobbit, rose]);
        assert_eq!(q("", Some(Finished), SortOrder::Title), vec![zen]);
        assert!(q("nothing matches", None, SortOrder::Title).is_empty());
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
}
