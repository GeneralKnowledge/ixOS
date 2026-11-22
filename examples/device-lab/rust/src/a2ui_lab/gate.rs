//! The gate every agent-to-renderer message passes before Swift sees it.
//!
//! A2UI's Swift renderer validates messages against the catalog too (with
//! `ValidationConfig.strict`). This gate is the app's own check, in Rust
//! and tested on Linux, of the rules the catalog can't express:
//!
//! - only the eight catalog components, on the lab's one surface and catalog;
//! - no function calls of any kind (`@call`, `functionCall`,
//!   `callRendererFunction`): the agent can draw, not act;
//! - an action may only be the registered review event, with exactly its
//!   registered context, and no `userMessage` (a summary the surface writes
//!   is never shown as the change set);
//! - an update can't add, remove or change the action of a component that
//!   already exists, so a later message can't change what a button does;
//! - after the surface exists, an update may redraw only the components in
//!   `UPDATABLE` (the intro text), so it can't relabel the review button;
//! - data updates hold only known books, checked like a proposal, with each
//!   book's own title and author.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::plan::{self, EditedBook};
use super::surface::{
    review_context, CATALOG_ID, COMPONENTS, REVIEW_COMPONENT, REVIEW_EVENT, SURFACE_ID, UPDATABLE,
};

/// The largest payload accepted, in bytes.
pub const MAX_BYTES: usize = 64 * 1024;
/// The most components one message may define.
pub const MAX_COMPONENTS: usize = 64;

/// Why a payload was refused: a stable code for logs and tests, and a
/// sentence for the screen. Neither quotes the person's data.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub code: &'static str,
    pub reason: String,
}

fn refuse(code: &'static str, reason: impl Into<String>) -> Refusal {
    Refusal {
        code,
        reason: reason.into(),
    }
}

/// What the gate remembers about the surface Swift is showing.
#[derive(Debug, Default, Clone)]
pub struct GateState {
    pub created: bool,
    /// Each component id the surface has, and its action (canonical JSON),
    /// or `None` if it has none.
    pub actions: BTreeMap<String, Option<String>>,
}

/// What an accepted payload changes.
#[derive(Debug, Default)]
pub struct Accepted {
    /// The messages, unchanged, for Swift.
    pub messages: Vec<Value>,
    /// The books the surface now holds, if the payload set them.
    pub books: Option<BTreeSet<&'static str>>,
}

/// Checks `payload` (a message or an array of them) against `state`. On
/// success `state` is updated; on refusal it is untouched.
pub fn check(payload: &str, state: &mut GateState) -> Result<Accepted, Refusal> {
    if payload.len() > MAX_BYTES {
        return Err(refuse("too_large", "The update is too large."));
    }
    let value: Value = serde_json::from_str(payload)
        .map_err(|_| refuse("malformed", "The update isn't valid JSON."))?;
    let messages = match value {
        Value::Array(items) => items,
        single @ Value::Object(_) => vec![single],
        _ => return Err(refuse("malformed", "The update isn't an A2UI message.")),
    };
    if messages.is_empty() {
        return Err(refuse("malformed", "The update holds no messages."));
    }
    let mut next = state.clone();
    let mut books = None;
    for message in &messages {
        if let Some(ids) = check_message(message, &mut next)? {
            books = Some(ids);
        }
    }
    *state = next;
    Ok(Accepted { messages, books })
}

