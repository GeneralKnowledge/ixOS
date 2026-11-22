//! Smart playlists (M10): rules over the library and your plays, worked out
//! each time, against a canned take server and an imported file.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::library::track_id;
use soundscape_core::{
    DateField, DateTest, MusicFile, NumberField, NumberTest, SmartRule, SmartRules, SmartSort,
    SoundscapeCore, SoundscapeError, TextField, TextTest, TrackEdit,
};

const SERVER: &str = "http://100.64.0.7:8642";

const CATALOG: &str = r#"{"version": 1,
 "artists": [{"id": "after-hours", "name": "AFTER HOURS", "artwork": null},
             {"id": "lowbridge", "name": "LOWBRIDGE", "artwork": null}],
 "albums": [{"id": "after-hours", "title": "AFTER HOURS", "artist": "after-hours", "artwork": null}],
 "tracks": [
  {"slug": "ah-one-more-night", "title": "One More Night", "artist": "after-hours", "album": "after-hours",
   "number": 1, "artwork": null, "pick": null, "updated": 300,
   "takes": [{"id": "r/night-1", "file": "r/night-1.mp3", "label": "a", "mtime": 100, "seconds": 225},
             {"id": "r/night-2", "file": "r/night-2.mp3", "label": "b", "mtime": 300, "seconds": 230}]},
  {"slug": "ah-daybreak", "title": "Daybreak", "artist": "after-hours", "album": "after-hours",
   "number": 2, "artwork": null, "pick": null, "updated": 200,
   "takes": [{"id": "r/day-1", "file": "r/day-1.mp3", "label": "a", "mtime": 200, "seconds": 95}]},
  {"slug": "lb-anchor", "title": "Anchor", "artist": "lowbridge", "album": null, "number": null,
   "artwork": null, "pick": null, "updated": 150,
   "takes": [{"id": "r/lb-1", "file": "r/lb-1.mp3", "label": "a", "mtime": 150, "seconds": 400}]}
 ]}"#;

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    let server = StaticClient::new().route(
        Method::Get,
        format!("{SERVER}/api/catalog"),
        Ok(Response::new(200, CATALOG)),
    );
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(server),
    )
    .unwrap()
}

/// The take server's three songs and one file of your own, added now (the
/// server's songs were made in 1970). Returns the file's id.
fn ready(dir: &tempfile::TempDir) -> (Arc<SoundscapeCore>, String) {
    let core = open(dir);
    block(core.set_listen_server(SERVER.into())).unwrap();
    block(core.refresh_library(false, None)).unwrap();
    let path = dir.path().join("mine.m4a");
    std::fs::write(&path, b"my own song").unwrap();
    let file = MusicFile {
        path: path.to_string_lossy().into_owned(),
        file_name: "Harbour.m4a".into(),
        title: Some("Harbour".into()),
        artist: Some("Lowbridge".into()),
        album: None,
        number: None,
        genre: Some("Folk".into()),
        duration_seconds: Some(180.0),
        artwork_path: None,
    };
    let mine = block(core.import_music(vec![file])).unwrap().added[0].clone();
    (core, mine)
}

fn play(core: &SoundscapeCore, id: &str, times: usize) {
    for _ in 0..times {
        block(core.record_play(id.into(), "r".into())).unwrap();
    }
}

fn titles(core: &SoundscapeCore, id: &str) -> Vec<String> {
    core.smart_playlist(id.into())
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.track.title)
        .collect()
}

fn rules(match_all: bool, rules: Vec<SmartRule>, sort: SmartSort) -> SmartRules {
    SmartRules {
        match_all,
        rules,
        sort,
        limit: None,
    }
}

#[test]
fn the_built_in_smart_playlists_follow_plays_and_favourites() {
    let dir = tempfile::tempdir().unwrap();
    let (core, mine) = ready(&dir);
    let names: Vec<String> = core
        .collection()
        .smart_playlists
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(
        names,
        [
            "Recently Added",
            "Most Played",
            "Never Played",
            "Forgotten Favourites"
        ]
    );
    // Your file came in last; the server's songs by their oldest take.
    assert_eq!(
        titles(&core, "built-in:recently-added"),
        ["Harbour", "Daybreak", "Anchor", "One More Night"]
    );
    assert_eq!(titles(&core, "built-in:never-played").len(), 4);
    assert!(titles(&core, "built-in:most-played").is_empty());

    let night = track_id("ah-one-more-night");
    play(&core, &night, 3);
    play(&core, &mine, 1);
    let most = core.smart_playlist("built-in:most-played".into()).unwrap();
    let counted: Vec<(String, u32)> = most
        .entries
        .iter()
        .map(|e| (e.track.title.clone(), e.play_count))
        .collect();
    assert_eq!(
        counted,
        [
            ("One More Night".to_string(), 3),
            ("Harbour".to_string(), 1)
        ]
    );
    assert!(most.entries[0].last_played_ms.is_some());
    assert_eq!(
        titles(&core, "built-in:never-played"),
        ["Daybreak", "Anchor"]
    );

    // A favourite just played isn't forgotten; one never played is.
    block(core.set_favourite(night.clone(), true)).unwrap();
    block(core.set_favourite(track_id("lb-anchor"), true)).unwrap();
    assert_eq!(titles(&core, "built-in:forgotten-favourites"), ["Anchor"]);

    // Clearing the history forgets the counts too.
    block(core.clear_history()).unwrap();
    assert!(titles(&core, "built-in:most-played").is_empty());
}

