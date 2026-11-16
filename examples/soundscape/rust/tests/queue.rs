//! The play queue: order, shuffle, repeat, and the take each song plays.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::queue::{
    preferred_take, queue_add, queue_advance, queue_back, queue_current, queue_jump, queue_remove,
    queue_set_shuffle, queue_start, queue_upcoming, repeat_after,
};
use soundscape_core::{QueueTrack, RepeatMode, SongDetail, SoundscapeCore, Take};

fn take(id: &str, stars: u8, picked: bool, made_ms: i64) -> Take {
    Take {
        id: id.into(),
        label: id.into(),
        audio_url: format!("http://x/audio/{id}.mp3"),
        seed: None,
        model: None,
        bpm: None,
        key: None,
        caption: None,
        lyrics: None,
        lufs: None,
        stars,
        note: String::new(),
        picked,
        made_ms,
    }
}

fn track(slug: &str) -> QueueTrack {
    QueueTrack {
        slug: slug.into(),
        song_title: slug.into(),
        act: None,
        take: take(slug, 0, false, 0),
    }
}

fn tracks(n: usize) -> Vec<QueueTrack> {
    (0..n).map(|i| track(&format!("s{i}"))).collect()
}

fn now(q: &soundscape_core::PlayQueue) -> String {
    queue_current(q.clone()).unwrap().slug
}

#[test]
fn a_song_plays_its_pick_else_its_best_rated_else_its_newest_take() {
    let song = |takes| SongDetail {
        slug: "s".into(),
        title: "S".into(),
        act: None,
        takes,
        caption: None,
        lyrics: None,
    };
    let picked = song(vec![take("a", 5, false, 3), take("b", 1, true, 1)]);
    assert_eq!(preferred_take(picked).unwrap().id, "b");
    let rated = song(vec![
        take("a", 2, false, 3),
        take("b", 4, false, 1),
        take("c", 4, false, 2),
    ]);
    assert_eq!(preferred_take(rated).unwrap().id, "c");
    let unrated = song(vec![take("a", 0, false, 3), take("b", 0, false, 9)]);
    assert_eq!(preferred_take(unrated).unwrap().id, "b");
    assert_eq!(preferred_take(song(vec![])), None);
}

#[test]
fn songs_play_in_order_and_stop_at_the_end() {
    let q = queue_start(tracks(3), 1, false, RepeatMode::Off, 0);
    assert_eq!(now(&q), "s1");
    let q = queue_advance(q, false).unwrap();
    assert_eq!(now(&q), "s2");
    assert_eq!(queue_advance(q.clone(), false), None);
    assert_eq!(queue_advance(q.clone(), true), None);
    let q = queue_back(queue_back(queue_back(q)));
    assert_eq!(now(&q), "s0", "back stops at the first song");
    assert_eq!(now(&queue_jump(q.clone(), 2)), "s2");
    assert_eq!(
        now(&queue_jump(q, 9)),
        "s0",
        "a jump past the end is ignored"
    );
}

#[test]
fn repeat_all_wraps_and_repeat_one_replays_until_next_is_tapped() {
    let q = queue_start(tracks(2), 1, false, RepeatMode::All, 0);
    let q = queue_advance(q, false).unwrap();
    assert_eq!(now(&q), "s0");
    assert_eq!(now(&queue_back(q)), "s1", "back from the first wraps too");

    let q = queue_start(tracks(2), 0, false, RepeatMode::One, 0);
    assert_eq!(now(&queue_advance(q.clone(), false).unwrap()), "s0");
    assert_eq!(now(&queue_advance(q, true).unwrap()), "s1");

    assert_eq!(repeat_after(RepeatMode::Off), RepeatMode::All);
    assert_eq!(repeat_after(RepeatMode::All), RepeatMode::One);
    assert_eq!(repeat_after(RepeatMode::One), RepeatMode::Off);
}

