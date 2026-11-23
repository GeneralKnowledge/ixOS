//! What a checklist is: items with titles, done or not, in the order they
//! were added, each with an optional due day. Plain Rust, no runtime and no
//! FFI.

use serde::{Deserialize, Serialize};

use crate::ChecklistError;

/// The longest title, in characters.
pub const MAX_TITLE_CHARS: u32 = 200;

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub title: String,
    pub done: bool,
    /// The local day it is due, as `YYYY-MM-DD`, or none. A day, not a
    /// moment: the phone decides which day is today (see [`agenda`]).
    /// Added after version 1 shipped; a file without it opens with none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
}

/// The items still to do that have a due day, sorted by that day (then in
/// the order they were added): what is late, what is due today, what is
/// coming.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Eq)]
pub struct Agenda {
    pub overdue: Vec<Item>,
    pub today: Vec<Item>,
    pub upcoming: Vec<Item>,
}

#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub total: u32,
    pub done: u32,
}

impl Summary {
    pub(crate) fn of(items: &[Item]) -> Self {
        Self {
            total: items.len() as u32,
            done: items.iter().filter(|item| item.done).count() as u32,
        }
    }
}

/// The saved form (version 1 of checklist.json).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Saved {
    next_id: u64,
    items: Vec<Item>,
}

/// Saved as [`Saved`] (`checklist.json`, through `forge_app::Document`).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
#[serde(from = "Saved", into = "Saved")]
pub(crate) struct Checklist {
    next_id: u64,
    items: Vec<Item>,
}

impl From<Saved> for Checklist {
    fn from(saved: Saved) -> Self {
        Checklist::from_saved(saved)
    }
}

impl From<Checklist> for Saved {
    fn from(list: Checklist) -> Self {
        list.to_saved()
    }
}

impl Checklist {
    pub fn from_saved(saved: Saved) -> Self {
        // Never hand out an id twice, even if the file was edited.
        let highest = saved.items.iter().map(|item| item.id).max().unwrap_or(0);
        Self {
            next_id: saved.next_id.max(highest),
            items: saved.items,
        }
    }

    pub fn to_saved(&self) -> Saved {
        Saved {
            next_id: self.next_id,
            items: self.items.clone(),
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn add(&mut self, title: &str) -> Result<u64, ChecklistError> {
        let title = valid_title(title)?;
        self.next_id += 1;
        self.items.push(Item {
            id: self.next_id,
            title,
            done: false,
            due: None,
        });
        Ok(self.next_id)
    }

    pub fn toggle(&mut self, id: u64) -> Result<(), ChecklistError> {
        let item = self.find(id)?;
        item.done = !item.done;
        Ok(())
    }

    pub fn rename(&mut self, id: u64, title: &str) -> Result<(), ChecklistError> {
        let title = valid_title(title)?;
        self.find(id)?.title = title;
        Ok(())
    }

    /// Sets the day an item is due (`YYYY-MM-DD`), or clears it.
    pub fn set_due(&mut self, id: u64, due: Option<&str>) -> Result<(), ChecklistError> {
        let due = due.map(valid_day).transpose()?;
        self.find(id)?.due = due;
        Ok(())
    }

    pub fn remove(&mut self, id: u64) -> Result<(), ChecklistError> {
        let index = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(ChecklistError::NotFound { id })?;
        self.items.remove(index);
        Ok(())
    }

    pub fn clear_done(&mut self) -> u32 {
        let before = self.items.len();
        self.items.retain(|item| !item.done);
        (before - self.items.len()) as u32
    }

    fn find(&mut self, id: u64) -> Result<&mut Item, ChecklistError> {
        self.items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(ChecklistError::NotFound { id })
    }
}

fn valid_title(title: &str) -> Result<String, ChecklistError> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(ChecklistError::EmptyTitle);
    }
    if title.chars().count() > MAX_TITLE_CHARS as usize {
        return Err(ChecklistError::TitleTooLong {
            max: MAX_TITLE_CHARS,
        });
    }
    Ok(title)
}

/// A due day as saved: `YYYY-MM-DD`, a real day of the Gregorian calendar,
/// years 1 to 9999 (so the text sorts in day order).
pub(crate) fn valid_day(text: &str) -> Result<String, ChecklistError> {
    let text = text.trim();
    let invalid = || ChecklistError::InvalidDay {
        day: text.to_string(),
    };
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(invalid());
    }
    let number = |range: std::ops::Range<usize>| -> Result<u32, ChecklistError> {
        let part = &text[range];
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        part.parse().map_err(|_| invalid())
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(invalid()),
    };
    if year == 0 || day == 0 || day > days_in_month {
        return Err(invalid());
    }
    Ok(text.to_string())
}

/// The agenda on `today` (`YYYY-MM-DD`, the phone's local day): items not
/// yet done that have a due day. Done items and items with no day are left
/// out.
pub fn agenda(items: &[Item], today: &str) -> Agenda {
    let mut due: Vec<&Item> = items
        .iter()
        .filter(|item| !item.done && item.due.is_some())
        .collect();
    // Stable, so items due the same day keep the order they were added.
    due.sort_by(|a, b| a.due.cmp(&b.due));
    let mut agenda = Agenda::default();
    for item in due {
        let day = item.due.as_deref().unwrap_or_default();
        let list = match day.cmp(today) {
            std::cmp::Ordering::Less => &mut agenda.overdue,
            std::cmp::Ordering::Equal => &mut agenda.today,
            std::cmp::Ordering::Greater => &mut agenda.upcoming,
        };
        list.push(item.clone());
    }
    agenda
}

