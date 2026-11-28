//! Live radio: real broadcasting stations from around the world, found
//! through the Radio Browser directory (radio-browser.info: community-run,
//! free, no key, its data in the public domain) and played as live streams.
//!
//! A live station is not a song. It never enters the play queue: playing
//! one sets the queue aside, and the player shows a live state with no
//! scrubber, skip, repeat or Like (`docs/podcasts-and-radio.md`).
//!
//! What is kept (`live_radio.json`) is only what the person did: stations
//! they saved and stations they played recently, each by Radio Browser's
//! stable station UUID with the rest kept as a cache. A cached stream
//! address older than [`STALE_MS`], or one that failed, is looked up again
//! by UUID ([`Directory::station`]), so a saved station follows a station
//! that moves its stream.
//!
//! Radio Browser is volunteer-run: requests go to one of its published
//! servers (asked for once, then reused, moving to another when one fails),
//! with the app's plain User-Agent. Search terms are never logged.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use forge_net::{CancelHandle, HttpClient, NetError, Request};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::listen::encode;
use crate::model::SoundscapeError;

pub const LIVE_RADIO_VERSION: u32 = 1;
/// Servers tried, in this order, until one names the current list. These
/// are only a way in: the list Radio Browser publishes (`/json/servers`)
/// is used once one answers.
pub const SEED_SERVERS: &[&str] = &[
    "de1.api.radio-browser.info",
    "de2.api.radio-browser.info",
    "fi1.api.radio-browser.info",
];
/// Stations asked for in one list.
pub const PAGE: u32 = 60;
/// Categories (countries, languages, genres) asked for in one list.
pub const CATEGORY_PAGE: u32 = 200;
/// The most recently played stations kept.
pub const MAX_RECENTS: usize = 30;
/// The most saved stations.
pub const MAX_SAVED: usize = 500;
/// A cached stream address older than this is looked up again: one day.
pub const STALE_MS: i64 = 24 * 60 * 60 * 1000;
/// The longest search text, in characters.
pub const MAX_QUERY_CHARS: usize = 100;
/// The longest station name, country or tag kept, in characters.
const MAX_TEXT_CHARS: usize = 200;
/// The most tags or languages kept for one station.
const MAX_LIST: usize = 12;
const TIMEOUT: Duration = Duration::from_secs(12);
/// The largest answer accepted.
const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;

/// Stream formats AVPlayer can't play. A station whose stream Radio Browser
/// reports in one of these is never offered. (Anything else, including an
/// unknown format, is offered; the stream's own content type is checked
/// when it connects, in L2.)
const UNPLAYABLE_CODECS: &[&str] = &["OGG", "VORBIS", "OPUS", "FLAC", "WMA", "ASF", "FLV"];

/// One live station, as Radio Browser lists it.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LiveStation {
    /// Radio Browser's station UUID: what identifies the station for good.
    pub uuid: String,
    pub name: String,
    /// What AVPlayer plays: the stream address with playlists resolved.
    pub stream_url: String,
    /// The address the station listed, tried when `stream_url` fails.
    pub listed_url: String,
    pub homepage: Option<String>,
    /// The station's logo, when it has one (often missing or small).
    pub artwork_url: Option<String>,
    pub country: String,
    /// ISO 3166-1 alpha-2, upper case (`GB`, `JP`); empty when not given.
    pub country_code: String,
    pub languages: Vec<String>,
    pub tags: Vec<String>,
    /// As Radio Browser reports it (`MP3`, `AAC`, `AAC+`), upper case;
    /// empty when unknown.
    pub codec: String,
    pub bitrate_kbps: u32,
    pub hls: bool,
    /// Whether `stream_url` is `https`. A plain `http` stream still plays;
    /// the station's details say it isn't encrypted.
    pub secure: bool,
    /// Radio Browser's play count (its popularity ranking).
    pub plays: u64,
}

/// What kind of list to browse.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCategoryKind {
    Country,
    Language,
    Genre,
}

