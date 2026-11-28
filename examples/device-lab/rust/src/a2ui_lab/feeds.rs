//! C3 of the app capability audit: Commonplace's "organise my feeds" drawn
//! as an A2UI surface (docs/research/app-capability-audit.md, section 3).
//!
//! The capability is Commonplace's (`commonplace_core::organise`): the
//! snapshot, the typed folder plan, its check, and the apply step on
//! Commonplace's own core. This file only draws it, checks the surface's
//! messages and events, and keeps the review revision. Swift carries the
//! snapshot from Commonplace and the confirmed plan back to it, as JSON of
//! these same Rust types; Commonplace checks the plan again before it moves
//! anything.

use std::collections::BTreeMap;
use std::sync::Mutex;

use commonplace_core::organise::{self, FeedMove, FolderPlan, OrganiseSnapshot, Target, MAX_FEEDS};
use forge_runtime::{lock, logging};
use serde_json::{json, Value};

use super::gate::{self, EventRule, GateState, Refusal, Registration};
use super::review::Reviews;
use super::{A2uiLabError, A2uiPayload};

pub const SURFACE_ID: &str = "organiseFeeds";
pub const REVIEW_EVENT: &str = "reviewFolders";
pub const REVIEW_COMPONENT: &str = "review";
const NO_FOLDER: &str = "none";

fn review_context() -> Value {
    json!({ "feeds": { "@path": "/feeds" } })
}

/// The choices a feed's picker offers: the library's folders, the
/// proposal's new folders, and no folder. Values are the surface's; Rust
/// maps them back to [`Target`]s.
#[derive(Debug, Clone, PartialEq)]
pub struct Choices {
    entries: Vec<(String, String, Target)>,
}

impl Choices {
    fn new(snapshot: &OrganiseSnapshot, plan: &FolderPlan) -> Self {
        let mut entries: Vec<(String, String, Target)> = snapshot
            .folders
            .iter()
            .map(|f| {
                (
                    format!("folder_{}", f.id),
                    f.name.clone(),
                    Target::Folder { id: f.id },
                )
            })
            .collect();
        let mut new_names: Vec<&str> = Vec::new();
        for m in &plan.moves {
            if let Target::NewFolder { name } = &m.to {
                if !new_names.contains(&name.as_str()) {
                    new_names.push(name);
                }
            }
        }
        for (i, name) in new_names.iter().enumerate() {
            entries.push((
                format!("new_{i}"),
                format!("{name} (new)"),
                Target::NewFolder {
                    name: (*name).to_string(),
                },
            ));
        }
        entries.push((NO_FOLDER.into(), "No folder".into(), Target::NoFolder));
        Self { entries }
    }

    fn value_of(&self, target: &Target) -> &str {
        self.entries
            .iter()
            .find(|(_, _, t)| t == target)
            .map_or(NO_FOLDER, |(v, _, _)| v)
    }

    fn target(&self, value: &str) -> Option<&Target> {
        self.entries
            .iter()
            .find(|(v, _, _)| v == value)
            .map(|(_, _, t)| t)
    }

    fn options(&self) -> Value {
        Value::Array(
            self.entries
                .iter()
                .map(|(value, label, _)| json!({ "label": label, "value": value }))
                .collect(),
        )
    }
}

/// The surface for one snapshot and proposal.
pub struct FeedsSurface {
    pub snapshot: OrganiseSnapshot,
    pub choices: Choices,
    /// Where the proposal puts each feed (feeds it doesn't move stay).
    proposed: BTreeMap<i64, Target>,
}

impl FeedsSurface {
    pub fn new(snapshot: OrganiseSnapshot, plan: &FolderPlan) -> Self {
        let choices = Choices::new(&snapshot, plan);
        let proposed = plan
            .moves
            .iter()
            .map(|m| (m.feed_id, m.to.clone()))
            .collect();
        Self {
            snapshot,
            choices,
            proposed,
        }
    }

    fn current(&self, feed_id: i64) -> Target {
        match self.snapshot.feed(feed_id).and_then(|f| f.folder_id) {
            Some(id) => Target::Folder { id },
            None => Target::NoFolder,
        }
    }

    fn data(&self) -> Value {
        Value::Array(
            self.snapshot
                .feeds
                .iter()
                .map(|f| {
                    let target = self
                        .proposed
                        .get(&f.id)
                        .cloned()
                        .unwrap_or_else(|| self.current(f.id));
                    json!({
                        "id": f.id.to_string(),
                        "title": f.title,
                        "folder": [self.choices.value_of(&target)],
                    })
                })
                .collect(),
        )
    }

