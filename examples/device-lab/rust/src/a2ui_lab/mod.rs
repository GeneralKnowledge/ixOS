//! A2UI Lab (A2UI-1): one agent-proposed reading plan, rendered natively
//! with the official A2UI SwiftUI renderer and ForgeUI-backed components,
//! edited by the person, reviewed and confirmed through app-owned controls.
//! Plan and scope: docs/research/external-projects/assessments/ai/a2ui.md.
//!
//! The flow, and who owns each step:
//!
//! 1. A model fills a typed [`plan::Proposal`] (here a fixed one: A2UI-1
//!    has no model).
//! 2. Rust checks it and builds the A2UI messages ([`surface`]).
//! 3. Every message passes [`gate`] before Swift renders it.
//! 4. The person edits the draft; the surface's review button sends the
//!    one registered event, [`surface::REVIEW_EVENT`].
//! 5. Rust checks the edited plan and derives the change set from its own
//!    books, never from text the surface wrote ([`A2uiLab::review`]).
//! 6. The app's own Confirm, outside the surface, applies it, only if
//!    nothing changed since the review ([`A2uiLab::confirm`]).
//!
//! Nothing here touches another app's data: the library is a stand-in.
//! Logs carry codes and counts only, never the plan or a payload.

pub mod feeds;
pub mod gate;
pub mod plan;
pub mod playlist;
pub mod review;
pub mod surface;
pub mod task_card;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use forge_runtime::{lock, logging};

use gate::{GateState, Refusal};
use plan::{Entry, PlanError, Proposal};

/// The model's first proposal, as a model would fill [`Proposal`].
const PROPOSAL: &str = r#"{"books": [
    {"book_id": "piranesi", "priority": "high", "start": "2026-12-01", "finish": "2026-12-07"},
    {"book_id": "remains", "priority": "normal", "start": "2026-12-08", "finish": "2026-12-18"},
    {"book_id": "left_hand", "priority": "normal", "start": "2026-12-19", "finish": "2027-01-05"},
    {"book_id": "middlemarch", "priority": "low", "start": "2027-01-06", "finish": "2027-01-31"}
]}"#;

/// The model's revision, sent as the incremental update.
const REVISION: &str = r#"{"books": [
    {"book_id": "piranesi", "priority": "high", "start": "2026-12-01", "finish": "2026-12-08"},
    {"book_id": "remains", "priority": "normal", "start": "2026-12-09", "finish": "2026-12-19"},
    {"book_id": "left_hand", "priority": "normal", "start": "2026-12-20", "finish": "2027-01-31"},
    {"book_id": "middlemarch", "priority": "low", "start": "2027-02-01", "finish": "2027-02-28"}
]}"#;

#[derive(uniffi::Error, Debug, Clone, PartialEq, thiserror::Error)]
pub enum A2uiLabError {
    /// A payload, event or plan was refused. `code` is stable (tests,
    /// logs); `reason` is for the screen.
    #[error("{reason}")]
    Refused { code: String, reason: String },
    /// The plan changed after the review: review it again.
    #[error("{reason}")]
    Stale { reason: String },
}

impl From<Refusal> for A2uiLabError {
    fn from(r: Refusal) -> Self {
        logging::info!(target: "a2ui_lab", "refused: {}", r.code);
        Self::Refused {
            code: r.code.into(),
            reason: r.reason,
        }
    }
}

impl From<PlanError> for A2uiLabError {
    fn from(e: PlanError) -> Self {
        logging::info!(target: "a2ui_lab", "refused: plan");
        Self::Refused {
            code: "plan".into(),
            reason: e.0,
        }
    }
}

/// Messages for Swift's renderer, already through the gate.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiPayload {
    /// A2UI v1.0 messages, one JSON object each, in order.
    pub messages: Vec<String>,
    /// Bumped by every accepted payload and every review; a change set is
    /// only good for the revision it was made at.
    pub revision: u64,
}

/// One line of a change set: what would change for one book. Titles and
/// authors come from the app's books.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiChangeRow {
    pub book_id: String,
    pub title: String,
    pub author: String,
    /// "High priority, 1 Dec 2026 to 7 Dec 2026".
    pub plan: String,
    /// The plan it replaces, if the book had one.
    pub was: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiChangeSet {
    /// This review's own revision: Confirm must name it, so an older review
    /// can't confirm a newer one's entries.
    pub revision: u64,
    /// Only books whose plan would change.
    pub rows: Vec<A2uiChangeRow>,
    /// Books in the plan that wouldn't change.
    pub unchanged: u32,
}

/// One saved plan in the stand-in library.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiSavedPlan {
    pub title: String,
    pub plan: String,
}

/// A deliberately bad payload, for checking the gate on the phone.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiHostileCase {
    pub id: String,
    pub title: String,
}

struct Pending {
    revision: u64,
    entries: Vec<Entry>,
}