fn check_message(
    message: &Value,
    state: &mut GateState,
) -> Result<Option<BTreeSet<&'static str>>, Refusal> {
    let object = message
        .as_object()
        .ok_or_else(|| refuse("malformed", "A message isn't an object."))?;
    if object.get("version").and_then(Value::as_str) != Some("1.0") {
        return Err(refuse("version", "Only A2UI 1.0 messages are accepted."));
    }
    let kinds: Vec<&String> = object.keys().filter(|k| *k != "version").collect();
    let [kind] = kinds.as_slice() else {
        return Err(refuse(
            "malformed",
            "A message must hold exactly one operation.",
        ));
    };
    let body = object[kind.as_str()]
        .as_object()
        .ok_or_else(|| refuse("malformed", "A message's operation isn't an object."))?;
    no_functions(&object[kind.as_str()])?;
    match kind.as_str() {
        "createSurface" => {
            if state.created {
                return Err(refuse("lifecycle", "The surface already exists."));
            }
            surface_id(body)?;
            only_keys(body, &["surfaceId", "catalogId", "components", "dataModel"])?;
            if body.get("catalogId").and_then(Value::as_str) != Some(CATALOG_ID) {
                return Err(refuse("catalog", "The surface must use the lab's catalog."));
            }
            state.created = true;
            if let Some(components) = body.get("components") {
                check_components(components, state)?;
            }
            match body.get("dataModel") {
                Some(model) => data_model(None, model).map(Some),
                None => Ok(None),
            }
        }
        "updateComponents" => {
            created(state)?;
            surface_id(body)?;
            only_keys(body, &["surfaceId", "components"])?;
            let components = body
                .get("components")
                .ok_or_else(|| refuse("malformed", "updateComponents has no components."))?;
            check_components(components, state)?;
            let locked = components.as_array().is_some_and(|list| {
                list.iter().any(|c| {
                    !c.get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| UPDATABLE.contains(&id))
                })
            });
            if locked {
                return Err(refuse(
                    "component_locked",
                    "An update may only change the introduction, not the plan's controls.",
                ));
            }
            Ok(None)
        }
        "updateDataModel" => {
            created(state)?;
            surface_id(body)?;
            only_keys(body, &["surfaceId", "path", "value"])?;
            let path = body.get("path").map(|p| {
                p.as_str()
                    .ok_or_else(|| refuse("malformed", "A data path isn't a string."))
            });
            let path = path.transpose()?;
            let value = body
                .get("value")
                .ok_or_else(|| refuse("malformed", "updateDataModel has no value."))?;
            data_model(path, value).map(Some)
        }
        "callRendererFunction" => Err(refuse(
            "function_call",
            "The agent can't call app functions in this lab.",
        )),
        "deleteSurface" => Err(refuse("lifecycle", "The agent can't remove the surface.")),
        _ => Err(refuse("unknown_message", "Unknown A2UI message type.")),
    }
}

fn created(state: &GateState) -> Result<(), Refusal> {
    if state.created {
        Ok(())
    } else {
        Err(refuse("lifecycle", "The surface doesn't exist yet."))
    }
}

fn surface_id(body: &Map<String, Value>) -> Result<(), Refusal> {
    if body.get("surfaceId").and_then(Value::as_str) == Some(SURFACE_ID) {
        Ok(())
    } else {
        Err(refuse(
            "surface",
            "Messages may only address the lab's surface.",
        ))
    }
}

fn only_keys(body: &Map<String, Value>, allowed: &[&str]) -> Result<(), Refusal> {
    match body.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(_) => Err(refuse(
            "malformed",
            "A message holds a field the lab doesn't accept.",
        )),
        None => Ok(()),
    }
}

