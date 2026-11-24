//! Exploring Jamendo beyond one week's most popular tracks: artists to
//! discover, and each artist's releases as albums, EPs and singles.
//!
//! Many Jamendo songs are not in a proper album: an artist uploads one
//! single at a time. Soundscape groups them into *functional albums*:
//! singles of one artist that share the same cover art belong together,
//! and a song left on its own is a single. Jamendo gives every single its
//! own artwork address even when the picture is the same, so covers are
//! compared by the image files themselves (their SHA-256), fetched once.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, Request};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::catalogue::CatalogueTrack;
use crate::jamendo::{self, text, Ask, API};
use crate::library::tag;
use crate::model::SoundscapeError;

/// Artists asked for in one list.
pub const ARTISTS_PAGE: u32 = 30;
/// The most covers fetched to compare for one artist's page.
pub const MAX_COVERS: usize = 80;
/// The largest cover file compared; a larger one is not fetched.
pub const MAX_COVER_BYTES: usize = 2 * 1024 * 1024;
const COVER_TIMEOUT: Duration = Duration::from_secs(10);
/// An EP has at most this many songs, and lasts under [`EP_SECONDS`].
pub const EP_MAX_TRACKS: usize = 6;
pub const EP_SECONDS: f64 = 30.0 * 60.0;

/// A Jamendo artist.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct JamendoArtist {
    /// Jamendo's artist id (digits).
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    /// When they joined Jamendo (`2019-05-03`).
    pub joined: Option<String>,
}

/// Which artists to list.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum JamendoArtistList {
    /// This week's most popular artists.
    Popular,
    /// The newest artists on Jamendo.
    New,
}

/// What a release is, by its songs (the way Apple Music names them).
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseKind {
    Album,
    Ep,
    Single,
}

/// One release on an artist's page.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct JamendoRelease {
    /// `album:<id>` for Jamendo's own album, `cover:<sha256>` for singles
    /// grouped by their cover, `single:<track id>` for a single.
    pub id: String,
    pub title: String,
    pub kind: ReleaseKind,
    /// True when Soundscape grouped singles by their cover; Jamendo has no
    /// such album.
    pub grouped: bool,
    /// The earliest song's release date (`2019-05-03`).
    pub released: Option<String>,
    pub artwork_url: Option<String>,
    pub duration_seconds: f64,
    /// In order: Jamendo's track order, else by release date.
    pub tracks: Vec<CatalogueTrack>,
}

/// An artist and their releases, newest first.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct JamendoArtistPage {
    pub artist: JamendoArtist,
    pub releases: Vec<JamendoRelease>,
}

/// The `/artists` request for `list`.
pub fn artists_url(client_id: &str, list: JamendoArtistList) -> String {
    let order = match list {
        JamendoArtistList::Popular => "popularity_week",
        JamendoArtistList::New => "joindate_desc",
    };
    format!(
        "{API}/artists/?client_id={}&format=json&limit={ARTISTS_PAGE}&order={order}",
        crate::listen::encode(client_id, false)
    )
}

/// The `/artists` request for one artist.
pub fn artist_url(client_id: &str, artist_id: &str) -> String {
    format!(
        "{API}/artists/?client_id={}&format=json&id={}",
        crate::listen::encode(client_id, false),
        crate::listen::encode(artist_id, false)
    )
}

/// An artist id as given, checked: digits only.
pub fn clean_artist_id(id: &str) -> Result<String, SoundscapeError> {
    let id = id.trim();
    if id.is_empty() || id.len() > 20 || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(SoundscapeError::BadAddress {
            reason: "that isn't a Jamendo artist".into(),
        });
    }
    Ok(id.to_string())
}

/// Artists to discover. Ones without a picture come last.
pub fn artists(
    http: &dyn HttpClient,
    client_id: &str,
    list: JamendoArtistList,
    cancel: &CancelHandle,
) -> Result<Vec<JamendoArtist>, SoundscapeError> {
    let mut found: Vec<JamendoArtist> =
        jamendo::results_at(http, &artists_url(client_id, list), cancel)?
            .iter()
            .filter_map(artist)
            .collect();
    // Stable: Jamendo's order is kept within each half.
    found.sort_by_key(|a| a.image_url.is_none());
    Ok(found)
}

