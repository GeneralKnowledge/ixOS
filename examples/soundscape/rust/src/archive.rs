//! The Internet Archive: live recordings, net labels and historical audio,
//! searched and streamed with its public APIs (no key).
//!
//! An Archive *item* is usually an album or a concert: one identifier with
//! many files. Search finds items (`/advancedsearch.php`); opening one reads
//! its files (`/metadata/<identifier>`) and makes a track of each piece of
//! audio, in the best MP3 the Archive has for it (AVPlayer streams MP3 with
//! byte ranges; FLAC and Ogg originals are left for their MP3 copies). Track
//! ids are `archive:<identifier>/<file>`, which never change. Rights vary
//! item by item, so each track carries the item's licence.

use std::collections::BTreeMap;
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, Request};
use serde_json::Value;

use crate::catalogue::{rights_from_licence, CatalogueTrack};
use crate::library::tag;
use crate::listen::encode;
use crate::model::SoundscapeError;

pub const PROVIDER: &str = "Internet Archive";
/// Prefix of an Archive track's id: `archive:<identifier>/<file>`.
pub const PREFIX: &str = "archive:";
pub const SITE: &str = "https://archive.org";
/// Items asked for in one search.
pub const PAGE: u32 = 50;
/// The most tracks made from one item (a long concert run can have more
/// files than anyone would page through).
pub const MAX_TRACKS: usize = 500;
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_ANSWER_BYTES: usize = 16 * 1024 * 1024;

/// One Archive item from a search: an album, concert or recording.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ArchiveItem {
    pub identifier: String,
    pub title: String,
    pub creator: Option<String>,
    pub year: Option<String>,
    /// Short licence name, or "Licence not stated".
    pub licence: String,
    pub artwork_url: String,
}

