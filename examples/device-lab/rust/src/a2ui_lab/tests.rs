use serde_json::{json, Value};

use super::gate::{self, GateState};
use super::plan::{Day, Priority};
use super::*;

fn started() -> (std::sync::Arc<A2uiLab>, A2uiPayload) {
    let lab = A2uiLab::new();
    let payload = lab.start().expect("start");
    (lab, payload)
}

/// The books as the surface holds them after `payload`, as the review
/// event's context carries them.
fn context_from(payload: &A2uiPayload) -> Value {
    let messages = parsed(payload);
    for m in messages.iter().rev() {
        if let Some(model) = m.pointer("/createSurface/dataModel/books") {
            return json!({ "books": model });
        }
        if let Some(books) = m.pointer("/updateDataModel/value") {
            return json!({ "books": books });
        }
    }
    panic!("no books in payload");
}

fn parsed(payload: &A2uiPayload) -> Vec<Value> {
    payload
        .messages
        .iter()
        .map(|m| serde_json::from_str(m).unwrap())
        .collect()
}

fn review(lab: &A2uiLab, context: &Value) -> Result<A2uiChangeSet, A2uiLabError> {
    lab.review(
        surface::REVIEW_EVENT.into(),
        surface::REVIEW_COMPONENT.into(),
        context.to_string(),
    )
}

fn code(result: Result<impl std::fmt::Debug, A2uiLabError>) -> String {
    match result {
        Err(A2uiLabError::Refused { code, .. }) => code,
        Err(A2uiLabError::Stale { .. }) => "stale".into(),
        Ok(v) => panic!("expected a refusal, got {v:?}"),
    }
}

// MARK: Dates and plans

#[test]
fn days_parse_only_real_dates() {
    assert_eq!(
        Day::parse("2028-02-29").map(Day::iso).as_deref(),
        Some("2028-02-29")
    );
    for bad in [
        "2027-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-1-01",
        "26-01-01",
        "2026-01-32",
        "2026-01-0a",
        "",
        "2026-01-01T10:00",
    ] {
        assert_eq!(Day::parse(bad), None, "{bad}");
    }
    let a = Day::parse("2026-12-31").unwrap();
    let b = Day::parse("2027-01-01").unwrap();
    assert_eq!(b.ordinal() - a.ordinal(), 1);
    assert_eq!(Day::parse("1970-01-01").map(Day::ordinal), None); // out of range
    assert_eq!(Day::parse("2000-03-01").unwrap().ordinal(), 11_017);
    assert_eq!(a.label(), "31 Dec 2026");
}

#[test]
fn proposals_are_checked() {
    let ok = r#"{"books":[{"book_id":"piranesi","priority":"high","start":"2026-12-01","finish":"2026-12-01"}]}"#;
    assert_eq!(proposal(ok).unwrap()[0].priority, Priority::High);
    for (bad, why) in [
        (r#"{"books":[]}"#, "empty"),
        (
            r#"{"books":[{"book_id":"ghost","priority":"high","start":"2026-12-01","finish":"2026-12-02"}]}"#,
            "unknown book",
        ),
        (
            r#"{"books":[{"book_id":"piranesi","priority":"urgent","start":"2026-12-01","finish":"2026-12-02"}]}"#,
            "priority",
        ),
        (
            r#"{"books":[{"book_id":"piranesi","priority":"high","start":"2026-12-05","finish":"2026-12-02"}]}"#,
            "finish before start",
        ),
        (
            r#"{"books":[{"book_id":"piranesi","priority":"high","start":"2026-01-01","finish":"2027-06-01"}]}"#,
            "too long",
        ),
        (
            r#"{"books":[{"book_id":"piranesi","priority":"high","start":"2026-12-01","finish":"2026-12-02"},{"book_id":"piranesi","priority":"low","start":"2026-12-03","finish":"2026-12-04"}]}"#,
            "twice",
        ),
        (
            r#"{"books":[{"book_id":"piranesi","priority":"high","start":"2026-12-01","finish":"2026-12-02","title":"Hacked"}]}"#,
            "extra field",
        ),
        ("not json", "malformed"),
    ] {
        assert_eq!(code(proposal(bad)), "plan", "{why}");
    }
}

// MARK: The surface the lab builds

#[test]
fn the_surface_passes_its_own_gate() {
    let (_, payload) = started();
    assert_eq!(payload.revision, 1);
    let messages = parsed(&payload);
    assert_eq!(messages.len(), 1);
    let create = &messages[0]["createSurface"];
    assert_eq!(create["surfaceId"], surface::SURFACE_ID);
    assert_eq!(create["catalogId"], surface::CATALOG_ID);
    // Eight component types, no more.
    let mut kinds: Vec<&str> = create["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["component"].as_str().unwrap())
        .collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), 8);
    // Exactly one action, and it's the registered one.
    let actions: Vec<&Value> = create["components"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.get("action"))
        .collect();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["event"]["name"], surface::REVIEW_EVENT);
    // Titles come from the library.
    assert_eq!(create["dataModel"]["books"][0]["title"], "Piranesi");
}