fn artist(item: &Value) -> Option<JamendoArtist> {
    Some(JamendoArtist {
        id: clean_artist_id(&text(&item["id"])?).ok()?,
        name: tag(text(&item["name"]).as_deref())?,
        image_url: text(&item["image"]).filter(|u| u.starts_with("https://")),
        joined: text(&item["joindate"]),
    })
}

/// Covers already compared: artwork address to its SHA-256, so a page
/// opened again fetches nothing new.
#[derive(Default)]
pub struct Covers {
    known: Mutex<HashMap<String, String>>,
}

impl Covers {
    fn get(&self, url: &str) -> Option<String> {
        self.known
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(url)
            .cloned()
    }

    fn put(&self, url: &str, hash: String) {
        self.known
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(url.to_string(), hash);
    }
}

/// One artist's page: who they are and their releases. `artist_id` is
/// Jamendo's id; covers of singles are fetched (at most [`MAX_COVERS`])
/// to find the ones that share a picture.
pub fn artist_page(
    http: &dyn HttpClient,
    client_id: &str,
    artist_id: &str,
    covers: &Covers,
    cancel: &CancelHandle,
) -> Result<JamendoArtistPage, SoundscapeError> {
    let artist_id = clean_artist_id(artist_id)?;
    let items = jamendo::results(http, client_id, &Ask::Releases(artist_id.clone()), cancel)?;
    let named = jamendo::results_at(http, &artist_url(client_id, &artist_id), cancel)
        .ok()
        .and_then(|found| found.iter().find_map(artist));
    let artist = named
        .or_else(|| {
            let item = items.first()?;
            Some(JamendoArtist {
                id: artist_id.clone(),
                name: tag(text(&item["artist_name"]).as_deref())?,
                image_url: None,
                joined: None,
            })
        })
        .ok_or_else(|| SoundscapeError::Server {
            reason: "Jamendo has no songs by this artist".into(),
        })?;
    let songs = songs(&items);
    let mut hashes = HashMap::new();
    for url in loose_covers(&songs).into_iter().take(MAX_COVERS) {
        if cancel.is_cancelled() {
            break;
        }
        if let Some(hash) = cover_hash(http, &url, covers, cancel) {
            hashes.insert(url, hash);
        }
    }
    Ok(JamendoArtistPage {
        artist,
        releases: releases(songs, &hashes),
    })
}

/// The artist id of the Jamendo track `track_id` (`jamendo:<id>`).
pub fn artist_of(
    http: &dyn HttpClient,
    client_id: &str,
    track_id: &str,
    cancel: &CancelHandle,
) -> Result<String, SoundscapeError> {
    let number = jamendo::track_number(track_id).ok_or_else(|| SoundscapeError::BadAddress {
        reason: "this song isn't from Jamendo".into(),
    })?;
    jamendo::results(http, client_id, &Ask::Track(number.into()), cancel)?
        .iter()
        .find(|item| text(&item["id"]).as_deref() == Some(number))
        .and_then(|item| text(&item["artist_id"]))
        .and_then(|id| clean_artist_id(&id).ok())
        .ok_or_else(|| SoundscapeError::Server {
            reason: "Jamendo doesn't say who made this song".into(),
        })
}