/// One country, language or genre with how many stations Radio Browser
/// lists for it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LiveCategory {
    pub kind: LiveCategoryKind,
    /// What to ask for: a country code (`JP`), a language or a tag.
    pub key: String,
    /// The name to show.
    pub name: String,
    pub station_count: u32,
}

/// Which stations to list.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum LiveAsk {
    /// The most played stations everywhere.
    Top,
    /// Stations whose name matches the text.
    Search { text: String },
    /// One country's stations, most played first (`code` is ISO 3166-1).
    Country { code: String },
    /// Stations in one language, most played first.
    Language { name: String },
    /// Stations with one tag (a genre such as `jazz`), most played first.
    Genre { tag: String },
}

/// `live_radio.json`: only what the person did.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LiveRadioFile {
    #[serde(default)]
    pub saved: Vec<KeptStation>,
    /// Newest first.
    #[serde(default)]
    pub recent: Vec<KeptStation>,
}

/// A station kept by UUID, with its details as last seen.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct KeptStation {
    pub station: LiveStation,
    /// When the details (above all the stream address) were last looked up.
    pub checked_ms: i64,
    /// When it was saved, or last played (for recents).
    pub at_ms: i64,
}

/// What Swift shows: saved and recent stations, and whether each one's
/// stream address should be looked up again before playing.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LiveRadio {
    pub saved: Vec<LiveStation>,
    pub recent: Vec<LiveStation>,
}

impl LiveRadioFile {
    pub fn is_saved(&self, uuid: &str) -> bool {
        self.saved.iter().any(|k| k.station.uuid == uuid)
    }

    /// Saves `station` (newest last, as the person built the list). Saving
    /// one already saved refreshes its details and keeps its place.
    pub fn save(&mut self, station: LiveStation, now: i64) -> Result<(), SoundscapeError> {
        if let Some(kept) = self
            .saved
            .iter_mut()
            .find(|k| k.station.uuid == station.uuid)
        {
            // Details from a list or Now Playing, not a lookup: the check
            // time stays that of the last lookup.
            kept.station = station;
            return Ok(());
        }
        if self.saved.len() >= MAX_SAVED {
            return Err(SoundscapeError::OutOfRange {
                setting: format!("saved stations (at most {MAX_SAVED})"),
            });
        }
        // Checked when another copy (Recent) was looked up; else never.
        let checked_ms = self.checked_ms(&station.uuid).unwrap_or(0);
        self.saved.push(KeptStation {
            station,
            checked_ms,
            at_ms: now,
        });
        Ok(())
    }

    /// Removes a saved station. True when one was removed.
    pub fn unsave(&mut self, uuid: &str) -> bool {
        let before = self.saved.len();
        self.saved.retain(|k| k.station.uuid != uuid);
        self.saved.len() != before
    }

    /// Puts `station` at the front of the recent list, once. `checked_ms`
    /// is when its details were looked up, if they just were; otherwise the
    /// time already known for it is kept (none: never checked), so playing
    /// unchecked details never makes them count as fresh.
    pub fn played(&mut self, station: LiveStation, now: i64, checked_ms: Option<i64>) {
        // Unchecked details inherit a check time only from a copy with the
        // same stream address: a lookup of another address says nothing
        // about this one.
        let checked_ms = checked_ms.unwrap_or_else(|| {
            self.saved
                .iter()
                .chain(self.recent.iter())
                .filter(|k| {
                    k.station.uuid == station.uuid && k.station.stream_url == station.stream_url
                })
                .map(|k| k.checked_ms)
                .max()
                .unwrap_or(0)
        });
        self.recent.retain(|k| k.station.uuid != station.uuid);
        self.recent.insert(
            0,
            KeptStation {
                station,
                checked_ms,
                at_ms: now,
            },
        );
        self.recent.truncate(MAX_RECENTS);
    }

