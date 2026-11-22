//! Stations: endless playlists made from a seed (a genre, a song or an
//! artist), a batch at a time (`docs/design.md`, Stations). A smarter
//! shuffle: songs you like and play come up more, songs you disliked never,
//! one artist doesn't play twice close together, and part of every batch is
//! music you haven't heard. The mix is pure and repeatable from a seed
//! number, so it is tested on Linux; the core gathers the candidates
//! (`SoundscapeCore::station_batch`).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::collection::CollectionFile;
use crate::jamendo;
use crate::library::{Library, LibraryTrack};

/// Songs in each batch the queue asks for.
pub const BATCH: usize = 10;
/// Share of a batch that is music you haven't heard, when there is some.
pub const NEW_SHARE: f64 = 0.4;
/// The same artist plays at most once in this many songs.
pub const ARTIST_GAP: usize = 4;
/// Suggested stations on the Listen screen.
pub const SUGGESTED: usize = 8;

/// How the person feels about a song, for stations: liked songs come up
/// more, disliked ones never.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Feeling {
    Like,
    Dislike,
}

/// What a station is made from.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StationSeed {
    /// A genre, by name as songs carry it ("Jazz", "lo-fi").
    Genre { name: String },
    /// Songs like this one (by track id).
    Track { id: String },
    /// This artist (by library artist id) and others like them.
    Artist { id: String },
}

/// A station to start, for the Listen screen's shelf.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Station {
    pub title: String,
    /// Why it is offered ("From your library", "On Jamendo").
    pub subtitle: String,
    pub seed: StationSeed,
    /// Up to four covers from it, for the tile.
    pub artwork_urls: Vec<String>,
    /// The person saved it: it comes first.
    pub saved: bool,
}

/// A song that could play next on a station.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub id: String,
    /// For spacing; compared without case.
    pub artist: String,
    /// How strongly it belongs (above 0).
    pub weight: f64,
    /// Played before (in the history).
    pub heard: bool,
}

/// What the station remembers between batches.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Played {
    /// Songs the station has already given.
    pub ids: HashSet<String>,
    /// The last few artists, newest last, so the next batch keeps the gap.
    pub artists: Vec<String>,
}

/// The next `size` song ids: about [`NEW_SHARE`] of them unheard, the rest
/// heard songs picked by weight, never one already played on the station
/// while others remain, and no artist within [`ARTIST_GAP`] songs of
/// itself unless nothing else is left. Repeatable for the same `seed`.
pub fn mix(candidates: &[Candidate], played: &Played, size: usize, seed: u64) -> Vec<String> {
    let mut rng = SplitMix(seed);
    let fresh: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.weight > 0.0 && !played.ids.contains(&c.id))
        .collect();
    // A station that has played everything starts over rather than stop.
    let pool: Vec<&Candidate> = if fresh.is_empty() {
        candidates.iter().filter(|c| c.weight > 0.0).collect()
    } else {
        fresh
    };
    let (unheard, heard): (Vec<&Candidate>, Vec<&Candidate>) =
        pool.into_iter().partition(|c| !c.heard);
    let want_new = ((size as f64) * NEW_SHARE).ceil() as usize;
    let mut chosen = weighted_sample(&unheard, want_new.min(unheard.len()), &mut rng);
    let rest = size.saturating_sub(chosen.len());
    chosen.extend(weighted_sample(&heard, rest.min(heard.len()), &mut rng));
    // Not enough heard songs: fill up with more new ones.
    if chosen.len() < size {
        let taken: HashSet<&str> = chosen.iter().map(|c| c.id.as_str()).collect();
        let more: Vec<&Candidate> = unheard
            .iter()
            .copied()
            .filter(|c| !taken.contains(c.id.as_str()))
            .collect();
        chosen.extend(weighted_sample(
            &more,
            (size - chosen.len()).min(more.len()),
            &mut rng,
        ));
    }
    shuffle(&mut chosen, &mut rng);
    space_artists(chosen, &played.artists)
        .into_iter()
        .map(|c| c.id.clone())
        .collect()
}

/// `count` candidates without repeats, each picked with a chance in
/// proportion to its weight (Efraimidis–Spirakis keys).
fn weighted_sample<'a>(
    from: &[&'a Candidate],
    count: usize,
    rng: &mut SplitMix,
) -> Vec<&'a Candidate> {
    let mut keyed: Vec<(f64, &'a Candidate)> = from
        .iter()
        .map(|c| {
            let u = rng.unit().max(f64::MIN_POSITIVE);
            (u.powf(1.0 / c.weight), *c)
        })
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    keyed.into_iter().take(count).map(|(_, c)| c).collect()
}