    fn intro(&self) -> String {
        let moved = self.proposed.len();
        match moved {
            0 => "No suggestions for these feeds. Choose a folder for any of them, then review the changes.".into(),
            1 => "A suggested folder for 1 feed. Change any folder, then review the changes.".into(),
            n => format!("Suggested folders for {n} feeds. Change any folder, then review the changes."),
        }
    }

    pub fn create(&self) -> Value {
        json!([{
            "version": "1.0",
            "createSurface": {
                "surfaceId": SURFACE_ID,
                "catalogId": gate::CATALOG_ID,
                "components": [
                    { "id": "root", "component": "Column",
                      "children": ["intro", "feeds", "divider", "review"] },
                    { "id": "intro", "component": "Text", "text": self.intro(), "variant": "body" },
                    { "id": "feeds", "component": "Column",
                      "children": { "componentId": "feed", "path": "/feeds" } },
                    { "id": "feed", "component": "Card", "child": "feedColumn",
                      "@accessibility": { "label": { "@path": "title" } } },
                    { "id": "feedColumn", "component": "Column",
                      "children": ["feedTitle", "feedFolder"] },
                    { "id": "feedTitle", "component": "Text", "text": { "@path": "title" }, "variant": "body" },
                    { "id": "feedFolder", "component": "ChoicePicker", "label": "Folder",
                      "variant": "mutuallyExclusive", "displayStyle": "chips",
                      "options": self.choices.options(),
                      "value": { "@path": "folder" } },
                    { "id": "divider", "component": "Divider" },
                    { "id": "review", "component": "Button", "child": "reviewLabel", "variant": "primary",
                      "action": { "event": { "name": REVIEW_EVENT, "context": review_context() } } },
                    { "id": "reviewLabel", "component": "Text", "text": "Review changes" }
                ],
                "dataModel": { "feeds": self.data() }
            }
        }])
    }

    /// The surface's feeds as a folder plan: every feed, from where the
    /// snapshot has it to the folder chosen. Checks each id, title and
    /// choice against the snapshot.
    pub fn plan_from(&self, feeds: &Value) -> Result<FolderPlan, Refusal> {
        let bad = |why: &str| gate::refuse("data", why.to_string());
        let list = feeds
            .as_array()
            .filter(|l| !l.is_empty() && l.len() <= MAX_FEEDS)
            .ok_or_else(|| bad("The feeds aren't in the expected form."))?;
        let mut moves = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for item in list {
            let object = item
                .as_object()
                .filter(|o| {
                    o.keys()
                        .all(|k| ["id", "title", "folder"].contains(&k.as_str()))
                })
                .ok_or_else(|| bad("A feed isn't in the expected form."))?;
            let id: i64 = object
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.parse().ok())
                .ok_or_else(|| bad("A feed has no valid id."))?;
            let feed = self
                .snapshot
                .feed(id)
                .ok_or_else(|| bad("A feed isn't in the library."))?;
            if !seen.insert(id) {
                return Err(bad("A feed appears twice."));
            }
            if let Some(title) = object.get("title") {
                if title.as_str() != Some(feed.title.as_str()) {
                    return Err(bad("A feed's title isn't the app's."));
                }
            }
            let target = match object
                .get("folder")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
            {
                Some([value]) => value
                    .as_str()
                    .and_then(|v| self.choices.target(v))
                    .ok_or_else(|| bad("A feed's folder isn't one of the choices."))?,
                _ => return Err(bad("Each feed needs one folder.")),
            };
            moves.push(FeedMove {
                feed_id: id,
                from: feed.folder_id,
                to: target.clone(),
            });
        }
        // The surface shows every feed in the snapshot, so a replacement
        // must hold exactly that set: none added, none dropped.
        if seen.len() != self.snapshot.feeds.len() {
            return Err(bad("The feeds aren't the ones the surface showed."));
        }
        let plan = FolderPlan { moves };
        organise::check(&self.snapshot, &plan).map_err(|e| gate::refuse("data", e.0))?;
        Ok(plan)
    }
}

