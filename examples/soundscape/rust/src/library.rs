//! The music library: tracks, the recordings of each, and where their audio
//! lives, kept apart (see `docs/library.md`).
//!
//! - A **track** is the musical work: title, artist, album, number, genre,
//!   artwork. One track has one or more **recordings** (generated takes,
//!   mixes, imported files), and one of them is preferred.
//! - A recording's **audio source** says where its audio lives: on the take
//!   server, in a downloaded copy, or in a file the person imported.
//!
//! The phone keeps the last catalogue the take server sent (a snapshot),
//! the person's edits and their own artwork, in `library.json`.
//! [`build_library`] makes the browsable library from those three, so a
//! refresh never undoes an edit and the library browses without the server.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::listen::{self, SongDetail, SongSummary, Take};
use crate::model::SoundscapeError;
use crate::queue::QueueTrack;

pub const LIBRARY_VERSION: u32 = 1;
/// Prefix of a take-server track's id: `take-server:<slug>`.
pub const TAKE_SERVER_PREFIX: &str = "take-server:";
/// Prefix of an imported file's track id: `file:<hash>`.
pub const IMPORTED_PREFIX: &str = "file:";
/// Shown for a track with no artist.
pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
/// The longest title, artist, album or genre, in characters.
pub const MAX_FIELD_CHARS: usize = 200;

// ------------------------------------------------------------ what is kept

/// `library.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LibraryFile {
    /// The take server's catalogue as last received.
    #[serde(default)]
    pub snapshot: Option<Snapshot>,
    /// The person's changes, by track id.
    #[serde(default)]
    pub edits: BTreeMap<String, TrackEdit>,
    /// The person's artwork, by artist, album or track id: a file name in
    /// the artwork store.
    #[serde(default)]
    pub artwork: BTreeMap<String, String>,
    /// The person's own music files, by track id (`file:<hash>`).
    #[serde(default)]
    pub imported: BTreeMap<String, ImportedFile>,
}

impl LibraryFile {
    /// Every artwork file the library names, for sweeping the store.
    pub fn artwork_files(&self) -> std::collections::HashSet<String> {
        self.artwork
            .values()
            .cloned()
            .chain(self.imported.values().filter_map(|i| i.artwork.clone()))
            .collect()
    }

    /// Every imported music file, by its name in the music store.
    pub fn music_files(&self) -> std::collections::HashSet<String> {
        self.imported.values().map(|i| i.file.clone()).collect()
    }

    /// Whether the library has a track with this id now.
    pub fn has_track(&self, id: &str) -> bool {
        self.imported.contains_key(id)
            || self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.tracks.iter().any(|t| track_id(&t.slug) == id))
    }
}

/// A music file the person brought in, with the tags it had.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImportedFile {
    /// Its copy in the music store.
    pub file: String,
    /// The name it had when picked.
    pub file_name: String,
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub seconds: Option<f64>,
    /// Its cover picture, in the artwork store.
    #[serde(default)]
    pub artwork: Option<String>,
    pub added_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub server: String,
    pub received_ms: i64,
    pub tracks: Vec<SnapTrack>,
    /// Artwork the server has for artists and albums, as absolute URLs, by
    /// lower-case artist name and by `artist/album` (lower case).
    #[serde(default)]
    pub artist_art: BTreeMap<String, String>,
    #[serde(default)]
    pub album_art: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SnapTrack {
    pub slug: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub number: Option<u32>,
    pub artwork: Option<String>,
    pub pick: Option<String>,
    pub updated_ms: i64,
    pub takes: Vec<SnapTake>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SnapTake {
    pub id: String,
    pub audio_url: String,
    pub label: String,
    pub made_ms: i64,
    #[serde(default)]
    pub seconds: Option<f64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub lyrics: Option<String>,
    #[serde(default)]
    pub lufs: Option<f64>,
    #[serde(default)]
    pub stars: u8,
    #[serde(default)]
    pub note: String,
}

/// The person's changes to a track. `None` keeps what the source says; an
/// empty text clears it back to the source.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct TrackEdit {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
}

