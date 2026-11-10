//! What a checklist is: items with titles, done or not, in the order they
//! were added. Plain Rust, no runtime and no FFI.

use serde::{Deserialize, Serialize};

use crate::ChecklistError;

/// The longest title, in characters.
pub const MAX_TITLE_CHARS: u32 = 200;

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub title: String,
    pub done: bool,
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