#[derive(Default)]
struct LabState {
    gate: GateState,
    revision: u64,
    /// The books the surface holds now.
    books: BTreeSet<&'static str>,
    pending: Option<Pending>,
    updated: bool,
    /// The stand-in for the app's data: what Confirm applied.
    saved: BTreeMap<&'static str, Entry>,
}

/// The lab. One per screen; thread-safe.
#[derive(uniffi::Object)]
pub struct A2uiLab {
    state: Mutex<LabState>,
}

impl Default for A2uiLab {
    fn default() -> Self {
        Self {
            state: Mutex::new(LabState::default()),
        }
    }
}

fn proposal(json: &str) -> Result<Vec<Entry>, A2uiLabError> {
    let proposal: Proposal = serde_json::from_str(json).map_err(|_| A2uiLabError::Refused {
        code: "plan".into(),
        reason: "The proposal isn't in the expected form.".into(),
    })?;
    Ok(proposal.check()?)
}

impl LabState {
    /// Passes `messages` through the gate; on success the surface's state
    /// moves to a new revision and any reviewed change set is dropped.
    fn accept(&mut self, messages: &str) -> Result<A2uiPayload, A2uiLabError> {
        let accepted = surface::check(messages, &mut self.gate)?;
        if let Some(books) = accepted.data {
            self.books = books;
        }
        self.revision += 1;
        self.pending = None;
        Ok(A2uiPayload {
            messages: accepted.messages.iter().map(|m| m.to_string()).collect(),
            revision: self.revision,
        })
    }
}

#[uniffi::export]
impl A2uiLab {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        forge_runtime::logging::init();
        std::sync::Arc::new(Self::default())
    }

    /// The surface id and catalog id Swift registers.
    pub fn surface_id(&self) -> String {
        surface::SURFACE_ID.into()
    }

    pub fn catalog_id(&self) -> String {
        surface::CATALOG_ID.into()
    }

    /// Starts over: a new surface for the model's proposal. Saved plans are
    /// kept, so a second review shows what they replace.
    pub fn start(&self) -> Result<A2uiPayload, A2uiLabError> {
        let entries = proposal(PROPOSAL)?;
        let mut state = lock(&self.state);
        state.gate = GateState::default();
        state.updated = false;
        let payload = state.accept(&surface::create(&entries).to_string())?;
        logging::info!(target: "a2ui_lab", "surface created: {} books", entries.len());
        Ok(payload)
    }

    /// The agent's incremental update: a changed text and new dates. Drops
    /// any reviewed change set, so Confirm can't apply what the person
    /// reviewed before it.
    pub fn agent_update(&self) -> Result<A2uiPayload, A2uiLabError> {
        let entries = proposal(REVISION)?;
        let mut state = lock(&self.state);
        if state.updated {
            return Err(A2uiLabError::Refused {
                code: "lifecycle".into(),
                reason: "The agent has already sent its update. Start over to see it again.".into(),
            });
        }
        let payload = state.accept(&surface::update(&entries).to_string())?;
        state.updated = true;
        logging::info!(target: "a2ui_lab", "agent update accepted");
        Ok(payload)
    }

    /// Bad payloads to try on the phone; each must be refused.
    pub fn hostile_cases(&self) -> Vec<A2uiHostileCase> {
        hostile::HOSTILE
            .iter()
            .map(|(id, title, _)| A2uiHostileCase {
                id: (*id).into(),
                title: (*title).into(),
            })
            .collect()
    }

    /// Sends hostile case `id` through the gate. Returns the payload if it
    /// were accepted (a bug), or the refusal as the error.
    pub fn send_hostile(&self, id: String) -> Result<A2uiPayload, A2uiLabError> {
        let (_, _, payload) = hostile::HOSTILE
            .iter()
            .find(|(case, _, _)| *case == id)
            .ok_or_else(|| A2uiLabError::Refused {
                code: "unknown_case".into(),
                reason: "No such test case.".into(),
            })?;
        lock(&self.state).accept(payload)
    }

    /// The surface's review event, as Swift's renderer resolved it. Checks
    /// the event against the registry and the edited plan, and derives the
    /// change set. Swift shows only what this returns.
    pub fn review(
        &self,
        name: String,
        source_component_id: String,
        context_json: String,
    ) -> Result<A2uiChangeSet, A2uiLabError> {
        if name != surface::REVIEW_EVENT || source_component_id != surface::REVIEW_COMPONENT {
            return Err(Refusal {
                code: "unregistered_event",
                reason: "The surface sent an event the app hasn't registered.".into(),
            }
            .into());
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Context {
            books: Vec<plan::EditedBook>,
        }
        let context: Context =
            serde_json::from_str(&context_json).map_err(|_| A2uiLabError::Refused {
                code: "data".into(),
                reason: "The plan isn't in the expected form.".into(),
            })?;
        let mut state = lock(&self.state);
        let entries = plan::check_edited(&context.books, &state.books)?;
        let mut rows = Vec::new();
        let mut unchanged = 0;
        for e in &entries {
            let was = state.saved.get(e.book.id);
            if was == Some(e) {
                unchanged += 1;
                continue;
            }
            rows.push(A2uiChangeRow {
                book_id: e.book.id.into(),
                title: e.book.title.into(),
                author: e.book.author.into(),
                plan: e.describe(),
                was: was.map(Entry::describe),
            });
        }
        // Each review gets its own revision, so a confirmation names this
        // change set and no other.
        state.revision += 1;
        let revision = state.revision;
        state.pending = Some(Pending { revision, entries });
        logging::info!(target: "a2ui_lab", "reviewed: {} changes", rows.len());
        Ok(A2uiChangeSet {
            revision,
            rows,
            unchanged,
        })
    }

    /// The person changed something on the surface: a reviewed change set
    /// no longer matches what they see. Returns whether one was dropped.
    pub fn surface_edited(&self) -> bool {
        lock(&self.state).pending.take().is_some()
    }

    /// Applies the change set reviewed at `revision`, if nothing has changed
    /// since. In A2UI-1 it writes only the stand-in library.
    pub fn confirm(&self, revision: u64) -> Result<u32, A2uiLabError> {
        let mut state = lock(&self.state);
        let current = state.revision;
        let Some(pending) = state.pending.take() else {
            return Err(A2uiLabError::Stale {
                reason: "The plan changed after you reviewed it. Review it again.".into(),
            });
        };
        if pending.revision != revision || revision != current {
            return Err(A2uiLabError::Stale {
                reason: "The plan changed after you reviewed it. Review it again.".into(),
            });
        }
        let mut applied = 0;
        for e in pending.entries {
            if state.saved.get(e.book.id) != Some(&e) {
                applied += 1;
                state.saved.insert(e.book.id, e);
            }
        }
        logging::info!(target: "a2ui_lab", "confirmed: {applied} changes");
        Ok(applied)
    }

    /// What Confirm has applied so far, in library order.
    pub fn saved_plans(&self) -> Vec<A2uiSavedPlan> {
        let state = lock(&self.state);
        plan::LIBRARY
            .iter()
            .filter_map(|b| state.saved.get(b.id))
            .map(|e| A2uiSavedPlan {
                title: e.book.title.into(),
                plan: e.describe(),
            })
            .collect()
    }
}