impl TrackEdit {
    /// Applies `change` over this edit: a text set to empty is dropped (the
    /// source's value shows again), a number of 0 likewise.
    pub fn merge(&mut self, change: TrackEdit) -> Result<(), SoundscapeError> {
        fn field(
            slot: &mut Option<String>,
            value: Option<String>,
            name: &str,
        ) -> Result<(), SoundscapeError> {
            if let Some(v) = value {
                let v = v.trim().to_string();
                if v.chars().count() > MAX_FIELD_CHARS {
                    return Err(SoundscapeError::range(name));
                }
                *slot = if v.is_empty() { None } else { Some(v) };
            }
            Ok(())
        }
        field(&mut self.title, change.title, "title")?;
        field(&mut self.artist, change.artist, "artist")?;
        field(&mut self.album, change.album, "album")?;
        field(&mut self.genre, change.genre, "genre")?;
        if let Some(n) = change.number {
            self.number = (n > 0).then_some(n);
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self == &TrackEdit::default()
    }
}

// ------------------------------------------------------------ what Swift sees

#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum AudioSource {
    /// Streamed from the take server.
    TakeServer { server: String },
    /// A file the person imported, by its name in the music store.
    LocalFile { path: String },
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingKind {
    /// Made by a music model (a JewGrass take).
    Generated,
    /// An ordinary audio file.
    Imported,
}

/// How a generated recording was made.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Generation {
    pub seed: Option<i64>,
    pub model: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
}

/// One audio version of a track.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Recording {
    pub id: String,
    pub label: String,
    pub kind: RecordingKind,
    pub source: AudioSource,
    /// What AVPlayer plays.
    pub audio_url: String,
    pub made_ms: i64,
    pub duration_seconds: Option<f64>,
    pub lufs: Option<f64>,
    pub stars: u8,
    pub note: String,
    /// The one that plays for the track.
    pub preferred: bool,
    pub generation: Option<Generation>,
    /// The words it sings, with section tags; takes of one song can differ.
    pub lyrics: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LibraryTrack {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    /// The artist's name, or [`UNKNOWN_ARTIST`].
    pub artist: String,
    pub album_id: Option<String>,
    pub album: Option<String>,
    pub number: Option<u32>,
    pub genre: Option<String>,
    /// The preferred recording's length, when known.
    pub duration_seconds: Option<f64>,
    /// The track's own artwork, else its album's.
    pub artwork_url: Option<String>,
    pub recording_count: u32,
    /// The recording that plays.
    pub preferred: Option<Recording>,
    /// True when one of its recordings was picked on purpose (not just the
    /// best rated or newest).
    pub picked: bool,
    /// The take server's slug, for the takes and comparison screen.
    pub slug: Option<String>,
    pub updated_ms: i64,
    /// True when the person changed its details.
    pub edited: bool,
    /// True when its preferred recording plays from the phone (downloaded
    /// or imported).
    pub downloaded: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LibraryAlbum {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist: String,
    pub artwork_url: Option<String>,
    pub track_count: u32,
    /// When its newest track changed.
    pub updated_ms: i64,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LibraryArtist {
    pub id: String,
    pub name: String,
    pub artwork_url: Option<String>,
    pub album_count: u32,
    pub track_count: u32,
}

/// The whole library, sorted: artists and albums by name, tracks by title.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct Library {
    pub artists: Vec<LibraryArtist>,
    pub albums: Vec<LibraryAlbum>,
    pub tracks: Vec<LibraryTrack>,
    /// When the server's catalogue was last received.
    pub refreshed_ms: Option<i64>,
}

/// A track with all its recordings, newest last, and the words that went
/// into generating it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TrackDetail {
    pub track: LibraryTrack,
    pub recordings: Vec<Recording>,
    pub caption: Option<String>,
    pub lyrics: Option<String>,
}

/// What a picture is for.
#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum ArtworkTarget {
    Artist { id: String },
    Album { id: String },
    Track { id: String },
}

impl ArtworkTarget {
    pub fn id(&self) -> &str {
        match self {
            ArtworkTarget::Artist { id }
            | ArtworkTarget::Album { id }
            | ArtworkTarget::Track { id } => id,
        }
    }
}

