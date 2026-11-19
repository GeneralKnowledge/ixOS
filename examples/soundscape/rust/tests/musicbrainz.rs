//! MusicBrainz recognises a song: a lookup ranks what it knows against the
//! song's title, artist and length, and nothing changes until the person
//! links a match, against canned answers in MusicBrainz's shapes.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::catalogue::rights_from_licence;
use soundscape_core::musicbrainz::{is_mbid, parse, rank, search_url, Wanted};
use soundscape_core::{CatalogueTrack, SoundscapeCore, TrackEdit};

const LANTERN: &str = "0b7f9c55-1c5e-4b8e-9d4a-2f0a6c1d2e31";
const LANTERN_LIVE: &str = "6c1d2e31-0b7f-4c55-9e8b-4d4a2f0a6c1d";
const OTHER: &str = "9e8b4d4a-2f0a-4c1d-8e31-0b7f9c551c5e";
const ALBUM: &str = "4a2f0a6c-1d2e-4310-b7f9-c551c5e4b8e9";
const ALBUM_GROUP: &str = "2e310b7f-9c55-41c5-a4b8-e9d4a2f0a6c1";
const SINGLE: &str = "1d2e310b-7f9c-4551-85e4-b8e9d4a2f0a6";

fn answer() -> String {
    format!(
        r#"{{"created": "2026-11-18T20:00:00Z", "count": 4, "offset": 0, "recordings": [
  {{"id": "{LANTERN_LIVE}", "score": 100, "title": "Lantern", "length": 412000,
    "artist-credit": [{{"name": "Kalte Ohren", "joinphrase": "", "artist": {{"name": "Kalte Ohren"}}}}]}},
  {{"id": "{LANTERN}", "score": 97, "title": "Lantern", "length": 183400,
    "first-release-date": "2006-03-01",
    "artist-credit": [{{"name": "Kalte Ohren", "joinphrase": " & "}}, {{"name": "Guest", "joinphrase": ""}}],
    "releases": [
      {{"id": "{SINGLE}", "title": "Lantern", "status": "Official", "date": "2005-11-01",
        "release-group": {{"id": "{SINGLE}", "primary-type": "Single"}},
        "media": [{{"position": 1, "track": [{{"number": "1", "title": "Lantern"}}]}}]}},
      {{"id": "{ALBUM}", "title": "Night Signals", "status": "Official", "date": "2006-03-01",
        "release-group": {{"id": "{ALBUM_GROUP}", "primary-type": "Album"}},
        "media": [{{"position": 1, "track": [{{"number": "2", "title": "Lantern"}}]}}]}}
    ]}},
  {{"id": "{OTHER}", "score": "64", "title": "Lanterns of the North", "length": 183000,
    "artist-credit": [{{"name": "Somebody Else", "joinphrase": ""}}]}},
  {{"id": "not-an-id", "score": 90, "title": "Broken"}}
]}}"#
    )
}

fn wanted() -> Wanted {
    Wanted {
        title: "Lantern".into(),
        artist: Some("Kalte Ohren & Guest".into()),
        duration_seconds: Some(183.0),
    }
}

fn song() -> CatalogueTrack {
    CatalogueTrack {
        id: "jamendo:42".into(),
        provider: "Jamendo".into(),
        title: "lantern (final)".into(),
        artist: Some("Kalte Ohren & Guest".into()),
        album: None,
        number: None,
        genre: None,
        duration_seconds: Some(183.0),
        artwork_url: None,
        audio_url: "https://prod-1.storage.jamendo.com/?trackid=42&format=mp32".into(),
        rights: rights_from_licence(None, false, "c", None),
        added_ms: 0,
    }
}

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    let wanted = Wanted {
        title: "lantern (final)".into(),
        ..wanted()
    };
    let http = StaticClient::new().route(
        Method::Get,
        search_url(&wanted),
        Ok(Response::new(200, answer())),
    );
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(http),
    )
    .unwrap()
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

