//! Turns a checked reading plan into A2UI v1.0 messages. Only this file
//! writes interface, and it writes the same tree every time: the model's
//! proposal changes data, never components or actions.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::gate::{self, Accepted, EventRule, GateState, Refusal, Registration};
use super::plan::{self, EditedBook, Entry};

/// The one surface the lab shows.
pub const SURFACE_ID: &str = "readingPlan";
/// The ixOS catalog the surface uses: the lab's one catalog.
pub use super::gate::CATALOG_ID;
/// The only event a surface may send: asks Rust to propose a change set.
pub const REVIEW_EVENT: &str = "proposeReadingPlan";
/// The only component allowed to send it.
pub const REVIEW_COMPONENT: &str = "review";

/// The context the review event must carry, exactly.
pub fn review_context() -> Value {
    json!({ "books": { "@path": "/books" } })
}

fn review_action() -> Value {
    json!({ "event": { "name": REVIEW_EVENT, "context": review_context() } })
}

/// The components an update may redraw once the surface exists. Every
/// other component, the review button and its label included, stays as
/// created.
pub const UPDATABLE: &[&str] = &["intro"];

const INTRO: &str = "A proposed order for your next four books. Change any priority or date, then review the changes.";
/// The text the agent's update puts in place of [`INTRO`].
pub const UPDATED_INTRO: &str =
    "Updated proposal: Middlemarch moves to February, and Piranesi gets an extra day.";

fn components(intro: &str) -> Value {
    json!([
        { "id": "root", "component": "Column",
          "children": ["intro", "books", "divider", "review"] },
        { "id": "intro", "component": "Text", "text": intro, "variant": "body" },
        { "id": "books", "component": "Column",
          "children": { "componentId": "book", "path": "/books" } },
        { "id": "book", "component": "Card", "child": "bookColumn",
          "@accessibility": { "label": { "@path": "title" } } },
        { "id": "bookColumn", "component": "Column",
          "children": ["bookTitle", "bookAuthor", "bookPriority", "bookDates"] },
        { "id": "bookTitle", "component": "Text", "text": { "@path": "title" }, "variant": "body" },
        { "id": "bookAuthor", "component": "Text", "text": { "@path": "author" }, "variant": "caption" },
        { "id": "bookPriority", "component": "ChoicePicker", "label": "Priority",
          "variant": "mutuallyExclusive", "displayStyle": "chips",
          "options": [
              { "label": "High", "value": "high" },
              { "label": "Normal", "value": "normal" },
              { "label": "Low", "value": "low" }
          ],
          "value": { "@path": "priority" } },
        { "id": "bookDates", "component": "Row", "children": ["bookStart", "bookFinish"] },
        { "id": "bookStart", "component": "DateTimeInput", "label": "Start",
          "enableDate": true, "value": { "@path": "start" } },
        { "id": "bookFinish", "component": "DateTimeInput", "label": "Finish",
          "enableDate": true, "value": { "@path": "finish" } },
        { "id": "divider", "component": "Divider" },
        { "id": "review", "component": "Button", "child": "reviewLabel", "variant": "primary",
          "action": review_action() },
        { "id": "reviewLabel", "component": "Text", "text": "Review changes" }
    ])
}

/// The surface's data model for `entries`: titles and authors from the
/// app's own books.
pub fn books_value(entries: &[Entry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.book.id,
                    "title": e.book.title,
                    "author": e.book.author,
                    "priority": [e.priority.value()],
                    "start": e.start.iso(),
                    "finish": e.finish.iso(),
                })
            })
            .collect(),
    )
}

/// The first message: the whole surface, its tree and its data, at once.
pub fn create(entries: &[Entry]) -> Value {
    json!([{
        "version": "1.0",
        "createSurface": {
            "surfaceId": SURFACE_ID,
            "catalogId": CATALOG_ID,
            "components": components(INTRO),
            "dataModel": { "books": books_value(entries) }
        }
    }])
}

/// The agent's revision: one changed text and new data. The review button
/// and its action are untouched.
pub fn update(entries: &[Entry]) -> Value {
    json!([
        {
            "version": "1.0",
            "updateComponents": {
                "surfaceId": SURFACE_ID,
                "components": [
                    { "id": "intro", "component": "Text", "text": UPDATED_INTRO, "variant": "body" }
                ]
            }
        },
        {
            "version": "1.0",
            "updateDataModel": {
                "surfaceId": SURFACE_ID,
                "path": "/books",
                "value": books_value(entries)
            }
        }
    ])
}

/// The reading plan's registration with the gate: one surface, one review
/// event from the review button, only the intro redrawn by updates, data
/// that holds only known books with their own titles and authors.
pub struct ReadingPlan;

impl Registration for ReadingPlan {
    type Data = BTreeSet<&'static str>;

    fn surface_id(&self) -> &'static str {
        SURFACE_ID
    }

    fn events(&self) -> Vec<EventRule> {
        vec![EventRule {
            name: REVIEW_EVENT,
            component: REVIEW_COMPONENT,
            context: review_context(),
        }]
    }

    fn updatable(&self) -> &'static [&'static str] {
        UPDATABLE
    }

    fn check_data(&self, path: Option<&str>, value: &Value) -> Result<Self::Data, Refusal> {
        data_model(path, value)
    }
}

/// Checks `payload` for the reading-plan surface.
pub fn check(
    payload: &str,
    state: &mut GateState,
) -> Result<Accepted<BTreeSet<&'static str>>, Refusal> {
    gate::check(payload, &ReadingPlan, state)
}

/// Checks data for the surface: `{"books": [...]}` at the root, or the
/// list at `/books`. Returns the book ids it holds.
fn data_model(path: Option<&str>, value: &Value) -> Result<BTreeSet<&'static str>, Refusal> {
    let books = match path {
        None | Some("/") => {
            let object = value
                .as_object()
                .filter(|o| o.len() == 1)
                .ok_or_else(|| gate::refuse("data", "The data model holds only the books."))?;
            object
                .get("books")
                .ok_or_else(|| gate::refuse("data", "The data model holds only the books."))?
        }
        Some("/books") => value,
        Some(_) => return Err(gate::refuse("data", "Updates may only replace the books.")),
    };
    let edited: Vec<EditedBook> = serde_json::from_value(books.clone())
        .map_err(|_| gate::refuse("data", "The books aren't in the expected form."))?;
    let entries = edited
        .iter()
        .map(|b| match b.priority.as_slice() {
            [priority] => plan::entry(&b.id, priority, &b.start, &b.finish),
            _ => Err(plan::PlanError("Each book needs one priority.".into())),
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| gate::refuse("data", e.0))?;
    // The surface shows these, so they must be the book's own.
    for (b, e) in edited.iter().zip(&entries) {
        let title = b.title.as_ref().and_then(Value::as_str);
        let author = b.author.as_ref().and_then(Value::as_str);
        if title != Some(e.book.title) || author != Some(e.book.author) {
            return Err(gate::refuse(
                "data",
                "A book's title or author isn't the app's.",
            ));
        }
    }
    let ids: BTreeSet<&'static str> = entries.iter().map(|e| e.book.id).collect();
    if ids.len() != entries.len() || ids.is_empty() || ids.len() > plan::MAX_BOOKS {
        return Err(gate::refuse(
            "data",
            "The books must be distinct, one to twelve of them.",
        ));
    }
    Ok(ids)
}