fn cover_hash(
    http: &dyn HttpClient,
    url: &str,
    covers: &Covers,
    cancel: &CancelHandle,
) -> Option<String> {
    if let Some(hash) = covers.get(url) {
        return Some(hash);
    }
    let response = http
        .send_cancellable(&Request::get(url).timeout(COVER_TIMEOUT), cancel)
        .ok()?;
    if !(200..300).contains(&response.status)
        || response.body.is_empty()
        || response.body.len() > MAX_COVER_BYTES
    {
        return None;
    }
    let hash = hex(&Sha256::digest(&response.body));
    covers.put(url, hash.clone());
    Some(hash)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A song with what grouping needs from Jamendo's answer.
#[derive(Clone, Debug)]
pub struct Song {
    pub track: CatalogueTrack,
    /// Jamendo's album id, when the song is in one.
    pub album_id: Option<String>,
    pub album_name: Option<String>,
    pub released: Option<String>,
}

/// The songs in a `/tracks` answer, each once.
pub fn songs(items: &[Value]) -> Vec<Song> {
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .filter_map(|item| {
            let track = jamendo::track(item)?;
            Some(Song {
                album_id: text(&item["album_id"]).filter(|id| id != "0"),
                album_name: tag(text(&item["album_name"]).as_deref()),
                released: text(&item["releasedate"]).filter(|d| d.len() == 10),
                track,
            })
        })
        .filter(|s| seen.insert(s.track.id.clone()))
        .collect()
}

/// Album ids with two or more songs in `songs`: Jamendo's own albums.
fn real_albums(songs: &[Song]) -> BTreeMap<String, usize> {
    let mut count = BTreeMap::new();
    for s in songs {
        if let Some(id) = &s.album_id {
            *count.entry(id.clone()).or_insert(0) += 1;
        }
    }
    count.retain(|_, n| *n >= 2);
    count
}

/// The covers of songs not in a real album, each once, newest song first.
pub fn loose_covers(songs: &[Song]) -> Vec<String> {
    let albums = real_albums(songs);
    let mut urls = Vec::new();
    for s in songs {
        let loose = s
            .album_id
            .as_ref()
            .is_none_or(|id| !albums.contains_key(id));
        if let (true, Some(url)) = (loose, &s.track.artwork_url) {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
    }
    urls
}

/// `songs` as releases, newest first. Jamendo's albums of two or more
/// songs stay as they are. The other songs are grouped by cover: songs
/// whose artwork has the same SHA-256 in `hashes` (or the same address)
/// make one functional album, named by the name they share or else after
/// the earliest song; a song left alone is a single.
pub fn releases(songs: Vec<Song>, hashes: &HashMap<String, String>) -> Vec<JamendoRelease> {
    let albums = real_albums(&songs);
    // Groups keyed by release id, in first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Song>> = HashMap::new();
    for s in songs {
        let key = match &s.album_id {
            Some(id) if albums.contains_key(id) => format!("album:{id}"),
            _ => match s.track.artwork_url.as_ref() {
                Some(url) => format!(
                    "cover:{}",
                    hashes.get(url).cloned().unwrap_or_else(|| url.clone())
                ),
                None => format!("single:{}", s.track.id),
            },
        };
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(s);
    }
    let mut out: Vec<JamendoRelease> = order
        .into_iter()
        .filter_map(|key| {
            let songs = groups.remove(&key)?;
            Some(release(key, songs))
        })
        .collect();
    // Newest first; a release without a date goes last.
    out.sort_by(|a, b| match (&a.released, &b.released) {
        (Some(x), Some(y)) => y.cmp(x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    out
}

fn release(key: String, mut songs: Vec<Song>) -> JamendoRelease {
    let real = key.starts_with("album:");
    if real {
        songs.sort_by(|a, b| {
            a.track
                .number
                .unwrap_or(u32::MAX)
                .cmp(&b.track.number.unwrap_or(u32::MAX))
                .then_with(|| a.track.title.cmp(&b.track.title))
        });
    } else {
        // Oldest first, as the songs came out.
        songs.sort_by(|a, b| {
            a.released
                .cmp(&b.released)
                .then_with(|| a.track.title.cmp(&b.track.title))
        });
    }
    let duration: f64 = songs.iter().filter_map(|s| s.track.duration_seconds).sum();
    let kind = if songs.len() == 1 {
        ReleaseKind::Single
    } else if songs.len() <= EP_MAX_TRACKS && duration < EP_SECONDS {
        ReleaseKind::Ep
    } else {
        ReleaseKind::Album
    };
    let released = songs.iter().filter_map(|s| s.released.clone()).min();
    let first = &songs[0];
    let shared_name = first
        .album_name
        .clone()
        .filter(|name| songs.iter().all(|s| s.album_name.as_ref() == Some(name)));
    let title = if real || songs.len() > 1 {
        shared_name.unwrap_or_else(|| first.track.title.clone())
    } else {
        first.track.title.clone()
    };
    let id = if songs.len() == 1 && !real {
        format!("single:{}", first.track.id)
    } else {
        key
    };
    JamendoRelease {
        id,
        title,
        kind,
        grouped: !real && songs.len() > 1,
        released,
        artwork_url: first.track.artwork_url.clone(),
        duration_seconds: duration,
        tracks: songs.into_iter().map(|s| s.track).collect(),
    }
}
