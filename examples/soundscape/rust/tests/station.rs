//! Stations (radio): the mix rules on their own, then a station from a
//! real core's library with likes and dislikes.

use std::collections::HashSet;
use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::library::track_id;
use soundscape_core::station::{self, Candidate, Played, ARTIST_GAP};
use soundscape_core::{Feeling, SoundscapeCore, SoundscapeError, StationSeed, TrackEdit};

fn candidate(id: &str, artist: &str, weight: f64, heard: bool) -> Candidate {
    Candidate {
        id: id.into(),
        artist: artist.into(),
        weight,
        heard,
    }
}

fn many(artists: usize, songs_each: usize, heard: bool) -> Vec<Candidate> {
    let mut out = Vec::new();
    for a in 0..artists {
        for s in 0..songs_each {
            out.push(candidate(
                &format!("{}{a}-{s}", if heard { "h" } else { "n" }),
                &format!("Artist {a}"),
                1.0,
                heard,
            ));
        }
    }
    out
}

#[test]
fn a_batch_has_new_music_no_repeats_and_spaced_artists() {
    let mut all = many(6, 5, true);
    all.extend(many(6, 5, false).into_iter().map(|mut c| {
        c.artist = format!("New {}", c.artist);
        c
    }));
    let batch = station::mix(&all, &Played::default(), 10, 7);
    assert_eq!(batch.len(), 10);
    assert_eq!(
        batch.iter().collect::<HashSet<_>>().len(),
        10,
        "no song twice"
    );
    let new = batch.iter().filter(|id| id.starts_with('n')).count();
    assert_eq!(new, 4, "two in five are new");
    let artist = |id: &String| all.iter().find(|c| &c.id == id).unwrap().artist.clone();
    for (i, id) in batch.iter().enumerate() {
        for earlier in batch[i.saturating_sub(ARTIST_GAP - 1)..i].iter() {
            assert_ne!(
                artist(id),
                artist(earlier),
                "artist repeated within the gap: {batch:?}"
            );
        }
    }
}

#[test]
fn the_same_seed_gives_the_same_mix_and_another_seed_another() {
    let all = many(8, 4, true);
    let a = station::mix(&all, &Played::default(), 10, 42);
    assert_eq!(a, station::mix(&all, &Played::default(), 10, 42));
    assert_ne!(a, station::mix(&all, &Played::default(), 10, 43));
}

#[test]
fn the_next_batch_leaves_out_what_played_and_keeps_the_gap_across_batches() {
    let all = many(5, 4, true);
    let first = station::mix(&all, &Played::default(), 10, 1);
    let artist = |id: &String| all.iter().find(|c| &c.id == id).unwrap().artist.clone();
    let played = Played {
        ids: first.iter().cloned().collect(),
        artists: first.iter().map(artist).collect(),
    };
    let second = station::mix(&all, &played, 10, 2);
    assert!(second.iter().all(|id| !played.ids.contains(id)));
    assert_ne!(artist(&second[0]), artist(first.last().unwrap()));
    // Everything played: it starts over rather than stopping.
    let everything = Played {
        ids: all.iter().map(|c| c.id.clone()).collect(),
        artists: Vec::new(),
    };
    assert_eq!(station::mix(&all, &everything, 5, 3).len(), 5);
}

#[test]
fn heavier_songs_come_up_more_and_weightless_ones_never() {
    let mut all = many(10, 1, true);
    all[0].weight = 20.0;
    all[1].weight = 0.0;
    let mut first = 0;
    for seed in 0..200 {
        let batch = station::mix(&all, &Played::default(), 3, seed);
        assert!(!batch.contains(&all[1].id));
        if batch.contains(&all[0].id) {
            first += 1;
        }
    }
    assert!(
        first > 180,
        "a liked song should nearly always make a batch: {first}"
    );
}

#[test]
fn one_artist_alone_still_plays() {
    let all = many(1, 6, true);
    assert_eq!(station::mix(&all, &Played::default(), 6, 9).len(), 6);
}

#[test]
fn genres_match_without_case_or_punctuation_and_find_jamendo_tags() {
    assert_eq!(station::genre_key("Lo-Fi"), station::genre_key("lofi"));
    assert_eq!(station::jamendo_tag("Hip Hop").as_deref(), Some("hiphop"));
    assert_eq!(
        station::jamendo_tag("Electronic").as_deref(),
        Some("electronic")
    );
    assert_eq!(station::jamendo_tag(""), None);
}

// ------------------------------------------------------------ with a core

const SERVER: &str = "http://100.64.0.7:8642";