fn shuffle<T>(items: &mut [T], rng: &mut SplitMix) {
    for i in (1..items.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/// Orders the batch so an artist doesn't come back within [`ARTIST_GAP`]
/// songs (counting the artists just played), when another song can go
/// there instead.
fn space_artists<'a>(mut left: Vec<&'a Candidate>, recent: &[String]) -> Vec<&'a Candidate> {
    let mut lately: Vec<String> = recent.iter().map(|a| a.to_lowercase()).collect();
    let mut out = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let window: HashSet<&str> = lately
            .iter()
            .rev()
            .take(ARTIST_GAP - 1)
            .map(String::as_str)
            .collect();
        let at = left
            .iter()
            .position(|c| !window.contains(c.artist.to_lowercase().as_str()))
            .unwrap_or(0);
        let next = left.remove(at);
        lately.push(next.artist.to_lowercase());
        out.push(next);
    }
    out
}

/// A small, repeatable random number generator (SplitMix64).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ------------------------------------------------------------ the library's part

/// Lower case, letters and digits only: "Lo-Fi" and "lofi" match.
pub fn genre_key(genre: &str) -> String {
    genre
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The genre a seed plays: the genre itself, the song's, or the artist's
/// most common one.
pub fn seed_genre(seed: &StationSeed, library: &Library) -> Option<String> {
    match seed {
        StationSeed::Genre { name } => Some(name.clone()),
        StationSeed::Track { id } => library
            .tracks
            .iter()
            .find(|t| &t.id == id)
            .and_then(|t| t.genre.clone()),
        StationSeed::Artist { id } => {
            let tracks: Vec<&LibraryTrack> = library
                .tracks
                .iter()
                .filter(|t| t.artist_id.as_deref() == Some(id.as_str()))
                .collect();
            most_common(tracks.iter().filter_map(|t| t.genre.as_deref()))
        }
    }
}

/// The artist id a seed is about, if any.
fn seed_artist(seed: &StationSeed, library: &Library) -> Option<String> {
    match seed {
        StationSeed::Genre { .. } => None,
        StationSeed::Track { id } => library
            .tracks
            .iter()
            .find(|t| &t.id == id)
            .and_then(|t| t.artist_id.clone()),
        StationSeed::Artist { id } => Some(id.clone()),
    }
}

/// Times each track was played.
pub fn play_counts(collection: &CollectionFile) -> HashMap<&str, u32> {
    let mut counts = HashMap::new();
    for play in &collection.history {
        *counts.entry(play.track_id.as_str()).or_insert(0) += 1;
    }
    counts
}

/// The library's songs that belong on the station, weighted: in the seed's
/// genre (or by the seed's artist), up for favourites, likes and plays;
/// disliked songs left out. With no genre to go on, the whole library at
/// a low weight, so a station always plays something.
pub fn library_candidates(
    seed: &StationSeed,
    library: &Library,
    collection: &CollectionFile,
) -> Vec<Candidate> {
    // A song from a catalogue that isn't in the library: the catalogue
    // finds songs like it, not the whole library.
    if let StationSeed::Track { id } = seed {
        if !library.tracks.iter().any(|t| &t.id == id) {
            return Vec::new();
        }
    }
    let genre = seed_genre(seed, library)
        .map(|g| genre_key(&g))
        .filter(|g| !g.is_empty());
    let artist = seed_artist(seed, library);
    let plays = play_counts(collection);
    library
        .tracks
        .iter()
        .filter(|t| t.preferred.is_some())
        .filter(|t| collection.feelings.get(&t.id) != Some(&Feeling::Dislike))
        .filter_map(|t| {
            let same_genre = match (&genre, &t.genre) {
                (Some(g), Some(tg)) => genre_key(tg) == *g,
                _ => false,
            };
            let same_artist = artist.is_some() && t.artist_id == artist;
            let mut weight = match (same_genre, same_artist, genre.is_some() || artist.is_some()) {
                (_, true, _) => 2.0,
                (true, false, _) => 1.5,
                (false, false, true) => return None,
                (false, false, false) => 0.5,
            };
            if collection.is_favourite(&t.id) {
                weight += 2.0;
            }
            if collection.feelings.get(&t.id) == Some(&Feeling::Like) {
                weight += 3.0;
            }
            let count = plays.get(t.id.as_str()).copied().unwrap_or(0);
            weight += (count as f64 * 0.5).min(3.0);
            Some(Candidate {
                id: t.id.clone(),
                artist: t.artist.clone(),
                weight,
                heard: count > 0,
            })
        })
        .collect()
}

/// Jamendo's tag for a genre, when Jamendo has one by that name or tag.
pub fn jamendo_tag(genre: &str) -> Option<String> {
    let key = genre_key(genre);
    if key.is_empty() {
        return None;
    }
    jamendo::genres()
        .into_iter()
        .find(|g| genre_key(&g.tag) == key || genre_key(&g.name) == key)
        .map(|g| g.tag)
        .or_else(|| jamendo::clean_tag(&key).ok())
}

/// Jamendo's genres for Discover's shelves, `limit` of them: the ones your
/// library plays and keeps most first (only genres Jamendo has), then
/// Jamendo's own order.
pub fn discover_genres(
    library: &Library,
    collection: &CollectionFile,
    limit: usize,
) -> Vec<jamendo::JamendoGenre> {
    let plays = play_counts(collection);
    let mut score: BTreeMap<String, f64> = BTreeMap::new();
    for t in &library.tracks {
        if let Some(g) = t.genre.as_deref() {
            let key = genre_key(g);
            if !key.is_empty() {
                *score.entry(key).or_insert(0.0) +=
                    1.0 + plays.get(t.id.as_str()).copied().unwrap_or(0) as f64;
            }
        }
    }
    let all = jamendo::genres();
    let mut ranked: Vec<(f64, usize)> = all
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let mine = score
                .get(&genre_key(&g.name))
                .or_else(|| score.get(&genre_key(&g.tag)))
                .copied()
                .unwrap_or(0.0);
            (mine, i)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, i)| all[i].clone())
        .collect()
}

