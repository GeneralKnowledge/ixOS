//! Turns a checked reading plan into A2UI v1.0 messages. Only this file
//! writes interface, and it writes the same tree every time: the model's
//! proposal changes data, never components or actions.

use serde_json::{json, Value};

use super::plan::Entry;

/// The one surface the lab shows.
pub const SURFACE_ID: &str = "readingPlan";
/// The ixOS catalog the surface uses (registered in Swift by
/// `A2uiLabSurface`, `A2uiLabCatalog.id`).
pub const CATALOG_ID: &str = "org.forgemobile.devicelab:a2ui-reading-plan:1";
/// The only event a surface may send: asks Rust to propose a change set.
pub const REVIEW_EVENT: &str = "proposeReadingPlan";
/// The only component allowed to send it.
pub const REVIEW_COMPONENT: &str = "review";

/// The eight components of the ixOS catalog. Anything else is refused,
/// including Image, Video and AudioPlayer (their URLs could carry data out)
/// and Modal.
pub const COMPONENTS: [&str; 8] = [
    "Text",
    "Column",
    "Row",
    "Card",
    "Divider",
    "Button",
    "DateTimeInput",
    "ChoicePicker",
];

/// The context the review event must carry, exactly.
pub fn review_context() -> Value {
    json!({ "books": { "@path": "/books" } })
}

fn review_action() -> Value {
    json!({ "event": { "name": REVIEW_EVENT, "context": review_context() } })
}

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