/// Search results, each list in library order.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct LibrarySearch {
    pub artists: Vec<LibraryArtist>,
    pub albums: Vec<LibraryAlbum>,
    pub tracks: Vec<LibraryTrack>,
}

// ------------------------------------------------------------ building

pub fn artist_id(name: &str) -> String {
    format!("artist:{}", name.trim().to_lowercase())
}

pub fn album_id(artist: Option<&str>, title: &str) -> String {
    format!(
        "album:{}/{}",
        artist.map(|a| a.trim().to_lowercase()).unwrap_or_default(),
        title.trim().to_lowercase()
    )
}

pub fn track_id(slug: &str) -> String {
    format!("{TAKE_SERVER_PREFIX}{slug}")
}

/// `file://` URL of a stored artwork file.
pub fn file_url(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    format!("file://{}", listen::encode(&text, true))
}

/// Where kept files are, as URLs Swift can load (or `None` if a file is
/// gone): stored artwork, and imported music in the music store.
pub struct Locate<'a> {
    pub artwork: &'a dyn Fn(&str) -> Option<String>,
    pub music: &'a dyn Fn(&str) -> Option<String>,
}

/// The library from what is kept.
pub fn build_library(file: &LibraryFile, locate: &Locate<'_>) -> Library {
    let built = build(file, locate);
    Library {
        artists: built.artists,
        albums: built.albums,
        tracks: built.tracks.into_iter().map(|(t, _)| t).collect(),
        refreshed_ms: file.snapshot.as_ref().map(|s| s.received_ms),
    }
}

/// One track with its recordings.
pub fn build_track(file: &LibraryFile, locate: &Locate<'_>, id: &str) -> Option<TrackDetail> {
    build(file, locate)
        .tracks
        .into_iter()
        .find(|(t, _)| t.id == id)
        .map(|(track, origin)| match origin {
            Origin::Server(snap) => {
                let server = file
                    .snapshot
                    .as_ref()
                    .map(|s| s.server.clone())
                    .unwrap_or_default();
                let recordings = recordings(&snap, &server);
                let caption = most_common(snap.takes.iter().map(|t| t.caption.clone()));
                let lyrics = most_common(snap.takes.iter().map(|t| t.lyrics.clone()));
                TrackDetail {
                    track,
                    recordings,
                    caption,
                    lyrics,
                }
            }
            Origin::Imported => TrackDetail {
                recordings: track.preferred.clone().into_iter().collect(),
                track,
                caption: None,
                lyrics: None,
            },
        })
}

/// Where a built track came from.
enum Origin {
    Server(SnapTrack),
    Imported,
}

struct Built {
    artists: Vec<LibraryArtist>,
    albums: Vec<LibraryAlbum>,
    tracks: Vec<(LibraryTrack, Origin)>,
}

/// One track's details as its source has them, before edits.
struct Source {
    id: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    number: Option<u32>,
    genre: Option<String>,
    artwork: Option<String>,
    recording_count: u32,
    preferred: Option<Recording>,
    picked: bool,
    slug: Option<String>,
    updated_ms: i64,
}

type ArtistEntry = (LibraryArtist, std::collections::BTreeSet<String>);

