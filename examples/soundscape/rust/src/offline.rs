//! Songs kept on the phone (see `docs/offline.md`). The person chooses
//! songs; the phone keeps each one's preferred recording. Kept in
//! `offline.json`, apart from the library, with the files in the
//! `downloads` store.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use forge_net::{CancelHandle, HttpClient, Request};
use serde::{Deserialize, Serialize};

use crate::library::{AudioSource, Library, LibraryTrack, Recording};
use crate::model::SoundscapeError;

pub const OFFLINE_VERSION: u32 = 1;
/// The largest recording downloaded (an hour of 48 kHz stereo 16-bit WAV is
/// about 700 MB; takes are minutes long).
pub const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;
/// Songs one request may add.
pub const MAX_WANTED: usize = 20_000;

// ------------------------------------------------------------ what is kept

/// `offline.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OfflineFile {
    /// The songs the person wants on the phone, with when.
    #[serde(default)]
    pub wanted: BTreeMap<String, i64>,
    /// Downloaded recordings, by [`download_key`].
    #[serde(default)]
    pub files: BTreeMap<String, Download>,
    /// Numbers the downloaded files.
    #[serde(default)]
    pub next_file: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Download {
    /// Its name in the downloads store.
    pub file: String,
    pub track_id: String,
    pub recording_id: String,
    pub bytes: u64,
    pub at_ms: i64,
}

/// A recording's key among the downloads: a take's id is only unique within
/// its song.
pub fn download_key(track_id: &str, recording_id: &str) -> String {
    format!("{track_id}|{recording_id}")
}

impl OfflineFile {
    pub fn set_wanted(
        &mut self,
        track_ids: &[String],
        on: bool,
        now: i64,
    ) -> Result<(), SoundscapeError> {
        if on && self.wanted.len() + track_ids.len() > MAX_WANTED {
            return Err(SoundscapeError::range("downloads"));
        }
        // A millisecond apart, so an album comes down in its order.
        for (i, id) in track_ids.iter().enumerate() {
            if on {
                self.wanted.entry(id.clone()).or_insert(now + i as i64);
            } else {
                self.wanted.remove(id);
            }
        }
        Ok(())
    }

    /// Every file the downloads name, for sweeping the store.
    pub fn file_names(&self) -> HashSet<String> {
        self.files.values().map(|d| d.file.clone()).collect()
    }

    pub fn bytes(&self) -> u64 {
        self.files.values().map(|d| d.bytes).sum()
    }
}

/// The name of the `number`th downloaded file.
pub fn file_name(number: u64, extension: &str) -> String {
    format!("download-{number}.{extension}")
}

// ------------------------------------------------------------ the rules

/// Whether a recording may be kept on the phone. Take-server recordings
/// may; an imported file already is; a catalogue track only where its
/// provider allows it (`docs/sources.md`).
pub fn may_download(recording: &Recording) -> bool {
    match recording.source {
        AudioSource::TakeServer { .. } => true,
        AudioSource::Catalogue { .. } => recording.rights.as_ref().is_some_and(|r| r.may_download),
        AudioSource::LocalFile { .. } => false,
    }
}

/// One recording to download.
#[derive(Clone, Debug, PartialEq)]
pub struct Fetch {
    pub key: String,
    pub track_id: String,
    pub recording_id: String,
    pub url: String,
    /// The file's extension, from its URL.
    pub extension: String,
}

/// What to download and what to remove to match what is wanted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// In the order the songs were chosen.
    pub fetch: Vec<Fetch>,
    /// Keys of downloads nothing wants.
    pub remove: Vec<String>,
}

/// Compares what is wanted with what is kept. `library` is the library as
/// built from what is kept (its URLs are the server's). A wanted song keeps
/// its preferred recording; a download no wanted song's preferred recording
/// is goes, including the old take when another was picked.
pub fn plan(offline: &OfflineFile, library: &Library) -> Plan {
    let mut wanted: Vec<(&String, &i64)> = offline.wanted.iter().collect();
    wanted.sort_by(|a, b| a.1.cmp(b.1).then(a.0.cmp(b.0)));
    let tracks: HashMap<&str, &LibraryTrack> =
        library.tracks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut keep = HashSet::new();
    let mut fetch = Vec::new();
    for (id, _) in wanted {
        let Some(recording) = tracks.get(id.as_str()).and_then(|t| t.preferred.as_ref()) else {
            continue;
        };
        if !may_download(recording) {
            continue;
        }
        // A recording a merged song brought is kept under that song, as
        // localize_track looks for it.
        let owner = recording.merged_from.as_deref().unwrap_or(id.as_str());
        let key = download_key(owner, &recording.id);
        if !offline.files.contains_key(&key) {
            fetch.push(Fetch {
                key: key.clone(),
                track_id: owner.to_string(),
                recording_id: recording.id.clone(),
                url: recording.audio_url.clone(),
                extension: extension_of(&recording.audio_url),
            });
        }
        keep.insert(key);
    }
    let remove = offline
        .files
        .keys()
        .filter(|k| !keep.contains(*k))
        .cloned()
        .collect();
    Plan { fetch, remove }
}

/// The extension of a URL's last path segment, lower case, when it is an
/// audio file's; `mp3` otherwise.
pub fn extension_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    last.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| crate::library::MUSIC_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or_else(|| "mp3".into())
}

