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

/// The search request: tracks matching `query`, or this week's most
/// popular when it is empty. Streams are asked for as MP3 at the higher
/// quality Jamendo offers (`mp32`).
pub fn search_url(client_id: &str, query: &str) -> String {
    let query: String = query.trim().chars().take(MAX_QUERY_CHARS).collect();
    let what = if query.is_empty() {
        "order=popularity_week".to_string()
    } else {
        format!("search={}", crate::listen::encode(&query, false))
    };
    format!(
        "{API}/tracks/?client_id={}&format=json&limit={PAGE}&{what}&include=musicinfo&audioformat=mp32&imagesize=300",
        crate::listen::encode(client_id, false)
    )
}

/// Searches Jamendo. On-screen work, cancelled with the screen.
pub fn search(
    http: &dyn HttpClient,
    client_id: &str,
    query: &str,
    cancel: &CancelHandle,
) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
    let response = http
        .send_cancellable(
            &Request::get(search_url(client_id, query)).timeout(TIMEOUT),
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
    parse_tracks(&response.body)
}

/// Tracks from a `/tracks` answer. A track without an id, title or stream
/// is left out; a failed answer is an error with Jamendo's reason.
pub fn parse_tracks(body: &[u8]) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
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
    let results = json["results"].as_array().cloned().unwrap_or_default();
    Ok(results.iter().filter_map(track).collect())
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
    (!s.is_empty()).then(|| s.chars().take(MAX_FIELD_CHARS * 4).collect())
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
