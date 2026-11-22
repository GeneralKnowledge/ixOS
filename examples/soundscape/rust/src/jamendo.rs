//! Jamendo: independent music under Creative Commons licences, searched and
//! streamed through its API (v3.0, developer.jamendo.com).
//!
//! The API needs the person's own client ID (free for non-commercial apps).
//! Requests go through the app's Rust networking (URLSession on the phone).
//! Track ids become `jamendo:<id>`, which never change, so a track found
//! again is the same track. Each track carries its licence and whether
//! Jamendo allows downloading it.

use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, Request};
use serde_json::Value;

use crate::catalogue::{rights_from_licence, CatalogueTrack};
use crate::library::{tag, MAX_FIELD_CHARS};
use crate::model::SoundscapeError;

pub const PROVIDER: &str = "Jamendo";
/// Prefix of a Jamendo track's id: `jamendo:<id>`.
pub const PREFIX: &str = "jamendo:";
pub const API: &str = "https://api.jamendo.com/v3.0";
/// Results asked for in one search.
pub const PAGE: u32 = 50;
/// The longest search text, in characters.
pub const MAX_QUERY_CHARS: usize = 200;
/// The longest client ID accepted.
pub const MAX_CLIENT_ID_CHARS: usize = 64;
const TIMEOUT: Duration = Duration::from_secs(15);
/// The largest answer accepted.
const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;

/// A client ID as typed, tidied: letters and digits only. Empty forgets it.
pub fn clean_client_id(text: &str) -> Result<Option<String>, SoundscapeError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.len() > MAX_CLIENT_ID_CHARS || !text.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(SoundscapeError::BadAddress {
            reason: "a Jamendo client ID is letters and digits only".into(),
        });
    }
    Ok(Some(text.to_string()))
}

/// What to ask Jamendo's `/tracks` for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Tracks matching the text; this week's most popular when it is empty.
    Search(String),
    /// One artist's tracks (Jamendo's artist id), most played first.
    Artist(String),
    /// One genre's tracks (a Jamendo tag such as `jazz`), this week's most
    /// popular first.
    Genre(String),
    /// One track (Jamendo's track id), for its artist and genres.
    Track(String),
}

/// What "more from Jamendo" means for a track that is playing.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum JamendoMore {
    /// More by the same artist.
    Artist,
    /// More in the track's (first) genre.
    Genre,
}

/// Jamendo's tracks related to one track.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct JamendoMoreTracks {
    /// The artist's name, or the genre's.
    pub name: String,
    /// The tracks, best first, without the track asked about.
    pub tracks: Vec<CatalogueTrack>,
}

/// A genre to browse: Jamendo's tag and the name to show.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct JamendoGenre {
    pub tag: String,
    pub name: String,
}

/// The genres Discover offers, in the order shown. Tags are Jamendo's own
/// (`musicinfo.tags.genres`); `tags=jazz` was checked live on 2026-11-18.
pub const GENRES: &[(&str, &str)] = &[
    ("pop", "Pop"),
    ("rock", "Rock"),
    ("electronic", "Electronic"),
    ("hiphop", "Hip-Hop"),
    ("jazz", "Jazz"),
    ("classical", "Classical"),
    ("ambient", "Ambient"),
    ("chillout", "Chillout"),
    ("lounge", "Lounge"),
    ("folk", "Folk"),
    ("indie", "Indie"),
    ("metal", "Metal"),
    ("punk", "Punk"),
    ("blues", "Blues"),
    ("reggae", "Reggae"),
    ("latin", "Latin"),
    ("world", "World"),
    ("soundtrack", "Soundtrack"),
    ("dance", "Dance"),
    ("relaxation", "Relaxation"),
];

/// Jamendo's `type` for singles and album tracks both.
const BOTH_TYPES: &str = "single%20albumtrack";

/// The longest genre tag accepted.
const MAX_TAG_CHARS: usize = 32;

/// The genres to browse.
pub fn genres() -> Vec<JamendoGenre> {
    GENRES
        .iter()
        .map(|(tag, name)| JamendoGenre {
            tag: (*tag).into(),
            name: (*name).into(),
        })
        .collect()
}

/// A genre tag's name to show: Discover's own name for it, else the tag
/// with a capital (`newage` is "New Age", `darkwave` is "Darkwave").
pub fn genre_name(tag: &str) -> String {
    match tag {
        "newage" => "New Age".into(),
        "rnb" => "R&B".into(),
        _ => GENRES
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, name)| (*name).to_string())
            .unwrap_or_else(|| capitalise(tag)),
    }
}