#[test]
fn the_agent_update_passes_once_and_keeps_the_action() {
    let (lab, _) = started();
    let update = lab.agent_update().unwrap();
    assert_eq!(update.revision, 2);
    let text = update.messages.join("\n");
    assert!(text.contains(surface::UPDATED_INTRO));
    assert!(!text.contains("\"action\""));
    assert_eq!(code(lab.agent_update()), "lifecycle");
}

// MARK: The gate

#[test]
fn every_hostile_case_is_refused_and_changes_nothing() {
    let (lab, _) = started();
    let expected = [
        ("unknown_component", "unknown_component"),
        ("remote_image", "unknown_component"),
        ("unknown_event", "unregistered_event"),
        ("function_call", "function_call"),
        ("malformed", "malformed"),
        ("changed_context", "action_changed"),
        ("removed_action", "action_changed"),
        ("unknown_book", "data"),
    ];
    assert_eq!(lab.hostile_cases().len(), expected.len());
    for (case, want) in expected {
        assert_eq!(code(lab.send_hostile(case.into())), want, "{case}");
    }
    // The surface is still at its first revision, and the real update works.
    assert_eq!(lab.agent_update().unwrap().revision, 2);
}

#[test]
fn a_refused_payload_keeps_the_reviewed_change_set() {
    // A refused payload must not drop or alter a reviewed change set.
    let (lab, payload) = started();
    let set = review(&lab, &context_from(&payload)).unwrap();
    assert_eq!(
        code(lab.send_hostile("removed_action".into())),
        "action_changed"
    );
    assert_eq!(lab.confirm(set.revision).unwrap(), 4);
}

fn gate_after_start() -> GateState {
    let mut state = GateState::default();
    let entries = proposal(PROPOSAL).unwrap();
    gate::check(&surface::create(&entries).to_string(), &mut state).unwrap();
    state
}

