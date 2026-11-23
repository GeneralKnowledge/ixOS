//! Listen: play, compare and pick generated song takes kept on another
//! computer.
//!
//! A take server (for example JewGrass's `tools/player/serve.py` on the band
//! laptop) lists songs and their takes, streams the audio and saves which
//! take the person picked. This module is the client: the saved server
//! address, the requests and their answers, and how loud each take of an
//! A/B comparison should play. AVPlayer streams the audio from the URLs it
//! hands out.
//!
//! The server's API:
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /api/library` | `{"songs": [{slug, title, act?, actTitle?, takes, updated, pick, pickLabel, rated}]}` |
//! | `GET /api/song?slug=S` | `{slug, title, actTitle?, pick, takes: [{id, file, label, seed, model, bpm, key, caption, lyrics, lufs, stars, note}]}` |
//! | `GET /audio/<file>` | the audio, with byte ranges |
//! | `POST /api/pick` | `{song, take, pick?, stars?, note?}` |

use std::net::IpAddr;
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, NetError, Request};
use serde::{Deserialize, Serialize};

use crate::model::SoundscapeError;

/// How long the server may take to answer.
const TIMEOUT: Duration = Duration::from_secs(15);
/// The port `serve.py` listens on, used when the address names none.
pub const DEFAULT_PORT: u16 = 8642;
/// The most stars a take can have.
pub const MAX_STARS: u8 = 5;
/// The longest note on a take, in characters (the server keeps 2000).
pub const MAX_NOTE_CHARS: u32 = 2000;
/// A comparison never turns a take down by more than this, in decibels, so
/// a wrong measurement can't make one silent.
pub const MAX_MATCH_DB: f64 = 24.0;

/// What Listen keeps on the phone: the take server's address, the
/// person's Jamendo client ID and how songs run into each other.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ListenSettings {
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub jamendo_client_id: Option<String>,
    /// How many seconds the end of one song overlaps the start of the
    /// next; 0 plays them back to back with no gap.
    #[serde(default)]
    pub crossfade_seconds: u32,
}

/// The longest crossfade offered.
pub const MAX_CROSSFADE_SECONDS: u32 = 12;

pub const SETTINGS_VERSION: u32 = 1;

/// One song on the server, for the list.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SongSummary {
    pub slug: String,
    pub title: String,
    pub take_count: u32,
    /// When its newest take was made, in milliseconds since 1970.
    pub updated_ms: i64,
    /// The picked take's label ("seed 7202 · xl-turbo"), if one is picked.
    pub pick_label: Option<String>,
    /// How many of its takes have stars.
    pub rated_count: u32,
    /// The act or album it belongs to ("AFTER HOURS"), when the server
    /// knows; the list is sectioned by it.
    pub act: Option<String>,
}

/// One section of the song list: an act's songs, newest first.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SongGroup {
    /// `None` for songs the server puts in no act.
    pub act: Option<String>,
    pub songs: Vec<SongSummary>,
}

/// One take of a song.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Take {
    /// The server's id for it (its path without the extension).
    pub id: String,
    pub label: String,
    /// Where AVPlayer streams it from.
    pub audio_url: String,
    pub seed: Option<i64>,
    pub model: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub caption: Option<String>,
    pub lyrics: Option<String>,
    /// Integrated loudness in LUFS, once the server has measured it.
    pub lufs: Option<f64>,
    /// From 0 (not rated) to 5.
    pub stars: u8,
    pub note: String,
    pub picked: bool,
    /// When it was made, in milliseconds since 1970.
    pub made_ms: i64,
}

/// A song with its takes, oldest first.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SongDetail {
    pub slug: String,
    pub title: String,
    /// The act or album it belongs to, when the server knows.
    pub act: Option<String>,
    pub takes: Vec<Take>,
    /// The caption and lyrics most of its takes share (Kaggle renders all
    /// carry the same ones), to show once above the takes.
    pub caption: Option<String>,
    pub lyrics: Option<String>,
}

/// How loud to play two takes so neither wins by being louder.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct LoudnessMatch {
    /// AVPlayer volumes, 0 to 1: the quieter take plays as it is, the
    /// louder one is turned down.
    pub a_volume: f32,
    pub b_volume: f32,
    /// False when a take hasn't been measured yet: both play as they are.
    pub matched: bool,
    /// How much louder A is than B, in decibels (0 when not matched).
    pub difference_db: f64,
}

/// What changes on a take: the pick, its stars, its note. `None` leaves it.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct TakeMark {
    /// True picks this take for its song (replacing any other pick); false
    /// clears the pick if it was this take.
    pub picked: Option<bool>,
    pub stars: Option<u8>,
    pub note: Option<String>,
}

/// Volumes for an A/B comparison at matched loudness.
#[uniffi::export]
pub fn loudness_match(a_lufs: Option<f64>, b_lufs: Option<f64>) -> LoudnessMatch {
    match (a_lufs, b_lufs) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() => {
            let difference = a - b;
            let down = difference.abs().min(MAX_MATCH_DB);
            let quieter = 10f64.powf(-down / 20.0) as f32;
            let (a_volume, b_volume) = if difference > 0.0 {
                (quieter, 1.0)
            } else {
                (1.0, quieter)
            };
            LoudnessMatch {
                a_volume,
                b_volume,
                matched: true,
                difference_db: difference,
            }
        }
        _ => LoudnessMatch {
            a_volume: 1.0,
            b_volume: 1.0,
            matched: false,
            difference_db: 0.0,
        },
    }
}

/// Checks and tidies a server address typed by the person:
/// `100.101.102.103` becomes `http://100.101.102.103:8642`. Plain `http`
/// needs a numeric address, because iOS refuses plain http to a host name
/// (App Transport Security); `https` takes any host.
pub fn normalize_server(address: &str) -> Result<String, SoundscapeError> {
    let invalid = |reason: &str| SoundscapeError::BadAddress {
        reason: reason.to_string(),
    };
    let text = address.trim().trim_end_matches('/');
    if text.is_empty() {
        return Err(invalid(
            "type the computer's address, such as 100.101.102.103",
        ));
    }
    let (scheme, rest) = match text.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => ("http".to_string(), text),
    };
    if scheme != "http" && scheme != "https" {
        return Err(invalid("the address starts with http:// or https://"));
    }
    if rest.contains('/') || rest.contains('?') || rest.contains('#') || rest.contains('@') {
        return Err(invalid("type only the address and port, without a path"));
    }
    let (host, port) = split_host_port(rest).ok_or_else(|| invalid("that port isn't a number"))?;
    if host.is_empty() {
        return Err(invalid(
            "type the computer's address, such as 100.101.102.103",
        ));
    }
    let numeric = host
        .trim_matches(|c| c == '[' || c == ']')
        .parse::<IpAddr>()
        .is_ok();
    if !numeric
        && (host.contains(' ')
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
    {
        return Err(invalid("that isn't an address"));
    }
    if scheme == "http" && !numeric {
        return Err(invalid(
            "iOS only allows plain http to a numeric address: use the computer's IP address (its Tailscale address starts with 100.)",
        ));
    }
    let port = port.unwrap_or(if scheme == "https" { 443 } else { DEFAULT_PORT });
    Ok(format!("{scheme}://{host}:{port}"))
}