fn build(file: &LibraryFile, locate: &Locate<'_>) -> Built {
    let own_art = |id: &str| file.artwork.get(id).and_then(|name| (locate.artwork)(name));
    let empty = BTreeMap::new();
    let (artist_art, album_art) = match &file.snapshot {
        Some(s) => (&s.artist_art, &s.album_art),
        None => (&empty, &empty),
    };

    let mut albums: BTreeMap<String, LibraryAlbum> = BTreeMap::new();
    let mut artists: BTreeMap<String, ArtistEntry> = BTreeMap::new();
    let mut tracks = Vec::new();

    let mut add = |source: Source, origin: Origin| {
        let id = source.id;
        let edit = file.edits.get(&id).cloned().unwrap_or_default();
        let title = edit.title.clone().unwrap_or(source.title);
        let artist_name = edit.artist.clone().or(source.artist);
        let album_title = edit.album.clone().or(source.album);
        let number = edit.number.or(source.number);
        let a_id = artist_name.as_deref().map(artist_id);
        let al_id = album_title
            .as_deref()
            .map(|t| album_id(artist_name.as_deref(), t));

        if let Some(al_id) = &al_id {
            let album = albums.entry(al_id.clone()).or_insert_with(|| {
                let key = format!(
                    "{}/{}",
                    artist_name.as_deref().unwrap_or("").to_lowercase(),
                    album_title.as_deref().unwrap_or("").to_lowercase()
                );
                LibraryAlbum {
                    id: al_id.clone(),
                    title: album_title.clone().unwrap_or_default(),
                    artist_id: a_id.clone(),
                    artist: artist_name.clone().unwrap_or_else(|| UNKNOWN_ARTIST.into()),
                    artwork_url: own_art(al_id).or_else(|| album_art.get(&key).cloned()),
                    track_count: 0,
                    updated_ms: i64::MIN,
                }
            });
            album.track_count += 1;
            album.updated_ms = album.updated_ms.max(source.updated_ms);
            // An imported album takes the first cover one of its songs has.
            if album.artwork_url.is_none() {
                if let Origin::Imported = origin {
                    album.artwork_url = source.artwork.clone();
                }
            }
        }
        if let (Some(a_id), Some(name)) = (&a_id, &artist_name) {
            let (artist, album_ids) = artists.entry(a_id.clone()).or_insert_with(|| {
                (
                    LibraryArtist {
                        id: a_id.clone(),
                        name: name.clone(),
                        artwork_url: own_art(a_id)
                            .or_else(|| artist_art.get(&name.to_lowercase()).cloned()),
                        album_count: 0,
                        track_count: 0,
                    },
                    Default::default(),
                )
            });
            artist.track_count += 1;
            if let Some(al_id) = &al_id {
                album_ids.insert(al_id.clone());
            }
        }

        let album_cover = al_id
            .as_ref()
            .and_then(|al| albums.get(al))
            .and_then(|a| a.artwork_url.clone());
        let track = LibraryTrack {
            title,
            artist_id: a_id,
            artist: artist_name.unwrap_or_else(|| UNKNOWN_ARTIST.into()),
            album_id: al_id,
            album: album_title,
            number,
            genre: edit.genre.clone().or(source.genre),
            duration_seconds: source.preferred.as_ref().and_then(|r| r.duration_seconds),
            artwork_url: own_art(&id).or(source.artwork).or(album_cover),
            recording_count: source.recording_count,
            preferred: source.preferred,
            picked: source.picked,
            slug: source.slug,
            updated_ms: source.updated_ms,
            edited: !edit.is_empty(),
            downloaded: matches!(origin, Origin::Imported),
            id,
        };
        tracks.push((track, origin));
    };

    if let Some(snapshot) = &file.snapshot {
        for snap in &snapshot.tracks {
            let preferred = preferred_snap_take(snap);
            add(
                Source {
                    id: track_id(&snap.slug),
                    title: snap.title.clone(),
                    artist: snap.artist.clone(),
                    album: snap.album.clone(),
                    number: snap.number,
                    genre: None,
                    artwork: snap.artwork.clone(),
                    recording_count: snap.takes.len() as u32,
                    preferred: preferred.map(|t| recording(t, snap, &snapshot.server)),
                    picked: snap
                        .pick
                        .as_ref()
                        .is_some_and(|p| snap.takes.iter().any(|t| &t.id == p)),
                    slug: Some(snap.slug.clone()),
                    updated_ms: snap.updated_ms,
                },
                Origin::Server(snap.clone()),
            );
        }
    }
    for (id, imported) in &file.imported {
        // A copy that has gone missing isn't offered as playable.
        let Some(url) = (locate.music)(&imported.file) else {
            continue;
        };
        add(
            Source {
                id: id.clone(),
                title: imported.title.clone(),
                artist: imported.artist.clone(),
                album: imported.album.clone(),
                number: imported.number,
                genre: imported.genre.clone(),
                artwork: imported
                    .artwork
                    .as_deref()
                    .and_then(|n| (locate.artwork)(n)),
                recording_count: 1,
                preferred: Some(imported_recording(id, imported, url)),
                picked: false,
                slug: None,
                updated_ms: imported.added_ms,
            },
            Origin::Imported,
        );
    }

    let mut artists: Vec<LibraryArtist> = artists
        .into_values()
        .map(|(mut a, albums)| {
            a.album_count = albums.len() as u32;
            a
        })
        .collect();
    artists.sort_by_key(|a| sort_key(&a.name));
    let mut albums: Vec<LibraryAlbum> = albums.into_values().collect();
    albums.sort_by(|a, b| {
        sort_key(&a.title)
            .cmp(&sort_key(&b.title))
            .then(sort_key(&a.artist).cmp(&sort_key(&b.artist)))
    });
    tracks.sort_by(|(a, _), (b, _)| {
        sort_key(&a.title)
            .cmp(&sort_key(&b.title))
            .then(a.id.cmp(&b.id))
    });
    Built {
        artists,
        albums,
        tracks,
    }
}