    /// Fresh details for a station everywhere it is kept.
    pub fn refreshed(&mut self, station: &LiveStation, now: i64) {
        for kept in self.saved.iter_mut().chain(self.recent.iter_mut()) {
            if kept.station.uuid == station.uuid {
                kept.station = station.clone();
                kept.checked_ms = now;
            }
        }
    }

    /// The stream of `uuid` failed: its kept details are looked up again
    /// next time, even if that lookup can't happen now.
    pub fn mark_stale(&mut self, uuid: &str) {
        for kept in self.saved.iter_mut().chain(self.recent.iter_mut()) {
            if kept.station.uuid == uuid {
                kept.checked_ms = 0;
            }
        }
    }

    /// Clears the recent list.
    pub fn clear_recent(&mut self) {
        self.recent.clear();
    }

    /// Whether the kept stream address for `uuid` is old enough to look up
    /// again. Unknown stations are always looked up.
    pub fn is_stale(&self, uuid: &str, now: i64) -> bool {
        self.checked_ms(uuid)
            .is_none_or(|checked| now - checked > STALE_MS)
    }

    /// When the details kept for `uuid` were last looked up; `None` when
    /// they never were (a kept `checked_ms` of 0).
    fn checked_ms(&self, uuid: &str) -> Option<i64> {
        self.saved
            .iter()
            .chain(self.recent.iter())
            .filter(|k| k.station.uuid == uuid && k.checked_ms > 0)
            .map(|k| k.checked_ms)
            .max()
    }

    /// The kept details for `uuid`, the most recently checked.
    pub fn kept(&self, uuid: &str) -> Option<LiveStation> {
        self.saved
            .iter()
            .chain(self.recent.iter())
            .filter(|k| k.station.uuid == uuid)
            .max_by_key(|k| k.checked_ms)
            .map(|k| k.station.clone())
    }

    pub fn view(&self) -> LiveRadio {
        LiveRadio {
            saved: self.saved.iter().map(|k| k.station.clone()).collect(),
            recent: self.recent.iter().map(|k| k.station.clone()).collect(),
        }
    }
}

/// A UUID as Radio Browser writes them: hex digits and dashes, 36 long.
pub fn clean_uuid(uuid: &str) -> Result<String, SoundscapeError> {
    let uuid = uuid.trim().to_ascii_lowercase();
    if uuid.len() != 36 || !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(SoundscapeError::BadAddress {
            reason: "that isn't a Radio Browser station".into(),
        });
    }
    Ok(uuid)
}

/// The path and query asking Radio Browser for `ask` (no server).
pub fn stations_path(ask: &LiveAsk) -> Result<String, SoundscapeError> {
    let common = format!("hidebroken=true&order=clickcount&reverse=true&limit={PAGE}");
    Ok(match ask {
        LiveAsk::Top => format!("/json/stations/search?{common}"),
        LiveAsk::Search { text } => {
            // cut: a search; the first 100 characters find the same stations, and nothing is kept
            let text: String = text.trim().chars().take(MAX_QUERY_CHARS).collect();
            if text.is_empty() {
                format!("/json/stations/search?{common}")
            } else {
                format!(
                    "/json/stations/search?name={}&{common}",
                    encode(&text, false)
                )
            }
        }
        LiveAsk::Country { code } => {
            let code = code.trim().to_ascii_uppercase();
            if code.len() != 2 || !code.chars().all(|c| c.is_ascii_uppercase()) {
                return Err(SoundscapeError::BadAddress {
                    reason: "that isn't a country code".into(),
                });
            }
            format!("/json/stations/search?countrycode={code}&{common}")
        }
        LiveAsk::Language { name } => format!(
            "/json/stations/search?language={}&languageExact=true&{common}",
            encode(&category_key(name)?, false)
        ),
        LiveAsk::Genre { tag } => format!(
            "/json/stations/search?tag={}&tagExact=true&{common}",
            encode(&category_key(tag)?, false)
        ),
    })
}

