//! MusicBrainz: the open music encyclopedia, used to recognise a song.
//!
//! A song in the library (an imported file with poor tags, a Jamendo or
//! Internet Archive track, a take) is looked up by its title, artist and
//! length (`/ws/2/recording`). MusicBrainz answers with recordings and its
//! own relevance score; Soundscape ranks them again against what it knows
//! about the song, so a different song with the same name, or a live
//! version twice as long, is not offered as likely. Nothing is changed by a
//! search: the person picks a match, and only then is the song linked to it
//! (and, if they ask, given its title, artist, album and cover).
//!
//! A linked song carries its MusicBrainz recording id. Two songs from
//! different sources linked to the same id are the same recording, which is
//! how the library will recognise one song at several sources.
//!
//! MusicBrainz asks every client to say who it is in its User-Agent and to
//! send at most one request a second; lookups here are one at a time, when
//! the person asks.

use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, Request};
use serde_json::Value;

use crate::listen::encode;
use crate::model::SoundscapeError;

pub const SITE: &str = "https://musicbrainz.org";
/// Who is asking, as MusicBrainz requires: the app and where it lives.
pub const USER_AGENT: &str = "Soundscape/1.0 ( https://github.com/GeneralKnowledge/ixOS )";
/// Recordings asked for in one search.
pub const LIMIT: u32 = 25;
/// Tries of one search while MusicBrainz says it is busy (503), which it
/// does often, for everyone at once, waiting what it asks between them.
pub const ATTEMPTS: u32 = 4;
/// The longest wait between tries, whatever `Retry-After` says.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(3);
/// A match whose length differs by more than this, in seconds, is a
/// different recording (a live take, a radio edit).
pub const LENGTH_TOLERANCE_SECONDS: f64 = 5.0;
/// From this confidence (0 to 100) a match is offered as likely.
pub const LIKELY: u8 = 80;
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;
const MAX_QUERY_CHARS: usize = 200;

/// One recording MusicBrainz knows that might be the song.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MusicBrainzMatch {
    /// The MusicBrainz recording id (a UUID).
    pub recording_id: String,
    pub title: String,
    /// What tells it apart from recordings of the same name ("live,
    /// 1979-01-24: Deutschlandhalle, Berlin").
    pub disambiguation: Option<String>,
    /// A live recording: said so, or only released on live albums.
    pub live: bool,
    /// The artist credit as MusicBrainz writes it ("A & B").
    pub artist: Option<String>,
    /// The release it is best known from: an official album, earliest
    /// first.
    pub album: Option<String>,
    pub release_id: Option<String>,
    pub release_group_id: Option<String>,
    /// The year of that release, or of the recording's first release.
    pub year: Option<String>,
    /// Its track number on that release.
    pub number: Option<u32>,
    pub duration_seconds: Option<f64>,
    /// The release group's front cover at the Cover Art Archive, when the
    /// release group is known (some have no cover; it then shows none).
    pub artwork_url: Option<String>,
    /// How sure Soundscape is that this is the song, 0 to 100.
    pub confidence: u8,
    /// Confident enough to be offered first.
    pub likely: bool,
    /// The recording's page on musicbrainz.org.
    pub page_url: String,
}

/// What is known about the song being looked up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wanted {
    pub title: String,
    pub artist: Option<String>,
    pub duration_seconds: Option<f64>,
}