fn imported_recording(id: &str, imported: &ImportedFile, url: String) -> Recording {
    Recording {
        id: id.to_string(),
        label: imported.file_name.clone(),
        kind: RecordingKind::Imported,
        source: AudioSource::LocalFile {
            path: imported.file.clone(),
        },
        audio_url: url,
        made_ms: imported.added_ms,
        duration_seconds: imported.seconds,
        lufs: None,
        stars: 0,
        note: String::new(),
        preferred: true,
        generation: None,
        lyrics: None,
    }
}

/// "The Lowbridge" sorts under L, case ignored.
fn sort_key(text: &str) -> String {
    let lower = text.trim().to_lowercase();
    lower
        .strip_prefix("the ")
        .map(str::to_string)
        .unwrap_or(lower)
}

/// The pick, else the most stars (newer wins a tie), else the newest, as
/// [`crate::queue::preferred_take`] does for a song.
fn preferred_snap_take(snap: &SnapTrack) -> Option<&SnapTake> {
    if let Some(pick) = &snap.pick {
        if let Some(t) = snap.takes.iter().find(|t| &t.id == pick) {
            return Some(t);
        }
    }
    snap.takes
        .iter()
        .max_by(|a, b| a.stars.cmp(&b.stars).then(a.made_ms.cmp(&b.made_ms)))
}

fn recording(take: &SnapTake, snap: &SnapTrack, server: &str) -> Recording {
    let preferred = preferred_snap_take(snap).is_some_and(|p| p.id == take.id);
    Recording {
        id: take.id.clone(),
        label: take.label.clone(),
        kind: RecordingKind::Generated,
        source: AudioSource::TakeServer {
            server: server.to_string(),
        },
        audio_url: take.audio_url.clone(),
        made_ms: take.made_ms,
        duration_seconds: take.seconds,
        lufs: take.lufs,
        stars: take.stars,
        note: take.note.clone(),
        preferred,
        generation: Some(Generation {
            seed: take.seed,
            model: take.model.clone(),
            bpm: take.bpm,
            key: take.key.clone(),
        }),
        lyrics: take.lyrics.clone(),
    }
}

fn recordings(snap: &SnapTrack, server: &str) -> Vec<Recording> {
    snap.takes
        .iter()
        .map(|t| recording(t, snap, server))
        .collect()
}

fn most_common(values: impl Iterator<Item = Option<String>>) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for v in values.flatten() {
        match counts.iter_mut().find(|(k, _)| *k == v) {
            Some((_, n)) => *n += 1,
            None => counts.push((v, 1)),
        }
    }
    let mut best: Option<(String, usize)> = None;
    for (k, n) in counts {
        if best.as_ref().is_none_or(|(_, m)| n > *m) {
            best = Some((k, n));
        }
    }
    best.map(|(k, _)| k)
}

/// Artists, albums and tracks whose names match `query` (case ignored;
/// tracks also by artist and album). An empty query finds nothing.
pub fn search(library: &Library, query: &str) -> LibrarySearch {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return LibrarySearch::default();
    }
    let has = |s: &str| s.to_lowercase().contains(&q);
    LibrarySearch {
        artists: library
            .artists
            .iter()
            .filter(|a| has(&a.name))
            .cloned()
            .collect(),
        albums: library
            .albums
            .iter()
            .filter(|a| has(&a.title) || has(&a.artist))
            .cloned()
            .collect(),
        tracks: library
            .tracks
            .iter()
            .filter(|t| {
                has(&t.title)
                    || has(&t.artist)
                    || t.album.as_deref().is_some_and(has)
                    || t.genre.as_deref().is_some_and(has)
            })
            .cloned()
            .collect(),
    }
}

