//! The typed reading plan: what a model proposes, what the person edits,
//! and the change set the app would apply. Rust owns all of it; the A2UI
//! surface only shows a draft of it.
//!
//! A model never writes interface. It fills [`Proposal`] (book ids,
//! priorities and dates, nothing else), and [`super::surface`] turns a valid
//! proposal into A2UI messages.

use std::collections::BTreeSet;

use serde::Deserialize;

/// The most books one plan may hold.
pub const MAX_BOOKS: usize = 12;
/// The longest a book may be planned for, in days.
pub const MAX_SPAN_DAYS: i64 = 366;

/// One of the app's own books. The titles and authors the confirmation shows
/// come from here, never from the surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub id: &'static str,
    pub title: &'static str,
    pub author: &'static str,
}

/// A stand-in for Pocket Library's books: A2UI-1 never touches a real app's
/// data (A2UI-5 does).
pub const LIBRARY: [Book; 4] = [
    Book {
        id: "piranesi",
        title: "Piranesi",
        author: "Susanna Clarke",
    },
    Book {
        id: "remains",
        title: "The Remains of the Day",
        author: "Kazuo Ishiguro",
    },
    Book {
        id: "left_hand",
        title: "The Left Hand of Darkness",
        author: "Ursula K. Le Guin",
    },
    Book {
        id: "middlemarch",
        title: "Middlemarch",
        author: "George Eliot",
    },
];

pub fn book(id: &str) -> Option<&'static Book> {
    LIBRARY.iter().find(|b| b.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High,
    Normal,
    Low,
}

impl Priority {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "high" => Some(Self::High),
            "normal" => Some(Self::Normal),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    pub fn value(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Normal => "Normal",
            Self::Low => "Low",
        }
    }
}

/// A calendar date, `yyyy-MM-dd` (what A2UI's date-only `DateTimeInput`
/// writes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Day {
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let digits = |range: std::ops::Range<usize>| -> Option<u32> {
            let part = &text[range];
            part.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| part.parse().ok())
                .flatten()
        };
        let year = digits(0..4)? as i32;
        let month = digits(5..7)?;
        let day = digits(8..10)?;
        if !(2000..=2100).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// "1 Dec 2026".
    pub fn label(self) -> String {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "{} {} {}",
            self.day,
            MONTHS[self.month as usize - 1],
            self.year
        )
    }

    /// Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    pub fn ordinal(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// One book in a plan, checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub book: &'static Book,
    pub priority: Priority,
    pub start: Day,
    pub finish: Day,
}

impl Entry {
    pub fn describe(&self) -> String {
        format!(
            "{} priority, {} to {}",
            self.priority.label(),
            self.start.label(),
            self.finish.label()
        )
    }
}

/// What a model fills in: ids, priorities and dates. Nothing it writes is
/// shown as interface or used as a title.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub books: Vec<ProposedBook>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedBook {
    pub book_id: String,
    pub priority: String,
    pub start: String,
    pub finish: String,
}

/// One book as the surface sends it back in the review event's context:
/// its data-model object. `title` and `author` are there because the surface
/// shows them; they are ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditedBook {
    pub id: String,
    #[serde(default)]
    pub title: Option<serde_json::Value>,
    #[serde(default)]
    pub author: Option<serde_json::Value>,
    /// A2UI's ChoicePicker binds to a list of strings.
    pub priority: Vec<String>,
    pub start: String,
    pub finish: String,
}

/// Why a plan was refused. The text names fields and ids, never what a
/// person typed.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanError(pub String);

/// Checks one book's values and returns the entry.
pub fn entry(id: &str, priority: &str, start: &str, finish: &str) -> Result<Entry, PlanError> {
    let book = book(id).ok_or_else(|| PlanError(format!("Unknown book id \"{id}\".")))?;
    let priority = Priority::parse(priority).ok_or_else(|| {
        PlanError(format!(
            "{}: priority must be high, normal or low.",
            book.title
        ))
    })?;
    let start = Day::parse(start)
        .ok_or_else(|| PlanError(format!("{}: the start isn't a date.", book.title)))?;
    let finish = Day::parse(finish)
        .ok_or_else(|| PlanError(format!("{}: the finish isn't a date.", book.title)))?;
    if finish < start {
        return Err(PlanError(format!(
            "{}: the finish is before the start.",
            book.title
        )));
    }
    if finish.ordinal() - start.ordinal() > MAX_SPAN_DAYS {
        return Err(PlanError(format!(
            "{}: a book can be planned for a year at most.",
            book.title
        )));
    }
    Ok(Entry {
        book,
        priority,
        start,
        finish,
    })
}

fn check_set(entries: &[Entry]) -> Result<(), PlanError> {
    if entries.is_empty() {
        return Err(PlanError("The plan has no books.".into()));
    }
    if entries.len() > MAX_BOOKS {
        return Err(PlanError(format!(
            "A plan holds {MAX_BOOKS} books at most."
        )));
    }
    let mut seen = BTreeSet::new();
    for e in entries {
        if !seen.insert(e.book.id) {
            return Err(PlanError(format!("{} is in the plan twice.", e.book.title)));
        }
    }
    Ok(())
}

impl Proposal {
    pub fn check(&self) -> Result<Vec<Entry>, PlanError> {
        let entries = self
            .books
            .iter()
            .map(|b| entry(&b.book_id, &b.priority, &b.start, &b.finish))
            .collect::<Result<Vec<_>, _>>()?;
        check_set(&entries)?;
        Ok(entries)
    }
}

/// Checks the person's edited plan against the books the current proposal
/// holds: they may change priorities and dates, not which books are in it.
pub fn check_edited(
    edited: &[EditedBook],
    expected: &BTreeSet<&'static str>,
) -> Result<Vec<Entry>, PlanError> {
    let entries = edited
        .iter()
        .map(|b| {
            let [priority] = b.priority.as_slice() else {
                return Err(PlanError(format!(
                    "{}: pick exactly one priority.",
                    book(&b.id).map(|b| b.title).unwrap_or("A book")
                )));
            };
            entry(&b.id, priority, &b.start, &b.finish)
        })
        .collect::<Result<Vec<_>, _>>()?;
    check_set(&entries)?;
    let ids: BTreeSet<&'static str> = entries.iter().map(|e| e.book.id).collect();
    if &ids != expected {
        return Err(PlanError(
            "The books in the plan don't match the proposal.".into(),
        ));
    }
    Ok(entries)
}
