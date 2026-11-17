//! Your collection: favourites, playlists and listening history (see
//! `docs/collection.md`). Kept by library id in `collection.json`, apart
//! from the library, and shown against the library as it is now, so a
//! track that leaves the library is hidden, not forgotten.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::library::{Library, LibraryAlbum, LibraryArtist, LibraryTrack};
use crate::model::{clean_name, SoundscapeError};

pub const COLLECTION_VERSION: u32 = 1;
pub const MAX_PLAYLISTS: usize = 500;
pub const MAX_PLAYLIST_TRACKS: usize = 5_000;
/// Plays kept in the history, newest last.
pub const MAX_HISTORY: usize = 2_000;
/// Different songs in Recently Played.
pub const RECENT_SONGS: usize = 50;
/// A song counts as played after this long, or half of it when shorter.
pub const PLAYED_AFTER_SECONDS: f64 = 30.0;

// ------------------------------------------------------------ what is kept

/// `collection.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct CollectionFile {
    /// Track, album and artist ids, with when they became favourites.
    #[serde(default)]
    pub favourites: BTreeMap<String, i64>,
    #[serde(default)]
    pub playlists: Vec<StoredPlaylist>,
    /// Newest last.
    #[serde(default)]
    pub history: Vec<Play>,
    #[serde(default)]
    pub next_playlist: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StoredPlaylist {
    pub id: u64,
    pub name: String,
    pub track_ids: Vec<String>,
    pub created_ms: i64,
    pub modified_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Play {
    pub track_id: String,
    pub recording_id: String,
    pub at_ms: i64,
}

impl CollectionFile {
    pub fn is_favourite(&self, id: &str) -> bool {
        self.favourites.contains_key(id)
    }

    pub fn set_favourite(&mut self, id: &str, on: bool, now: i64) -> Result<(), SoundscapeError> {
        if id.trim().is_empty() {
            return Err(SoundscapeError::NotInLibrary { id: id.into() });
        }
        if on {
            self.favourites.entry(id.to_string()).or_insert(now);
        } else {
            self.favourites.remove(id);
        }
        Ok(())
    }

    pub fn playlist_mut(&mut self, id: u64) -> Result<&mut StoredPlaylist, SoundscapeError> {
        self.playlists
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| no_playlist(id))
    }

    pub fn create_playlist(
        &mut self,
        name: &str,
        track_ids: Vec<String>,
        now: i64,
    ) -> Result<u64, SoundscapeError> {
        let name = clean_name(name)?;
        if self.playlists.len() >= MAX_PLAYLISTS {
            return Err(SoundscapeError::range("playlists"));
        }
        if track_ids.len() > MAX_PLAYLIST_TRACKS {
            return Err(SoundscapeError::range("playlist songs"));
        }
        self.next_playlist += 1;
        let id = self.next_playlist;
        self.playlists.push(StoredPlaylist {
            id,
            name,
            track_ids,
            created_ms: now,
            modified_ms: now,
        });
        Ok(id)
    }

    pub fn rename_playlist(
        &mut self,
        id: u64,
        name: &str,
        now: i64,
    ) -> Result<(), SoundscapeError> {
        let name = clean_name(name)?;
        let list = self.playlist_mut(id)?;
        list.name = name;
        list.modified_ms = now;
        Ok(())
    }

    pub fn delete_playlist(&mut self, id: u64) -> Result<(), SoundscapeError> {
        let before = self.playlists.len();
        self.playlists.retain(|p| p.id != id);
        if self.playlists.len() == before {
            return Err(no_playlist(id));
        }
        Ok(())
    }

    /// Adds songs at the end.
    pub fn add_to_playlist(
        &mut self,
        id: u64,
        track_ids: Vec<String>,
        now: i64,
    ) -> Result<(), SoundscapeError> {
        let list = self.playlist_mut(id)?;
        if list.track_ids.len() + track_ids.len() > MAX_PLAYLIST_TRACKS {
            return Err(SoundscapeError::range("playlist songs"));
        }
        list.track_ids.extend(track_ids);
        list.modified_ms = now;
        Ok(())
    }

    /// Replaces the order and contents, as the edit screen left them: the
    /// remaining entries by their place in the stored list. Places that
    /// don't exist are refused, so nothing is lost by mistake.
    pub fn arrange_playlist(
        &mut self,
        id: u64,
        places: Vec<u32>,
        now: i64,
    ) -> Result<(), SoundscapeError> {
        let list = self.playlist_mut(id)?;
        let mut seen = HashSet::new();
        let mut arranged = Vec::with_capacity(places.len());
        for place in places {
            let Some(track) = list.track_ids.get(place as usize) else {
                return Err(SoundscapeError::range("playlist place"));
            };
            if !seen.insert(place) {
                return Err(SoundscapeError::range("playlist place"));
            }
            arranged.push(track.clone());
        }
        list.track_ids = arranged;
        list.modified_ms = now;
        Ok(())
    }

    /// Notes a play, keeping the newest [`MAX_HISTORY`].
    pub fn record_play(&mut self, track_id: &str, recording_id: &str, now: i64) {
        self.history.push(Play {
            track_id: track_id.into(),
            recording_id: recording_id.into(),
            at_ms: now,
        });
        if self.history.len() > MAX_HISTORY {
            let extra = self.history.len() - MAX_HISTORY;
            self.history.drain(..extra);
        }
    }
}