/// A genre tag as given, checked: lowercase letters, digits and dashes.
pub fn clean_tag(tag: &str) -> Result<String, SoundscapeError> {
    let tag = tag.trim().to_lowercase();
    if tag.is_empty()
        || tag.chars().count() > MAX_TAG_CHARS
        || !tag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(SoundscapeError::BadAddress {
            reason: "that isn't a Jamendo genre".into(),
        });
    }
    Ok(tag)
}

/// Jamendo's own id from `jamendo:<id>` (or a bare id): digits only.
pub fn track_number(id: &str) -> Option<&str> {
    let id = id.strip_prefix(PREFIX).unwrap_or(id);
    (!id.is_empty() && id.len() <= 20 && id.chars().all(|c| c.is_ascii_digit())).then_some(id)
}

/// The search request: tracks matching `query`, or this week's most
/// popular when it is empty.
pub fn search_url(client_id: &str, query: &str) -> String {
    tracks_url(client_id, &Ask::Search(query.into()))
}

/// The `/tracks` request for `ask`. Streams are asked for as MP3 at the
/// higher quality Jamendo offers (`mp32`).
pub fn tracks_url(client_id: &str, ask: &Ask) -> String {
    use crate::listen::encode;
    let what = match ask {
        Ask::Search(query) => {
            // cut: a search; the first 200 characters find the same tracks, and nothing is kept
            let query: String = query.trim().chars().take(MAX_QUERY_CHARS).collect();
            if query.is_empty() {
                "order=popularity_week".to_string()
            } else {
                format!("search={}", encode(&query, false))
            }
        }
        // Singles too, not only album tracks: an artist's other songs may
        // all be singles.
        Ask::Artist(id) => format!(
            "artist_id={}&order=popularity_total&type={BOTH_TYPES}",
            encode(id, false)
        ),
        Ask::Genre(tag) => format!(
            "tags={}&order=popularity_week&type={BOTH_TYPES}",
            encode(tag, false)
        ),
        Ask::Track(id) => format!("id={}", encode(id, false)),
    };
    format!(
        "{API}/tracks/?client_id={}&format=json&limit={PAGE}&{what}&include=musicinfo&audioformat=mp32&imagesize=300",
        encode(client_id, false)
    )
}

/// Searches Jamendo. On-screen work, cancelled with the screen.
pub fn search(
    http: &dyn HttpClient,
    client_id: &str,
    query: &str,
    cancel: &CancelHandle,
) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
    tracks(http, client_id, &Ask::Search(query.into()), cancel)
}

/// The tracks Jamendo has for `ask`, best first.
pub fn tracks(
    http: &dyn HttpClient,
    client_id: &str,
    ask: &Ask,
    cancel: &CancelHandle,
) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
    Ok(results(http, client_id, ask, cancel)?
        .iter()
        .filter_map(track)
        .collect())
}

/// More from Jamendo for the track `track_id` (`jamendo:<id>`): its
/// artist's other tracks, or others in its genre. Jamendo is asked about
/// the track first, so this works for a track added to the library before
/// Soundscape kept its artist.
pub fn more(
    http: &dyn HttpClient,
    client_id: &str,
    track_id: &str,
    by: JamendoMore,
    cancel: &CancelHandle,
) -> Result<JamendoMoreTracks, SoundscapeError> {
    let number = track_number(track_id).ok_or_else(|| SoundscapeError::BadAddress {
        reason: "this song isn't from Jamendo".into(),
    })?;
    let found = results(http, client_id, &Ask::Track(number.into()), cancel)?;
    let item = found
        .iter()
        .find(|item| text(&item["id"]).as_deref() == Some(number))
        .ok_or_else(|| SoundscapeError::Server {
            reason: "Jamendo no longer has this song".into(),
        })?;
    let (name, ask) = match by {
        JamendoMore::Artist => {
            let artist = text(&item["artist_id"]).ok_or_else(|| SoundscapeError::Server {
                reason: "Jamendo doesn't say who made this song".into(),
            })?;
            let name =
                tag(text(&item["artist_name"]).as_deref()).unwrap_or_else(|| "this artist".into());
            (name, Ask::Artist(artist))
        }
        JamendoMore::Genre => {
            let genre = item["musicinfo"]["tags"]["genres"]
                .as_array()
                .and_then(|g| {
                    g.iter()
                        .filter_map(Value::as_str)
                        .find_map(|g| clean_tag(g).ok())
                })
                .ok_or_else(|| SoundscapeError::Server {
                    reason: "Jamendo gives this song no genre".into(),
                })?;
            (genre_name(&genre), Ask::Genre(genre))
        }
    };
    let own = format!("{PREFIX}{number}");
    let tracks = tracks(http, client_id, &ask, cancel)?
        .into_iter()
        .filter(|t| t.id != own)
        .collect();
    Ok(JamendoMoreTracks { name, tracks })
}

