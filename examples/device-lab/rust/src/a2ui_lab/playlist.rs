//! C4 of the app capability audit: Soundscape's "make me a playlist" drawn
//! as an A2UI surface (docs/research/app-capability-audit.md, section 3).
//!
//! The capability is Soundscape's (`soundscape_core::playlist_plan`): the
//! library snapshot, the typed plan, its check, and the apply step on
//! Soundscape's own core. This file only draws it: the proposed songs, each
//! with Include or Leave Out, and the review event. Swift carries the
//! snapshot from Soundscape and the confirmed plan back to it, as JSON of
//! these same Rust types; Soundscape checks the plan again before it makes
//! the playlist. Nothing here searches, downloads or plays.

use std::sync::Mutex;

use forge_runtime::{lock, logging};
use serde_json::{json, Value};
use soundscape_core::playlist_plan::{self, PlaylistPlan, PlaylistSnapshot};

use super::gate::{self, EventRule, GateState, Refusal, Registration};
use super::review::Reviews;
use super::{A2uiLabError, A2uiPayload};

pub const SURFACE_ID: &str = "playlist";
pub const REVIEW_EVENT: &str = "reviewPlaylist";
pub const REVIEW_COMPONENT: &str = "review";
const INCLUDE: &str = "include";
const LEAVE_OUT: &str = "leave_out";

fn review_context() -> Value {
    json!({ "tracks": { "@path": "/tracks" } })
}

/// The surface for one proposal: only the proposal's songs may appear.
pub struct PlaylistSurface {
    pub snapshot: PlaylistSnapshot,
    pub plan: PlaylistPlan,
}

impl PlaylistSurface {
    fn data(&self) -> Value {
        Value::Array(
            self.plan
                .track_ids
                .iter()
                .filter_map(|id| self.snapshot.track(id))
                .map(|t| {
                    json!({
                        "id": t.id,
                        "title": t.title,
                        "artist": t.artist,
                        "include": [INCLUDE],
                    })
                })
                .collect(),
        )
    }

    pub fn create(&self) -> Value {
        let intro = format!(
            "A proposed playlist, \u{201C}{}\u{201D}, from songs in your library. Leave out any song, then review it.",
            self.plan.name
        );
        json!([{
            "version": "1.0",
            "createSurface": {
                "surfaceId": SURFACE_ID,
                "catalogId": gate::CATALOG_ID,
                "components": [
                    { "id": "root", "component": "Column",
                      "children": ["intro", "tracks", "divider", "review"] },
                    { "id": "intro", "component": "Text", "text": intro, "variant": "body" },
                    { "id": "tracks", "component": "Column",
                      "children": { "componentId": "track", "path": "/tracks" } },
                    { "id": "track", "component": "Card", "child": "trackColumn",
                      "@accessibility": { "label": { "@path": "title" } } },
                    { "id": "trackColumn", "component": "Column",
                      "children": ["trackTitle", "trackArtist", "trackInclude"] },
                    { "id": "trackTitle", "component": "Text", "text": { "@path": "title" }, "variant": "body" },
                    { "id": "trackArtist", "component": "Text", "text": { "@path": "artist" }, "variant": "caption" },
                    { "id": "trackInclude", "component": "ChoicePicker", "label": "In the playlist",
                      "variant": "mutuallyExclusive", "displayStyle": "chips",
                      "options": [
                          { "label": "Include", "value": INCLUDE },
                          { "label": "Leave Out", "value": LEAVE_OUT }
                      ],
                      "value": { "@path": "include" } },
                    { "id": "divider", "component": "Divider" },
                    { "id": "review", "component": "Button", "child": "reviewLabel", "variant": "primary",
                      "action": { "event": { "name": REVIEW_EVENT, "context": review_context() } } },
                    { "id": "reviewLabel", "component": "Text", "text": "Review playlist" }
                ],
                "dataModel": { "tracks": self.data() }
            }
        }])
    }