#[test]
fn an_answer_is_read_with_the_release_it_is_best_known_from() {
    let found = parse(answer().as_bytes()).unwrap();
    assert_eq!(found.len(), 3, "a bad id is left out");
    let lantern = &found[1];
    assert_eq!(lantern.recording_id, LANTERN);
    assert_eq!(lantern.artist.as_deref(), Some("Kalte Ohren & Guest"));
    assert_eq!(
        lantern.album.as_deref(),
        Some("Night Signals"),
        "the album, not the earlier single"
    );
    assert_eq!(lantern.release_id.as_deref(), Some(ALBUM));
    assert_eq!(lantern.number, Some(2));
    assert_eq!(lantern.year.as_deref(), Some("2006"));
    assert_eq!(lantern.duration_seconds, Some(183.4));
    assert_eq!(
        lantern.artwork_url.as_deref(),
        Some(format!("https://coverartarchive.org/release-group/{ALBUM_GROUP}/front-250").as_str())
    );
    assert_eq!(found[2].confidence, 64, "a score written as text");
    assert!(parse(br#"{"error": "Invalid query"}"#).is_err());
    assert!(parse(br#"{"recordings": []}"#).unwrap().is_empty());
}

#[test]
fn the_same_song_at_the_same_length_ranks_first() {
    let ranked = rank(parse(answer().as_bytes()).unwrap(), &wanted());
    assert_eq!(ranked[0].recording_id, LANTERN);
    assert!(ranked[0].likely);
    let live = ranked
        .iter()
        .find(|m| m.recording_id == LANTERN_LIVE)
        .unwrap();
    assert!(!live.likely, "the same name but more than twice as long");
    assert!(live.confidence < ranked[0].confidence);
    let other = ranked.iter().find(|m| m.recording_id == OTHER).unwrap();
    assert!(!other.likely);

    // Without a length, a song is judged on its title and artist.
    let ranked = rank(
        parse(answer().as_bytes()).unwrap(),
        &Wanted {
            duration_seconds: None,
            ..wanted()
        },
    );
    assert!(ranked[0].likely);
}

#[test]
fn a_search_cannot_be_reshaped_by_what_is_typed() {
    let url = search_url(&Wanted {
        title: "Song\" OR artist:(x)".into(),
        artist: None,
        duration_seconds: None,
    });
    assert!(url.starts_with("https://musicbrainz.org/ws/2/recording?query="));
    assert!(url.ends_with("&fmt=json"));
    assert!(!url.contains("%28"), "brackets are taken out: {url}");
    assert_eq!(url.matches("%22").count(), 2, "only our own quotes: {url}");
    assert!(is_mbid(LANTERN));
    assert!(!is_mbid("../../etc/passwd-0000-0000-000000000000"));
}

#[test]
fn a_song_is_linked_and_takes_the_details_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block(core.add_catalogue_tracks(vec![song()])).unwrap();

    let found = block(core.identify_track("jamendo:42".into(), None)).unwrap();
    assert_eq!(found[0].recording_id, LANTERN);
    assert!(found[0].likely, "an untidy title still finds it");
    let before = core.library_track("jamendo:42".into()).unwrap();
    assert_eq!(
        before.track.musicbrainz_id, None,
        "a lookup changes nothing"
    );

    let linked =
        block(core.link_musicbrainz("jamendo:42".into(), found[0].clone(), false)).unwrap();
    assert_eq!(linked.track.musicbrainz_id.as_deref(), Some(LANTERN));
    assert_eq!(linked.track.title, "lantern (final)", "details kept");
    assert!(
        linked.track.artwork_url.unwrap().contains(ALBUM_GROUP),
        "a song with no picture shows the release's cover"
    );

    let linked = block(core.link_musicbrainz("jamendo:42".into(), found[0].clone(), true)).unwrap();
    assert_eq!(linked.track.title, "Lantern");
    assert_eq!(linked.track.album.as_deref(), Some("Night Signals"));
    assert_eq!(linked.track.number, Some(2));
    assert!(
        linked.track.edited,
        "as the person's changes, so Reset undoes them"
    );

    // Kept across a restart; unlinking keeps the details.
    drop(core);
    let core = open(&dir);
    let back = core.library_track("jamendo:42".into()).unwrap();
    assert_eq!(back.track.musicbrainz_id.as_deref(), Some(LANTERN));
    let unlinked = block(core.unlink_musicbrainz("jamendo:42".into())).unwrap();
    assert_eq!(unlinked.track.musicbrainz_id, None);
    assert_eq!(unlinked.track.title, "Lantern");
    block(core.edit_track(
        "jamendo:42".into(),
        TrackEdit {
            title: Some(String::new()),
            ..Default::default()
        },
    ))
    .unwrap();
    assert_eq!(
        core.library_track("jamendo:42".into()).unwrap().track.title,
        "lantern (final)"
    );
}

#[test]
fn two_sources_of_one_recording_know_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let archive = CatalogueTrack {
        id: "archive:ccr042/02 Lantern.mp3".into(),
        provider: "Internet Archive".into(),
        audio_url: "https://archive.org/download/ccr042/02%20Lantern.mp3".into(),
        ..song()
    };
    block(core.add_catalogue_tracks(vec![song(), archive.clone()])).unwrap();
    let found = block(core.identify_track("jamendo:42".into(), None)).unwrap();
    for id in ["jamendo:42", archive.id.as_str()] {
        block(core.link_musicbrainz(id.into(), found[0].clone(), false)).unwrap();
    }
    let detail = core.library_track("jamendo:42".into()).unwrap();
    assert_eq!(detail.same_recording.len(), 1);
    assert_eq!(detail.same_recording[0].id, archive.id);

    // Removing one source takes its link with it.
    block(core.remove_imported(archive.id.clone())).unwrap();
    assert!(core
        .library_track("jamendo:42".into())
        .unwrap()
        .same_recording
        .is_empty());

    let bad = soundscape_core::MusicBrainzMatch {
        recording_id: "../x".into(),
        ..found[0].clone()
    };
    assert!(block(core.link_musicbrainz("jamendo:42".into(), bad, false)).is_err());
    assert!(block(core.link_musicbrainz("jamendo:404".into(), found[0].clone(), false)).is_err());
}

/// A real answer (2026-11-19, `recording:"Bohemian Rhapsody" AND
/// artist:"Queen"`, limit 3; each artist's aliases cut to one): the top
/// three, all scored 100, are concerts.
const LIVE_ANSWER: &str = include_str!("data/musicbrainz-bohemian-rhapsody.json");

#[test]
fn a_live_answer_reads_and_concerts_are_not_offered_for_a_studio_song() {
    let found = parse(LIVE_ANSWER.as_bytes()).unwrap();
    assert_eq!(found.len(), 3);
    let first = &found[0];
    assert_eq!(first.artist.as_deref(), Some("Queen"));
    assert_eq!(first.duration_seconds, None, "no length given");
    assert_eq!(first.number, Some(4));
    assert_eq!(first.year.as_deref(), Some("2005"));
    assert!(first.live, "only on a live album");
    assert_eq!(
        first.artwork_url.as_deref(),
        Some("https://coverartarchive.org/release-group/c9a223e1-377f-363f-9699-4d209006a2b7/front-250")
    );
    assert_eq!(
        found[2].disambiguation.as_deref(),
        Some("live, 1979-01-24: Deutschlandhalle, Berlin, Germany")
    );
    assert_eq!(
        found[2].album.as_deref(),
        Some("1979-01-24: Deutschlandhalle, Berlin, Germany"),
        "a bootleg, the only release"
    );
    assert!(found.iter().all(|m| m.live));

    let studio = Wanted {
        title: "Bohemian Rhapsody".into(),
        artist: Some("Queen".into()),
        duration_seconds: Some(354.0),
    };
    assert!(rank(found.clone(), &studio).iter().all(|m| !m.likely));
    let unknown_length = Wanted {
        duration_seconds: None,
        ..studio.clone()
    };
    assert!(rank(found.clone(), &unknown_length)
        .iter()
        .all(|m| !m.likely));
    // A song that says it is live may be one of them.
    let live = Wanted {
        title: "Bohemian Rhapsody (Live)".into(),
        duration_seconds: None,
        ..studio
    };
    assert!(rank(found, &live).iter().any(|m| m.likely));
}

/// Busy for everyone, then an answer, as MusicBrainz was on 2026-11-19.
struct Busy {
    busy: usize,
    asked: std::sync::atomic::AtomicUsize,
}

impl forge_net::HttpClient for Busy {
    fn send(&self, request: &forge_net::Request) -> Result<Response, forge_net::NetError> {
        assert!(request
            .headers
            .iter()
            .any(|h| h.name == "User-Agent" && h.value.starts_with("Soundscape/")));
        let n = self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n < self.busy {
            let mut busy = Response::new(
                503,
                r#"{"error": "The MusicBrainz web server is currently busy. Please try again later."}"#,
            );
            busy.headers.push(forge_net::Header {
                name: "retry-after".into(),
                value: "0".into(),
            });
            Ok(busy)
        } else {
            Ok(Response::new(200, LIVE_ANSWER))
        }
    }
}

#[test]
fn a_busy_answer_is_asked_again() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cancel = forge_net::CancelHandle::new();
    let busy = Busy {
        busy: 3,
        asked: AtomicUsize::new(0),
    };
    let found = soundscape_core::musicbrainz::identify(&busy, &wanted(), &cancel).unwrap();
    assert_eq!(found.len(), 3);
    assert_eq!(busy.asked.load(Ordering::SeqCst), 4);

    let always = Busy {
        busy: usize::MAX,
        asked: AtomicUsize::new(0),
    };
    let err = soundscape_core::musicbrainz::identify(&always, &wanted(), &cancel).unwrap_err();
    assert!(err.to_string().contains("busy"), "{err}");
    assert_eq!(
        always.asked.load(Ordering::SeqCst),
        soundscape_core::musicbrainz::ATTEMPTS as usize
    );
}
