//! One song, several sources: merging songs that are the same song.
//!
//! The same song can reach the library more than once: from the take
//! server, as your own file, and from Jamendo or the Internet Archive.
//! Merging makes them one song whose recordings come from every source;
//! the merged songs leave the lists and their recordings show under the
//! song kept, each marked with where it came from. Your own file plays
//! when there is one. Nothing is deleted, so separating a song brings it
//! back as it was.
//!
//! Soundscape only offers merges ([`duplicates`]): songs linked to the same
//! MusicBrainz recording, or with the same title and artist and the same
//! length where both lengths are known. It never merges by itself.
//! Favourites, playlists, history and the songs wanted offline move to the
//! song kept when a song is merged into it.

use std::collections::{BTreeMap, HashSet};

use crate::collection::CollectionFile;
use crate::library::{
    merged_root, DuplicateReason, DuplicateSongs, Library, LibraryFile, LibraryTrack,
    RecordingKind, UNKNOWN_ARTIST,
};
use crate::model::SoundscapeError;
use crate::offline::OfflineFile;

/// The most songs merged in one go.
pub const MAX_MERGE: usize = 50;
/// Lengths closer than this count as the same, in seconds.
pub const SAME_LENGTH_SECONDS: f64 = 3.0;

/// Checks a merge of `ids` into `into` against the library as built:
/// `into` is a song in the lists, and every one of `ids` is a song other
/// than `into`.
pub fn check(
    file: &LibraryFile,
    library: &Library,
    into: &str,
    ids: &[String],
) -> Result<(), SoundscapeError> {
    if ids.is_empty() || ids.len() > MAX_MERGE {
        return Err(SoundscapeError::range("songs"));
    }
    if !library.tracks.iter().any(|t| t.id == into) {
        return Err(SoundscapeError::NotInLibrary { id: into.into() });
    }
    for id in ids {
        if id == into || merged_root(file, id).as_deref() == Some(into) {
            continue;
        }
        if !file.has_track(id) {
            return Err(SoundscapeError::NotInLibrary { id: id.clone() });
        }
    }
    Ok(())
}

/// Merges `ids` into `into` in the library file. A song already merged
/// elsewhere moves; songs merged into one of `ids` follow it.
pub fn merge(file: &mut LibraryFile, into: &str, ids: &[String]) {
    for id in ids.iter().filter(|id| id.as_str() != into) {
        file.merged.insert(id.clone(), into.to_string());
        for target in file.merged.values_mut() {
            if target == id {
                *target = into.to_string();
            }
        }
    }
    // Never a song merged into itself.
    file.merged.retain(|from, to| from != to);
    // Merges collapse: a song merged into one of `ids` now hangs off
    // `into`, and so does any choice of its recording.
    let merged = &file.merged;
    file.prefer.retain(|kept, _| !merged.contains_key(kept));
}

/// Brings `id` back as a song of its own. Merges collapse into the song
/// kept (see [`merge`]), so songs that were merged into `id` before it was
/// merged stay with the kept song; only `id` comes back.
pub fn separate(file: &mut LibraryFile, id: &str) -> bool {
    file.prefer.retain(|_, chosen| chosen != id);
    file.merged.remove(id).is_some()
}

/// Points favourites, playlists, history and offline choices that still
/// name a merged-away song at the song it was merged into. A merge saves
/// the library first and these after; if one of those later saves failed,
/// this finishes the move the next time the app opens. True when it
/// changed something.
pub fn repair_collection(file: &LibraryFile, collection: &mut CollectionFile) -> bool {
    let before = collection.clone();
    for from in file.merged.keys() {
        // Only into a song that is still there: a merge into a song that
        // has gone leaves the song on its own.
        if let Some(root) = merged_root(file, from).filter(|r| file.has_track(r)) {
            move_collection(collection, from, &root);
        }
    }
    *collection != before
}

/// [`repair_collection`] for the songs chosen for the phone.
pub fn repair_offline(file: &LibraryFile, offline: &mut OfflineFile) -> bool {
    let before = offline.wanted.clone();
    for from in file.merged.keys() {
        // Only into a song that is still there: a merge into a song that
        // has gone leaves the song on its own.
        if let Some(root) = merged_root(file, from).filter(|r| file.has_track(r)) {
            move_offline(offline, from, &root);
        }
    }
    offline.wanted != before
}