/// The path listing countries, languages or genres, those with the most
/// stations first. Genres are Radio Browser's tags: only well-used ones.
pub fn categories_path(kind: LiveCategoryKind) -> String {
    let order = format!("order=stationcount&reverse=true&hidebroken=true&limit={CATEGORY_PAGE}");
    match kind {
        LiveCategoryKind::Country => format!("/json/countries?{order}"),
        LiveCategoryKind::Language => format!("/json/languages?{order}"),
        LiveCategoryKind::Genre => format!("/json/tags?{order}"),
    }
}

/// The path looking up one station by UUID.
pub fn station_path(uuid: &str) -> String {
    format!("/json/stations/byuuid/{uuid}")
}

fn category_key(text: &str) -> Result<String, SoundscapeError> {
    let text = text.trim().to_lowercase();
    if text.is_empty() || text.chars().count() > 64 {
        return Err(SoundscapeError::BadAddress {
            reason: "that isn't a language or genre".into(),
        });
    }
    Ok(text)
}

/// Stations from a station list. One without a UUID, a name or an http(s)
/// stream, or in a format AVPlayer can't play, is left out; so is a second
/// copy of the same UUID.
pub fn parse_stations(body: &[u8]) -> Result<Vec<LiveStation>, SoundscapeError> {
    let items = parse_array(body)?;
    let mut seen = std::collections::HashSet::new();
    Ok(items
        .iter()
        .filter_map(station)
        .filter(|s| seen.insert(s.uuid.clone()))
        .collect())
}

/// Countries, languages or genres from a category list. Empty names and
/// those with no stations are left out.
pub fn parse_categories(
    kind: LiveCategoryKind,
    body: &[u8],
) -> Result<Vec<LiveCategory>, SoundscapeError> {
    let items = parse_array(body)?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let count = item["stationcount"]
                .as_u64()
                .or_else(|| item["stationcount"].as_str()?.parse().ok())?;
            if count == 0 {
                return None;
            }
            let name = short(&item["name"])?;
            let (key, name) = match kind {
                LiveCategoryKind::Country => {
                    let code = short(&item["iso_3166_1"])?.to_ascii_uppercase();
                    (code.len() == 2).then_some((code, name))?
                }
                LiveCategoryKind::Language => (name.to_lowercase(), title_case(&name)),
                LiveCategoryKind::Genre => (name.to_lowercase(), title_case(&name)),
            };
            Some(LiveCategory {
                kind,
                key,
                name,
                station_count: u32::try_from(count).unwrap_or(u32::MAX),
            })
        })
        .collect())
}

