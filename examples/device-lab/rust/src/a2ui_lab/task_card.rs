//! C2 of the app capability audit: the Bonsai Harness's verified-task card,
//! the second A2UI surface and the first read-only one
//! (docs/research/app-capability-audit.md, section 3, Device Lab).
//!
//! Everything on it comes from the harness's own record of the task: the
//! request, the acceptance criteria, the evidence the harness checked, the
//! files the task left and how many steps it took. Never the model's own
//! summary ("the model's own word never counts"). The card registers no
//! events and nothing may be redrawn, so the surface can only be shown.

use serde_json::{json, Map, Value};

use super::gate::{self, EventRule, GateState, Refusal, Registration};
use super::{A2uiLabError, A2uiPayload};
use crate::harness::journal::TaskStatus;
use crate::harness::{BonsaiHarness, HarnessTaskRecord};

pub const SURFACE_ID: &str = "taskResult";
/// The most criteria and files the card lists.
pub const MAX_ITEMS: usize = 12;
/// The longest text one line of the card holds, in characters.
pub const MAX_TEXT: usize = 400;

/// What the card shows, as Rust read it from the harness.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub task: String,
    pub evidence: String,
    pub criteria: Vec<String>,
    pub files: Vec<String>,
    pub steps: u32,
    pub attempts: u32,
}

fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_TEXT {
        return text.to_string();
    }
    // cut: the harness's own text, shortened for the card with "…" at the end
    let mut short: String = text.chars().take(MAX_TEXT - 1).collect();
    short.push('\u{2026}');
    short
}

impl Card {
    /// The card for a task the harness verified, or `None` for one it
    /// didn't (still running, failed, stopped).
    pub fn verified(record: &HarnessTaskRecord, files: &[String], steps: u32) -> Option<Self> {
        if record.status != TaskStatus::Completed {
            return None;
        }
        Some(Self {
            task: clip(&record.description),
            evidence: clip(record.result.as_deref().unwrap_or("")),
            criteria: record
                .acceptance
                .iter()
                .take(MAX_ITEMS)
                .map(|c| clip(c))
                .collect(),
            files: files.iter().take(MAX_ITEMS).map(|f| clip(f)).collect(),
            steps,
            attempts: record.attempts,
        })
    }

    fn steps_line(&self) -> String {
        let steps = if self.steps == 1 {
            "1 step".to_string()
        } else {
            format!("{} steps", self.steps)
        };
        let tries = if self.attempts <= 1 {
            "verified on the first check".to_string()
        } else {
            format!("verified on check {}", self.attempts)
        };
        format!("{steps}, {tries}")
    }

    fn value(&self) -> Value {
        let items = |list: &[String]| -> Value {
            Value::Array(list.iter().map(|t| json!({ "text": t })).collect())
        };
        json!({
            "task": self.task,
            "status": "Verified by the harness",
            "evidence": self.evidence,
            "steps": self.steps_line(),
            "criteria": items(&self.criteria),
            "files": items(&self.files),
        })
    }
}

fn components() -> Value {
    json!([
        { "id": "root", "component": "Card", "child": "body",
          "@accessibility": { "label": "Verified task" } },
        { "id": "body", "component": "Column",
          "children": ["status", "task", "steps", "divider1", "criteriaTitle", "criteria",
                       "evidenceTitle", "evidence", "divider2", "filesTitle", "files"] },
        { "id": "status", "component": "Text", "text": { "@path": "/card/status" }, "variant": "caption" },
        { "id": "task", "component": "Text", "text": { "@path": "/card/task" }, "variant": "body" },
        { "id": "steps", "component": "Text", "text": { "@path": "/card/steps" }, "variant": "caption" },
        { "id": "divider1", "component": "Divider" },
        { "id": "criteriaTitle", "component": "Text", "text": "What proves it done", "variant": "body" },
        { "id": "criteria", "component": "Column",
          "children": { "componentId": "criterion", "path": "/card/criteria" } },
        { "id": "criterion", "component": "Text", "text": { "@path": "text" }, "variant": "caption" },
        { "id": "evidenceTitle", "component": "Text", "text": "What the harness saw", "variant": "body" },
        { "id": "evidence", "component": "Text", "text": { "@path": "/card/evidence" }, "variant": "caption" },
        { "id": "divider2", "component": "Divider" },
        { "id": "filesTitle", "component": "Text", "text": "Files", "variant": "body" },
        { "id": "files", "component": "Column",
          "children": { "componentId": "file", "path": "/card/files" } },
        { "id": "file", "component": "Text", "text": { "@path": "text" }, "variant": "caption" }
    ])
}

/// The card's one message: the whole surface at once.
pub fn create(card: &Card) -> Value {
    json!([{
        "version": "1.0",
        "createSurface": {
            "surfaceId": SURFACE_ID,
            "catalogId": gate::CATALOG_ID,
            "components": components(),
            "dataModel": { "card": card.value() }
        }
    }])
}

/// The card's registration: no events, nothing redrawn, and data only in
/// the card's own shape, with bounded text.
pub struct TaskCard;

fn text_field(object: &Map<String, Value>, key: &str) -> Result<(), Refusal> {
    match object.get(key).and_then(Value::as_str) {
        Some(text) if text.chars().count() <= MAX_TEXT => Ok(()),
        _ => Err(gate::refuse("data", "The card's data isn't in its form.")),
    }
}

fn list_field(object: &Map<String, Value>, key: &str) -> Result<(), Refusal> {
    let bad = || gate::refuse("data", "The card's data isn't in its form.");
    let list = object.get(key).and_then(Value::as_array).ok_or_else(bad)?;
    if list.len() > MAX_ITEMS {
        return Err(bad());
    }
    for item in list {
        let item = item.as_object().filter(|o| o.len() == 1).ok_or_else(bad)?;
        text_field(item, "text")?;
    }
    Ok(())
}

