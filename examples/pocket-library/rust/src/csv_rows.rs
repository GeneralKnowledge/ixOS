//! The spreadsheet form of a book, for CSV export and import
//! (`forge_storage::csv_file`). Column names match what people and other
//! reading apps use ("Title", "Author", "Status", "Pages Read", ...);
//! import matches them ignoring case, spaces and underscores, and only
//! Title is required.

use serde::{Deserialize, Serialize};

use crate::{Book, BookInput, LibraryError, ReadingStatus};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
pub struct BookRow {
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "Author")]
    pub author: String,
    /// "Want to Read", "Reading" or "Finished".
    #[serde(rename = "Status")]
    pub status: String,
    #[serde(rename = "Pages Read")]
    pub pages_read: Option<u32>,
    #[serde(rename = "Total Pages")]
    pub total_pages: Option<u32>,
    #[serde(rename = "Rating")]
    pub rating: Option<u8>,
    #[serde(rename = "Notes")]
    pub notes: String,
}

impl From<&crate::domain::StoredBook> for BookRow {
    fn from(book: &crate::domain::StoredBook) -> Self {
        let book: Book = book.to_book(std::path::Path::new(""));
        Self {
            title: book.title,
            author: book.author,
            status: match book.status {
                ReadingStatus::WantToRead => "Want to Read",
                ReadingStatus::Reading => "Reading",
                ReadingStatus::Finished => "Finished",
            }
            .into(),
            pages_read: Some(book.current_page),
            total_pages: book.total_pages,
            rating: book.rating,
            notes: book.notes,
        }
    }
}

impl BookRow {
    pub fn into_input(self) -> Result<BookInput, LibraryError> {
        let status = match self.status.to_lowercase().replace(['-', '_'], " ").trim() {
            "" | "want to read" | "to read" | "to-read" => ReadingStatus::WantToRead,
            "reading" | "currently reading" => ReadingStatus::Reading,
            "finished" | "read" => ReadingStatus::Finished,
            other => {
                return Err(LibraryError::InvalidInput {
                    reason: format!("unknown status \"{other}\""),
                })
            }
        };
        Ok(BookInput {
            title: self.title,
            author: self.author,
            description: String::new(),
            status,
            current_page: self.pages_read.unwrap_or(0),
            total_pages: self.total_pages,
            rating: self.rating.filter(|r| *r > 0),
            notes: self.notes,
            cover_url: None,
        })
    }
}