/// An album's tracks in order: by number, then title.
pub fn album_tracks(library: &Library, album_id: &str) -> Vec<LibraryTrack> {
    let mut tracks: Vec<LibraryTrack> = library
        .tracks
        .iter()
        .filter(|t| t.album_id.as_deref() == Some(album_id))
        .cloned()
        .collect();
    tracks.sort_by(|a, b| {
        a.number
            .unwrap_or(u32::MAX)
            .cmp(&b.number.unwrap_or(u32::MAX))
            .then(sort_key(&a.title).cmp(&sort_key(&b.title)))
    });
    tracks
}

/// An artist's albums, newest first.
pub fn artist_albums(library: &Library, artist_id: &str) -> Vec<LibraryAlbum> {
    let mut albums: Vec<LibraryAlbum> = library
        .albums
        .iter()
        .filter(|a| a.artist_id.as_deref() == Some(artist_id))
        .cloned()
        .collect();
    albums.sort_by_key(|a| std::cmp::Reverse(a.updated_ms));
    albums
}

/// What the queue plays for a track: its preferred recording.
pub fn queue_track(track: &LibraryTrack) -> Option<QueueTrack> {
    let recording = track.preferred.clone()?;
    Some(QueueTrack {
        track_id: track.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        artwork_url: track.artwork_url.clone(),
        recording,
    })
}

/// One recording of a track, to play it rather than the preferred one.
pub fn queue_track_with(track: &LibraryTrack, recording: Recording) -> QueueTrack {
    QueueTrack {
        track_id: track.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        artwork_url: track.artwork_url.clone(),
        recording,
    }
}

// ------------------------------------------------------------ from the server

#[derive(Deserialize)]
struct CatalogJson {
    #[serde(default)]
    artists: Vec<CatalogArtist>,
    #[serde(default)]
    albums: Vec<CatalogAlbum>,
    #[serde(default)]
    tracks: Vec<CatalogTrack>,
}

#[derive(Deserialize)]
struct CatalogArtist {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    artwork: Option<String>,
}

#[derive(Deserialize)]
struct CatalogAlbum {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    artwork: Option<String>,
}

#[derive(Deserialize)]
struct CatalogTrack {
    slug: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    number: Option<u32>,
    #[serde(default)]
    artwork: Option<String>,
    #[serde(default)]
    pick: Option<String>,
    #[serde(default)]
    updated: f64,
    #[serde(default)]
    takes: Vec<serde_json::Value>,
}

fn absolute(server: &str, url: Option<String>) -> Option<String> {
    let url = url?.trim().to_string();
    if url.is_empty() {
        None
    } else if url.starts_with("http://") || url.starts_with("https://") {
        Some(url)
    } else {
        Some(format!("{server}/{}", url.trim_start_matches('/')))
    }
}

