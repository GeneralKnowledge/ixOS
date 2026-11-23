//! "Make me a playlist": a typed proposal of a name and songs already in
//! the library, the check that turns it into a playlist, and (in `lib.rs`)
//! the apply step that runs it through `create_playlist`.
//!
//! This is Soundscape's own capability, with nothing of A2UI in it
//! (docs/research/app-capability-audit.md, 4.2). Device Lab's A2UI Lab draws
//! a surface for it.
//!
//! A plan only names songs in the library: it searches no catalogue,
//! downloads nothing and plays nothing. Applying re-checks it against the
//! library as it is then.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Library, MAX_NAME_CHARS};

/// The most songs one plan may hold.
pub const MAX_TRACKS: usize = 25;
/// How many songs the stand-in proposal picks.
pub const PROPOSED_TRACKS: usize = 12;
/// The most library songs a snapshot carries.
pub const MAX_SNAPSHOT: usize = 500;

/// The library's songs, as far as a playlist plan goes. No artwork (remote
/// addresses) and no recordings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlaylistSnapshot {
    pub tracks: Vec<SnapshotTrack>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SnapshotTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub genre: Option<String>,
    pub duration_seconds: Option<f64>,
}

/// A proposal: a name and songs in order.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlaylistPlan {
    pub name: String,
    pub track_ids: Vec<String>,
}

/// Why a plan was refused. Never quotes a song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError(pub String);

fn err(text: impl Into<String>) -> PlanError {
    PlanError(text.into())
}

pub fn snapshot(library: &Library) -> PlaylistSnapshot {
    PlaylistSnapshot {
        tracks: library
            .tracks
            .iter()
            .take(MAX_SNAPSHOT)
            .map(|t| SnapshotTrack {
                id: t.id.clone(),
                title: t.title.clone(),
                artist: t.artist.clone(),
                genre: t.genre.clone().filter(|g| !g.trim().is_empty()),
                duration_seconds: t.duration_seconds,
            })
            .collect(),
    }
}

impl PlaylistSnapshot {
    pub fn track(&self, id: &str) -> Option<&SnapshotTrack> {
        self.tracks.iter().find(|t| t.id == id)
    }
}