const CATALOG: &str = r#"{"version": 1,
 "artists": [{"id": "a", "name": "A", "artwork": null}, {"id": "b", "name": "B", "artwork": null}],
 "albums": [],
 "tracks": [
  {"slug": "one", "title": "One", "artist": "a", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/one", "file": "r/one.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "two", "title": "Two", "artist": "b", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/two", "file": "r/two.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "three", "title": "Three", "artist": "a", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/three", "file": "r/three.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "four", "title": "Four", "artist": "b", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/four", "file": "r/four.mp3", "label": "a", "mtime": 1, "seconds": 100}]}
 ]}"#;

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    let http = StaticClient::new().route(
        Method::Get,
        format!("{SERVER}/api/catalog"),
        Ok(Response::new(200, CATALOG)),
    );
    let core = SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(http),
    )
    .unwrap();
    block(core.set_listen_server(SERVER.into())).unwrap();
    block(core.refresh_library(false, None)).unwrap();
    for (slug, genre) in [
        ("one", "Jazz"),
        ("two", "jazz"),
        ("three", "Jazz"),
        ("four", "Rock"),
    ] {
        let edit = TrackEdit {
            title: None,
            artist: None,
            album: None,
            number: None,
            genre: Some(genre.into()),
        };
        block(core.edit_track(track_id(slug), edit)).unwrap();
    }
    core
}

fn ids(tracks: &[soundscape_core::QueueTrack]) -> Vec<String> {
    tracks.iter().map(|t| t.track_id.clone()).collect()
}

#[test]
fn a_genre_station_plays_that_genre_and_never_a_disliked_song() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block(core.set_feeling(track_id("two"), Some(Feeling::Dislike))).unwrap();
    assert_eq!(core.feeling(track_id("two")), Some(Feeling::Dislike));
    let seed = StationSeed::Genre {
        name: "JAZZ".into(),
    };
    let batch = block(core.station_batch(seed, vec![], vec![], 10, 5, None)).unwrap();
    let mut got = ids(&batch);
    got.sort();
    assert_eq!(got, [track_id("one"), track_id("three")]);

    // Forgetting the dislike lets it back.
    block(core.set_feeling(track_id("two"), None)).unwrap();
    let seed = StationSeed::Genre {
        name: "Jazz".into(),
    };
    let batch = block(core.station_batch(seed, vec![], vec![], 10, 5, None)).unwrap();
    assert_eq!(batch.len(), 3);
}

#[test]
fn a_song_station_starts_with_the_song_and_survives_a_restart_of_feelings() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block(core.set_feeling(track_id("three"), Some(Feeling::Like))).unwrap();
    let seed = StationSeed::Track {
        id: track_id("one"),
    };
    let batch = block(core.station_batch(seed, vec![], vec![], 10, 1, None)).unwrap();
    assert_eq!(batch[0].track_id, track_id("one"));
    assert!(
        !ids(&batch).contains(&track_id("four")),
        "rock isn't on a jazz song's station"
    );
    drop(core);
    let core = open(&dir);
    assert_eq!(core.feeling(track_id("three")), Some(Feeling::Like));
}

#[test]
fn a_station_with_nothing_to_play_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let seed = StationSeed::Genre {
        name: "Polka".into(),
    };
    let error = block(core.station_batch(seed, vec![], vec![], 10, 1, None)).unwrap_err();
    assert!(matches!(error, SoundscapeError::NotInLibrary { .. }));
}

#[test]
fn suggested_stations_lead_with_your_genres() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let stations = core.suggested_stations();
    assert_eq!(stations[0].title, "Jazz Station");
    assert!(stations.iter().any(|s| s.title == "Rock Station"));
    // No Jamendo client ID: no Jamendo genres.
    assert!(stations
        .iter()
        .all(|s| s.subtitle != "New music on Jamendo"));
}

#[test]
fn jamendo_failing_with_nothing_else_to_play_is_its_error_not_an_empty_station() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    // The test server answers nothing for Jamendo.
    block(core.set_jamendo_client_id("abc123".into())).unwrap();
    let seed = StationSeed::Genre {
        name: "Polka".into(),
    };
    let error = block(core.station_batch(seed, vec![], vec![], 10, 1, None)).unwrap_err();
    assert!(
        !matches!(error, SoundscapeError::NotInLibrary { .. }),
        "a failed request is worth retrying: {error:?}"
    );
    // With your own music in the genre, the station plays it anyway.
    let seed = StationSeed::Genre {
        name: "Jazz".into(),
    };
    assert_eq!(
        block(core.station_batch(seed, vec![], vec![], 10, 1, None))
            .unwrap()
            .len(),
        3
    );
}