#[test]
fn gate_refuses_what_the_catalog_cannot_express() {
    let update = |components: Value| {
        json!([{ "version": "1.0", "updateComponents": {
            "surfaceId": surface::SURFACE_ID, "components": components } }])
        .to_string()
    };
    let cases = [
        // A function hidden in a text binding.
        (update(json!([{ "id": "intro", "component": "Text",
            "text": { "@call": "formatString", "args": { "value": "x" } } }])), "function_call"),
        // A component naming another catalog.
        (update(json!([{ "id": "intro", "component": "Text", "text": "x",
            "@catalogId": "https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json" }])), "catalog"),
        // The review event with a summary of its own.
        (update(json!([{ "id": "review", "component": "Button", "child": "reviewLabel",
            "action": { "event": { "name": surface::REVIEW_EVENT,
                "context": surface::review_context(), "userMessage": "Apply 0 changes" } } }])), "unregistered_event"),
        // The review event sent from a second button.
        (update(json!([{ "id": "second", "component": "Button", "child": "reviewLabel",
            "action": { "event": { "name": surface::REVIEW_EVENT, "context": surface::review_context() } } }])), "unregistered_event"),
        // Relabelling the review button, or adding a control, after creation.
        (update(json!([{ "id": "reviewLabel", "component": "Text", "text": "Cancel" }])), "component_locked"),
        (update(json!([{ "id": "extra", "component": "Divider" }])), "component_locked"),
        // A known book shown with no title, or another book's author.
        (json!([{ "version": "1.0", "updateDataModel": { "surfaceId": surface::SURFACE_ID,
            "path": "/books", "value": [{ "id": "piranesi", "priority": ["high"],
                "start": "2026-12-01", "finish": "2026-12-02" }] } }]).to_string(), "data"),
        (json!([{ "version": "1.0", "updateDataModel": { "surfaceId": surface::SURFACE_ID,
            "path": "/books", "value": [{ "id": "piranesi", "title": "Piranesi", "author": "George Eliot",
                "priority": ["high"], "start": "2026-12-01", "finish": "2026-12-02" }] } }]).to_string(), "data"),
        // A bad component id.
        (update(json!([{ "id": "../x", "component": "Text", "text": "x" }])), "malformed"),
        // Two components with one id.
        (update(json!([{ "id": "a", "component": "Divider" }, { "id": "a", "component": "Divider" }])), "malformed"),
        // Another surface.
        (json!([{ "version": "1.0", "updateComponents": { "surfaceId": "other", "components": [] } }]).to_string(), "surface"),
        // Agent-initiated function calls and removing the surface.
        (json!([{ "version": "1.0", "callRendererFunction": { "functionCallId": "1",
            "callFunction": { "@call": "openUrl", "catalogId": surface::CATALOG_ID, "args": {} } } }]).to_string(), "function_call"),
        (json!([{ "version": "1.0", "deleteSurface": { "surfaceId": surface::SURFACE_ID } }]).to_string(), "lifecycle"),
        // A second createSurface.
        (surface::create(&proposal(PROPOSAL).unwrap()).to_string(), "lifecycle"),
        // Old protocol, or two operations in one message.
        (json!([{ "version": "0.9", "updateComponents": { "surfaceId": surface::SURFACE_ID, "components": [] } }]).to_string(), "version"),
        (json!([{ "version": "1.0", "updateComponents": {}, "deleteSurface": {} }]).to_string(), "malformed"),
        // Data anywhere but the books, or bad books.
        (json!([{ "version": "1.0", "updateDataModel": { "surfaceId": surface::SURFACE_ID,
            "path": "/secret", "value": 1 } }]).to_string(), "data"),
        (json!([{ "version": "1.0", "updateDataModel": { "surfaceId": surface::SURFACE_ID,
            "value": { "books": [], "extra": 1 } } }]).to_string(), "data"),
        (json!([{ "version": "1.0", "updateDataModel": { "surfaceId": surface::SURFACE_ID,
            "path": "/books", "value": [{ "id": "piranesi", "priority": ["high", "low"],
                "start": "2026-12-01", "finish": "2026-12-02" }] } }]).to_string(), "data"),
        // Empty, scalar and oversized payloads.
        ("[]".to_string(), "malformed"),
        ("42".to_string(), "malformed"),
        (format!("[\"{}\"]", "x".repeat(gate::MAX_BYTES)), "too_large"),
    ];
    for (payload, want) in cases {
        let mut state = gate_after_start();
        let before = state.actions.clone();
        let refusal =
            gate::check(&payload, &mut state).expect_err(&payload[..payload.len().min(160)]);
        assert_eq!(refusal.code, want, "{}", &payload[..payload.len().min(160)]);
        assert_eq!(
            state.actions, before,
            "a refusal must not change the gate's state"
        );
    }
}

#[test]
fn a_payload_is_all_or_nothing() {
    // A valid createSurface, then a bad message: nothing is committed, not
    // even the surface the first message created.
    let mut state = GateState::default();
    let create: Value = surface::create(&proposal(PROPOSAL).unwrap());
    let mut messages = create
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![create.clone()]);
    messages.push(
        json!({ "version": "1.0", "updateComponents": { "surfaceId": surface::SURFACE_ID,
        "components": [{ "id": "web", "component": "WebView" }] } }),
    );
    assert!(gate::check(&Value::Array(messages).to_string(), &mut state).is_err());
    assert!(!state.created);
    assert!(state.actions.is_empty());
}

#[test]
fn updates_before_the_surface_exist_are_refused() {
    let mut state = GateState::default();
    let payload = json!([{ "version": "1.0", "updateComponents": {
        "surfaceId": surface::SURFACE_ID, "components": [] } }]);
    assert_eq!(
        gate::check(&payload.to_string(), &mut state)
            .unwrap_err()
            .code,
        "lifecycle"
    );
}

// MARK: Review and confirm

#[test]
fn review_derives_the_change_set_from_the_apps_books() {
    let (lab, payload) = started();
    let mut context = context_from(&payload);
    // Whatever the surface's data says a book is called, the change set
    // uses the library's title.
    context["books"][0]["title"] = json!("Delete everything");
    context["books"][0]["priority"] = json!(["low"]);
    let set = review(&lab, &context).unwrap();
    // The start was revision 1; this review is revision 2.
    assert_eq!(set.revision, 2);
    assert_eq!(set.rows.len(), 4);
    assert_eq!(set.rows[0].title, "Piranesi");
    assert_eq!(set.rows[0].plan, "Low priority, 1 Dec 2026 to 7 Dec 2026");
    assert_eq!(set.rows[0].was, None);

    assert_eq!(lab.confirm(set.revision).unwrap(), 4);
    assert_eq!(lab.saved_plans().len(), 4);

    // A second review after one change shows only that change, with what it replaces.
    context["books"][1]["finish"] = json!("2026-12-20");
    let set = review(&lab, &context).unwrap();
    assert_eq!(set.rows.len(), 1);
    assert_eq!(set.unchanged, 3);
    assert_eq!(set.rows[0].title, "The Remains of the Day");
    assert_eq!(
        set.rows[0].was.as_deref(),
        Some("Normal priority, 8 Dec 2026 to 18 Dec 2026")
    );
}