fn split_host_port(rest: &str) -> Option<(&str, Option<u16>)> {
    // [v6]:port, v4:port, host:port, or no port.
    if let Some(end) = rest.strip_prefix('[').and_then(|r| r.find(']')) {
        let host = &rest[..end + 2];
        let after = &rest[end + 2..];
        return match after.strip_prefix(':') {
            Some(p) => p.parse().ok().map(|p| (host, Some(p))),
            None if after.is_empty() => Some((host, None)),
            None => None,
        };
    }
    match rest.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => p.parse().ok().map(|p| (h, Some(p))),
        Some(_) => Some((rest, None)), // a bare IPv6 address
        None => Some((rest, None)),
    }
}

/// Percent-encodes everything but unreserved characters (and `/` when
/// `keep_slashes`).
pub fn encode(text: &str, keep_slashes: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b'/' if keep_slashes => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Undoes [`encode`]: `%XX` becomes its byte. A `%` not followed by two
/// hex digits is kept as it is.
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------ the server's JSON

#[derive(Deserialize)]
struct LibraryJson {
    #[serde(default)]
    songs: Vec<SongJson>,
}

#[derive(Deserialize)]
struct SongJson {
    slug: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    takes: u32,
    #[serde(default)]
    updated: f64,
    #[serde(default, rename = "pickLabel")]
    pick_label: Option<String>,
    #[serde(default)]
    rated: u32,
    #[serde(default)]
    act: Option<String>,
    #[serde(default, rename = "actTitle")]
    act_title: Option<String>,
}

#[derive(Deserialize)]
struct DetailJson {
    slug: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "actTitle")]
    act_title: Option<String>,
    #[serde(default)]
    pick: Option<String>,
    #[serde(default)]
    takes: Vec<TakeJson>,
}