impl Registration for TaskCard {
    type Data = ();

    fn surface_id(&self) -> &'static str {
        SURFACE_ID
    }

    fn events(&self) -> Vec<EventRule> {
        Vec::new()
    }

    fn updatable(&self) -> &'static [&'static str] {
        &[]
    }

    fn check_data(&self, path: Option<&str>, value: &Value) -> Result<(), Refusal> {
        if path.is_some() {
            return Err(gate::refuse(
                "data",
                "The card can't be changed once shown.",
            ));
        }
        let card = value
            .as_object()
            .filter(|o| o.len() == 1)
            .and_then(|o| o.get("card"))
            .and_then(Value::as_object)
            .filter(|c| c.len() == 6)
            .ok_or_else(|| gate::refuse("data", "The card's data isn't in its form."))?;
        for key in ["task", "status", "evidence", "steps"] {
            text_field(card, key)?;
        }
        list_field(card, "criteria")?;
        list_field(card, "files")
    }
}

/// Passes `payload` through the card's gate, on a fresh surface.
pub fn check(payload: &str) -> Result<Vec<String>, Refusal> {
    let accepted = gate::check(payload, &TaskCard, &mut GateState::default())?;
    Ok(accepted.messages.iter().map(Value::to_string).collect())
}

#[uniffi::export]
impl BonsaiHarness {
    /// The verified-task card for the last task, as A2UI messages through
    /// the gate. Refused for a task the harness hasn't verified.
    pub fn result_card(&self) -> Result<A2uiPayload, A2uiLabError> {
        let snapshot = self.snapshot();
        let files: Vec<String> = self.files().into_iter().map(|f| f.path).collect();
        let card = snapshot
            .task
            .as_ref()
            .and_then(|task| Card::verified(task, &files, snapshot.steps.len() as u32))
            .ok_or_else(|| A2uiLabError::Refused {
                code: "not_verified".into(),
                reason: "The card is only for a task the harness verified.".into(),
            })?;
        let messages = check(&create(&card).to_string())?;
        forge_runtime::logging::info!(target: "a2ui_lab", "task card: {} files", card.files.len());
        Ok(A2uiPayload {
            messages,
            revision: 1,
        })
    }

    /// The card's surface id, for Swift's renderer.
    pub fn result_card_surface_id(&self) -> String {
        SURFACE_ID.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(status: TaskStatus) -> HarnessTaskRecord {
        HarnessTaskRecord {
            id: "t1".into(),
            description: "Write hello.py that prints hello".into(),
            status,
            dependencies: Vec::new(),
            acceptance: vec!["hello.py prints hello".into()],
            attempts: 1,
            result: Some("Ran hello.py: hello".into()),
            model_id: "bonsai".into(),
        }
    }

    #[test]
    fn only_a_verified_task_gets_a_card() {
        for status in [TaskStatus::Running, TaskStatus::Failed, TaskStatus::Stopped] {
            assert!(Card::verified(&record(status), &[], 3).is_none());
        }
        let card = Card::verified(&record(TaskStatus::Completed), &["hello.py".into()], 3).unwrap();
        assert_eq!(card.steps_line(), "3 steps, verified on the first check");
        let messages = check(&create(&card).to_string()).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Ran hello.py: hello"));
    }

    #[test]
    fn the_card_has_no_actions_and_cant_be_changed() {
        let card = Card::verified(&record(TaskStatus::Completed), &[], 1).unwrap();
        let mut state = GateState::default();
        gate::check(&create(&card).to_string(), &TaskCard, &mut state).unwrap();
        assert!(state.actions.values().all(Option::is_none));
        let hostile = [
            // A changed claim after it is shown.
            json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
                "path": "/card/status", "value": "Approved" } }]),
            json!([{ "version": "1.0", "updateComponents": { "surfaceId": SURFACE_ID,
                "components": [{ "id": "status", "component": "Text", "text": "Approved" }] } }]),
            // Another surface's id.
            json!([{ "version": "1.0", "updateDataModel": { "surfaceId": "readingPlan",
                "path": "/books", "value": [] } }]),
        ];
        for payload in hostile {
            let mut s = state.clone();
            assert!(
                gate::check(&payload.to_string(), &TaskCard, &mut s).is_err(),
                "{payload}"
            );
        }
        // A button on a new card: the card registers no events.
        let button = json!([{ "version": "1.0", "createSurface": { "surfaceId": SURFACE_ID,
            "catalogId": gate::CATALOG_ID, "components": [
                { "id": "root", "component": "Button", "child": "x",
                  "action": { "event": { "name": "proposeReadingPlan",
                    "context": { "books": { "@path": "/books" } } } } }] } }]);
        let refusal = check(&button.to_string()).unwrap_err();
        assert_eq!(refusal.code, "unregistered_event");
        // Extra data in the card is refused too.
        let mut value = create(&card);
        value[0]["createSurface"]["dataModel"]["card"]["approved"] = json!(true);
        assert!(check(&value.to_string()).is_err());
    }

    #[test]
    fn long_text_is_clipped_to_the_card() {
        let mut r = record(TaskStatus::Completed);
        r.result = Some("x".repeat(MAX_TEXT * 2));
        let card = Card::verified(&r, &[], 1).unwrap();
        assert_eq!(card.evidence.chars().count(), MAX_TEXT);
        assert!(check(&create(&card).to_string()).is_ok());
    }
}