/// Moves favourites, playlist entries, history and play counts from `from`
/// to `to`.
pub fn move_collection(collection: &mut CollectionFile, from: &str, to: &str) {
    if let Some(at) = collection.favourites.remove(from) {
        let kept = collection.favourites.entry(to.to_string()).or_insert(at);
        *kept = (*kept).min(at);
    }
    for playlist in &mut collection.playlists {
        if playlist.track_ids.iter().any(|id| id == to) {
            // Already there: the merged song isn't listed twice.
            playlist.track_ids.retain(|id| id != from);
        } else {
            for id in &mut playlist.track_ids {
                if id == from {
                    *id = to.to_string();
                }
            }
        }
    }
    for play in &mut collection.history {
        if play.track_id == from {
            play.track_id = to.to_string();
        }
    }
    if let Some(theirs) = collection.plays.remove(from) {
        collection
            .plays
            .entry(to.to_string())
            .and_modify(|t| *t = t.join(theirs))
            .or_insert(theirs);
    }
}

/// A song wanted offline makes the song it was merged into wanted.
pub fn move_offline(offline: &mut OfflineFile, from: &str, to: &str) {
    if let Some(at) = offline.wanted.remove(from) {
        let kept = offline.wanted.entry(to.to_string()).or_insert(at);
        *kept = (*kept).min(at);
    }
}

/// The key a group of songs is remembered by when the person keeps them
/// apart: their ids, sorted.
pub fn group_key(ids: &[String]) -> String {
    let mut ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.join("\n")
}

/// Songs that look like one song, best first in each group, leaving out
/// groups the person kept apart.
pub fn duplicates(file: &LibraryFile, library: &Library) -> Vec<DuplicateSongs> {
    let mut grouped: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();

    let mut by_recording: BTreeMap<&str, Vec<&LibraryTrack>> = BTreeMap::new();
    for track in &library.tracks {
        if let Some(mbid) = &track.musicbrainz_id {
            by_recording.entry(mbid).or_default().push(track);
        }
    }
    for tracks in by_recording.into_values().filter(|t| t.len() > 1) {
        grouped.extend(tracks.iter().map(|t| t.id.as_str()));
        out.push(group(DuplicateReason::SameRecording, tracks));
    }

    let mut by_details: BTreeMap<(String, String), Vec<&LibraryTrack>> = BTreeMap::new();
    for track in &library.tracks {
        if grouped.contains(track.id.as_str()) || track.artist == UNKNOWN_ARTIST {
            continue;
        }
        let key = (fold(&track.title), fold(&track.artist));
        if key.0.is_empty() || key.1.is_empty() {
            continue;
        }
        by_details.entry(key).or_default().push(track);
    }
    for tracks in by_details.into_values() {
        for same in by_length(tracks) {
            if same.len() > 1 {
                out.push(group(DuplicateReason::SameDetails, same));
            }
        }
    }

    out.retain(|g| {
        let ids: Vec<String> = g.tracks.iter().map(|t| t.id.clone()).collect();
        !file.distinct.contains(&group_key(&ids))
    });
    out
}

/// Splits songs with the same title and artist by length: songs of
/// clearly different lengths (a live version, an edit) are different
/// songs. Known lengths are grouped first; a song of unknown length joins
/// them only when they all agree, so it never ties two different lengths
/// together.
fn by_length(tracks: Vec<&LibraryTrack>) -> Vec<Vec<&LibraryTrack>> {
    let (mut known, unknown): (Vec<&LibraryTrack>, Vec<&LibraryTrack>) =
        tracks.into_iter().partition(|t| seconds(t).is_some());
    known.sort_by(|a, b| {
        seconds(a)
            .unwrap_or(0.0)
            .total_cmp(&seconds(b).unwrap_or(0.0))
    });
    let mut groups: Vec<Vec<&LibraryTrack>> = Vec::new();
    for track in known {
        match groups.last_mut() {
            Some(last) if same_length(last[0], track) => last.push(track),
            _ => groups.push(vec![track]),
        }
    }
    match groups.len() {
        0 => groups.push(unknown),
        1 => groups[0].extend(unknown),
        // Unknown lengths can't say which group they belong to.
        _ => {}
    }
    groups
}

fn seconds(track: &LibraryTrack) -> Option<f64> {
    track.duration_seconds
}

fn group(reason: DuplicateReason, mut tracks: Vec<&LibraryTrack>) -> DuplicateSongs {
    tracks.sort_by_key(|t| (rank(t), t.updated_ms, t.id.clone()));
    DuplicateSongs {
        reason,
        tracks: tracks.into_iter().cloned().collect(),
    }
}

/// Which song to keep first: the take server's, then a catalogue's, then
/// your file.
fn rank(track: &LibraryTrack) -> u8 {
    match track.preferred.as_ref().map(|r| &r.kind) {
        Some(RecordingKind::Generated) => 0,
        Some(RecordingKind::Catalogue) => 1,
        _ => 2,
    }
}

fn same_length(a: &LibraryTrack, b: &LibraryTrack) -> bool {
    match (a.duration_seconds, b.duration_seconds) {
        (Some(x), Some(y)) => (x - y).abs() <= SAME_LENGTH_SECONDS,
        _ => true,
    }
}

/// Letters and digits only, lower case: "Wise Up!" and "wise up" match.
fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