/// Plays downloaded recordings from the phone: their `audio_url` becomes
/// the file. `url` turns a stored file name into a URL (or `None` when the
/// file has gone).
pub fn localize_track(
    track: &mut LibraryTrack,
    offline: &OfflineFile,
    url: &dyn Fn(&str) -> Option<String>,
) {
    if let Some(recording) = &mut track.preferred {
        // A recording a merged song brought is kept under that song.
        let owner = recording
            .merged_from
            .clone()
            .unwrap_or_else(|| track.id.clone());
        if localize(&owner, recording, offline, url) {
            track.downloaded = true;
        }
    }
}

/// Points one recording at its downloaded file, if it has one. True when it
/// did.
pub fn localize(
    track_id: &str,
    recording: &mut Recording,
    offline: &OfflineFile,
    url: &dyn Fn(&str) -> Option<String>,
) -> bool {
    let Some(file) = offline.files.get(&download_key(track_id, &recording.id)) else {
        return false;
    };
    match url(&file.file) {
        Some(local) => {
            recording.audio_url = local;
            true
        }
        None => false,
    }
}

// ------------------------------------------------------------ what is shown

#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum DownloadState {
    /// Not chosen for the phone.
    NotDownloaded,
    /// Chosen, not started yet.
    Waiting,
    Downloading,
    /// On the phone.
    Downloaded,
    /// The last try failed; it is tried again on the next pass.
    Failed {
        reason: String,
    },
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct OfflineTrack {
    pub track: LibraryTrack,
    pub state: DownloadState,
}

/// What the person chose for the phone, newest choice first.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct OfflineSummary {
    pub tracks: Vec<OfflineTrack>,
    /// Space the downloads take.
    pub bytes: u64,
    pub downloaded: u32,
    /// Waiting or downloading.
    pub pending: u32,
    pub failed: u32,
    /// True while downloads are running.
    pub syncing: bool,
}

/// What is happening now, kept in memory only.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub syncing: bool,
    /// The key being downloaded.
    pub current: Option<String>,
    /// Why keys failed on this pass.
    pub failures: HashMap<String, String>,
}

/// A wanted song's state. `track` is as built from what is kept.
pub fn state_of(offline: &OfflineFile, progress: &Progress, track: &LibraryTrack) -> DownloadState {
    if !offline.wanted.contains_key(&track.id) {
        return DownloadState::NotDownloaded;
    }
    let Some(recording) = &track.preferred else {
        return DownloadState::Failed {
            reason: "it has no recording".into(),
        };
    };
    if let AudioSource::LocalFile { .. } = recording.source {
        return DownloadState::Downloaded;
    }
    if !may_download(recording) {
        return DownloadState::Failed {
            reason: "its source doesn't allow downloading".into(),
        };
    }
    let owner = recording
        .merged_from
        .as_deref()
        .unwrap_or(track.id.as_str());
    let key = download_key(owner, &recording.id);
    if offline.files.contains_key(&key) {
        DownloadState::Downloaded
    } else if progress.current.as_deref() == Some(key.as_str()) {
        DownloadState::Downloading
    } else if let Some(reason) = progress.failures.get(&key) {
        DownloadState::Failed {
            reason: reason.clone(),
        }
    } else {
        DownloadState::Waiting
    }
}

/// The summary. `library` is as built from what is kept; the tracks shown
/// are localized with `url`.
pub fn summary(
    offline: &OfflineFile,
    progress: &Progress,
    library: &Library,
    url: &dyn Fn(&str) -> Option<String>,
) -> OfflineSummary {
    let mut wanted: Vec<(&String, &i64)> = offline.wanted.iter().collect();
    wanted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut out = OfflineSummary {
        bytes: offline.bytes(),
        syncing: progress.syncing,
        ..Default::default()
    };
    for (id, _) in wanted {
        let Some(track) = library.tracks.iter().find(|t| &t.id == id) else {
            continue;
        };
        let state = state_of(offline, progress, track);
        match state {
            DownloadState::Downloaded => out.downloaded += 1,
            DownloadState::Waiting | DownloadState::Downloading => out.pending += 1,
            DownloadState::Failed { .. } => out.failed += 1,
            DownloadState::NotDownloaded => {}
        }
        let mut track = track.clone();
        localize_track(&mut track, offline, url);
        out.tracks.push(OfflineTrack { track, state });
    }
    out
}

// ------------------------------------------------------------ downloading

/// Downloads `fetch` into `destination`, streamed to disk, and returns its
/// size. Nothing is left at `destination` unless all of it arrived.
pub fn download(
    http: &dyn HttpClient,
    fetch: &Fetch,
    destination: &Path,
    cancel: &CancelHandle,
) -> Result<u64, SoundscapeError> {
    // A file the app made itself (the test catalogue) is copied. No other
    // song's download may read a file on the phone.
    if let Some(path) = fetch.url.strip_prefix("file://") {
        if crate::test_catalogue::spec(&fetch.recording_id).is_none() {
            return Err(SoundscapeError::Server {
                reason: "this recording isn't a download".into(),
            });
        }
        let source = crate::listen::decode(path);
        return std::fs::copy(&source, destination).map_err(|err| SoundscapeError::Storage {
            reason: format!("copy the recording: {err}"),
        });
    }
    let response = forge_net::download_to_file(
        http,
        &Request::get(fetch.url.clone()),
        destination,
        cancel,
        Some(MAX_DOWNLOAD_BYTES),
    )?;
    if !response.is_success() {
        return Err(SoundscapeError::Server {
            reason: match response.status {
                404 => "the server doesn't have this recording any more".into(),
                status => format!("the server answered {status}"),
            },
        });
    }
    std::fs::metadata(destination)
        .map(|m| m.len())
        .map_err(|err| SoundscapeError::Storage {
            reason: format!("read the download: {err}"),
        })
}