    /// The surface's songs as a plan: the proposal's name and the songs
    /// still included, in the proposal's order. Each song must be one the
    /// proposal named, with its own title and artist.
    pub fn plan_from(&self, tracks: &Value) -> Result<(PlaylistPlan, u32), Refusal> {
        let bad = |why: &str| gate::refuse("data", why.to_string());
        let list = tracks
            .as_array()
            .filter(|l| l.len() == self.plan.track_ids.len())
            .ok_or_else(|| bad("The songs aren't the proposed ones."))?;
        let mut included = Vec::new();
        let mut left_out = 0;
        for (item, proposed) in list.iter().zip(&self.plan.track_ids) {
            let object = item
                .as_object()
                .filter(|o| {
                    o.keys()
                        .all(|k| ["id", "title", "artist", "include"].contains(&k.as_str()))
                })
                .ok_or_else(|| bad("A song isn't in the expected form."))?;
            if object.get("id").and_then(Value::as_str) != Some(proposed.as_str()) {
                return Err(bad("The songs aren't the proposed ones."));
            }
            let track = self
                .snapshot
                .track(proposed)
                .ok_or_else(|| bad("A song isn't in the library."))?;
            for (key, own) in [("title", &track.title), ("artist", &track.artist)] {
                if let Some(value) = object.get(key) {
                    if value.as_str() != Some(own.as_str()) {
                        return Err(bad("A song's details aren't the app's."));
                    }
                }
            }
            match object
                .get("include")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
            {
                Some([v]) if v == INCLUDE => included.push(proposed.clone()),
                Some([v]) if v == LEAVE_OUT => left_out += 1,
                _ => return Err(bad("Each song is either included or left out.")),
            }
        }
        if included.is_empty() {
            return Err(bad("Include at least one song."));
        }
        let plan = PlaylistPlan {
            name: self.plan.name.clone(),
            track_ids: included,
        };
        playlist_plan::check(&self.snapshot, &plan).map_err(|e| gate::refuse("data", e.0))?;
        Ok((plan, left_out))
    }
}

impl Registration for PlaylistSurface {
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
        let tracks = match path {
            None | Some("/") => value
                .as_object()
                .filter(|o| o.len() == 1)
                .and_then(|o| o.get("tracks"))
                .ok_or_else(|| gate::refuse("data", "The data model holds only the songs."))?,
            Some("/tracks") => value,
            Some(_) => return Err(gate::refuse("data", "Updates may only replace the songs.")),
        };
        let all_named = tracks.as_array().is_some_and(|l| {
            l.iter()
                .all(|t| t.get("title").is_some() && t.get("artist").is_some())
        });
        if !all_named {
            return Err(gate::refuse("data", "A song's details aren't the app's."));
        }
        // Showing every song left out is allowed while editing; only a
        // review needs one included.
        match self.plan_from(tracks) {
            Err(r) if r.reason == "Include at least one song." => Ok(()),
            other => other.map(|_| ()),
        }
    }
}

/// One song of the change set, from Soundscape's library.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiPlaylistSong {
    pub track_id: String,
    pub title: String,
    pub artist: String,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct A2uiPlaylistChangeSet {
    pub revision: u64,
    /// The playlist's name, from the checked plan.
    pub name: String,
    pub songs: Vec<A2uiPlaylistSong>,
    pub left_out: u32,
    /// The songs' total length, where known.
    pub duration_seconds: f64,
}

#[derive(Default)]
struct PlaylistState {
    surface: Option<PlaylistSurface>,
    gate: GateState,
    reviews: Reviews<PlaylistPlan>,
}

/// The "Make a Playlist" surface. One per screen; thread-safe.
#[derive(uniffi::Object, Default)]
pub struct A2uiPlaylist {
    state: Mutex<PlaylistState>,
}