/// Checks `plan`: a clean name, one to [`MAX_TRACKS`] distinct songs, all
/// in the library. Returns the name cleaned and the songs in order.
pub fn check<'a>(
    snapshot: &'a PlaylistSnapshot,
    plan: &PlaylistPlan,
) -> Result<(String, Vec<&'a SnapshotTrack>), PlanError> {
    let name = plan.name.trim();
    if name.is_empty() {
        return Err(err("A playlist needs a name."));
    }
    if name.chars().count() > MAX_NAME_CHARS as usize {
        return Err(err(format!(
            "Playlist names are at most {MAX_NAME_CHARS} characters."
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(err("A playlist name can't hold control characters."));
    }
    if plan.track_ids.is_empty() || plan.track_ids.len() > MAX_TRACKS {
        return Err(err(format!(
            "A playlist plan holds one to {MAX_TRACKS} songs."
        )));
    }
    let mut seen = BTreeSet::new();
    let mut tracks = Vec::new();
    for id in &plan.track_ids {
        if !seen.insert(id.as_str()) {
            return Err(err("A song appears twice."));
        }
        tracks.push(
            snapshot
                .track(id)
                .ok_or_else(|| err("A song isn't in the library."))?,
        );
    }
    Ok((name.to_string(), tracks))
}

/// A proposed name that passes [`check`]: no control characters (a
/// catalogue's artist or genre may hold them), within the name limit.
fn proposed_name(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    // cut: a proposed name from a long genre or artist, kept to the name
    // limit; the person sees the songs in full on the surface and the review
    let cut: String = clean.trim().chars().take(MAX_NAME_CHARS as usize).collect();
    cut.trim_end().to_string()
}

/// The stand-in for a model: the library's most common genre with at least
/// three songs ("Folk mix"), else its most common artist ("More of ..."),
/// up to [`PROPOSED_TRACKS`] songs in library order. `None` for a library
/// with nothing to go on.
pub fn propose(snapshot: &PlaylistSnapshot) -> Option<PlaylistPlan> {
    fn most_common<'a>(keys: impl Iterator<Item = &'a str>) -> Option<(&'a str, usize)> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for key in keys {
            *counts.entry(key).or_default() += 1;
        }
        // Highest count; the first name alphabetically on a tie.
        counts
            .into_iter()
            .fold(None, |best: Option<(&str, usize)>, (k, n)| match best {
                Some((_, m)) if m >= n => best,
                _ => Some((k, n)),
            })
    }
    let pick = |name: String, keep: &dyn Fn(&SnapshotTrack) -> bool| PlaylistPlan {
        name,
        track_ids: snapshot
            .tracks
            .iter()
            .filter(|t| keep(t))
            .take(PROPOSED_TRACKS)
            .map(|t| t.id.clone())
            .collect(),
    };
    if let Some((genre, n)) = most_common(snapshot.tracks.iter().filter_map(|t| t.genre.as_deref()))
    {
        if n >= 3 {
            let genre = genre.to_string();
            let name = proposed_name(&format!("{genre} mix"));
            return Some(pick(name, &|t| t.genre.as_deref() == Some(genre.as_str())));
        }
    }
    let (artist, _) = most_common(snapshot.tracks.iter().map(|t| t.artist.as_str()))?;
    let artist = artist.to_string();
    let name = proposed_name(&format!("More of {artist}"));
    Some(pick(name, &|t| t.artist == artist))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, artist: &str, genre: Option<&str>) -> SnapshotTrack {
        SnapshotTrack {
            id: id.into(),
            title: format!("Song {id}"),
            artist: artist.into(),
            genre: genre.map(Into::into),
            duration_seconds: Some(180.0),
        }
    }

    fn snap() -> PlaylistSnapshot {
        PlaylistSnapshot {
            tracks: vec![
                track("a", "Ann", Some("Folk")),
                track("b", "Bob", Some("Folk")),
                track("c", "Ann", Some("Jazz")),
                track("d", "Ann", Some("Folk")),
            ],
        }
    }

    #[test]
    fn the_stand_in_picks_the_common_genre_then_the_common_artist() {
        let plan = propose(&snap()).unwrap();
        assert_eq!(plan.name, "Folk mix");
        assert_eq!(plan.track_ids, ["a", "b", "d"]);
        assert!(check(&snap(), &plan).is_ok());

        let mut s = snap();
        for t in &mut s.tracks {
            t.genre = None;
        }
        let plan = propose(&s).unwrap();
        assert_eq!(plan.name, "More of Ann");
        assert_eq!(plan.track_ids, ["a", "c", "d"]);
        assert!(propose(&PlaylistSnapshot::default()).is_none());

        // A long genre still gives a name the check accepts.
        let mut s = snap();
        for t in &mut s.tracks {
            t.genre = Some("g".repeat(200));
        }
        let plan = propose(&s).unwrap();
        assert_eq!(plan.name.chars().count(), MAX_NAME_CHARS as usize);
        assert!(check(&s, &plan).is_ok());

        // A catalogue artist with a control character still gives a name
        // the check accepts.
        let mut s = snap();
        for t in &mut s.tracks {
            t.genre = None;
            t.artist = "Ann\nBand".into();
        }
        let plan = propose(&s).unwrap();
        assert_eq!(plan.name, "More of Ann Band");
        assert!(check(&s, &plan).is_ok());
    }

    #[test]
    fn refusals() {
        let plan = |name: &str, ids: &[&str]| PlaylistPlan {
            name: name.into(),
            track_ids: ids.iter().map(|s| s.to_string()).collect(),
        };
        for bad in [
            plan(" ", &["a"]),
            plan(&"x".repeat(61), &["a"]),
            plan("a\nb", &["a"]),
            plan("Ok", &[]),
            plan("Ok", &["a", "a"]),
            plan("Ok", &["ghost"]),
        ] {
            assert!(check(&snap(), &bad).is_err(), "{bad:?}");
        }
        let many: Vec<String> = (0..26).map(|i| i.to_string()).collect();
        let s = PlaylistSnapshot {
            tracks: many.iter().map(|id| track(id, "A", None)).collect(),
        };
        let p = PlaylistPlan {
            name: "Ok".into(),
            track_ids: many,
        };
        assert!(check(&s, &p).is_err());
        let s = snap();
        let (name, tracks) = check(&s, &plan("  Mine ", &["d", "a"])).unwrap();
        assert_eq!(name, "Mine");
        assert_eq!(tracks[0].id, "d");
    }
}