#[test]
fn your_smart_playlist_keeps_its_rules_and_follows_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let (core, mine) = ready(&dir);
    let after_hours = rules(
        true,
        vec![
            SmartRule::Text {
                field: TextField::Artist,
                test: TextTest::Is,
                value: "  after   hours ".into(),
            },
            SmartRule::Number {
                field: NumberField::LengthSeconds,
                test: NumberTest::AtLeast,
                value: 120.0,
            },
        ],
        SmartSort::Title,
    );
    assert_eq!(core.smart_playlist_count(after_hours.clone()).unwrap(), 1);
    let made = block(core.create_smart_playlist("Long nights".into(), after_hours)).unwrap();
    let id = made.playlist.id.clone();
    assert!(id.starts_with("smart:"));
    assert!(!made.playlist.built_in);
    assert_eq!(titles(&core, &id), ["One More Night"]);

    // An edit to your file's artist brings it in, without saving the list.
    block(core.edit_track(
        mine.clone(),
        TrackEdit {
            artist: Some("After Hours".into()),
            ..Default::default()
        },
    ))
    .unwrap();
    assert_eq!(titles(&core, &id), ["Harbour", "One More Night"]);

    // Kept across a restart.
    drop(core);
    let core = open(&dir);
    let smart = core.collection().smart_playlists;
    let kept = smart.iter().find(|p| p.id == id).unwrap();
    assert_eq!(kept.name, "Long nights");
    assert_eq!(kept.track_count, 2);

    // Any one rule: Folk, or added in the last week.
    let any = rules(
        false,
        vec![
            SmartRule::Text {
                field: TextField::Genre,
                test: TextTest::Contains,
                value: "folk".into(),
            },
            SmartRule::Date {
                field: DateField::Added,
                test: DateTest::InLast,
                days: 7,
            },
        ],
        SmartSort::Artist,
    );
    let edited = block(core.edit_smart_playlist(id.clone(), "Folk".into(), any)).unwrap();
    assert_eq!(edited.playlist.name, "Folk");
    assert_eq!(titles(&core, &id), ["Harbour"]);

    // Built-in ones can't be changed or deleted.
    let fixed = rules(true, vec![], SmartSort::Title);
    assert!(
        block(core.edit_smart_playlist("built-in:most-played".into(), "Mine".into(), fixed))
            .is_err()
    );
    assert!(block(core.delete_smart_playlist("built-in:most-played".into())).is_err());

    let after = block(core.delete_smart_playlist(id.clone())).unwrap();
    assert!(after.smart_playlists.iter().all(|p| p.id != id));
    assert!(core.smart_playlist(id).is_err());
}

#[test]
fn a_genre_rule_with_nothing_finds_songs_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = ready(&dir);
    let no_genre = rules(
        true,
        vec![SmartRule::Text {
            field: TextField::Genre,
            test: TextTest::Is,
            value: String::new(),
        }],
        SmartSort::Album,
    );
    let made = block(core.create_smart_playlist("No genre".into(), no_genre)).unwrap();
    let got: Vec<String> = made.entries.into_iter().map(|e| e.track.title).collect();
    // Album order (none first), then the number on the album.
    assert_eq!(got, ["Anchor", "One More Night", "Daybreak"]);
}

#[test]
fn play_counts_outlast_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = ready(&dir);
    let day = track_id("ah-daybreak");
    play(&core, &day, 2_001);
    drop(core);
    let core = open(&dir);
    let most = core.smart_playlist("built-in:most-played".into()).unwrap();
    assert_eq!(most.entries[0].play_count, 2_001);
}

#[test]
fn plays_from_before_counts_were_kept_are_counted_on_opening() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = ready(&dir);
    let night = track_id("ah-one-more-night");
    play(&core, &night, 2);
    drop(core);

    // As a collection saved before this version: a history, no counts.
    let path = dir.path().join("data").join("collection.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let value = json.get_mut("data").expect("a versioned document");
    value.as_object_mut().unwrap().remove("plays");
    std::fs::write(&path, json.to_string()).unwrap();

    let core = open(&dir);
    let most = core.smart_playlist("built-in:most-played".into()).unwrap();
    assert_eq!(most.entries.len(), 1);
    assert_eq!(most.entries[0].play_count, 2);
}

#[test]
fn a_merged_song_takes_its_plays_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let (core, mine) = ready(&dir);
    let anchor = track_id("lb-anchor");
    play(&core, &anchor, 2);
    play(&core, &mine, 3);
    block(core.merge_songs(anchor.clone(), vec![mine])).unwrap();
    let most = core.smart_playlist("built-in:most-played".into()).unwrap();
    assert_eq!(most.entries.len(), 1);
    assert_eq!(most.entries[0].track.id, anchor);
    assert_eq!(most.entries[0].play_count, 5);
}

#[test]
fn rules_that_mean_nothing_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = ready(&dir);
    let refused = |r: SmartRules| {
        matches!(
            core.smart_playlist_count(r),
            Err(SoundscapeError::OutOfRange { .. })
        )
    };
    let one = |rule: SmartRule| rules(true, vec![rule], SmartSort::Title);
    assert!(refused(one(SmartRule::Text {
        field: TextField::Title,
        test: TextTest::Contains,
        value: "  ".into(),
    })));
    assert!(refused(one(SmartRule::Number {
        field: NumberField::PlayCount,
        test: NumberTest::AtLeast,
        value: f64::NAN,
    })));
    assert!(refused(one(SmartRule::Date {
        field: DateField::LastPlayed,
        test: DateTest::InLast,
        days: 0,
    })));
    assert!(refused(SmartRules {
        limit: Some(0),
        ..rules(true, vec![], SmartSort::Title)
    }));
    assert!(refused(rules(
        true,
        vec![SmartRule::Favourite { is: true }; 21],
        SmartSort::Title
    )));
    assert!(matches!(
        block(core.create_smart_playlist(" ".into(), rules(true, vec![], SmartSort::Title))),
        Err(SoundscapeError::EmptyName)
    ));
}
