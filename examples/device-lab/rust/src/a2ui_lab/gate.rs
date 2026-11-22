//! The gate every agent-to-renderer message passes before Swift sees it.
//!
//! A2UI's Swift renderer validates messages against the catalog too (with
//! `ValidationConfig.strict`). This gate is the app's own check, in Rust
//! and tested on Linux, of the rules the catalog can't express. Since C1 of
//! the app capability audit (docs/research/app-capability-audit.md, 4.3) it
//! has two halves:
//!
//! **General rules, for every surface** (this file):
//!
//! - only the eight catalog components, each with only the properties the
//!   ForgeUI adapter draws (dates only, chips only);
//! - no function calls of any kind (`@call`, `functionCall`,
//!   `callRendererFunction`) and no `deleteSurface`: the agent can draw,
//!   not act;
//! - an action may only be one of the surface's registered events, from its
//!   registered component, with exactly its registered context, and no
//!   `userMessage` (a summary the surface writes is never shown as the
//!   change set);
//! - an update can't add, remove or change the action of a component that
//!   already exists, so a later message can't change what a button does;
//! - after the surface exists, an update may redraw only the components the
//!   surface lists as updatable;
//! - all or nothing: a refused payload changes nothing.
//!
//! **A registration, per surface** ([`Registration`]): its surface id, its
//! events, what may be redrawn, and the check of its data (known books,
//! known feeds, the app's own titles).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

/// The one ixOS catalog every lab surface uses (registered in Swift by
/// `A2uiSurfaceHost`, `A2uiLabCatalog.make`).
pub const CATALOG_ID: &str = "org.forgemobile.devicelab:a2ui-reading-plan:1";

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

/// The properties every component may carry.
pub const COMMON_PROPERTIES: &[&str] = &["id", "component", "@accessibility"];

/// The properties the ForgeUI adapter (`A2uiForgeCatalog.swift`) draws for
/// each component. The gate refuses any other, so an agent can't ask for a
/// look or behaviour the adapter doesn't have (a time picker, a filter
/// field, A2UI's own styles).
pub fn properties(component: &str) -> &'static [&'static str] {
    match component {
        "Text" => &["text", "variant"],
        "Column" | "Row" => &["children"],
        "Card" => &["child"],
        "Button" => &["child", "variant", "action"],
        "DateTimeInput" => &["label", "enableDate", "value"],
        "ChoicePicker" => &["label", "variant", "displayStyle", "options", "value"],
        _ => &[],
    }
}

/// One event a surface registers: its name, the one component that may
/// send it, and the exact context it carries.
#[derive(Debug, Clone, PartialEq)]
pub struct EventRule {
    pub name: &'static str,
    pub component: &'static str,
    pub context: Value,
}

/// What a surface registers with the gate. The general rules above apply to
/// every surface; these are the parts that differ.
pub trait Registration {
    /// What a checked data model gives back (the ids it holds, say).
    type Data;
    fn surface_id(&self) -> &'static str;
    /// The events the surface may send; none for a read-only surface.
    fn events(&self) -> Vec<EventRule>;
    /// The components an update may redraw once the surface exists.
    fn updatable(&self) -> &'static [&'static str];
    /// Checks data for the surface: the whole model at creation
    /// (`path` `None`), or an update at `path`.
    fn check_data(&self, path: Option<&str>, value: &Value) -> Result<Self::Data, Refusal>;
}

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