impl Registration for FeedsSurface {
    type Data = ();

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
        &["intro"]
    }

    fn check_data(&self, path: Option<&str>, value: &Value) -> Result<(), Refusal> {
        let feeds = match path {
            None | Some("/") => value
                .as_object()
                .filter(|o| o.len() == 1)
                .and_then(|o| o.get("feeds"))
                .ok_or_else(|| gate::refuse("data", "The data model holds only the feeds."))?,
            Some("/feeds") => value,
            Some(_) => return Err(gate::refuse("data", "Updates may only replace the feeds.")),
        };
        // Every feed must carry its own title: the surface shows it.
        let all_titled = feeds
            .as_array()
            .is_some_and(|l| l.iter().all(|f| f.get("title").is_some()));
        if !all_titled {
            return Err(gate::refuse("data", "A feed's title isn't the app's."));
        }
        self.plan_from(feeds).map(|_| ())
    }
}

/// One line of the change set: a feed and where it would go. Names come
/// from Commonplace's snapshot and the checked plan.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiFolderChange {
    pub feed_id: i64,
    pub feed_title: String,
    pub from: String,
    pub to: String,
    /// The folder would be made.
    pub new_folder: bool,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiFolderChangeSet {
    pub revision: u64,
    pub rows: Vec<A2uiFolderChange>,
    pub unchanged: u32,
}

#[derive(Default)]
struct FeedsState {
    surface: Option<FeedsSurface>,
    gate: GateState,
    reviews: Reviews<FolderPlan>,
}

/// The "Organise Feeds" surface. One per screen; thread-safe.
#[derive(uniffi::Object, Default)]
pub struct A2uiFeeds {
    state: Mutex<FeedsState>,
}