/// Server names from `/json/servers`, each once, in the order given.
pub fn parse_servers(body: &[u8]) -> Result<Vec<String>, SoundscapeError> {
    let items = parse_array(body)?;
    let mut names: Vec<String> = Vec::new();
    for item in &items {
        if let Some(name) = item["name"].as_str() {
            let name = name.trim().to_ascii_lowercase();
            let fits = name.ends_with(".api.radio-browser.info")
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
            if fits && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    Ok(names)
}

fn parse_array(body: &[u8]) -> Result<Vec<Value>, SoundscapeError> {
    if body.len() > MAX_ANSWER_BYTES {
        return Err(server("Radio Browser's answer is too large"));
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Array(items)) => Ok(items),
        _ => Err(server("Radio Browser's answer can't be read")),
    }
}

fn station(item: &Value) -> Option<LiveStation> {
    let uuid = clean_uuid(item["stationuuid"].as_str()?).ok()?;
    let name = short(&item["name"])?;
    let listed_url = web_url(&item["url"]);
    let stream_url = web_url(&item["url_resolved"]).or_else(|| listed_url.clone())?;
    let codec = short(&item["codec"])
        .map(|c| c.to_ascii_uppercase())
        .filter(|c| c != "UNKNOWN")
        .unwrap_or_default();
    if UNPLAYABLE_CODECS.iter().any(|bad| codec.contains(bad)) {
        return None;
    }
    let hls = flag(&item["hls"]);
    Some(LiveStation {
        uuid,
        name,
        secure: stream_url.starts_with("https://"),
        listed_url: listed_url.unwrap_or_else(|| stream_url.clone()),
        stream_url,
        homepage: web_url(&item["homepage"]),
        artwork_url: web_url(&item["favicon"]),
        country: short(&item["country"]).unwrap_or_default(),
        country_code: short(&item["countrycode"])
            .map(|c| c.to_ascii_uppercase())
            .filter(|c| c.len() == 2)
            .unwrap_or_default(),
        languages: list(&item["language"]),
        tags: list(&item["tags"]),
        codec,
        bitrate_kbps: item["bitrate"]
            .as_u64()
            .and_then(|b| u32::try_from(b).ok())
            .unwrap_or(0),
        hls,
        plays: item["clickcount"].as_u64().unwrap_or(0),
    })
}

fn flag(value: &Value) -> bool {
    value.as_u64() == Some(1) || value.as_bool() == Some(true)
}

/// Trimmed text, cut to [`MAX_TEXT_CHARS`]; `None` when empty.
fn short(value: &Value) -> Option<String> {
    let text: String = value
        .as_str()?
        .trim()
        // cut: a directory's display text; the first 200 characters say enough
        .chars()
        .take(MAX_TEXT_CHARS)
        .collect();
    (!text.is_empty()).then_some(text)
}

/// An http(s) address, or `None`.
fn web_url(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    let lower = text.to_ascii_lowercase();
    let fits = (lower.starts_with("https://") || lower.starts_with("http://"))
        && text.len() <= 2048
        && !text.chars().any(char::is_whitespace);
    fits.then(|| text.to_string())
}

/// A comma-separated list, each item tidied, at most [`MAX_LIST`].
fn list(value: &Value) -> Vec<String> {
    let Some(text) = value.as_str() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for part in text.split(',') {
        // cut: a tag or language in Radio Browser's answer, not typed
        let part: String = part.trim().chars().take(40).collect();
        if !part.is_empty() && !out.iter().any(|p| p.eq_ignore_ascii_case(&part)) {
            out.push(part);
        }
        if out.len() == MAX_LIST {
            break;
        }
    }
    out
}

fn title_case(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn server(reason: &str) -> SoundscapeError {
    SoundscapeError::Server {
        reason: reason.into(),
    }
}

/// Radio Browser's servers: which to ask, moving to the next when one
/// can't be reached, answers with a server error, or says it is too busy
/// (429). A busy server isn't asked again until its `Retry-After` passes.
#[derive(Default)]
pub struct Directory {
    /// The published list once known; until then [`SEED_SERVERS`].
    servers: Mutex<Option<Vec<String>>>,
    /// Servers that asked to be left alone, and until when.
    resting: Mutex<HashMap<String, Instant>>,
}

/// How long a busy server rests when it doesn't say.
pub const BUSY_REST: Duration = Duration::from_secs(30);
/// The longest rest a server's `Retry-After` is followed for.
pub const MAX_REST: Duration = Duration::from_secs(300);

/// How long a server that answered 429 (or 503 with `Retry-After`) asked
/// to rest: its `Retry-After` in seconds, up to [`MAX_REST`], or
/// [`BUSY_REST`] when it gives none or gives a date.
pub fn rest_for(retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds).min(MAX_REST))
        .unwrap_or(BUSY_REST)
}

impl Directory {
    /// Stations for `ask`, most played first.
    pub fn stations(
        &self,
        http: &dyn HttpClient,
        ask: &LiveAsk,
        cancel: &CancelHandle,
    ) -> Result<Vec<LiveStation>, SoundscapeError> {
        let path = stations_path(ask)?;
        parse_stations(&self.get(http, &path, cancel)?)
    }

    /// Countries, languages or genres, those with most stations first.
    pub fn categories(
        &self,
        http: &dyn HttpClient,
        kind: LiveCategoryKind,
        cancel: &CancelHandle,
    ) -> Result<Vec<LiveCategory>, SoundscapeError> {
        parse_categories(kind, &self.get(http, &categories_path(kind), cancel)?)
    }