/// The snapshot from a `/api/catalog` answer.
pub fn parse_catalog(body: &[u8], server: &str, now: i64) -> Result<Snapshot, SoundscapeError> {
    let catalog: CatalogJson =
        serde_json::from_slice(body).map_err(|e| SoundscapeError::Server {
            reason: format!("the server's catalogue wasn't readable ({e})"),
        })?;
    let artist_names: BTreeMap<String, String> = catalog
        .artists
        .iter()
        .map(|a| (a.id.clone(), a.name.clone().unwrap_or_else(|| a.id.clone())))
        .collect();
    let album_titles: BTreeMap<String, String> = catalog
        .albums
        .iter()
        .map(|a| {
            (
                a.id.clone(),
                a.title.clone().unwrap_or_else(|| a.id.clone()),
            )
        })
        .collect();
    let artist_art = catalog
        .artists
        .iter()
        .filter_map(|a| {
            let name = artist_names.get(&a.id)?;
            Some((name.to_lowercase(), absolute(server, a.artwork.clone())?))
        })
        .collect();
    let album_art = catalog
        .albums
        .iter()
        .filter_map(|a| {
            let artist = a
                .artist
                .as_ref()
                .and_then(|id| artist_names.get(id))
                .map(|n| n.to_lowercase())
                .unwrap_or_default();
            let title = album_titles.get(&a.id)?.to_lowercase();
            Some((
                format!("{artist}/{title}"),
                absolute(server, a.artwork.clone())?,
            ))
        })
        .collect();
    let mut tracks = Vec::with_capacity(catalog.tracks.len());
    for t in catalog.tracks {
        // the takes are in /api/song's shape
        let song = serde_json::json!({
            "slug": t.slug, "title": t.title, "pick": t.pick, "takes": t.takes,
        });
        let detail = listen::parse_song(song.to_string().as_bytes(), server)?;
        let seconds: Vec<Option<f64>> = t
            .takes
            .iter()
            .map(|v| v.get("seconds").and_then(|s| s.as_f64()))
            .collect();
        tracks.push(SnapTrack {
            title: listen::non_empty(t.title).unwrap_or_else(|| t.slug.clone()),
            artist: t
                .artist
                .as_ref()
                .map(|id| artist_names.get(id).cloned().unwrap_or_else(|| id.clone())),
            album: t
                .album
                .as_ref()
                .map(|id| album_titles.get(id).cloned().unwrap_or_else(|| id.clone())),
            number: t.number,
            artwork: absolute(server, t.artwork),
            pick: t.pick,
            updated_ms: (t.updated * 1000.0) as i64,
            takes: snap_takes(&detail, &seconds),
            slug: t.slug,
        });
    }
    Ok(Snapshot {
        server: server.to_string(),
        received_ms: now,
        tracks,
        artist_art,
        album_art,
    })
}

fn snap_takes(detail: &SongDetail, seconds: &[Option<f64>]) -> Vec<SnapTake> {
    detail
        .takes
        .iter()
        .enumerate()
        .map(|(i, t)| snap_take(t, seconds.get(i).copied().flatten()))
        .collect()
}

fn snap_take(t: &Take, seconds: Option<f64>) -> SnapTake {
    SnapTake {
        id: t.id.clone(),
        audio_url: t.audio_url.clone(),
        label: t.label.clone(),
        made_ms: t.made_ms,
        seconds,
        seed: t.seed,
        model: t.model.clone(),
        bpm: t.bpm,
        key: t.key.clone(),
        caption: t.caption.clone(),
        lyrics: t.lyrics.clone(),
        lufs: t.lufs,
        stars: t.stars,
        note: t.note.clone(),
    }
}

/// A snapshot from an older server without `/api/catalog`: the song list
/// and each song's takes. The act is the artist and the album.
pub fn snapshot_from_songs(
    server: &str,
    now: i64,
    songs: &[(SongSummary, SongDetail)],
) -> Snapshot {
    Snapshot {
        server: server.to_string(),
        received_ms: now,
        tracks: songs
            .iter()
            .map(|(summary, detail)| SnapTrack {
                slug: summary.slug.clone(),
                title: detail.title.clone(),
                artist: detail.act.clone().or_else(|| summary.act.clone()),
                album: detail.act.clone().or_else(|| summary.act.clone()),
                number: None,
                artwork: None,
                pick: detail.takes.iter().find(|t| t.picked).map(|t| t.id.clone()),
                updated_ms: summary.updated_ms,
                takes: detail.takes.iter().map(|t| snap_take(t, None)).collect(),
            })
            .collect(),
        artist_art: BTreeMap::new(),
        album_art: BTreeMap::new(),
    }
}

/// Brings one song in the snapshot up to date after a pick, rating or note
/// (`detail` is the server's answer).
pub fn apply_song(snapshot: &mut Snapshot, detail: &SongDetail) {
    if let Some(track) = snapshot.tracks.iter_mut().find(|t| t.slug == detail.slug) {
        let seconds: BTreeMap<String, Option<f64>> = track
            .takes
            .iter()
            .map(|t| (t.id.clone(), t.seconds))
            .collect();
        track.pick = detail.takes.iter().find(|t| t.picked).map(|t| t.id.clone());
        track.takes = detail
            .takes
            .iter()
            .map(|t| snap_take(t, seconds.get(&t.id).copied().flatten()))
            .collect();
        track.updated_ms = track
            .updated_ms
            .max(detail.takes.iter().map(|t| t.made_ms).max().unwrap_or(0));
    }
}