#[uniffi::export]
impl A2uiFeeds {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        forge_runtime::logging::init();
        std::sync::Arc::new(Self::default())
    }

    pub fn surface_id(&self) -> String {
        SURFACE_ID.into()
    }

    /// A new surface for Commonplace's snapshot (its
    /// `organise_snapshot_json`), with the stand-in agent's proposal.
    pub fn start(&self, snapshot_json: String) -> Result<A2uiPayload, A2uiLabError> {
        let snapshot: OrganiseSnapshot =
            serde_json::from_str(&snapshot_json).map_err(|_| A2uiLabError::Refused {
                code: "data".into(),
                reason: "Commonplace's feeds aren't in the expected form.".into(),
            })?;
        if snapshot.feeds.is_empty() {
            return Err(A2uiLabError::Refused {
                code: "empty".into(),
                reason: "There are no feeds to organise here. Add some in Commonplace, or move feeds out of folders past the first eight."
                    .into(),
            });
        }
        let plan = organise::propose(&snapshot);
        let surface = FeedsSurface::new(snapshot, &plan);
        let mut state = lock(&self.state);
        let mut gate_state = GateState::default();
        let accepted = gate::check(&surface.create().to_string(), &surface, &mut gate_state)?;
        state.gate = gate_state;
        state.surface = Some(surface);
        let revision = state.reviews.accepted();
        logging::info!(target: "a2ui_lab", "feeds surface: {} moves proposed", plan.moves.len());
        Ok(A2uiPayload {
            messages: accepted.messages.iter().map(Value::to_string).collect(),
            revision,
        })
    }

    /// The review button's event: checks it, turns the surface's choices
    /// into a checked folder plan and returns the change set.
    pub fn review(
        &self,
        name: String,
        source_component_id: String,
        context_json: String,
    ) -> Result<A2uiFolderChangeSet, A2uiLabError> {
        if name != REVIEW_EVENT || source_component_id != REVIEW_COMPONENT {
            return Err(gate::refuse(
                "unregistered_event",
                "The surface sent an event the app hasn't registered.",
            )
            .into());
        }
        let context: Value =
            serde_json::from_str(&context_json).map_err(|_| A2uiLabError::Refused {
                code: "data".into(),
                reason: "The folders aren't in the expected form.".into(),
            })?;
        let feeds = context
            .as_object()
            .filter(|o| o.len() == 1)
            .and_then(|o| o.get("feeds"))
            .ok_or_else(|| A2uiLabError::Refused {
                code: "data".into(),
                reason: "The folders aren't in the expected form.".into(),
            })?;
        let mut state = lock(&self.state);
        let surface = state
            .surface
            .as_ref()
            .ok_or_else(|| A2uiLabError::Refused {
                code: "lifecycle".into(),
                reason: "There is no surface to review.".into(),
            })?;
        let plan = surface.plan_from(feeds)?;
        let moves =
            organise::check(&surface.snapshot, &plan).map_err(|e| gate::refuse("data", e.0))?;
        let rows: Vec<A2uiFolderChange> = moves
            .iter()
            .filter_map(|m| {
                let feed = surface.snapshot.feed(m.feed_id)?;
                let (to, new_folder) = match &m.to {
                    Target::Folder { id } => (surface.snapshot.place(Some(*id)), false),
                    Target::NoFolder => ("No folder".into(), false),
                    Target::NewFolder { name } => (name.clone(), true),
                };
                Some(A2uiFolderChange {
                    feed_id: m.feed_id,
                    feed_title: feed.title.clone(),
                    from: surface.snapshot.place(m.from),
                    to,
                    new_folder,
                })
            })
            .collect();
        let unchanged = (plan.moves.len() - rows.len()) as u32;
        let revision = state.reviews.reviewed(FolderPlan { moves });
        logging::info!(target: "a2ui_lab", "feeds reviewed: {} changes", rows.len());
        Ok(A2uiFolderChangeSet {
            revision,
            rows,
            unchanged,
        })
    }

    /// The person changed a folder on the surface.
    pub fn surface_edited(&self) -> bool {
        lock(&self.state).reviews.edited()
    }

    /// The plan reviewed at `revision`, as JSON for Commonplace's
    /// `apply_folder_plan_json`, if nothing changed since the review.
    pub fn confirm(&self, revision: u64) -> Result<String, A2uiLabError> {
        let plan = lock(&self.state).reviews.confirm(revision)?;
        logging::info!(target: "a2ui_lab", "feeds confirmed: {} moves", plan.moves.len());
        serde_json::to_string(&plan).map_err(|_| A2uiLabError::Refused {
            code: "data".into(),
            reason: "The plan couldn't be written.".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonplace_core::organise::{OrganiseFeed, OrganiseFolder};

    fn snapshot() -> OrganiseSnapshot {
        OrganiseSnapshot {
            feeds: vec![
                OrganiseFeed {
                    id: 1,
                    title: "Alpha".into(),
                    url: "https://alpha.example/feed".into(),
                    folder_id: None,
                },
                OrganiseFeed {
                    id: 2,
                    title: "Beta".into(),
                    url: "https://beta.example/feed".into(),
                    folder_id: Some(7),
                },
            ],
            folders: vec![OrganiseFolder {
                id: 7,
                name: "Reading".into(),
            }],
            other_names: Vec::new(),
        }
    }

    fn started() -> (std::sync::Arc<A2uiFeeds>, Value) {
        let feeds = A2uiFeeds::new();
        let payload = feeds
            .start(serde_json::to_string(&snapshot()).unwrap())
            .unwrap();
        let create: Value = serde_json::from_str(&payload.messages[0]).unwrap();
        (feeds, create)
    }

    fn context(feeds: Value) -> String {
        json!({ "feeds": feeds }).to_string()
    }

    #[test]
    fn the_surface_passes_its_gate_with_one_action() {
        let (_, create) = started();
        let components = create["createSurface"]["components"].as_array().unwrap();
        let actions: Vec<_> = components
            .iter()
            .filter(|c| c.get("action").is_some())
            .collect();
        assert_eq!(actions.len(), 1);
        let data = &create["createSurface"]["dataModel"]["feeds"];
        assert_eq!(data[0]["folder"], json!(["none"]));
        assert_eq!(data[1]["folder"], json!(["folder_7"]));
    }

    #[test]
    fn a_review_moves_only_what_changed_and_confirm_hands_back_the_plan() {
        let (feeds, _) = started();
        let set = feeds
            .review(
                REVIEW_EVENT.into(),
                REVIEW_COMPONENT.into(),
                context(json!([
                    { "id": "1", "title": "Alpha", "folder": ["folder_7"] },
                    { "id": "2", "title": "Beta", "folder": ["folder_7"] }
                ])),
            )
            .unwrap();
        assert_eq!(set.rows.len(), 1);
        assert_eq!(set.rows[0].feed_title, "Alpha");
        assert_eq!(set.rows[0].from, "No folder");
        assert_eq!(set.rows[0].to, "Reading");
        assert_eq!(set.unchanged, 1);
        let plan: FolderPlan = serde_json::from_str(&feeds.confirm(set.revision).unwrap()).unwrap();
        assert_eq!(
            plan.moves,
            vec![FeedMove {
                feed_id: 1,
                from: None,
                to: Target::Folder { id: 7 }
            }]
        );
        assert!(feeds.confirm(set.revision).is_err(), "once");
    }

    #[test]
    fn an_edit_after_review_makes_confirm_refuse() {
        let (feeds, _) = started();
        let set = feeds
            .review(
                REVIEW_EVENT.into(),
                REVIEW_COMPONENT.into(),
                context(json!([
                    { "id": "1", "title": "Alpha", "folder": ["folder_7"] },
                    { "id": "2", "title": "Beta", "folder": ["folder_7"] }
                ])),
            )
            .unwrap();
        assert!(feeds.surface_edited());
        assert!(matches!(
            feeds.confirm(set.revision),
            Err(A2uiLabError::Stale { .. })
        ));
    }

    #[test]
    fn bad_events_and_data_are_refused() {
        let (feeds, _) = started();
        let refused = |name: &str, source: &str, ctx: String| {
            feeds
                .review(name.into(), source.into(), ctx)
                .expect_err(name)
        };
        let ok = context(json!([
            { "id": "1", "title": "Alpha", "folder": ["none"] },
            { "id": "2", "title": "Beta", "folder": ["folder_7"] }
        ]));
        refused("deleteFeeds", REVIEW_COMPONENT, ok.clone());
        refused(REVIEW_EVENT, "intro", ok);
        for bad in [
            json!([{ "id": "99", "title": "Ghost", "folder": ["none"] }]),
            json!([{ "id": "1", "title": "Not Alpha", "folder": ["none"] }]),
            json!([{ "id": "1", "title": "Alpha", "folder": ["folder_99"] }]),
            json!([{ "id": "1", "title": "Alpha", "folder": ["none", "folder_7"] }]),
            json!([{ "id": "1", "title": "Alpha", "folder": ["none"], "url": "https://x" }]),
            json!([
                { "id": "1", "title": "Alpha", "folder": ["none"] },
                { "id": "1", "title": "Alpha", "folder": ["folder_7"] }
            ]),
            json!([]),
            // A feed dropped from the list.
            json!([{ "id": "1", "title": "Alpha", "folder": ["none"] }]),
        ] {
            refused(REVIEW_EVENT, REVIEW_COMPONENT, context(bad));
        }
    }

    #[test]
    fn the_gate_refuses_updates_that_change_the_choices_or_data() {
        let surface = FeedsSurface::new(snapshot(), &FolderPlan::default());
        let mut state = GateState::default();
        gate::check(&surface.create().to_string(), &surface, &mut state).unwrap();
        let hostile = [
            json!([{ "version": "1.0", "updateComponents": { "surfaceId": SURFACE_ID,
                "components": [{ "id": "feedFolder", "component": "ChoicePicker", "label": "Folder",
                    "displayStyle": "chips", "options": [{ "label": "Trash", "value": "trash" }],
                    "value": { "@path": "folder" } }] } }]),
            json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
                "path": "/feeds", "value": [{ "id": "1", "title": "Renamed", "folder": ["none"] }] } }]),
            json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
                "path": "/feeds", "value": [{ "id": "1", "folder": ["none"] }] } }]),
            json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
                "path": "/settings", "value": {} } }]),
            // A whole-model replace that drops a feed.
            json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
                "value": { "feeds": [{ "id": "1", "title": "Alpha", "folder": ["none"] }] } } }]),
            json!([{ "version": "1.0", "updateComponents": { "surfaceId": SURFACE_ID,
                "components": [{ "id": "review", "component": "Button", "child": "reviewLabel",
                    "action": { "event": { "name": "proposeReadingPlan",
                        "context": { "books": { "@path": "/books" } } } } }] } }]),
        ];
        for payload in hostile {
            let mut s = state.clone();
            assert!(
                gate::check(&payload.to_string(), &surface, &mut s).is_err(),
                "{payload}"
            );
        }
        // A data update that only changes a choice is fine.
        let ok = json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
            "path": "/feeds", "value": [
                { "id": "1", "title": "Alpha", "folder": ["folder_7"] },
                { "id": "2", "title": "Beta", "folder": ["folder_7"] }] } }]);
        assert!(gate::check(&ok.to_string(), &surface, &mut state).is_ok());
    }

    #[test]
    fn an_empty_library_is_refused_with_a_reason() {
        let feeds = A2uiFeeds::new();
        let empty = OrganiseSnapshot {
            feeds: Vec::new(),
            folders: Vec::new(),
            other_names: Vec::new(),
        };
        let err = feeds
            .start(serde_json::to_string(&empty).unwrap())
            .unwrap_err();
        assert!(matches!(err, A2uiLabError::Refused { code, .. } if code == "empty"));
    }
}