#[derive(Deserialize)]
struct TakeJson {
    id: String,
    file: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    mtime: f64,
    #[serde(default)]
    seed: Option<serde_json::Value>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    bpm: Option<serde_json::Value>,
    #[serde(default)]
    key: Option<serde_json::Value>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    lyrics: Option<String>,
    /// Timed lyrics (LRC), when the song generator wrote them.
    #[serde(default)]
    lrc: Option<String>,
    #[serde(default)]
    lufs: Option<f64>,
    #[serde(default)]
    stars: Option<f64>,
    #[serde(default)]
    note: Option<String>,
}

fn number(v: &Option<serde_json::Value>) -> Option<f64> {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: &Option<serde_json::Value>) -> Option<String> {
    match v {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn parse_library(body: &[u8]) -> Result<Vec<SongSummary>, SoundscapeError> {
    let library: LibraryJson = serde_json::from_slice(body).map_err(bad_answer)?;
    let mut songs: Vec<SongSummary> = library
        .songs
        .into_iter()
        .map(|s| {
            let act = non_empty(s.act_title).or_else(|| non_empty(s.act));
            SongSummary {
                title: song_title(s.title, act.as_deref(), &s.slug),
                act,
                slug: s.slug,
                take_count: s.takes,
                updated_ms: (s.updated * 1000.0) as i64,
                pick_label: non_empty(s.pick_label),
                rated_count: s.rated,
            }
        })
        .collect();
    songs.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.title.cmp(&b.title)));
    Ok(songs)
}

pub fn parse_song(body: &[u8], server: &str) -> Result<SongDetail, SoundscapeError> {
    let song: DetailJson = serde_json::from_slice(body).map_err(bad_answer)?;
    let takes: Vec<Take> = song
        .takes
        .into_iter()
        .map(|t| Take {
            audio_url: format!("{server}/audio/{}", encode(&t.file, true)),
            label: non_empty(t.label).unwrap_or_else(|| t.id.clone()),
            picked: song.pick.as_deref() == Some(t.id.as_str()),
            seed: number(&t.seed).map(|s| s as i64),
            model: non_empty(t.model),
            bpm: number(&t.bpm),
            key: text(&t.key),
            caption: non_empty(t.caption),
            // Timed lyrics when there are some: `lyric_lines` reads both.
            lyrics: non_empty(t.lrc).or_else(|| non_empty(t.lyrics)),
            lufs: t.lufs.filter(|l| l.is_finite()),
            stars: t
                .stars
                .map(|s| s.clamp(0.0, f64::from(MAX_STARS)) as u8)
                .unwrap_or(0),
            note: t.note.unwrap_or_default(),
            made_ms: (t.mtime * 1000.0) as i64,
            id: t.id,
        })
        .collect();
    let caption = most_common(takes.iter().map(|t| t.caption.clone()));
    let lyrics = most_common(takes.iter().map(|t| t.lyrics.clone()));
    let act = non_empty(song.act_title);
    Ok(SongDetail {
        title: song_title(song.title, act.as_deref(), &song.slug),
        act,
        slug: song.slug,
        takes,
        caption,
        lyrics,
    })
}

/// The song's own title: the server may put the act in front ("AFTER HOURS ·
/// One More Night") for players without sections; the act is shown apart.
fn song_title(title: Option<String>, act: Option<&str>, slug: &str) -> String {
    let title = non_empty(title).unwrap_or_else(|| slug.to_string());
    if let Some(act) = act {
        for sep in [" · ", " - ", ": "] {
            if let Some(rest) = title.strip_prefix(&format!("{act}{sep}")) {
                if !rest.trim().is_empty() {
                    return rest.trim().to_string();
                }
            }
        }
    }
    title
}

/// The song list in sections by act. Sections are ordered by their newest
/// song, songs within one newest first (then by title); songs with no act
/// come last. `unpicked_only` keeps the songs that have no pick yet, and
/// drops sections left empty.
#[uniffi::export]
pub fn group_songs(songs: Vec<SongSummary>, unpicked_only: bool) -> Vec<SongGroup> {
    let mut groups: Vec<SongGroup> = Vec::new();
    for song in songs {
        if unpicked_only && song.pick_label.is_some() {
            continue;
        }
        match groups.iter_mut().find(|g| g.act == song.act) {
            Some(g) => g.songs.push(song),
            None => groups.push(SongGroup {
                act: song.act.clone(),
                songs: vec![song],
            }),
        }
    }
    for g in &mut groups {
        g.songs
            .sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.title.cmp(&b.title)));
    }
    let newest = |g: &SongGroup| g.songs.first().map_or(i64::MIN, |s| s.updated_ms);
    groups.sort_by(|a, b| {
        a.act
            .is_none()
            .cmp(&b.act.is_none())
            .then(newest(b).cmp(&newest(a)))
            .then(a.act.cmp(&b.act))
    });
    groups
}

