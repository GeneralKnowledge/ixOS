//! Validating what a person typed, field by field, so the host can show
//! each problem under its own field (ForgeAppKit's `ForgeFieldErrors`,
//! ForgeUI's `ForgeField`). Rust owns the rules; Swift only shows them.
//!
//! ```
//! use forge_app::{Invalid, Validator};
//!
//! fn check(title: &str, pages: Option<u32>) -> Result<(), Invalid> {
//!     let mut v = Validator::new();
//!     v.required("title", title, "Enter a title.");
//!     v.max_chars("title", title, 200, "Keep the title under 200 characters.");
//!     v.check("total_pages", pages != Some(0), "A book has at least one page.");
//!     v.finish()
//! }
//!
//! let err = check(" ", Some(0)).unwrap_err();
//! assert_eq!(err.fields.len(), 2);
//! assert_eq!(err.fields[0].field, "title");
//! ```
//!
//! Field names are the app's own (use the input record's field names), and
//! messages are sentences for the person, written by the app.

use crate::contract::ErrorKind;

/// One problem with one field: `field` names it, `message` says what to do.
/// Part of the shared contract, so every app's errors carry the same shape
/// and ForgeKit ships its Swift.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

/// Input that was refused, with every problem found (at most one per
/// field, the first rule that failed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub fields: Vec<FieldError>,
}

impl Invalid {
    /// A single problem.
    pub fn field(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            fields: vec![FieldError {
                field: field.into(),
                message: message.into(),
            }],
        }
    }

    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Invalid
    }

    /// The message for `field`, if it has a problem.
    pub fn message(&self, field: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.field == field)
            .map(|f| f.message.as_str())
    }
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let messages: Vec<&str> = self.fields.iter().map(|e| e.message.as_str()).collect();
        f.write_str(&messages.join(" "))
    }
}

impl std::error::Error for Invalid {}

/// Collects problems while checking an input, then [`Validator::finish`]es.
/// Once a field has a problem, later rules for it are skipped, so the
/// person sees the first thing to fix.
#[derive(Debug, Default)]
pub struct Validator {
    fields: Vec<FieldError>,
}

impl Validator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `field` already has a problem.
    pub fn has(&self, field: &str) -> bool {
        self.fields.iter().any(|f| f.field == field)
    }

    /// Records `message` for `field` unless `ok`.
    pub fn check(&mut self, field: &str, ok: bool, message: &str) -> &mut Self {
        if !ok {
            self.add(field, message);
        }
        self
    }

    /// Records a problem with `field` (unless it already has one).
    pub fn add(&mut self, field: &str, message: &str) -> &mut Self {
        if !self.has(field) {
            self.fields.push(FieldError {
                field: field.to_string(),
                message: message.to_string(),
            });
        }
        self
    }

    /// `value` must have something other than spaces.
    pub fn required(&mut self, field: &str, value: &str, message: &str) -> &mut Self {
        self.check(field, !value.trim().is_empty(), message)
    }

    /// `value` (trimmed) must be at most `max` characters.
    pub fn max_chars(&mut self, field: &str, value: &str, max: usize, message: &str) -> &mut Self {
        self.check(field, value.trim().chars().count() <= max, message)
    }

    /// `value`, when given, must be within `range`.
    pub fn in_range<T: PartialOrd>(
        &mut self,
        field: &str,
        value: Option<T>,
        range: std::ops::RangeInclusive<T>,
        message: &str,
    ) -> &mut Self {
        self.check(field, value.is_none_or(|v| range.contains(&v)), message)
    }

    /// `Ok` when nothing was recorded, otherwise every problem in the order
    /// found.
    pub fn finish(self) -> Result<(), Invalid> {
        if self.fields.is_empty() {
            Ok(())
        } else {
            Err(Invalid {
                fields: self.fields,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_input_passes() {
        let mut v = Validator::new();
        v.required("title", "Dune", "Enter a title.")
            .max_chars("title", "Dune", 10, "Too long.")
            .in_range("rating", Some(3), 1..=5, "1 to 5 stars.")
            .in_range::<u8>("rating2", None, 1..=5, "unused");
        assert_eq!(v.finish(), Ok(()));
    }

    #[test]
    fn each_field_reports_its_first_problem_in_order() {
        let mut v = Validator::new();
        v.required("title", "   ", "Enter a title.")
            .max_chars("title", "   ", 0, "never reached")
            .in_range("rating", Some(9), 1..=5, "Give 1 to 5 stars.")
            .check("pages", true, "fine");
        let err = v.finish().unwrap_err();
        assert_eq!(
            err.fields,
            vec![
                FieldError {
                    field: "title".into(),
                    message: "Enter a title.".into()
                },
                FieldError {
                    field: "rating".into(),
                    message: "Give 1 to 5 stars.".into()
                },
            ]
        );
        assert_eq!(err.message("rating"), Some("Give 1 to 5 stars."));
        assert_eq!(err.message("pages"), None);
        assert_eq!(err.to_string(), "Enter a title. Give 1 to 5 stars.");
        assert_eq!(err.kind(), ErrorKind::Invalid);
    }

    #[test]
    fn max_chars_counts_characters_not_bytes() {
        let mut v = Validator::new();
        v.max_chars("title", " ééé ", 3, "Too long.");
        assert!(v.finish().is_ok());
        assert_eq!(Invalid::field("a", "b").fields.len(), 1);
    }
}