#[uniffi::export]
impl A2uiPlaylist {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        forge_runtime::logging::init();
        std::sync::Arc::new(Self::default())
    }

    pub fn surface_id(&self) -> String {
        SURFACE_ID.into()
    }

    /// A new surface for Soundscape's library snapshot (its
    /// `playlist_snapshot_json`), with the stand-in agent's proposal.
    pub fn start(&self, snapshot_json: String) -> Result<A2uiPayload, A2uiLabError> {
        let snapshot: PlaylistSnapshot =
            serde_json::from_str(&snapshot_json).map_err(|_| A2uiLabError::Refused {
                code: "data".into(),
                reason: "Soundscape's library isn't in the expected form.".into(),
            })?;
        let plan = playlist_plan::propose(&snapshot).ok_or_else(|| A2uiLabError::Refused {
            code: "empty".into(),
            reason:
                "Soundscape's library has no songs to choose from. Add some in Soundscape first."
                    .into(),
        })?;
        playlist_plan::check(&snapshot, &plan).map_err(|e| gate::refuse("data", e.0))?;
        let surface = PlaylistSurface { snapshot, plan };
        let mut state = lock(&self.state);
        let mut gate_state = GateState::default();
        let accepted = gate::check(&surface.create().to_string(), &surface, &mut gate_state)?;
        logging::info!(
            target: "a2ui_lab",
            "playlist surface: {} songs proposed",
            surface.plan.track_ids.len()
        );
        state.gate = gate_state;
        state.surface = Some(surface);
        let revision = state.reviews.accepted();
        Ok(A2uiPayload {
            messages: accepted.messages.iter().map(Value::to_string).collect(),
            revision,
        })
    }

    pub fn review(
        &self,
        name: String,
        source_component_id: String,
        context_json: String,
    ) -> Result<A2uiPlaylistChangeSet, A2uiLabError> {
        if name != REVIEW_EVENT || source_component_id != REVIEW_COMPONENT {
            return Err(gate::refuse(
                "unregistered_event",
                "The surface sent an event the app hasn't registered.",
            )
            .into());
        }
        let malformed = || A2uiLabError::Refused {
            code: "data".into(),
            reason: "The songs aren't in the expected form.".into(),
        };
        let context: Value = serde_json::from_str(&context_json).map_err(|_| malformed())?;
        let tracks = context
            .as_object()
            .filter(|o| o.len() == 1)
            .and_then(|o| o.get("tracks"))
            .ok_or_else(malformed)?;
        let mut state = lock(&self.state);
        let surface = state
            .surface
            .as_ref()
            .ok_or_else(|| A2uiLabError::Refused {
                code: "lifecycle".into(),
                reason: "There is no surface to review.".into(),
            })?;
        let (plan, left_out) = surface.plan_from(tracks)?;
        let songs: Vec<A2uiPlaylistSong> = plan
            .track_ids
            .iter()
            .filter_map(|id| surface.snapshot.track(id))
            .map(|t| A2uiPlaylistSong {
                track_id: t.id.clone(),
                title: t.title.clone(),
                artist: t.artist.clone(),
            })
            .collect();
        let duration_seconds = plan
            .track_ids
            .iter()
            .filter_map(|id| surface.snapshot.track(id)?.duration_seconds)
            .sum();
        let name = plan.name.clone();
        let revision = state.reviews.reviewed(plan);
        logging::info!(target: "a2ui_lab", "playlist reviewed: {} songs", songs.len());
        Ok(A2uiPlaylistChangeSet {
            revision,
            name,
            songs,
            left_out,
            duration_seconds,
        })
    }

    pub fn surface_edited(&self) -> bool {
        lock(&self.state).reviews.edited()
    }

    /// The plan reviewed at `revision`, as JSON for Soundscape's
    /// `apply_playlist_plan_json`, if nothing changed since the review.
    pub fn confirm(&self, revision: u64) -> Result<String, A2uiLabError> {
        let plan = lock(&self.state).reviews.confirm(revision)?;
        logging::info!(target: "a2ui_lab", "playlist confirmed: {} songs", plan.track_ids.len());
        serde_json::to_string(&plan).map_err(|_| A2uiLabError::Refused {
            code: "data".into(),
            reason: "The plan couldn't be written.".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundscape_core::playlist_plan::SnapshotTrack;

    fn snapshot() -> PlaylistSnapshot {
        let track = |id: &str, artist: &str| SnapshotTrack {
            id: id.into(),
            title: format!("Song {id}"),
            artist: artist.into(),
            genre: Some("Folk".into()),
            duration_seconds: Some(100.0),
        };
        PlaylistSnapshot {
            tracks: vec![track("a", "Ann"), track("b", "Bob"), track("c", "Ann")],
        }
    }

    fn started() -> std::sync::Arc<A2uiPlaylist> {
        let p = A2uiPlaylist::new();
        p.start(serde_json::to_string(&snapshot()).unwrap())
            .unwrap();
        p
    }

    fn song(id: &str, artist: &str, include: &str) -> Value {
        json!({ "id": id, "title": format!("Song {id}"), "artist": artist, "include": [include] })
    }

    fn review(p: &A2uiPlaylist, tracks: Value) -> Result<A2uiPlaylistChangeSet, A2uiLabError> {
        p.review(
            REVIEW_EVENT.into(),
            REVIEW_COMPONENT.into(),
            json!({ "tracks": tracks }).to_string(),
        )
    }

    #[test]
    fn leaving_a_song_out_and_confirming_hands_back_the_plan() {
        let p = started();
        let set = review(
            &p,
            json!([
                song("a", "Ann", INCLUDE),
                song("b", "Bob", LEAVE_OUT),
                song("c", "Ann", INCLUDE)
            ]),
        )
        .unwrap();
        assert_eq!(set.name, "Folk mix");
        assert_eq!(set.songs.len(), 2);
        assert_eq!(set.left_out, 1);
        assert_eq!(set.duration_seconds, 200.0);
        let plan: PlaylistPlan = serde_json::from_str(&p.confirm(set.revision).unwrap()).unwrap();
        assert_eq!(plan.track_ids, ["a", "c"]);
        assert!(p.confirm(set.revision).is_err());
    }

    #[test]
    fn songs_the_proposal_didnt_name_are_refused() {
        let p = started();
        for bad in [
            json!([song("a", "Ann", INCLUDE), song("b", "Bob", INCLUDE)]),
            json!([
                song("a", "Ann", INCLUDE),
                song("x", "Bob", INCLUDE),
                song("c", "Ann", INCLUDE)
            ]),
            json!([
                song("c", "Ann", INCLUDE),
                song("b", "Bob", INCLUDE),
                song("a", "Ann", INCLUDE)
            ]),
            json!([
                song("a", "Eve", INCLUDE),
                song("b", "Bob", INCLUDE),
                song("c", "Ann", INCLUDE)
            ]),
            json!([
                song("a", "Ann", "maybe"),
                song("b", "Bob", INCLUDE),
                song("c", "Ann", INCLUDE)
            ]),
            json!([
                song("a", "Ann", LEAVE_OUT),
                song("b", "Bob", LEAVE_OUT),
                song("c", "Ann", LEAVE_OUT)
            ]),
        ] {
            assert!(review(&p, bad.clone()).is_err(), "{bad}");
        }
        assert!(p
            .review("playNow".into(), REVIEW_COMPONENT.into(), "{}".into())
            .is_err());
    }

    #[test]
    fn an_edit_after_review_makes_confirm_refuse() {
        let p = started();
        let all = json!([
            song("a", "Ann", INCLUDE),
            song("b", "Bob", INCLUDE),
            song("c", "Ann", INCLUDE)
        ]);
        let set = review(&p, all).unwrap();
        assert!(p.surface_edited());
        assert!(matches!(
            p.confirm(set.revision),
            Err(A2uiLabError::Stale { .. })
        ));
    }

    #[test]
    fn the_gate_refuses_a_song_swapped_in_by_an_update() {
        let surface = PlaylistSurface {
            snapshot: snapshot(),
            plan: playlist_plan::propose(&snapshot()).unwrap(),
        };
        let mut state = GateState::default();
        gate::check(&surface.create().to_string(), &surface, &mut state).unwrap();
        let swapped = json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
            "path": "/tracks", "value": [song("x", "Ann", INCLUDE)] } }]);
        assert!(gate::check(&swapped.to_string(), &surface, &mut state.clone()).is_err());
        let all_out = json!([{ "version": "1.0", "updateDataModel": { "surfaceId": SURFACE_ID,
            "path": "/tracks", "value": [song("a", "Ann", LEAVE_OUT), song("b", "Bob", LEAVE_OUT), song("c", "Ann", LEAVE_OUT)] } }]);
        assert!(gate::check(&all_out.to_string(), &surface, &mut state).is_ok());
    }

    #[test]
    fn an_empty_library_is_refused_with_a_reason() {
        let p = A2uiPlaylist::new();
        let err = p
            .start(serde_json::to_string(&PlaylistSnapshot::default()).unwrap())
            .unwrap_err();
        assert!(matches!(err, A2uiLabError::Refused { code, .. } if code == "empty"));
    }
}