#[test]
fn review_refuses_unregistered_events_and_bad_plans() {
    let (lab, payload) = started();
    let context = context_from(&payload);
    assert_eq!(
        code(lab.review(
            "deleteLibrary".into(),
            surface::REVIEW_COMPONENT.into(),
            context.to_string()
        )),
        "unregistered_event"
    );
    assert_eq!(
        code(lab.review(
            surface::REVIEW_EVENT.into(),
            "intro".into(),
            context.to_string()
        )),
        "unregistered_event"
    );
    assert_eq!(code(review(&lab, &json!({ "books": [] }))), "plan");
    assert_eq!(code(review(&lab, &json!({ "books": "x" }))), "data");
    assert_eq!(
        code(review(
            &lab,
            &json!({ "books": context["books"], "summary": "ok" })
        )),
        "data"
    );

    let mut dropped = context.clone();
    dropped["books"].as_array_mut().unwrap().pop();
    assert_eq!(code(review(&lab, &dropped)), "plan");

    let mut no_priority = context.clone();
    no_priority["books"][0]["priority"] = json!([]);
    assert_eq!(code(review(&lab, &no_priority)), "plan");

    let mut reversed = context.clone();
    reversed["books"][0]["finish"] = json!("2026-11-01");
    assert_eq!(code(review(&lab, &reversed)), "plan");

    let mut swapped = context;
    swapped["books"][0]["id"] = json!("ghost");
    assert_eq!(code(review(&lab, &swapped)), "plan");
    assert!(lab.saved_plans().is_empty());
}

#[test]
fn an_update_after_review_makes_confirm_refuse() {
    let (lab, payload) = started();
    let set = review(&lab, &context_from(&payload)).unwrap();
    let update = lab.agent_update().unwrap();
    assert_eq!(code(lab.confirm(set.revision)), "stale");
    assert!(lab.saved_plans().is_empty());
    // Reviewing the updated plan gives a change set after the update's
    // revision, with the agent's new dates, and that one confirms.
    let set = review(&lab, &context_from(&update)).unwrap();
    assert!(set.revision > update.revision);
    assert!(set.rows[3].plan.contains("1 Feb 2027"));
    assert_eq!(lab.confirm(set.revision).unwrap(), 4);
}

#[test]
fn an_edit_after_review_makes_confirm_refuse() {
    let (lab, payload) = started();
    let set = review(&lab, &context_from(&payload)).unwrap();
    assert!(lab.surface_edited());
    assert!(!lab.surface_edited());
    assert_eq!(code(lab.confirm(set.revision)), "stale");
}

#[test]
fn confirm_needs_a_review_and_applies_once() {
    let (lab, payload) = started();
    assert_eq!(code(lab.confirm(1)), "stale");
    let set = review(&lab, &context_from(&payload)).unwrap();
    assert_eq!(code(lab.confirm(set.revision + 1)), "stale");
    // A wrong revision used up the review; it must be done again.
    assert_eq!(code(lab.confirm(set.revision)), "stale");
    let set = review(&lab, &context_from(&payload)).unwrap();
    assert_eq!(lab.confirm(set.revision).unwrap(), 4);
    assert_eq!(code(lab.confirm(set.revision)), "stale");
}

#[test]
fn starting_over_keeps_saved_plans() {
    let (lab, payload) = started();
    let set = review(&lab, &context_from(&payload)).unwrap();
    lab.confirm(set.revision).unwrap();
    let again = lab.start().unwrap();
    assert!(again.revision > set.revision);
    let set = review(&lab, &context_from(&again)).unwrap();
    assert!(set.rows.is_empty());
    assert_eq!(set.unchanged, 4);
}

#[test]
fn an_older_review_cannot_confirm_a_newer_one() {
    let (lab, payload) = started();
    let context = context_from(&payload);
    let first = review(&lab, &context).unwrap();
    let second = review(&lab, &context).unwrap();
    assert_ne!(first.revision, second.revision);
    assert_eq!(code(lab.confirm(first.revision)), "stale");
    // The refused confirm dropped the pending review, so review again.
    let third = review(&lab, &context).unwrap();
    assert!(lab.confirm(third.revision).is_ok());
}