/// The `results` of Jamendo's answer to `ask`. Jamendo sometimes answers
/// "success" with no results to a request that finds tracks a moment
/// later (seen live, 2026-11-18), so an empty answer is asked again before
/// it counts as none.
fn results(
    http: &dyn HttpClient,
    client_id: &str,
    ask: &Ask,
    cancel: &CancelHandle,
) -> Result<Vec<Value>, SoundscapeError> {
    let mut found = Vec::new();
    for _ in 0..ATTEMPTS {
        found = results_once(http, client_id, ask, cancel)?;
        if !found.is_empty() {
            break;
        }
    }
    Ok(found)
}

/// How many times an empty answer is asked for again, in all.
pub const ATTEMPTS: usize = 3;

fn results_once(
    http: &dyn HttpClient,
    client_id: &str,
    ask: &Ask,
    cancel: &CancelHandle,
) -> Result<Vec<Value>, SoundscapeError> {
    let response = http
        .send_cancellable(
            &Request::get(tracks_url(client_id, ask)).timeout(TIMEOUT),
            cancel,
        )
        .map_err(|err| match SoundscapeError::from(err) {
            SoundscapeError::Unreachable { reason } => SoundscapeError::Server {
                reason: format!("can't reach Jamendo: {reason}"),
            },
            other => other,
        })?;
    if !(200..300).contains(&response.status) {
        return Err(SoundscapeError::Server {
            reason: format!("Jamendo answered {}", response.status),
        });
    }
    parse_results(&response.body)
}

/// Tracks from a `/tracks` answer. A track without an id, title or stream
/// is left out. A failed answer is an error with Jamendo's reason: Jamendo
/// says so in `headers.status` (a wrong client ID is HTTP 200, code 5).
pub fn parse_tracks(body: &[u8]) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
    Ok(parse_results(body)?.iter().filter_map(track).collect())
}

fn parse_results(body: &[u8]) -> Result<Vec<Value>, SoundscapeError> {
    if body.len() > MAX_ANSWER_BYTES {
        return Err(SoundscapeError::Server {
            reason: "Jamendo's answer is too large".into(),
        });
    }
    let json: Value = serde_json::from_slice(body).map_err(|_| SoundscapeError::Server {
        reason: "Jamendo's answer can't be read".into(),
    })?;
    let headers = &json["headers"];
    if headers["status"].as_str().is_some_and(|s| s != "success") {
        let message = text(&headers["error_message"]).unwrap_or_else(|| "no reason given".into());
        return Err(SoundscapeError::Server {
            reason: format!("Jamendo refused the search: {message}"),
        });
    }
    Ok(json["results"].as_array().cloned().unwrap_or_default())
}

fn track(item: &Value) -> Option<CatalogueTrack> {
    let id = text(&item["id"])?;
    let title = tag(text(&item["name"]).as_deref())?;
    let audio_url = text(&item["audio"]).filter(|u| u.starts_with("https://"))?;
    let artist = tag(text(&item["artist_name"]).as_deref());
    let album = tag(text(&item["album_name"]).as_deref());
    let credit = format!(
        "\u{201c}{title}\u{201d} by {}, via Jamendo",
        artist.as_deref().unwrap_or("an unknown artist")
    );
    let genre = item["musicinfo"]["tags"]["genres"]
        .as_array()
        .and_then(|g| g.first())
        .and_then(Value::as_str)
        // Kept as before (not `genre_name`), so songs already in the library
        // stay in the same genre.
        .and_then(|g| tag(Some(&capitalise(g))));
    Some(CatalogueTrack {
        id: format!("{PREFIX}{id}"),
        provider: PROVIDER.into(),
        title,
        artist,
        album,
        number: item["position"]
            .as_u64()
            .or_else(|| text(&item["position"]).and_then(|p| p.parse().ok()))
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0),
        genre,
        duration_seconds: item["duration"]
            .as_f64()
            .or_else(|| text(&item["duration"]).and_then(|d| d.parse().ok()))
            .filter(|d| *d > 0.0),
        artwork_url: text(&item["image"])
            .or_else(|| text(&item["album_image"]))
            .filter(|u| u.starts_with("https://")),
        audio_url,
        rights: rights_from_licence(
            text(&item["license_ccurl"]).as_deref(),
            item["audiodownload_allowed"].as_bool().unwrap_or(false),
            &credit,
            text(&item["shareurl"]).filter(|u| u.starts_with("https://")),
        ),
        added_ms: 0,
    })
}

/// A non-empty string or number, trimmed and capped.
fn text(value: &Value) -> Option<String> {
    let s = match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    // cut: a field in Jamendo's answer, not typed
    (!s.is_empty()).then(|| s.chars().take(MAX_FIELD_CHARS * 4).collect())
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