fn no_playlist(id: u64) -> SoundscapeError {
    SoundscapeError::NotInLibrary {
        id: format!("playlist:{id}"),
    }
}

/// Whether `listened` seconds of a song `length` seconds long count as a
/// play: 30 seconds, or half of a shorter song.
#[uniffi::export]
pub fn counts_as_played(listened: f64, length: Option<f64>) -> bool {
    let needed = match length {
        Some(l) if l.is_finite() && l > 0.0 => PLAYED_AFTER_SECONDS.min(l / 2.0),
        _ => PLAYED_AFTER_SECONDS,
    };
    listened >= needed
}

// ------------------------------------------------------------ what is shown

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Playlist {
    pub id: u64,
    pub name: String,
    /// Songs in the library now.
    pub track_count: u32,
    /// Songs kept that the library no longer has.
    pub missing_count: u32,
    pub duration_seconds: f64,
    /// Up to four different pictures from its songs, for a mosaic.
    pub artwork_urls: Vec<String>,
    pub created_ms: i64,
    pub modified_ms: i64,
}

/// One song of a playlist with its place in the stored list (to remove or
/// move it).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PlaylistEntry {
    pub place: u32,
    pub track: LibraryTrack,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub entries: Vec<PlaylistEntry>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct RecentPlay {
    pub track: LibraryTrack,
    pub played_ms: i64,
}

/// Favourites, playlists and what was played, as the library has them now.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct MusicCollection {
    /// Every favourite id, including ones not in the library now, so a
    /// row can show its heart without asking.
    pub favourite_ids: Vec<String>,
    /// Newest favourite first.
    pub favourite_tracks: Vec<LibraryTrack>,
    pub favourite_albums: Vec<LibraryAlbum>,
    pub favourite_artists: Vec<LibraryArtist>,
    /// Most recently changed first.
    pub playlists: Vec<Playlist>,
    /// Different songs, most recently played first, at most [`RECENT_SONGS`].
    pub recently_played: Vec<RecentPlay>,
}

pub fn build_collection(file: &CollectionFile, library: &Library) -> MusicCollection {
    let mut by_newest: Vec<(&String, &i64)> = file.favourites.iter().collect();
    by_newest.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let pick = |id: &str| library.tracks.iter().find(|t| t.id == id).cloned();
    let favourite_tracks = by_newest.iter().filter_map(|(id, _)| pick(id)).collect();
    let favourite_albums = by_newest
        .iter()
        .filter_map(|(id, _)| library.albums.iter().find(|a| &a.id == *id).cloned())
        .collect();
    let favourite_artists = by_newest
        .iter()
        .filter_map(|(id, _)| library.artists.iter().find(|a| &a.id == *id).cloned())
        .collect();
    let mut playlists: Vec<Playlist> = file
        .playlists
        .iter()
        .map(|p| playlist(p, library))
        .collect();
    playlists.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(b.id.cmp(&a.id)));
    MusicCollection {
        favourite_ids: file.favourites.keys().cloned().collect(),
        favourite_tracks,
        favourite_albums,
        favourite_artists,
        playlists,
        recently_played: recently_played(file, library, RECENT_SONGS),
    }
}

/// Different songs, most recently played first.
pub fn recently_played(file: &CollectionFile, library: &Library, limit: usize) -> Vec<RecentPlay> {
    let mut seen = HashSet::new();
    let mut recent = Vec::new();
    for play in file.history.iter().rev() {
        if recent.len() >= limit {
            break;
        }
        if !seen.insert(play.track_id.as_str()) {
            continue;
        }
        if let Some(track) = library.tracks.iter().find(|t| t.id == play.track_id) {
            recent.push(RecentPlay {
                track: track.clone(),
                played_ms: play.at_ms,
            });
        }
    }
    recent
}

fn entries(p: &StoredPlaylist, library: &Library) -> Vec<PlaylistEntry> {
    p.track_ids
        .iter()
        .enumerate()
        .filter_map(|(place, id)| {
            library
                .tracks
                .iter()
                .find(|t| &t.id == id)
                .map(|track| PlaylistEntry {
                    place: place as u32,
                    track: track.clone(),
                })
        })
        .collect()
}

fn playlist(p: &StoredPlaylist, library: &Library) -> Playlist {
    let entries = entries(p, library);
    let mut artwork_urls: Vec<String> = Vec::new();
    for e in &entries {
        if artwork_urls.len() == 4 {
            break;
        }
        if let Some(url) = &e.track.artwork_url {
            if !artwork_urls.contains(url) {
                artwork_urls.push(url.clone());
            }
        }
    }
    Playlist {
        id: p.id,
        name: p.name.clone(),
        track_count: entries.len() as u32,
        missing_count: (p.track_ids.len() - entries.len()) as u32,
        duration_seconds: entries
            .iter()
            .filter_map(|e| e.track.duration_seconds)
            .sum(),
        artwork_urls,
        created_ms: p.created_ms,
        modified_ms: p.modified_ms,
    }
}

pub fn playlist_detail(
    file: &CollectionFile,
    library: &Library,
    id: u64,
) -> Result<PlaylistDetail, SoundscapeError> {
    let stored = file
        .playlists
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| no_playlist(id))?;
    Ok(PlaylistDetail {
        playlist: playlist(stored, library),
        entries: entries(stored, library),
    })
}