/// Whether `text` is a MusicBrainz id: a lower-case UUID.
pub fn is_mbid(text: &str) -> bool {
    text.len() == 36
        && text.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

/// Words for a search field: what was typed, with Lucene's special
/// characters taken out so they can't change the query's shape.
fn words(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .take(MAX_QUERY_CHARS)
        .map(|c| {
            if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The search request for `wanted`'s title, and its artist when known.
pub fn search_url(wanted: &Wanted) -> String {
    let mut query = format!("recording:\"{}\"", words(&wanted.title));
    if let Some(artist) = wanted.artist.as_deref().map(words) {
        if !artist.is_empty() {
            query.push_str(&format!(" AND artist:\"{artist}\""));
        }
    }
    format!(
        "{SITE}/ws/2/recording?query={}&limit={LIMIT}&fmt=json",
        encode(&query, false)
    )
}

pub fn recording_page(recording_id: &str) -> String {
    format!("{SITE}/recording/{recording_id}")
}

pub fn cover_url(release_group_id: &str) -> String {
    format!("https://coverartarchive.org/release-group/{release_group_id}/front-250")
}

/// Looks the song up, best match first. On-screen work.
pub fn identify(
    http: &dyn HttpClient,
    wanted: &Wanted,
    cancel: &CancelHandle,
) -> Result<Vec<MusicBrainzMatch>, SoundscapeError> {
    if words(&wanted.title).is_empty() {
        return Ok(Vec::new());
    }
    let request = Request::get(search_url(wanted))
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .timeout(TIMEOUT);
    let mut attempt = 1;
    let response = loop {
        let response = http.send_cancellable(&request, cancel).map_err(|err| {
            match SoundscapeError::from(err) {
                SoundscapeError::Unreachable { reason } => SoundscapeError::Server {
                    reason: format!("can't reach MusicBrainz: {reason}"),
                },
                other => other,
            }
        })?;
        if response.status != 503 || attempt >= ATTEMPTS {
            break response;
        }
        // Busy for everyone (seen live, 2026-11-19: three 503s in a row,
        // then an answer): wait what it asks, then try again.
        let wait = response
            .header("Retry-After")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(1))
            .clamp(Duration::from_millis(500), MAX_RETRY_WAIT);
        if cancel.wait_timeout(wait) {
            return Err(SoundscapeError::Stopped {
                kind: forge_app::ErrorKind::Cancelled,
            });
        }
        attempt += 1;
    };
    match response.status {
        200..=299 => {}
        503 => {
            return Err(SoundscapeError::Server {
                reason: "MusicBrainz is busy; try again in a moment".into(),
            })
        }
        status => {
            return Err(SoundscapeError::Server {
                reason: format!("MusicBrainz answered {status}"),
            })
        }
    }
    if response.body.len() > MAX_ANSWER_BYTES {
        return Err(SoundscapeError::Server {
            reason: "MusicBrainz's answer is too large".into(),
        });
    }
    Ok(rank(parse(&response.body)?, wanted))
}

/// Recordings from a `/ws/2/recording` answer, in MusicBrainz's order,
/// with its score as their confidence.
pub fn parse(body: &[u8]) -> Result<Vec<MusicBrainzMatch>, SoundscapeError> {
    let json: Value = serde_json::from_slice(body).map_err(|_| SoundscapeError::Server {
        reason: "MusicBrainz's answer can't be read".into(),
    })?;
    if let Some(error) = json["error"].as_str() {
        return Err(SoundscapeError::Server {
            reason: format!("MusicBrainz refused the search: {error}"),
        });
    }
    let recordings = json["recordings"].as_array().cloned().unwrap_or_default();
    Ok(recordings.iter().filter_map(recording).collect())
}

fn recording(json: &Value) -> Option<MusicBrainzMatch> {
    let id = json["id"].as_str()?;
    if !is_mbid(id) {
        return None;
    }
    let title = json["title"].as_str()?.trim();
    if title.is_empty() {
        return None;
    }
    let score = match &json["score"] {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
    .unwrap_or(0)
    .min(100) as u8;
    let artist: String = json["artist-credit"]
        .as_array()
        .map(|credits| {
            credits
                .iter()
                .map(|c| {
                    let name = c["name"]
                        .as_str()
                        .or_else(|| c["artist"]["name"].as_str())
                        .unwrap_or("");
                    format!("{name}{}", c["joinphrase"].as_str().unwrap_or(""))
                })
                .collect()
        })
        .unwrap_or_default();
    let release = best_release(json["releases"].as_array().map(|r| r.as_slice()));
    let release_group_id = release
        .and_then(|r| r["release-group"]["id"].as_str())
        .filter(|id| is_mbid(id))
        .map(str::to_string);
    let year = release
        .and_then(|r| r["date"].as_str())
        .or_else(|| json["first-release-date"].as_str())
        .and_then(|d| d.get(..4))
        .filter(|y| y.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string);
    let number = release
        .and_then(|r| r["media"].as_array())
        .and_then(|media| media.first())
        .and_then(|m| m["track"].as_array())
        .and_then(|t| t.first())
        .and_then(|t| t["number"].as_str())
        .and_then(|n| n.trim().parse().ok())
        .filter(|n| *n > 0);
    let disambiguation = json["disambiguation"]
        .as_str()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    let live = disambiguation
        .as_deref()
        .is_some_and(|d| d.to_lowercase().starts_with("live"))
        || release.is_some_and(|r| {
            r["release-group"]["secondary-types"]
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t.as_str() == Some("Live")))
        });
    Some(MusicBrainzMatch {
        recording_id: id.to_string(),
        title: title.to_string(),
        disambiguation,
        live,
        artist: Some(artist.trim().to_string()).filter(|a| !a.is_empty()),
        album: release
            .and_then(|r| r["title"].as_str())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        release_id: release
            .and_then(|r| r["id"].as_str())
            .filter(|id| is_mbid(id))
            .map(str::to_string),
        artwork_url: release_group_id.as_deref().map(cover_url),
        release_group_id,
        year,
        number,
        duration_seconds: json["length"]
            .as_f64()
            .filter(|ms| *ms > 0.0)
            .map(|ms| (ms / 100.0).round() / 10.0),
        confidence: score,
        likely: false,
        page_url: recording_page(id),
    })
}

/// The release a recording is best known from: official before anything
/// else, an album before a single or compilation, then the earliest.
fn best_release(releases: Option<&[Value]>) -> Option<&Value> {
    releases?.iter().min_by_key(|r| {
        let official = r["status"].as_str() == Some("Official");
        let group = &r["release-group"];
        let album = group["primary-type"].as_str() == Some("Album");
        let compilation = group["secondary-types"]
            .as_array()
            .is_some_and(|t| t.iter().any(|t| t.as_str() == Some("Compilation")));
        let date = r["date"]
            .as_str()
            .filter(|d| !d.is_empty())
            .unwrap_or("9999");
        (!official, !album, compilation, date.to_string())
    })
}

/// Letters and digits only, lower case, for comparing names.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A title without a bracketed or dashed version note ("Song (2011
/// Remaster)", "Song - Live").
fn bare_title(text: &str) -> String {
    let cut = text
        .find(['(', '['])
        .or_else(|| text.find(" - "))
        .unwrap_or(text.len());
    plain(&text[..cut])
}

/// Ranks MusicBrainz's answers against what is known about the song: the
/// same title, the same artist and the same length count for more than
/// MusicBrainz's text score, and a length far off counts against.
pub fn rank(found: Vec<MusicBrainzMatch>, wanted: &Wanted) -> Vec<MusicBrainzMatch> {
    let title = plain(&wanted.title);
    let bare = bare_title(&wanted.title);
    // A song that says it is live may be matched to a live recording.
    let wants_live = wanted.title.to_lowercase().contains("live");
    let artist = wanted
        .artist
        .as_deref()
        .map(plain)
        .filter(|a| !a.is_empty());
    let mut ranked: Vec<MusicBrainzMatch> = found
        .into_iter()
        .map(|mut m| {
            let mut points = f64::from(m.confidence) * 0.4;
            let theirs = plain(&m.title);
            if theirs == title {
                points += 30.0;
            } else if !bare.is_empty() && bare_title(&m.title) == bare {
                points += 20.0;
            } else if !theirs.is_empty() && (theirs.contains(&title) || title.contains(&theirs)) {
                points += 10.0;
            }
            match (&artist, m.artist.as_deref().map(plain)) {
                (Some(a), Some(b)) if *a == b => points += 20.0,
                (Some(a), Some(b)) if b.contains(a.as_str()) || a.contains(b.as_str()) => {
                    points += 10.0
                }
                (None, _) => points += 5.0,
                _ => {}
            }
            let length_ok = match (wanted.duration_seconds, m.duration_seconds) {
                (Some(a), Some(b)) => {
                    let off = (a - b).abs();
                    if off <= LENGTH_TOLERANCE_SECONDS {
                        points += 10.0;
                        true
                    } else {
                        points -= if off > 30.0 { 40.0 } else { 20.0 };
                        false
                    }
                }
                _ => true,
            };
            // The same name, played on another night (seen live, 2026-11-19:
            // MusicBrainz's top three for a studio song were all concerts).
            if m.live && !wants_live {
                points -= 15.0;
            }
            m.confidence = points.clamp(0.0, 100.0).round() as u8;
            m.likely = length_ok && m.confidence >= LIKELY;
            m
        })
        .collect();
    ranked.sort_by_key(|m| std::cmp::Reverse(m.confidence));
    ranked
}