/// Refuses any function call anywhere in `value`.
fn no_functions(value: &Value) -> Result<(), Refusal> {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                if key == "@call" || key == "functionCall" {
                    return Err(refuse(
                        "function_call",
                        "Function calls aren't allowed in this lab's surfaces.",
                    ));
                }
                if key == "@catalogId" {
                    return Err(refuse("catalog", "A component can't name another catalog."));
                }
                no_functions(inner)?;
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(no_functions),
        _ => Ok(()),
    }
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && id.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn check_components(components: &Value, state: &mut GateState) -> Result<(), Refusal> {
    let list = components
        .as_array()
        .ok_or_else(|| refuse("malformed", "components isn't a list."))?;
    if list.len() > MAX_COMPONENTS {
        return Err(refuse("too_large", "Too many components in one message."));
    }
    let mut seen = BTreeSet::new();
    for component in list {
        let object = component
            .as_object()
            .ok_or_else(|| refuse("malformed", "A component isn't an object."))?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_id(id))
            .ok_or_else(|| refuse("malformed", "A component has no valid id."))?;
        if !seen.insert(id) {
            return Err(refuse("malformed", "A component id appears twice."));
        }
        let kind = object
            .get("component")
            .and_then(Value::as_str)
            .ok_or_else(|| refuse("malformed", "A component has no type."))?;
        if !COMPONENTS.contains(&kind) {
            return Err(refuse(
                "unknown_component",
                format!("\"{kind}\" isn't in this lab's catalog."),
            ));
        }
        let action = match object.get("action") {
            Some(action) => Some(check_action(id, action)?),
            None => None,
        };
        if let Some(existing) = state.actions.get(id) {
            if *existing != action {
                return Err(refuse(
                    "action_changed",
                    "An update tried to change what an existing control does.",
                ));
            }
        }
        state.actions.insert(id.to_string(), action);
    }
    Ok(())
}

/// The registry: one event, from one component, with one context.
fn check_action(id: &str, action: &Value) -> Result<String, Refusal> {
    let event = action
        .as_object()
        .filter(|a| a.len() == 1)
        .and_then(|a| a.get("event"))
        .and_then(Value::as_object)
        .ok_or_else(|| refuse("unregistered_event", "Only the review event is registered."))?;
    if event.get("name").and_then(Value::as_str) != Some(REVIEW_EVENT) {
        return Err(refuse(
            "unregistered_event",
            "The surface asked for an event the app hasn't registered.",
        ));
    }
    if event.contains_key("userMessage") || event.len() != 2 {
        return Err(refuse(
            "unregistered_event",
            "The review event carries only its registered context.",
        ));
    }
    if event.get("context") != Some(&review_context()) {
        return Err(refuse(
            "action_changed",
            "The review event's context isn't the registered one.",
        ));
    }
    if id != REVIEW_COMPONENT {
        return Err(refuse(
            "unregistered_event",
            "Only the review button may send the review event.",
        ));
    }
    Ok(action.to_string())
}

/// Checks data for the surface: `{"books": [...]}` at the root, or the
/// list at `/books`. Returns the book ids it holds.
fn data_model(path: Option<&str>, value: &Value) -> Result<BTreeSet<&'static str>, Refusal> {
    let books = match path {
        None | Some("/") => {
            let object = value
                .as_object()
                .filter(|o| o.len() == 1)
                .ok_or_else(|| refuse("data", "The data model holds only the books."))?;
            object
                .get("books")
                .ok_or_else(|| refuse("data", "The data model holds only the books."))?
        }
        Some("/books") => value,
        Some(_) => return Err(refuse("data", "Updates may only replace the books.")),
    };
    let edited: Vec<EditedBook> = serde_json::from_value(books.clone())
        .map_err(|_| refuse("data", "The books aren't in the expected form."))?;
    let entries = edited
        .iter()
        .map(|b| match b.priority.as_slice() {
            [priority] => plan::entry(&b.id, priority, &b.start, &b.finish),
            _ => Err(plan::PlanError("Each book needs one priority.".into())),
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| refuse("data", e.0))?;
    // The surface shows these, so they must be the book's own.
    for (b, e) in edited.iter().zip(&entries) {
        let title = b.title.as_ref().and_then(Value::as_str);
        let author = b.author.as_ref().and_then(Value::as_str);
        if title != Some(e.book.title) || author != Some(e.book.author) {
            return Err(refuse("data", "A book's title or author isn't the app's."));
        }
    }
    let ids: BTreeSet<&'static str> = entries.iter().map(|e| e.book.id).collect();
    if ids.len() != entries.len() || ids.is_empty() || ids.len() > plan::MAX_BOOKS {
        return Err(refuse(
            "data",
            "The books must be distinct, one to twelve of them.",
        ));
    }
    Ok(ids)
}
