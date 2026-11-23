//! Smart playlists (M10 in `docs/roadmap.md`): playlists made from rules
//! about the library's details and your plays. Only the rules are kept;
//! the songs are worked out from the library each time, so a smart playlist
//! follows edits, merges, imports and plays without being saved again.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::collection::{CollectionFile, PlayTally, StoredSmartPlaylist};
use crate::library::{sort_key, Library, LibraryTrack};
use crate::model::SoundscapeError;

pub const MAX_SMART_PLAYLISTS: usize = 200;
pub const MAX_RULES: usize = 20;
/// The longest text a rule compares with, in characters.
pub const MAX_RULE_TEXT: usize = 200;
/// The longest "in the last ... days" (about a hundred years).
pub const MAX_DAYS: u32 = 36_500;
pub const MAX_LIMIT: u32 = 5_000;
const DAY_MS: i64 = 86_400_000;

const STORED: &str = "smart:";
const BUILT_IN: &str = "built-in:";

// ------------------------------------------------------------ the rules

/// What a smart playlist holds.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SmartRules {
    /// True: a song passes every rule. False: any one of them. No rules
    /// at all lets every song in.
    pub match_all: bool,
    pub rules: Vec<SmartRule>,
    pub sort: SmartSort,
    /// At most this many songs, after sorting.
    pub limit: Option<u32>,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SmartRule {
    /// Compares the song's details as they show (with your edits), ignoring
    /// capitals and extra spaces. A missing album or genre is empty text, so
    /// "genre equals" nothing finds songs without one.
    Text {
        field: SmartTextField,
        test: SmartTextTest,
        value: String,
    },
    /// A song with no known length fails every length rule.
    Number {
        field: SmartNumberField,
        test: SmartNumberTest,
        value: f64,
    },
    /// A song never played was not played "in the last" days.
    Date {
        field: SmartDateField,
        test: SmartDateTest,
        days: u32,
    },
    /// The song itself is a favourite (not its album or artist).
    Favourite { favourite: bool },
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmartTextField {
    Title,
    Artist,
    Album,
    Genre,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmartTextTest {
    Equals,
    DoesNotEqual,
    Contains,
    DoesNotContain,
    BeginsWith,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmartNumberField {
    PlayCount,
    LengthSeconds,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmartNumberTest {
    /// The same, to the second for a length.
    Equals,
    AtLeast,
    AtMost,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmartDateField {
    Added,
    LastPlayed,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmartDateTest {
    InLast,
    NotInLast,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmartSort {
    Title,
    Artist,
    /// By album, then the song's number on it.
    Album,
    RecentlyAdded,
    MostPlayed,
    RecentlyPlayed,
    /// Songs not played for longest first, never played first of all.
    LeastRecentlyPlayed,
}

/// Refuses rules that can't be kept: too many, text too long or empty
/// where it means nothing, numbers that aren't numbers, days or limits out
/// of range.
pub fn check(rules: &SmartRules) -> Result<(), SoundscapeError> {
    if rules.rules.len() > MAX_RULES {
        return Err(SoundscapeError::range("smart playlist rules"));
    }
    if let Some(limit) = rules.limit {
        if limit == 0 || limit > MAX_LIMIT {
            return Err(SoundscapeError::range("smart playlist limit"));
        }
    }
    for rule in &rules.rules {
        match rule {
            SmartRule::Text { test, value, .. } => {
                if value.chars().count() > MAX_RULE_TEXT {
                    return Err(SoundscapeError::range("rule text"));
                }
                let empty = normalise(value).is_empty();
                if empty && !matches!(test, SmartTextTest::Equals | SmartTextTest::DoesNotEqual) {
                    return Err(SoundscapeError::range("rule text"));
                }
            }
            SmartRule::Number { value, .. } => {
                if !value.is_finite() || *value < 0.0 {
                    return Err(SoundscapeError::range("rule number"));
                }
            }
            SmartRule::Date { days, .. } => {
                if *days == 0 || *days > MAX_DAYS {
                    return Err(SoundscapeError::range("rule days"));
                }
            }
            SmartRule::Favourite { .. } => {}
        }
    }
    Ok(())
}

// ------------------------------------------------------------ what is shown

/// A smart playlist as the library has it now.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SmartPlaylist {
    /// `smart:<n>` for yours, `built-in:<name>` for the ones Soundscape
    /// comes with.
    pub id: String,
    pub name: String,
    /// Built in: can't be changed or deleted (make your own from its rules).
    pub built_in: bool,
    pub rules: SmartRules,
    pub track_count: u32,
    pub duration_seconds: f64,
    /// Up to four different pictures from its first songs, for a mosaic.
    pub artwork_urls: Vec<String>,
    /// When you last changed it; 0 for a built-in one.
    pub modified_ms: i64,
}

/// A song in a smart playlist, with its plays.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SmartEntry {
    pub track: LibraryTrack,
    pub play_count: u32,
    pub last_played_ms: Option<i64>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SmartPlaylistDetail {
    pub playlist: SmartPlaylist,
    pub entries: Vec<SmartEntry>,
}

/// The id of one of your smart playlists.
pub fn stored_id(id: u64) -> String {
    format!("{STORED}{id}")
}

/// Your smart playlist's number, from its id.
pub fn stored_number(id: &str) -> Option<u64> {
    id.strip_prefix(STORED)?.parse().ok()
}

pub fn no_smart_playlist(id: &str) -> SoundscapeError {
    SoundscapeError::NotInLibrary { id: id.into() }
}

/// The smart playlists Soundscape comes with: (id, name, rules).
pub fn built_in() -> Vec<(String, String, SmartRules)> {
    let list = |name: &str, label: &str, match_all, rules, sort, limit| {
        (
            format!("{BUILT_IN}{name}"),
            label.to_string(),
            SmartRules {
                match_all,
                rules,
                sort,
                limit,
            },
        )
    };
    vec![
        list(
            "recently-added",
            "Recently Added",
            true,
            vec![],
            SmartSort::RecentlyAdded,
            Some(50),
        ),
        list(
            "most-played",
            "Most Played",
            true,
            vec![SmartRule::Number {
                field: SmartNumberField::PlayCount,
                test: SmartNumberTest::AtLeast,
                value: 1.0,
            }],
            SmartSort::MostPlayed,
            Some(25),
        ),
        list(
            "never-played",
            "Never Played",
            true,
            vec![SmartRule::Number {
                field: SmartNumberField::PlayCount,
                test: SmartNumberTest::Equals,
                value: 0.0,
            }],
            SmartSort::RecentlyAdded,
            None,
        ),
        list(
            "forgotten-favourites",
            "Forgotten Favourites",
            true,
            vec![
                SmartRule::Favourite { favourite: true },
                SmartRule::Date {
                    field: SmartDateField::LastPlayed,
                    test: SmartDateTest::NotInLast,
                    days: 60,
                },
            ],
            SmartSort::LeastRecentlyPlayed,
            None,
        ),
    ]
}

/// Every smart playlist: the built-in ones, then yours, most recently
/// changed first.
pub fn all(file: &CollectionFile, library: &Library, now: i64) -> Vec<SmartPlaylist> {
    let mut stored: Vec<&StoredSmartPlaylist> = file.smart_playlists.iter().collect();
    stored.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(b.id.cmp(&a.id)));
    let built = built_in()
        .into_iter()
        .map(|(id, name, rules)| (id, name, true, rules, 0));
    let yours = stored.into_iter().map(|s| {
        (
            stored_id(s.id),
            s.name.clone(),
            false,
            s.rules.clone(),
            s.modified_ms,
        )
    });
    built
        .chain(yours)
        .map(|(id, name, built_in, rules, modified_ms)| {
            let entries = songs(&rules, file, library, now);
            summary(id, name, built_in, rules, modified_ms, &entries)
        })
        .collect()
}

/// One smart playlist with its songs.
pub fn detail(
    file: &CollectionFile,
    library: &Library,
    id: &str,
    now: i64,
) -> Result<SmartPlaylistDetail, SoundscapeError> {
    let (name, built_in, rules, modified_ms) = if let Some(n) = stored_number(id) {
        let s = file
            .smart_playlists
            .iter()
            .find(|s| s.id == n)
            .ok_or_else(|| no_smart_playlist(id))?;
        (s.name.clone(), false, s.rules.clone(), s.modified_ms)
    } else {
        let (_, name, rules) = built_in()
            .into_iter()
            .find(|(b, _, _)| b == id)
            .ok_or_else(|| no_smart_playlist(id))?;
        (name, true, rules, 0)
    };
    let entries = songs(&rules, file, library, now);
    Ok(SmartPlaylistDetail {
        playlist: summary(id.to_string(), name, built_in, rules, modified_ms, &entries),
        entries,
    })
}

fn summary(
    id: String,
    name: String,
    built_in: bool,
    rules: SmartRules,
    modified_ms: i64,
    entries: &[SmartEntry],
) -> SmartPlaylist {
    let mut artwork_urls: Vec<String> = Vec::new();
    for e in entries {
        if artwork_urls.len() == 4 {
            break;
        }
        if let Some(url) = &e.track.artwork_url {
            if !artwork_urls.contains(url) {
                artwork_urls.push(url.clone());
            }
        }
    }
    SmartPlaylist {
        id,
        name,
        built_in,
        rules,
        track_count: entries.len() as u32,
        duration_seconds: entries
            .iter()
            .filter_map(|e| e.track.duration_seconds)
            .sum(),
        artwork_urls,
        modified_ms,
    }
}

/// The songs the rules let in, sorted and limited.
pub fn songs(
    rules: &SmartRules,
    file: &CollectionFile,
    library: &Library,
    now: i64,
) -> Vec<SmartEntry> {
    let tallies: HashMap<&str, PlayTally> =
        file.plays.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let tally = |id: &str| tallies.get(id).copied();
    let rule_text: Vec<Option<String>> = rules
        .rules
        .iter()
        .map(|r| match r {
            SmartRule::Text { value, .. } => Some(normalise(value)),
            _ => None,
        })
        .collect();
    let passes = |track: &LibraryTrack, i: usize, rule: &SmartRule| -> bool {
        match rule {
            SmartRule::Text { field, test, .. } => {
                let have = normalise(match field {
                    SmartTextField::Title => &track.title,
                    SmartTextField::Artist => &track.artist,
                    SmartTextField::Album => track.album.as_deref().unwrap_or(""),
                    SmartTextField::Genre => track.genre.as_deref().unwrap_or(""),
                });
                let want = rule_text[i].as_deref().unwrap_or("");
                match test {
                    SmartTextTest::Equals => have == want,
                    SmartTextTest::DoesNotEqual => have != want,
                    SmartTextTest::Contains => have.contains(want),
                    SmartTextTest::DoesNotContain => !have.contains(want),
                    SmartTextTest::BeginsWith => have.starts_with(want),
                }
            }
            SmartRule::Number { field, test, value } => {
                let have = match field {
                    SmartNumberField::PlayCount => {
                        Some(tally(&track.id).map_or(0, |t| t.count) as f64)
                    }
                    SmartNumberField::LengthSeconds => track.duration_seconds,
                };
                let Some(have) = have else {
                    return false;
                };
                match test {
                    SmartNumberTest::Equals => (have - value).abs() < 0.5,
                    SmartNumberTest::AtLeast => have >= *value,
                    SmartNumberTest::AtMost => have <= *value,
                }
            }
            SmartRule::Date { field, test, days } => {
                let since = now.saturating_sub(i64::from(*days) * DAY_MS);
                let when = match field {
                    SmartDateField::Added => Some(track.added_ms),
                    SmartDateField::LastPlayed => tally(&track.id).map(|t| t.last_ms),
                };
                let recent = when.is_some_and(|w| w >= since);
                match test {
                    SmartDateTest::InLast => recent,
                    SmartDateTest::NotInLast => !recent,
                }
            }
            SmartRule::Favourite { favourite } => file.is_favourite(&track.id) == *favourite,
        }
    };
    let mut entries: Vec<SmartEntry> = library
        .tracks
        .iter()
        .filter(|track| {
            let mut each = rules
                .rules
                .iter()
                .enumerate()
                .map(|(i, r)| passes(track, i, r));
            if rules.rules.is_empty() {
                true
            } else if rules.match_all {
                each.all(|p| p)
            } else {
                each.any(|p| p)
            }
        })
        .map(|track| {
            let t = tally(&track.id);
            SmartEntry {
                track: track.clone(),
                play_count: t.map_or(0, |t| t.count),
                last_played_ms: t.map(|t| t.last_ms),
            }
        })
        .collect();
    let by_title = |a: &SmartEntry, b: &SmartEntry| {
        sort_key(&a.track.title)
            .cmp(&sort_key(&b.track.title))
            .then(a.track.id.cmp(&b.track.id))
    };
    entries.sort_by(|a, b| {
        let first = match rules.sort {
            SmartSort::Title => Ordering::Equal,
            SmartSort::Artist => sort_key(&a.track.artist).cmp(&sort_key(&b.track.artist)),
            SmartSort::Album => sort_key(a.track.album.as_deref().unwrap_or(""))
                .cmp(&sort_key(b.track.album.as_deref().unwrap_or("")))
                .then(a.track.number.cmp(&b.track.number)),
            SmartSort::RecentlyAdded => b.track.added_ms.cmp(&a.track.added_ms),
            SmartSort::MostPlayed => b
                .play_count
                .cmp(&a.play_count)
                .then(b.last_played_ms.cmp(&a.last_played_ms)),
            SmartSort::RecentlyPlayed => b.last_played_ms.cmp(&a.last_played_ms),
            // None (never played) sorts before any time.
            SmartSort::LeastRecentlyPlayed => a.last_played_ms.cmp(&b.last_played_ms),
        };
        first.then_with(|| by_title(a, b))
    });
    if let Some(limit) = rules.limit {
        entries.truncate(limit as usize);
    }
    entries
}

/// Lower case, with runs of spaces as one and none at the ends.
fn normalise(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