/// Whether text is an Archive identifier (letters, digits, `.`, `-`, `_`).
pub fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 200
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The search request: audio items matching `query`, most downloaded
/// first; with no query, this week's most downloaded net-label releases.
pub fn search_url(query: &str) -> String {
    // cut: a search, as in jamendo::search_url
    let query: String = query
        .trim()
        .chars()
        .take(crate::jamendo::MAX_QUERY_CHARS)
        .collect();
    let (q, sort) = if query.is_empty() {
        (
            "collection:(netlabels) AND mediatype:(audio)".to_string(),
            "week desc",
        )
    } else {
        // Quotes and brackets in what was typed can't change the query's shape.
        let words: String = query
            .chars()
            .map(|c| {
                if "\"()[]{}:\\^~*?".contains(c) {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        (
            format!("({}) AND mediatype:(audio)", words.trim()),
            "downloads desc",
        )
    };
    let fields = ["identifier", "title", "creator", "year", "licenseurl"]
        .iter()
        .map(|f| format!("fl%5B%5D={f}"))
        .collect::<Vec<_>>()
        .join("&");
    format!(
        "{SITE}/advancedsearch.php?q={}&{fields}&sort%5B%5D={}&rows={PAGE}&page=1&output=json",
        encode(&q, false),
        encode(sort, false)
    )
}

pub fn metadata_url(identifier: &str) -> String {
    format!("{SITE}/metadata/{identifier}")
}

pub fn artwork_url(identifier: &str) -> String {
    format!("{SITE}/services/img/{identifier}")
}

/// Searches the Archive. On-screen work.
pub fn search(
    http: &dyn HttpClient,
    query: &str,
    cancel: &CancelHandle,
) -> Result<Vec<ArchiveItem>, SoundscapeError> {
    parse_search(&get(http, &search_url(query), cancel)?)
}

/// An item's playable tracks, in order. On-screen work.
pub fn item_tracks(
    http: &dyn HttpClient,
    identifier: &str,
    cancel: &CancelHandle,
) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
    if !is_identifier(identifier) {
        return Err(SoundscapeError::NotInLibrary {
            id: identifier.to_string(),
        });
    }
    parse_item(identifier, &get(http, &metadata_url(identifier), cancel)?)
}

fn get(
    http: &dyn HttpClient,
    url: &str,
    cancel: &CancelHandle,
) -> Result<Vec<u8>, SoundscapeError> {
    let response = http
        .send_cancellable(&Request::get(url).timeout(TIMEOUT), cancel)
        .map_err(|err| match SoundscapeError::from(err) {
            SoundscapeError::Unreachable { reason } => SoundscapeError::Server {
                reason: format!("can't reach the Internet Archive: {reason}"),
            },
            other => other,
        })?;
    if !(200..300).contains(&response.status) {
        return Err(SoundscapeError::Server {
            reason: format!("the Internet Archive answered {}", response.status),
        });
    }
    if response.body.len() > MAX_ANSWER_BYTES {
        return Err(SoundscapeError::Server {
            reason: "the Internet Archive's answer is too large".into(),
        });
    }
    Ok(response.body)
}

fn read(body: &[u8]) -> Result<Value, SoundscapeError> {
    serde_json::from_slice(body).map_err(|_| SoundscapeError::Server {
        reason: "the Internet Archive's answer can't be read".into(),
    })
}

/// Items from an `/advancedsearch.php` answer.
pub fn parse_search(body: &[u8]) -> Result<Vec<ArchiveItem>, SoundscapeError> {
    let json = read(body)?;
    if let Some(error) = text(&json["error"]) {
        return Err(SoundscapeError::Server {
            reason: format!("the Internet Archive refused the search: {error}"),
        });
    }
    let docs = json["response"]["docs"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(docs
        .iter()
        .filter_map(|doc| {
            let identifier = text(&doc["identifier"]).filter(|i| is_identifier(i))?;
            Some(ArchiveItem {
                title: tag(first(&doc["title"]).as_deref()).unwrap_or_else(|| identifier.clone()),
                creator: tag(first(&doc["creator"]).as_deref()),
                year: first(&doc["year"]),
                licence: rights_from_licence(first(&doc["licenseurl"]).as_deref(), false, "", None)
                    .licence,
                artwork_url: artwork_url(&identifier),
                identifier,
            })
        })
        .collect())
}

/// The MP3 formats the Archive makes, best first. Other audio (FLAC, Ogg,
/// WAV originals) plays through its MP3 copy.
const MP3_FORMATS: [&str; 5] = ["VBR MP3", "MP3", "320Kbps MP3", "128Kbps MP3", "64Kbps MP3"];

fn mp3_rank(format: &str) -> Option<usize> {
    MP3_FORMATS
        .iter()
        .position(|f| f.eq_ignore_ascii_case(format))
}

/// Tracks from a `/metadata/<identifier>` answer: one per piece of audio,
/// in its best MP3, ordered by track number then file name. An item that
/// doesn't exist or has no MP3 gives none.
pub fn parse_item(identifier: &str, body: &[u8]) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
    let json = read(body)?;
    let meta = &json["metadata"];
    if json["is_dark"].as_bool() == Some(true) || meta.is_null() {
        return Ok(Vec::new());
    }
    let item_title = tag(first(&meta["title"]).as_deref());
    let item_creator = tag(first(&meta["creator"]).as_deref());
    let licence = first(&meta["licenseurl"]);
    let restricted = text(&meta["access-restricted-item"]).as_deref() == Some("true");
    let files = json["files"].as_array().cloned().unwrap_or_default();

    // The best MP3 for each piece of audio, keyed by the original it was
    // made from (or its own name when it is the original).
    let mut best: BTreeMap<String, (usize, &Value)> = BTreeMap::new();
    for file in &files {
        let (Some(name), Some(rank)) = (
            text(&file["name"]),
            text(&file["format"]).and_then(|f| mp3_rank(&f)),
        ) else {
            continue;
        };
        if name.contains("..") || name.starts_with('/') {
            continue;
        }
        let key = text(&file["original"]).unwrap_or(name);
        match best.get(&key) {
            Some((r, _)) if *r <= rank => {}
            _ => {
                best.insert(key, (rank, file));
            }
        }
    }

    let page = format!("{SITE}/details/{identifier}");
    let mut tracks: Vec<(Option<u32>, String, CatalogueTrack)> = best
        .into_values()
        .filter_map(|(_, file)| {
            let name = text(&file["name"])?;
            let title = tag(text(&file["title"]).as_deref())
                .or_else(|| tag(Some(untracked(stem(&name)))))
                .unwrap_or_else(|| name.clone());
            let artist = tag(text(&file["creator"])
                .or_else(|| text(&file["artist"]))
                .as_deref())
            .or_else(|| item_creator.clone());
            let credit = format!(
                "\u{201c}{title}\u{201d} by {}, via the Internet Archive",
                artist.as_deref().unwrap_or("an unknown artist")
            );
            let number = text(&file["track"]).and_then(|t| track_number(&t));
            Some((
                number,
                name.clone(),
                CatalogueTrack {
                    id: format!("{PREFIX}{identifier}/{name}"),
                    provider: PROVIDER.into(),
                    title,
                    artist,
                    album: tag(text(&file["album"]).as_deref()).or_else(|| item_title.clone()),
                    number,
                    genre: tag(text(&file["genre"]).as_deref()),
                    duration_seconds: text(&file["length"]).and_then(|l| seconds(&l)),
                    artwork_url: Some(artwork_url(identifier)),
                    audio_url: format!("{SITE}/download/{identifier}/{}", encode(&name, true)),
                    rights: rights_from_licence(
                        licence.as_deref(),
                        !restricted,
                        &credit,
                        Some(page.clone()),
                    ),
                    added_ms: 0,
                },
            ))
        })
        .collect();
    tracks.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.cmp(&y).then(a.1.cmp(&b.1)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.1.cmp(&b.1),
    });
    Ok(tracks
        .into_iter()
        .take(MAX_TRACKS)
        .map(|(_, _, t)| t)
        .collect())
}

/// "03", "3/12" or "3" as 3.
fn track_number(text: &str) -> Option<u32> {
    text.split('/')
        .next()?
        .trim()
        .parse()
        .ok()
        .filter(|n| *n > 0)
}

/// "183.45", "03:03" or "1:02:03" as seconds.
fn seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    let value = if text.contains(':') {
        text.split(':').try_fold(0.0, |total, part| {
            part.trim().parse::<f64>().ok().map(|p| total * 60.0 + p)
        })?
    } else {
        text.parse().ok()?
    };
    (value.is_finite() && value > 0.0).then_some(value)
}

/// "01 - Static Field" as "Static Field": a file name's leading track
/// number, when something follows it.
fn untracked(name: &str) -> &str {
    let rest = name.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.len() == name.len() {
        return name;
    }
    let rest = rest.trim_start_matches([' ', '-', '.', '_']);
    if rest.is_empty() || rest.len() == name.len() {
        name
    } else {
        rest
    }
}

/// A file name without its folders or extension.
fn stem(name: &str) -> &str {
    let last = name.rsplit('/').next().unwrap_or(name);
    last.rsplit_once('.').map_or(last, |(s, _)| s)
}

/// A string, a number, or the first of a list of them.
fn first(value: &Value) -> Option<String> {
    match value {
        Value::Array(list) => list.iter().find_map(text),
        other => text(other),
    }
}

fn text(value: &Value) -> Option<String> {
    let s = match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}