fn most_common(values: impl Iterator<Item = Option<String>>) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for v in values.flatten() {
        match counts.iter_mut().find(|(k, _)| *k == v) {
            Some((_, n)) => *n += 1,
            None => counts.push((v, 1)),
        }
    }
    // the first one seen wins a tie
    let mut best: Option<(String, usize)> = None;
    for (k, n) in counts {
        if best.as_ref().is_none_or(|(_, m)| n > *m) {
            best = Some((k, n));
        }
    }
    best.map(|(k, _)| k)
}

/// The JSON body for `POST /api/pick`.
pub fn mark_body(slug: &str, take: &str, mark: &TakeMark) -> Result<Vec<u8>, SoundscapeError> {
    let mut body = serde_json::Map::new();
    body.insert("song".into(), slug.into());
    body.insert("take".into(), take.into());
    if let Some(p) = mark.picked {
        body.insert("pick".into(), p.into());
    }
    if let Some(s) = mark.stars {
        if s > MAX_STARS {
            return Err(SoundscapeError::range("stars"));
        }
        body.insert("stars".into(), s.into());
    }
    if let Some(n) = &mark.note {
        if n.chars().count() > MAX_NOTE_CHARS as usize {
            return Err(SoundscapeError::range("note"));
        }
        body.insert("note".into(), n.trim().into());
    }
    serde_json::to_vec(&body).map_err(|e| SoundscapeError::Server {
        reason: e.to_string(),
    })
}

fn bad_answer(err: serde_json::Error) -> SoundscapeError {
    SoundscapeError::Server {
        reason: format!("the server's answer wasn't a take list ({err}); is it the take server?"),
    }
}

// ------------------------------------------------------------ requests

fn send(
    http: &dyn HttpClient,
    request: Request,
    cancel: &CancelHandle,
) -> Result<Vec<u8>, SoundscapeError> {
    let response = http
        .send_cancellable(&request.timeout(TIMEOUT), cancel)
        .map_err(SoundscapeError::from)?;
    if !response.is_success() {
        return Err(SoundscapeError::Server {
            reason: match response.status {
                404 => "the server doesn't know that song any more; pull to refresh".into(),
                status => format!("the server answered {status}"),
            },
        });
    }
    Ok(response.body)
}

pub fn fetch_library(
    http: &dyn HttpClient,
    server: &str,
    fresh: bool,
    cancel: &CancelHandle,
) -> Result<Vec<SongSummary>, SoundscapeError> {
    let url = format!(
        "{server}/api/library{}",
        if fresh { "?fresh=1" } else { "" }
    );
    parse_library(&send(http, Request::get(url), cancel)?)
}

pub fn fetch_song(
    http: &dyn HttpClient,
    server: &str,
    slug: &str,
    cancel: &CancelHandle,
) -> Result<SongDetail, SoundscapeError> {
    let url = format!("{server}/api/song?slug={}", encode(slug, false));
    parse_song(&send(http, Request::get(url), cancel)?, server)
}

/// The server's whole catalogue (`/api/catalog`), or `None` from a server
/// too old to have it.
pub fn fetch_catalog(
    http: &dyn HttpClient,
    server: &str,
    fresh: bool,
    now: i64,
    cancel: &CancelHandle,
) -> Result<Option<crate::library::Snapshot>, SoundscapeError> {
    let url = format!(
        "{server}/api/catalog{}",
        if fresh { "?fresh=1" } else { "" }
    );
    let response = http
        .send_cancellable(&Request::get(url).timeout(TIMEOUT), cancel)
        .map_err(SoundscapeError::from)?;
    match response.status {
        404 => Ok(None),
        s if !(200..300).contains(&s) => Err(SoundscapeError::Server {
            reason: format!("the server answered {s}"),
        }),
        _ => crate::library::parse_catalog(&response.body, server, now).map(Some),
    }
}

pub fn send_mark(
    http: &dyn HttpClient,
    server: &str,
    slug: &str,
    take: &str,
    mark: &TakeMark,
    cancel: &CancelHandle,
) -> Result<(), SoundscapeError> {
    let request = Request::post(format!("{server}/api/pick"), mark_body(slug, take, mark)?)
        .header("Content-Type", "application/json");
    send(http, request, cancel).map(|_| ())
}

impl From<NetError> for SoundscapeError {
    fn from(err: NetError) -> Self {
        match err {
            NetError::Cancelled => SoundscapeError::Stopped {
                kind: forge_app::ErrorKind::Cancelled,
            },
            NetError::Offline { .. } | NetError::Timeout | NetError::Transport { .. } => {
                SoundscapeError::Unreachable {
                    reason: err.to_string(),
                }
            }
            other => SoundscapeError::Server {
                reason: other.to_string(),
            },
        }
    }
}