/// The hostile payloads, shared by the phone screen and the tests.
mod hostile {
    pub const HOSTILE: [(&str, &str, &str); 8] = [
        (
            "unknown_component",
            "A component the catalog doesn't have",
            r#"[{"version":"1.0","updateComponents":{"surfaceId":"readingPlan","components":[{"id":"intro","component":"WebView","url":"https://example.com"}]}}]"#,
        ),
        (
            "remote_image",
            "A remote image (could carry your data out)",
            r#"[{"version":"1.0","updateComponents":{"surfaceId":"readingPlan","components":[{"id":"cover","component":"Image","url":"https://example.com/p.png?books=piranesi"}]}}]"#,
        ),
        (
            "unknown_event",
            "A button with an unregistered event",
            r#"[{"version":"1.0","updateComponents":{"surfaceId":"readingPlan","components":[{"id":"wipe","component":"Button","child":"reviewLabel","action":{"event":{"name":"deleteLibrary","context":{}}}}]}}]"#,
        ),
        (
            "function_call",
            "A function call (openUrl)",
            r#"[{"version":"1.0","updateComponents":{"surfaceId":"readingPlan","components":[{"id":"link","component":"Button","child":"reviewLabel","action":{"functionCall":{"@call":"openUrl","args":{"url":"https://example.com"}}}}]}}]"#,
        ),
        (
            "malformed",
            "A broken message",
            r#"[{"version":"1.0","updateComponents":{"surfaceId":"readingPlan","components":"#,
        ),
        (
            "changed_context",
            "The review button pointed at other data",
            r#"[{"version":"1.0","updateComponents":{"surfaceId":"readingPlan","components":[{"id":"review","component":"Button","child":"reviewLabel","variant":"primary","action":{"event":{"name":"proposeReadingPlan","context":{"books":{"@path":"/archive"}}}}}]}}]"#,
        ),
        (
            "removed_action",
            "The review button turned into plain text",
            r#"[{"version":"1.0","updateComponents":{"surfaceId":"readingPlan","components":[{"id":"review","component":"Text","text":"Confirmed"}]}}]"#,
        ),
        (
            "unknown_book",
            "A book that isn't in the library",
            r#"[{"version":"1.0","updateDataModel":{"surfaceId":"readingPlan","path":"/books","value":[{"id":"ghost","title":"Ghost","author":"Nobody","priority":["high"],"start":"2026-12-01","finish":"2026-12-02"}]}}]"#,
        ),
    ];
}