/// The items whose title contains every word of `query`, ignoring case, or
/// `None` if `stopped` turned true first. An empty query matches all.
pub fn search(items: &[Item], query: &str, stopped: impl Fn() -> bool) -> Option<Vec<Item>> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut found = Vec::new();
    for item in items {
        if stopped() {
            return None;
        }
        let title = item.title.to_lowercase();
        if words.iter().all(|word| title.contains(word.as_str())) {
            found.push(item.clone());
        }
    }
    if stopped() {
        return None;
    }
    Some(found)
}

/// The checklist as plain text, one item per line.
pub fn as_text(items: &[Item]) -> String {
    let mut text = String::new();
    for item in items {
        text.push_str(if item.done { "[x] " } else { "[ ] " });
        text.push_str(&item.title);
        if let Some(due) = &item.due {
            text.push_str(" (due ");
            text.push_str(due);
            text.push(')');
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_trimmed_and_checked() {
        let mut list = Checklist::default();
        assert_eq!(list.add("  milk \n and  eggs "), Ok(1));
        assert_eq!(list.items()[0].title, "milk and eggs");
        assert_eq!(list.add("   "), Err(ChecklistError::EmptyTitle));
        let long = "x".repeat(MAX_TITLE_CHARS as usize + 1);
        assert_eq!(
            list.add(&long),
            Err(ChecklistError::TitleTooLong {
                max: MAX_TITLE_CHARS
            })
        );
    }

    #[test]
    fn ids_are_never_reused() {
        let mut list = Checklist::default();
        list.add("a").unwrap();
        let b = list.add("b").unwrap();
        list.remove(b).unwrap();
        assert_eq!(list.add("c"), Ok(3));
        // A file whose next_id is behind its items (edited by hand).
        let saved = Saved {
            next_id: 0,
            items: list.items().to_vec(),
        };
        let mut reopened = Checklist::from_saved(saved);
        assert_eq!(reopened.add("d"), Ok(4));
    }

    #[test]
    fn toggle_rename_remove_and_clear() {
        let mut list = Checklist::default();
        let a = list.add("a").unwrap();
        let b = list.add("b").unwrap();
        list.toggle(a).unwrap();
        list.rename(b, "bee").unwrap();
        assert_eq!(list.toggle(99), Err(ChecklistError::NotFound { id: 99 }));
        assert_eq!(Summary::of(list.items()), Summary { total: 2, done: 1 });
        assert_eq!(list.clear_done(), 1);
        assert_eq!(list.items()[0].title, "bee");
        assert_eq!(as_text(list.items()), "[ ] bee\n");
    }

    #[test]
    fn due_days_are_checked_set_and_cleared() {
        let mut list = Checklist::default();
        let a = list.add("a").unwrap();
        assert_eq!(list.set_due(a, Some(" 2028-02-29 ")), Ok(()));
        assert_eq!(list.items()[0].due.as_deref(), Some("2028-02-29"));
        for bad in [
            "2027-02-29",
            "2026-13-01",
            "2026-04-31",
            "2026-1-01",
            "0000-01-01",
            "2026/01/01",
            "+026-01-01",
            "tomorrow",
            "",
        ] {
            assert_eq!(
                list.set_due(a, Some(bad)),
                Err(ChecklistError::InvalidDay {
                    day: bad.trim().into()
                }),
                "{bad}"
            );
        }
        // A refused day leaves the old one.
        assert_eq!(list.items()[0].due.as_deref(), Some("2028-02-29"));
        assert_eq!(
            list.set_due(99, Some("2026-01-01")),
            Err(ChecklistError::NotFound { id: 99 })
        );
        list.set_due(a, None).unwrap();
        assert_eq!(list.items()[0].due, None);
    }

    #[test]
    fn the_agenda_splits_by_today_and_leaves_out_done_and_undated() {
        let mut list = Checklist::default();
        let late = list.add("late").unwrap();
        let later = list.add("later").unwrap();
        let today_b = list.add("today b").unwrap();
        let soon = list.add("soon").unwrap();
        let today_a = list.add("today a").unwrap();
        list.add("no day").unwrap();
        let done = list.add("done").unwrap();
        list.set_due(late, Some("2026-11-20")).unwrap();
        list.set_due(later, Some("2027-01-05")).unwrap();
        list.set_due(today_b, Some("2026-11-22")).unwrap();
        list.set_due(soon, Some("2026-11-23")).unwrap();
        list.set_due(today_a, Some("2026-11-22")).unwrap();
        list.set_due(done, Some("2026-11-01")).unwrap();
        list.toggle(done).unwrap();
        let agenda = agenda(list.items(), "2026-11-22");
        let titles = |items: &[Item]| items.iter().map(|i| i.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles(&agenda.overdue), ["late"]);
        assert_eq!(titles(&agenda.today), ["today b", "today a"]);
        assert_eq!(titles(&agenda.upcoming), ["soon", "later"]);
        assert_eq!(as_text(&list.items()[..1]), "[ ] late (due 2026-11-20)\n");
    }

    #[test]
    fn a_saved_item_without_a_due_day_opens_and_saves_without_one() {
        let saved: Item = serde_json::from_str(r#"{"id":1,"title":"a","done":false}"#).unwrap();
        assert_eq!(saved.due, None);
        assert_eq!(
            serde_json::to_string(&saved).unwrap(),
            r#"{"id":1,"title":"a","done":false}"#
        );
    }

    #[test]
    fn search_matches_every_word_and_stops() {
        let mut list = Checklist::default();
        list.add("Buy oat milk").unwrap();
        list.add("Call the vet").unwrap();
        let found = search(list.items(), "MILK oat", || false).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(search(list.items(), "", || false).unwrap().len(), 2);
        assert_eq!(search(list.items(), "milk", || true), None);
    }
}