pub fn refuse(code: &'static str, reason: impl Into<String>) -> Refusal {
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
#[derive(Debug)]
pub struct Accepted<D> {
    /// The messages, unchanged, for Swift.
    pub messages: Vec<Value>,
    /// What the surface's data now holds, if the payload set it.
    pub data: Option<D>,
}

/// Checks `payload` (a message or an array of them) for the surface `reg`
/// registers, against `state`. On success `state` is updated; on refusal it
/// is untouched.
pub fn check<R: Registration>(
    payload: &str,
    reg: &R,
    state: &mut GateState,
) -> Result<Accepted<R::Data>, Refusal> {
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
    let mut data = None;
    for message in &messages {
        if let Some(checked) = check_message(message, reg, &mut next)? {
            data = Some(checked);
        }
    }
    *state = next;
    Ok(Accepted { messages, data })
}

fn check_message<R: Registration>(
    message: &Value,
    reg: &R,
    state: &mut GateState,
) -> Result<Option<R::Data>, Refusal> {
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
            surface_id(body, reg)?;
            only_keys(body, &["surfaceId", "catalogId", "components", "dataModel"])?;
            if body.get("catalogId").and_then(Value::as_str) != Some(CATALOG_ID) {
                return Err(refuse("catalog", "The surface must use the lab's catalog."));
            }
            state.created = true;
            if let Some(components) = body.get("components") {
                check_components(components, reg, state)?;
            }
            match body.get("dataModel") {
                Some(model) => reg.check_data(None, model).map(Some),
                None => Ok(None),
            }
        }
        "updateComponents" => {
            created(state)?;
            surface_id(body, reg)?;
            only_keys(body, &["surfaceId", "components"])?;
            let components = body
                .get("components")
                .ok_or_else(|| refuse("malformed", "updateComponents has no components."))?;
            check_components(components, reg, state)?;
            let locked = components.as_array().is_some_and(|list| {
                list.iter().any(|c| {
                    !c.get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| reg.updatable().contains(&id))
                })
            });
            if locked {
                return Err(refuse(
                    "component_locked",
                    "An update may only change the introduction, not the surface's controls.",
                ));
            }
            Ok(None)
        }
        "updateDataModel" => {
            created(state)?;
            surface_id(body, reg)?;
            only_keys(body, &["surfaceId", "path", "value"])?;
            let path = body.get("path").map(|p| {
                p.as_str()
                    .ok_or_else(|| refuse("malformed", "A data path isn't a string."))
            });
            let path = path.transpose()?;
            let value = body
                .get("value")
                .ok_or_else(|| refuse("malformed", "updateDataModel has no value."))?;
            reg.check_data(path, value).map(Some)
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

fn surface_id<R: Registration>(body: &Map<String, Value>, reg: &R) -> Result<(), Refusal> {
    if body.get("surfaceId").and_then(Value::as_str) == Some(reg.surface_id()) {
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

fn check_components<R: Registration>(
    components: &Value,
    reg: &R,
    state: &mut GateState,
) -> Result<(), Refusal> {
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
        check_properties(kind, object)?;
        let action = match object.get("action") {
            Some(action) => Some(check_action(id, action, reg)?),
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

/// Only the properties the ForgeUI adapter draws, and only the forms it
/// draws: a date picker without a time, a choice picker as chips, a
/// primary or default button.
fn check_properties(kind: &str, object: &Map<String, Value>) -> Result<(), Refusal> {
    let allowed = properties(kind);
    let unsupported = || {
        refuse(
            "unsupported_property",
            format!("A {kind} asks for something this lab's catalog doesn't draw."),
        )
    };
    if object
        .keys()
        .any(|key| !COMMON_PROPERTIES.contains(&key.as_str()) && !allowed.contains(&key.as_str()))
    {
        return Err(unsupported());
    }
    let is = |key: &str, want: &Value| object.get(key).is_none_or(|v| v == want);
    let ok = match kind {
        "DateTimeInput" => object.get("enableDate") == Some(&Value::Bool(true)),
        "ChoicePicker" => is("displayStyle", &Value::from("chips")),
        // The adapter draws primary and default buttons; a borderless one
        // would come out tinted, which isn't what was asked for.
        "Button" => object
            .get("variant")
            .is_none_or(|v| v == "primary" || v == "default"),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(unsupported())
    }
}

/// The registry: an action must be one of the surface's events, sent by
/// its one component, with exactly its context.
fn check_action<R: Registration>(id: &str, action: &Value, reg: &R) -> Result<String, Refusal> {
    let event = action
        .as_object()
        .filter(|a| a.len() == 1)
        .and_then(|a| a.get("event"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            refuse(
                "unregistered_event",
                "Only the surface's registered events are allowed.",
            )
        })?;
    let name = event.get("name").and_then(Value::as_str);
    let rules = reg.events();
    let Some(rule) = rules.iter().find(|r| Some(r.name) == name) else {
        return Err(refuse(
            "unregistered_event",
            "The surface asked for an event the app hasn't registered.",
        ));
    };
    if event.contains_key("userMessage") || event.len() != 2 {
        return Err(refuse(
            "unregistered_event",
            "An event carries only its registered context.",
        ));
    }
    if event.get("context") != Some(&rule.context) {
        return Err(refuse(
            "action_changed",
            "The event's context isn't the registered one.",
        ));
    }
    if id != rule.component {
        return Err(refuse(
            "unregistered_event",
            "Only the registered control may send this event.",
        ));
    }
    Ok(action.to_string())
}