#[test]
fn shuffle_keeps_the_song_playing_and_plays_every_other_song_once() {
    let q = queue_start(tracks(8), 3, false, RepeatMode::Off, 0);
    let s = queue_set_shuffle(q, true, 42);
    assert!(s.shuffle);
    assert_eq!(now(&s), "s3");
    assert_eq!(s.position, 0);
    let mut seen: Vec<u32> = s.order.clone();
    seen.sort_unstable();
    assert_eq!(seen, (0..8).collect::<Vec<_>>());
    assert_ne!(
        s.order,
        vec![3, 0, 1, 2, 4, 5, 6, 7],
        "seed 42 changes the order"
    );
    let again = queue_set_shuffle(
        queue_start(tracks(8), 3, false, RepeatMode::Off, 0),
        true,
        42,
    );
    assert_eq!(s.order, again.order, "same seed, same order");

    let moved = queue_advance(s, true).unwrap();
    let playing = now(&moved);
    let off = queue_set_shuffle(moved, false, 0);
    assert!(!off.shuffle);
    assert_eq!(off.order, (0..8).collect::<Vec<_>>());
    assert_eq!(
        now(&off),
        playing,
        "turning shuffle off keeps the song playing"
    );

    let started = queue_start(tracks(5), 2, true, RepeatMode::Off, 7);
    assert_eq!(
        now(&started),
        "s2",
        "a shuffled start plays the chosen song first"
    );
}

#[test]
fn songs_can_be_added_next_or_last_and_removed() {
    let q = queue_start(tracks(3), 0, false, RepeatMode::Off, 0);
    let q = queue_add(q, vec![track("next")], true);
    let q = queue_add(q, vec![track("last")], false);
    let upcoming: Vec<String> = queue_upcoming(q.clone())
        .into_iter()
        .map(|e| e.track.slug)
        .collect();
    assert_eq!(upcoming, ["next", "s1", "s2", "last"]);

    let q = queue_jump(q, 2);
    assert_eq!(now(&q), "s1");
    let q = queue_remove(q, 0);
    assert_eq!(
        now(&q),
        "s1",
        "removing an earlier song keeps the one playing"
    );
    assert_eq!(q.position, 1);
    let same = queue_remove(q.clone(), q.position);
    assert_eq!(same, q, "the song playing can't be removed");
    let upcoming = queue_upcoming(q);
    assert_eq!(upcoming[0].position, 2);
    assert_eq!(upcoming[0].track.slug, "s2");

    let empty = queue_start(vec![], 0, true, RepeatMode::All, 1);
    assert_eq!(queue_current(empty.clone()), None);
    assert_eq!(queue_advance(empty.clone(), true), None);
    let added = queue_add(empty, vec![track("only")], true);
    assert_eq!(now(&added), "only");
}

const SONG_A: &str = r#"{"slug": "a", "title": "LOWBRIDGE · Alpha", "actTitle": "LOWBRIDGE", "pick": "a-2",
 "takes": [{"id": "a-1", "file": "a-1.mp3", "stars": 5}, {"id": "a-2", "file": "a-2.mp3"}]}"#;
const SONG_C: &str = r#"{"slug": "c", "title": "Gamma", "takes": []}"#;

#[test]
fn a_list_of_songs_becomes_tracks_with_their_preferred_takes() {
    const SERVER: &str = "http://100.64.0.7:8642";
    let http = StaticClient::new()
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=a"),
            Ok(Response::new(200, SONG_A)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=gone"),
            Ok(Response::new(404, "")),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=c"),
            Ok(Response::new(200, SONG_C)),
        );
    let dir = tempfile::tempdir().unwrap();
    let core = SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(http),
    )
    .unwrap();
    forge_runtime::block_on(core.set_listen_server(SERVER.into())).unwrap();
    let tracks = forge_runtime::block_on(
        core.listen_tracks(vec!["a".into(), "gone".into(), "c".into()], None),
    )
    .unwrap();
    assert_eq!(
        tracks.len(),
        1,
        "a song that's gone or has no takes is left out"
    );
    assert_eq!(tracks[0].take.id, "a-2");
    assert_eq!(tracks[0].song_title, "Alpha");
    assert_eq!(tracks[0].act.as_deref(), Some("LOWBRIDGE"));
    assert_eq!(tracks[0].take.audio_url, format!("{SERVER}/audio/a-2.mp3"));
}