/// Stations to offer: the saved ones, newest first, then
/// [`SUGGESTED`] more: your most played and most kept genres, your most
/// played artists, then (with a Jamendo client ID and room left) Jamendo's
/// genres, each once and none already saved.
pub fn suggested(
    library: &Library,
    collection: &CollectionFile,
    with_jamendo: bool,
) -> Vec<Station> {
    let saved: Vec<Station> = collection
        .saved_stations
        .iter()
        .map(|s| Station {
            title: s.title.clone(),
            subtitle: "Saved station".into(),
            artwork_urls: artwork_for(&s.seed, library),
            seed: s.seed.clone(),
            saved: true,
        })
        .collect();
    let mut more = generated(library, collection, with_jamendo);
    more.retain(|m| !saved.iter().any(|s| s.seed == m.seed));
    more.truncate(SUGGESTED);
    saved.into_iter().chain(more).collect()
}

fn generated(library: &Library, collection: &CollectionFile, with_jamendo: bool) -> Vec<Station> {
    let plays = play_counts(collection);
    let mut genre_score: BTreeMap<String, (String, f64)> = BTreeMap::new();
    let mut artist_score: BTreeMap<String, (String, f64)> = BTreeMap::new();
    for t in &library.tracks {
        let score = 1.0 + plays.get(t.id.as_str()).copied().unwrap_or(0) as f64;
        if let Some(g) = t.genre.as_deref() {
            let key = genre_key(g);
            if !key.is_empty() {
                genre_score
                    .entry(key)
                    .or_insert_with(|| (g.to_string(), 0.0))
                    .1 += score;
            }
        }
        if let Some(a) = &t.artist_id {
            artist_score
                .entry(a.clone())
                .or_insert_with(|| (t.artist.clone(), 0.0))
                .1 += plays.get(t.id.as_str()).copied().unwrap_or(0) as f64;
        }
    }
    let mut out = Vec::new();
    let mut genres: Vec<(String, f64)> = genre_score.into_values().collect();
    genres.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut seen_genres = HashSet::new();
    for (name, _) in genres.into_iter().take(SUGGESTED / 2) {
        seen_genres.insert(genre_key(&name));
        let seed = StationSeed::Genre { name: name.clone() };
        out.push(Station {
            title: format!("{name} Station"),
            subtitle: "From your library".into(),
            artwork_urls: artwork_for(&seed, library),
            seed,
            saved: false,
        });
    }
    let mut artists: Vec<(String, (String, f64))> = artist_score
        .into_iter()
        .filter(|(_, (_, s))| *s > 0.0)
        .collect();
    artists.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1).then_with(|| a.1 .0.cmp(&b.1 .0)));
    for (id, (name, _)) in artists.into_iter().take(SUGGESTED / 4) {
        let seed = StationSeed::Artist { id };
        out.push(Station {
            title: format!("{name} Station"),
            subtitle: "Your most played".into(),
            artwork_urls: artwork_for(&seed, library),
            seed,
            saved: false,
        });
    }
    if with_jamendo {
        for g in jamendo::genres() {
            if out.len() >= SUGGESTED {
                break;
            }
            if seen_genres.insert(genre_key(&g.name)) {
                out.push(Station {
                    title: format!("{} Station", g.name),
                    subtitle: "New music on Jamendo".into(),
                    seed: StationSeed::Genre { name: g.name },
                    artwork_urls: Vec::new(),
                    saved: false,
                });
            }
        }
    }
    out.truncate(SUGGESTED);
    out
}

fn artwork_for(seed: &StationSeed, library: &Library) -> Vec<String> {
    let genre = seed_genre(seed, library).map(|g| genre_key(&g));
    let artist = seed_artist(seed, library);
    let mut urls: Vec<String> = Vec::new();
    for t in &library.tracks {
        let fits = match seed {
            StationSeed::Artist { .. } => t.artist_id == artist,
            _ => genre.is_some() && t.genre.as_deref().map(genre_key) == genre,
        };
        if let (true, Some(url)) = (fits, &t.artwork_url) {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
        if urls.len() == 4 {
            break;
        }
    }
    urls
}

fn most_common<'a>(items: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for item in items {
        let key = genre_key(item);
        if key.is_empty() {
            continue;
        }
        counts.entry(key).or_insert_with(|| (item.to_string(), 0)).1 += 1;
    }
    counts
        .into_values()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(name, _)| name)
}