    /// One station's current details, by UUID. `None` when Radio Browser
    /// no longer lists it (or no longer offers a stream AVPlayer plays).
    pub fn station(
        &self,
        http: &dyn HttpClient,
        uuid: &str,
        cancel: &CancelHandle,
    ) -> Result<Option<LiveStation>, SoundscapeError> {
        let uuid = clean_uuid(uuid)?;
        let found = parse_stations(&self.get(http, &station_path(&uuid), cancel)?)?;
        Ok(found.into_iter().find(|s| s.uuid == uuid))
    }

    /// The servers to try, in order.
    pub fn servers(&self, http: &dyn HttpClient, cancel: &CancelHandle) -> Vec<String> {
        if let Some(known) = self
            .servers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return known;
        }
        for seed in SEED_SERVERS {
            if self.is_resting(seed) {
                continue;
            }
            let request = Request::get(format!("https://{seed}/json/servers")).timeout(TIMEOUT);
            let Ok(response) = http.send_cancellable(&request, cancel) else {
                if cancel.is_cancelled() {
                    break;
                }
                continue;
            };
            if response.status == 429
                || (response.status == 503 && response.header("Retry-After").is_some())
            {
                self.rest(seed, rest_for(response.header("Retry-After")));
                continue;
            }
            if !(200..300).contains(&response.status) {
                continue;
            }
            if let Ok(found) = parse_servers(&response.body) {
                if !found.is_empty() {
                    *self.servers.lock().unwrap_or_else(|e| e.into_inner()) = Some(found.clone());
                    return found;
                }
            }
        }
        SEED_SERVERS.iter().map(|s| (*s).to_string()).collect()
    }

    /// The body of a GET of `path`, from the first server that answers.
    /// A server that can't be reached or fails (5xx) is moved to the back.
    fn get(
        &self,
        http: &dyn HttpClient,
        path: &str,
        cancel: &CancelHandle,
    ) -> Result<Vec<u8>, SoundscapeError> {
        let servers = self.servers(http, cancel);
        let mut last = server("can't reach Radio Browser");
        let mut asked = false;
        for name in &servers {
            if self.is_resting(name) {
                continue;
            }
            asked = true;
            let request = Request::get(format!("https://{name}{path}")).timeout(TIMEOUT);
            match http.send_cancellable(&request, cancel) {
                Ok(response) if (200..300).contains(&response.status) => return Ok(response.body),
                Ok(response)
                    if response.status == 429
                        || (response.status == 503 && response.header("Retry-After").is_some()) =>
                {
                    self.rest(name, rest_for(response.header("Retry-After")));
                    last = server("Radio Browser is busy; try again in a minute");
                }
                Ok(response) if response.status < 500 => {
                    return Err(server(&format!(
                        "Radio Browser answered {}",
                        response.status
                    )));
                }
                Ok(response) => {
                    last = server(&format!("Radio Browser answered {}", response.status));
                }
                Err(NetError::Cancelled) => return Err(SoundscapeError::from(NetError::Cancelled)),
                Err(err) => {
                    if cancel.is_cancelled() {
                        return Err(SoundscapeError::from(err));
                    }
                    last = server("can't reach Radio Browser");
                }
            }
            forge_runtime::logging::warn!("Radio Browser server failed; trying the next");
            self.demote(name);
        }
        if !asked {
            return Err(server("Radio Browser is busy; try again in a minute"));
        }
        Err(last)
    }

    fn is_resting(&self, name: &str) -> bool {
        let mut resting = self.resting.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        resting.retain(|_, until| *until > now);
        resting.contains_key(name)
    }

    fn rest(&self, name: &str, rest: Duration) {
        let until = Instant::now() + rest;
        self.resting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_string(), until);
    }

    fn demote(&self, name: &str) {
        let mut servers = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = servers.as_mut() {
            if let Some(at) = list.iter().position(|s| s == name) {
                let moved = list.remove(at);
                list.push(moved);
            }
        }
    }
}
