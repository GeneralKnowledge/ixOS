//! The play queue: order, shuffle, repeat, and the take each song plays.

use soundscape_core::queue::{
    queue_add, queue_advance, queue_back, queue_current, queue_jump, queue_remove,
    queue_set_shuffle, queue_start, queue_upcoming, repeat_after,
};
use soundscape_core::{AudioSource, QueueTrack, Recording, RecordingKind, RepeatMode};

fn track(slug: &str) -> QueueTrack {
    QueueTrack {
        track_id: slug.into(),
        title: slug.into(),
        artist: "Band".into(),
        album: None,
        artwork_url: None,
        recording: Recording {
            id: slug.into(),
            label: slug.into(),
            kind: RecordingKind::Generated,
            source: AudioSource::TakeServer {
                server: "http://x".into(),
            },
            audio_url: format!("http://x/audio/{slug}.mp3"),
            made_ms: 0,
            duration_seconds: None,
            lufs: None,
            stars: 0,
            note: String::new(),
            preferred: true,
            generation: None,
            lyrics: None,
            rights: None,
            merged_from: None,
        },
    }
}

fn tracks(n: usize) -> Vec<QueueTrack> {
    (0..n).map(|i| track(&format!("s{i}"))).collect()
}

fn now(q: &soundscape_core::PlayQueue) -> String {
    queue_current(q.clone()).unwrap().track_id
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
        .map(|e| e.track.track_id)
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
    assert_eq!(upcoming[0].track.track_id, "s2");

    let empty = queue_start(vec![], 0, true, RepeatMode::All, 1);
    assert_eq!(queue_current(empty.clone()), None);
    assert_eq!(queue_advance(empty.clone(), true), None);
    let added = queue_add(empty, vec![track("only")], true);
    assert_eq!(now(&added), "only");
}