/// The slug of a take-server track id.
pub fn slug_of(track_id: &str) -> Option<&str> {
    track_id.strip_prefix(TAKE_SERVER_PREFIX)
}

// ------------------------------------------------------------ your own files

/// Audio files that can be imported, by extension.
pub const MUSIC_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "aac", "alac", "mp4", "wav", "aif", "aiff", "aifc", "flac", "caf",
];
/// Files one import may bring.
pub const MAX_IMPORT_FILES: usize = 1_000;

/// A music file the person picked, with the tags the phone read from it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MusicFile {
    /// Where it is now (readable while the import runs).
    pub path: String,
    /// The name it had, with its extension.
    pub file_name: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub number: Option<u32>,
    pub genre: Option<String>,
    pub duration_seconds: Option<f64>,
    /// Its cover picture, written out as a JPEG or PNG file.
    pub artwork_path: Option<String>,
}

/// What an import did.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct ImportResult {
    /// Track ids of the files added.
    pub added: Vec<String>,
    /// Files already in the library.
    pub already: u32,
    /// Names of files that couldn't be imported, with why.
    pub refused: Vec<String>,
}

/// The extension of an importable file name, lower case.
pub fn music_extension(file_name: &str) -> Option<String> {
    let (_, extension) = file_name.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    MUSIC_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

/// An imported file's track id from its SHA-256 (hex).
pub fn imported_id(sha256: &str) -> String {
    let short: String = sha256.chars().take(16).collect();
    format!("{IMPORTED_PREFIX}{short}")
}

/// A tag as kept: trimmed, empty dropped, at most [`MAX_FIELD_CHARS`].
pub fn tag(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(MAX_FIELD_CHARS).collect())
}

/// What is kept for a picked file, stored as `stored`.
pub fn imported_file(
    file: &MusicFile,
    stored: String,
    artwork: Option<String>,
    now: i64,
) -> ImportedFile {
    let stem = file
        .file_name
        .rsplit_once('.')
        .map_or(file.file_name.as_str(), |(stem, _)| stem)
        .replace('_', " ");
    ImportedFile {
        file: stored,
        file_name: tag(Some(&file.file_name)).unwrap_or_else(|| "Imported".into()),
        title: tag(file.title.as_deref())
            .or_else(|| tag(Some(&stem)))
            .unwrap_or_else(|| "Untitled".into()),
        artist: tag(file.artist.as_deref()),
        album: tag(file.album.as_deref()),
        number: file.number.filter(|n| *n > 0),
        genre: tag(file.genre.as_deref()),
        seconds: file.duration_seconds.filter(|s| s.is_finite() && *s > 0.0),
        artwork,
        added_ms: now,
    }
}

// ------------------------------------------------------------ takes and versions

/// How a track's recordings are listed.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingOrder {
    /// The one that plays first, then most stars, then newest.
    Best,
    Newest,
    Oldest,
}

/// A track's recordings in `order`.
pub fn sort_recordings(mut recordings: Vec<Recording>, order: RecordingOrder) -> Vec<Recording> {
    match order {
        RecordingOrder::Best => recordings.sort_by(|a, b| {
            b.preferred
                .cmp(&a.preferred)
                .then(b.stars.cmp(&a.stars))
                .then(b.made_ms.cmp(&a.made_ms))
        }),
        RecordingOrder::Newest => recordings.sort_by_key(|r| std::cmp::Reverse(r.made_ms)),
        RecordingOrder::Oldest => recordings.sort_by_key(|r| r.made_ms),
    }
    recordings
}

/// Songs waiting for a pick: from the take server, with two or more takes
/// and none picked yet. Most recently changed first.
pub fn to_pick(library: &Library) -> Vec<LibraryTrack> {
    let mut tracks: Vec<LibraryTrack> = library
        .tracks
        .iter()
        .filter(|t| t.slug.is_some() && t.recording_count >= 2 && !t.picked)
        .cloned()
        .collect();
    tracks.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.id.cmp(&b.id)));
    tracks
}
