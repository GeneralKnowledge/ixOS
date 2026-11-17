//! Soundscape: a small multi-track sound sketchpad. Record loops, layer
//! them, shape each with tone and echo, mix down to a WAV file.
//!
//! The app's whole Rust side. What a project is and how it mixes live in
//! [`project`]; reading and shaping sound in [`dsp`]; this file is the API
//! the SwiftUI app calls, built on the application runtime (`forge_app`):
//!
//! - **AVAudioEngine records and plays.** The app records a loop to a WAV
//!   file (`loop_frames` says how long) and passes the file to `set_clip`,
//!   which copies it into Soundscape's store. For playback each track is an
//!   `AVAudioPlayerNode` looping its clip through an `AVAudioUnitEQ`
//!   low-pass and an `AVAudioUnitDelay`, set from [`Track::mix`].
//! - **Rust owns the project and the mixdown.** Settings are checked and
//!   saved here, mapped to AVAudioEngine's units here, and the mixdown
//!   renders the same chain offline, so the export sounds like playback.
//! - **Commands** save the project list (one JSON document) before they
//!   answer; **queries** (`projects`, `project`) read a published copy.
//! - **Waveforms** are on-screen work; the **mixdown** is finishing work,
//!   which keeps going when the app leaves the screen while iOS allows.
//! - **Listen** ([`listen`]) plays, compares and picks song takes kept on
//!   another computer's take server. Rust keeps the address, asks the
//!   server (URLSession on iOS) and works out matched loudness; AVPlayer
//!   streams the audio. [`library`] makes the server's songs a music
//!   library (artists, albums, tracks with their recordings, artwork, the
//!   person's edits) kept on the phone; [`queue`] decides what plays next.

pub mod collection;
pub mod dsp;
pub mod library;
pub mod listen;
pub mod lyrics;
mod model;
pub mod offline;
pub mod project;
pub mod queue;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use forge_app::{Cancellation, DataDirectory, Document, ErrorKind, Published, Runtime};
use forge_net::{HttpClient, UreqClient};
use forge_runtime::now_ms;
use forge_storage::{files, FileStore};

use collection::CollectionFile;
pub use collection::{
    counts_as_played, MusicCollection, Playlist, PlaylistDetail, PlaylistEntry, RecentPlay,
};
use library::LibraryFile;
pub use library::{
    ArtworkTarget, AudioSource, Generation, ImportResult, Library, LibraryAlbum, LibraryArtist,
    LibrarySearch, LibraryTrack, MusicFile, Recording, RecordingKind, TrackDetail, TrackEdit,
};
pub use listen::{
    group_songs, loudness_match, ListenSettings, LoudnessMatch, SongDetail, SongGroup, SongSummary,
    Take, TakeMark,
};
pub use lyrics::{lyric_line_at, lyric_lines, lyric_word_at, LyricKind, LyricLine, LyricWord};
pub use model::{
    soundscape_error_kind, Clip, Echo, Mixdown, ProjectDetail, ProjectSummary, SoundscapeError,
    Track, TrackMix, TrackSettings, MAX_BARS, MAX_GAIN_DB, MAX_NAME_CHARS, MAX_TEMPO, MAX_TRACKS,
    MIN_GAIN_DB, MIN_TEMPO,
};
pub use offline::{DownloadState, OfflineSummary, OfflineTrack};
use offline::{OfflineFile, Progress};
use project::{Loaded, Project, Projects, SavedClip};
pub use queue::{PlayQueue, QueueEntry, QueueTrack, RepeatMode};

uniffi::setup_scaffolding!();

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes to the same
// log as Rust.
forge_runtime::log_export!();

// Native services the host implements, as `crate::native`; `http` sends
// Listen's requests through URLSession on iOS, so they follow the phone's
// VPN (Tailscale) and network settings.
forge_services::native_services!(http);

const SAVE_FILE: &str = "soundscape.json";
/// Listen's settings (the take server's address), apart from the projects.
const LISTEN_FILE: &str = "listen.json";
/// The music library (the server's last catalogue, edits, artwork names).
const LIBRARY_FILE: &str = "library.json";
/// Favourites, playlists and history.
const COLLECTION_FILE: &str = "collection.json";
/// What is kept on the phone to play offline.
const OFFLINE_FILE: &str = "offline.json";
/// The largest music file imported.
const MAX_IMPORT_BYTES: u64 = 1024 * 1024 * 1024;
/// The largest picture accepted as artwork.
const MAX_ARTWORK_BYTES: u64 = 20 * 1024 * 1024;
/// The largest answer the take server may give (a library of a few hundred
/// songs is well under this).
const MAX_ANSWER_BYTES: u64 = 4 * 1024 * 1024;
/// Recordings bigger than this are refused (five minutes of 48 kHz stereo
/// 32-bit float is about 115 MB).
const MAX_CLIP_BYTES: u64 = 128 * 1024 * 1024;
/// The mixdown's sample rate.
pub const MIXDOWN_RATE: u32 = 44_100;
/// The most passes of the loop one mixdown writes.
pub const MAX_PASSES: u32 = 16;

/// What the command thread owns.
struct State {
    projects: Document<Projects>,
    recordings: FileStore,
    published: Arc<Published<Projects>>,
    listen: Document<ListenSettings>,
    listen_published: Arc<Published<ListenSettings>>,
    library: Document<LibraryFile>,
    library_published: Arc<Published<LibraryFile>>,
    artwork: FileStore,
    collection: Document<CollectionFile>,
    collection_published: Arc<Published<CollectionFile>>,
    /// The person's own music files.
    imported: FileStore,
    offline: Document<OfflineFile>,
    offline_published: Arc<Published<OfflineFile>>,
    downloads: FileStore,
}

impl State {
    /// Changes the projects, saves them, publishes, and removes recordings
    /// no project uses any more. Only commands write recordings, one at a
    /// time, so nothing is being written meanwhile.
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut Projects) -> Result<T, SoundscapeError>,
    ) -> Result<T, SoundscapeError> {
        let before = self.projects.value().files();
        let value = self.projects.change(edit)?;
        let after = self.projects.value().files();
        for name in before.difference(&after) {
            self.recordings.remove(name);
        }
        self.published.set(self.projects.value().clone());
        Ok(value)
    }

    /// Changes the library, saves it, publishes, and removes artwork files
    /// nothing names any more.
    fn change_library<T>(
        &mut self,
        edit: impl FnOnce(&mut LibraryFile) -> Result<T, SoundscapeError>,
    ) -> Result<T, SoundscapeError> {
        let before = self.library.value().artwork_files();
        let music_before = self.library.value().music_files();
        let value = self.library.change(edit)?;
        for name in before.difference(&self.library.value().artwork_files()) {
            self.artwork.remove(name);
        }
        for name in music_before.difference(&self.library.value().music_files()) {
            self.imported.remove(name);
        }
        self.library_published.set(self.library.value().clone());
        Ok(value)
    }

    /// Changes favourites, playlists or history, saves and publishes.
    fn change_collection<T>(
        &mut self,
        edit: impl FnOnce(&mut CollectionFile) -> Result<T, SoundscapeError>,
    ) -> Result<T, SoundscapeError> {
        let value = self.collection.change(edit)?;
        self.collection_published
            .set(self.collection.value().clone());
        Ok(value)
    }

    /// Changes what is kept offline, saves, publishes, and removes
    /// downloaded files nothing names any more.
    fn change_offline<T>(
        &mut self,
        edit: impl FnOnce(&mut OfflineFile) -> Result<T, SoundscapeError>,
    ) -> Result<T, SoundscapeError> {
        let before = self.offline.value().file_names();
        let value = self.offline.change(edit)?;
        for name in before.difference(&self.offline.value().file_names()) {
            self.downloads.remove(name);
        }
        self.offline_published.set(self.offline.value().clone());
        Ok(value)
    }
}

/// The app's core. The Swift app opens one per process and shares it.
#[derive(uniffi::Object)]
pub struct SoundscapeCore {
    rt: Runtime<State>,
    projects: Arc<Published<Projects>>,
    recordings: PathBuf,
    caches: PathBuf,
    listen: Arc<Published<ListenSettings>>,
    library: Arc<Published<LibraryFile>>,
    artwork: PathBuf,
    collection: Arc<Published<CollectionFile>>,
    imported: PathBuf,
    offline: Arc<Published<OfflineFile>>,
    downloads: PathBuf,
    progress: Arc<Mutex<Progress>>,
    /// True while `sync_downloads` runs, so a second one returns at once.
    syncing: Arc<AtomicBool>,
    http: Arc<dyn HttpClient>,
}

#[uniffi::export]
impl SoundscapeCore {
    /// Opens the projects saved in `data_directory` (created if missing),
    /// holding the directory so no second core can write to it.
    /// `caches_directory` is for files the app can lose (mixdowns). A saved
    /// file that can't be read is an error (`Corrupt`); `start_fresh` sets
    /// it aside once the person agrees.
    #[uniffi::constructor]
    pub fn new(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, SoundscapeError> {
        forge_runtime::logging::init();
        Self::open(&data_directory, &caches_directory, false, default_http())
    }

    /// Opens with no projects in place of a file that can't be read, which
    /// is kept next to it. Refused for data from a newer version.
    #[uniffi::constructor]
    pub fn start_fresh(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, SoundscapeError> {
        forge_runtime::logging::init();
        Self::open(&data_directory, &caches_directory, true, default_http())
    }

    /// Every project, most recently changed first. Never waits for a
    /// command.
    pub fn projects(&self) -> Vec<ProjectSummary> {
        let all = self.projects.get();
        let mut list: Vec<ProjectSummary> = all.projects.iter().map(summary).collect();
        list.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(b.id.cmp(&a.id)));
        list
    }

    /// A project with its tracks, their recordings and their mix in
    /// AVAudioEngine's units. Never waits for a command.
    pub fn project(&self, id: u64) -> Result<ProjectDetail, SoundscapeError> {
        let all = self.projects.get();
        Ok(self.detail(all.get(id)?))
    }

    /// How many frames to record for one pass of the loop at
    /// `sample_rate`, so a recording loops cleanly.
    pub fn loop_frames(&self, id: u64, sample_rate: u32) -> Result<u64, SoundscapeError> {
        let all = self.projects.get();
        let seconds = all.get(id)?.loop_seconds();
        Ok((seconds * f64::from(sample_rate)).round() as u64)
    }

    /// A new project with one empty track. Tempo from 40 to 240 beats per
    /// minute, 1 to 16 bars of four beats.
    pub async fn create_project(
        &self,
        title: String,
        tempo_bpm: f64,
        bars: u32,
    ) -> Result<ProjectDetail, SoundscapeError> {
        let id = self
            .rt
            .command("create-project", move |state| {
                state.change(|all| all.create(&title, tempo_bpm, bars, now_ms()))
            })
            .await?;
        self.project(id)
    }

    pub async fn rename_project(&self, id: u64, title: String) -> Result<(), SoundscapeError> {
        self.rt
            .command("rename-project", move |state| {
                state.change(|all| {
                    all.edit(id, now_ms(), |p| {
                        p.title = model::clean_name(&title)?;
                        Ok(())
                    })
                })
            })
            .await
    }

    /// Changes tempo and length. Recordings stay as they are: a longer
    /// loop plays them followed by silence, a shorter one cuts them.
    pub async fn set_timing(
        &self,
        id: u64,
        tempo_bpm: f64,
        bars: u32,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.rt
            .command("set-timing", move |state| {
                state.change(|all| all.edit(id, now_ms(), |p| p.set_timing(tempo_bpm, bars)))
            })
            .await?;
        self.project(id)
    }

    /// Turns the metronome on or off for a project. It is never in the
    /// mixdown.
    pub async fn set_metronome(&self, id: u64, on: bool) -> Result<ProjectDetail, SoundscapeError> {
        self.edit_project("set-metronome", id, move |p| {
            p.metronome = on;
            Ok(())
        })
        .await
    }

    /// A WAV file of one pass of the project's metronome at `sample_rate`
    /// (see [`dsp::clicks`]): the app loops it under playback and plays its
    /// first bar as the count-in before a recording. Kept in the caches,
    /// one file per tempo, length and rate, written the first time.
    pub async fn click_track(&self, id: u64, sample_rate: u32) -> Result<String, SoundscapeError> {
        if sample_rate == 0 {
            return Err(SoundscapeError::range("sample rate"));
        }
        let (tempo_bpm, bars) = {
            let all = self.projects.get();
            let p = all.get(id)?;
            (p.tempo_bpm, p.bars)
        };
        let path = self.caches.join("clicks").join(format!(
            "{}-{bars}-{sample_rate}.wav",
            (tempo_bpm * 1000.0).round() as u64
        ));
        let work = self.rt.foreground(None);
        self.rt
            .run("click-track", work, move |_| {
                if !path.exists() {
                    if let Some(dir) = path.parent() {
                        std::fs::create_dir_all(dir).map_err(|err| SoundscapeError::Storage {
                            reason: format!("create the clicks folder: {err}"),
                        })?;
                    }
                    let partial = path.with_extension("partial");
                    dsp::write(
                        &partial,
                        &dsp::clicks(tempo_bpm, bars, sample_rate),
                        sample_rate,
                    )?;
                    std::fs::rename(&partial, &path).map_err(|err| SoundscapeError::Storage {
                        reason: format!("keep the click track: {err}"),
                    })?;
                }
                Ok(path.to_string_lossy().into_owned())
            })
            .await
    }

    /// Deletes a project and the recordings only it used.
    pub async fn delete_project(&self, id: u64) -> Result<(), SoundscapeError> {
        self.rt
            .command("delete-project", move |state| {
                state.change(|all| all.delete(id))
            })
            .await
    }

    /// Adds an empty track, named "Track N" when `name` is `None`.
    pub async fn add_track(
        &self,
        id: u64,
        name: Option<String>,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.rt
            .command("add-track", move |state| {
                state.change(|all| all.add_track(id, name.as_deref(), now_ms()))
            })
            .await?;
        self.project(id)
    }

    pub async fn remove_track(
        &self,
        id: u64,
        track_id: u64,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.edit_project("remove-track", id, move |p| p.remove_track(track_id))
            .await
    }

    pub async fn rename_track(
        &self,
        id: u64,
        track_id: u64,
        name: String,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.edit_project("rename-track", id, move |p| {
            p.track_mut(track_id)?.name = model::clean_name(&name)?;
            Ok(())
        })
        .await
    }

    /// Gain, pan, mute, solo, tone and echo, all at once (the mixer sends
    /// the whole strip). Checked before it is saved.
    pub async fn set_track_settings(
        &self,
        id: u64,
        track_id: u64,
        settings: TrackSettings,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.edit_project("set-track-settings", id, move |p| {
            project::check_settings(&settings)?;
            p.track_mut(track_id)?.settings = settings;
            Ok(())
        })
        .await
    }

    /// Makes the WAV file at `source_path` the track's loop, copied into
    /// Soundscape's store (the caller may delete its copy). Replaces any
    /// earlier recording.
    pub async fn set_clip(
        &self,
        id: u64,
        track_id: u64,
        source_path: String,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.rt
            .command("set-clip", move |state| {
                let source = Path::new(&source_path);
                state.published.get().get(id)?;
                let info = dsp::inspect(source)?;
                // Named by content, with the `.wav` AVAudioFile wants.
                let name = state
                    .recordings
                    .import_hashed_with_extension(source, Some("wav"))?
                    .name;
                let clip = SavedClip {
                    file: name.clone(),
                    sample_rate: info.sample_rate,
                    channels: info.channels,
                    frames: info.frames,
                };
                let result = state.change(|all| {
                    all.edit(id, now_ms(), |p| {
                        p.track_mut(track_id)?.clip = Some(clip);
                        Ok(())
                    })
                });
                if result.is_err() && !state.projects.value().files().contains(&name) {
                    state.recordings.remove(&name);
                }
                result
            })
            .await?;
        self.project(id)
    }

    /// Removes a track's recording; the track stays.
    pub async fn clear_clip(
        &self,
        id: u64,
        track_id: u64,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.edit_project("clear-clip", id, move |p| {
            p.track_mut(track_id)?.clip = None;
            Ok(())
        })
        .await
    }

    /// A track's waveform: the loudest sample in each of `buckets` slices
    /// of its recording, from 0 to 1. On-screen work: stopped by
    /// `cancellation`, the background and shutdown.
    pub async fn waveform(
        &self,
        id: u64,
        track_id: u64,
        buckets: u32,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<f32>, SoundscapeError> {
        let detail = self.project(id)?;
        let track = detail
            .tracks
            .into_iter()
            .find(|t| t.id == track_id)
            .ok_or(SoundscapeError::NotFound { id: track_id })?;
        let Some(clip) = track.clip else {
            return Ok(Vec::new());
        };
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("waveform", work, move |_| {
                let (frames, _) = dsp::read(Path::new(&clip.path))?;
                Ok(dsp::peaks(&frames, buckets as usize))
            })
            .await
    }

    /// Mixes the audible tracks for `passes` passes of the loop (1 to 16)
    /// into a 44.1 kHz stereo WAV file for sharing. Finishing work: it
    /// keeps going when the app leaves the screen, for as long as iOS
    /// gives the app; stopped by `cancellation` and shutdown.
    pub async fn mixdown(
        &self,
        id: u64,
        passes: u32,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Mixdown, SoundscapeError> {
        if !(1..=MAX_PASSES).contains(&passes) {
            return Err(SoundscapeError::OutOfRange {
                setting: "passes".into(),
            });
        }
        let all = self.projects.get();
        let project = all.get(id)?.clone();
        let recordings = self.recordings.clone();
        let path = self.caches.join("mixdowns").join(format!(
            "{}.wav",
            files::title_file_name(&project.title, "Mixdown")
        ));
        let work = self.rt.finishing(cancellation.as_deref());
        self.rt
            .run("mixdown", work, move |cancel| {
                let mut clips = Vec::new();
                for track in project.tracks.iter().filter(|t| project.audible(t)) {
                    let Some(clip) = &track.clip else { continue };
                    let (frames, sample_rate) = dsp::read(&recordings.join(&clip.file))?;
                    clips.push(Loaded {
                        track: track.id,
                        frames,
                        sample_rate,
                    });
                    if cancel.is_cancelled() {
                        return Err(stopped());
                    }
                }
                let mixed = project::mixdown(&project, &clips, MIXDOWN_RATE, passes, || {
                    cancel.is_cancelled()
                })?;
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|err| SoundscapeError::Storage {
                        reason: format!("create the mixdown folder: {err}"),
                    })?;
                }
                // Written beside, then moved in place, so a half-written
                // mixdown never sits where the share sheet looks.
                let partial = path.with_extension("partial");
                dsp::write(&partial, &mixed.frames, MIXDOWN_RATE)?;
                std::fs::rename(&partial, &path).map_err(|err| SoundscapeError::Storage {
                    reason: format!("keep the mixdown: {err}"),
                })?;
                Ok(Mixdown {
                    path: path.to_string_lossy().into_owned(),
                    sample_rate: MIXDOWN_RATE,
                    duration_ms: (mixed.frames.len() as i64 * 1000) / i64::from(MIXDOWN_RATE),
                    peak_db: 20.0 * f64::from(mixed.peak.max(1e-6)).log10(),
                    limited: mixed.limited,
                })
            })
            .await
    }

    // MARK: Listen

    /// The take server's address (`http://100.101.102.103:8642`), or `None`
    /// before the person has set one.
    pub fn listen_server(&self) -> Option<String> {
        self.listen.get().server.clone()
    }

    /// Saves the take server's address after tidying it (a bare
    /// `100.101.102.103` gets `http://` and port 8642). Plain http needs a
    /// numeric address, since iOS refuses plain http to a host name. An
    /// empty address forgets the server. Returns what was saved.
    pub async fn set_listen_server(
        &self,
        address: String,
    ) -> Result<Option<String>, SoundscapeError> {
        let server = if address.trim().is_empty() {
            None
        } else {
            Some(listen::normalize_server(&address)?)
        };
        self.rt
            .command("set-listen-server", move |state| {
                state.listen.change(|s| {
                    s.server = server.clone();
                    Ok::<_, SoundscapeError>(())
                })?;
                state.listen_published.set(state.listen.value().clone());
                Ok(server)
            })
            .await
    }

    /// The songs on the take server, newest first. `fresh` asks the server
    /// to look for new takes now rather than within 20 seconds. On-screen
    /// work.
    pub async fn listen_songs(
        &self,
        fresh: bool,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<SongSummary>, SoundscapeError> {
        let server = self.server()?;
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("listen-songs", work, move |cancel| {
                listen::fetch_library(http.as_ref(), &server, fresh, cancel)
            })
            .await
    }

    /// A song's takes, with their stream URLs, loudness, stars, notes and
    /// which one is picked. On-screen work.
    pub async fn listen_song(
        &self,
        slug: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<SongDetail, SoundscapeError> {
        let server = self.server()?;
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("listen-song", work, move |cancel| {
                listen::fetch_song(http.as_ref(), &server, &slug, cancel)
            })
            .await
    }

    /// Picks a take, rates it (0 to 5 stars) or notes something on it, on
    /// the server, and returns the song as it now is. Finishing work: a
    /// pick made just before leaving the app still arrives.
    pub async fn mark_take(
        &self,
        slug: String,
        take_id: String,
        mark: TakeMark,
    ) -> Result<SongDetail, SoundscapeError> {
        let server = self.server()?;
        listen::mark_body(&slug, &take_id, &mark)?;
        let http = Arc::clone(&self.http);
        let work = self.rt.finishing(None);
        let detail = self
            .rt
            .run("mark-take", work, move |cancel| {
                listen::send_mark(http.as_ref(), &server, &slug, &take_id, &mark, cancel)?;
                listen::fetch_song(http.as_ref(), &server, &slug, cancel)
            })
            .await?;
        // The library shows the pick and the stars too.
        let song = detail.clone();
        self.rt
            .command("library-apply-song", move |state| {
                if state.library.value().snapshot.is_none() {
                    return Ok(());
                }
                state.change_library(|lib| {
                    if let Some(snapshot) = &mut lib.snapshot {
                        library::apply_song(snapshot, &song);
                    }
                    Ok(())
                })
            })
            .await?;
        Ok(detail)
    }

    // MARK: Collection

    /// Favourites, playlists and recently played, as the library has them
    /// now. Never waits for a command.
    pub fn collection(&self) -> MusicCollection {
        collection::build_collection(&self.collection.get(), &self.library())
    }

    pub fn is_favourite(&self, id: String) -> bool {
        self.collection.get().is_favourite(&id)
    }

    /// Makes a song, album or artist (by library id) a favourite, or not.
    pub async fn set_favourite(
        &self,
        id: String,
        on: bool,
    ) -> Result<MusicCollection, SoundscapeError> {
        self.rt
            .command("collection-favourite", move |state| {
                state.change_collection(|c| c.set_favourite(&id, on, now_ms()))
            })
            .await?;
        Ok(self.collection())
    }

    /// A new playlist, empty or with `track_ids`.
    pub async fn create_playlist(
        &self,
        name: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistDetail, SoundscapeError> {
        let id = self
            .rt
            .command("playlist-create", move |state| {
                state.change_collection(|c| c.create_playlist(&name, track_ids, now_ms()))
            })
            .await?;
        self.playlist(id)
    }

    pub async fn rename_playlist(
        &self,
        id: u64,
        name: String,
    ) -> Result<PlaylistDetail, SoundscapeError> {
        self.rt
            .command("playlist-rename", move |state| {
                state.change_collection(|c| c.rename_playlist(id, &name, now_ms()))
            })
            .await?;
        self.playlist(id)
    }

    pub async fn delete_playlist(&self, id: u64) -> Result<MusicCollection, SoundscapeError> {
        self.rt
            .command("playlist-delete", move |state| {
                state.change_collection(|c| c.delete_playlist(id))
            })
            .await?;
        Ok(self.collection())
    }

    /// Adds songs at the end of a playlist.
    pub async fn add_to_playlist(
        &self,
        id: u64,
        track_ids: Vec<String>,
    ) -> Result<PlaylistDetail, SoundscapeError> {
        self.rt
            .command("playlist-add", move |state| {
                state.change_collection(|c| c.add_to_playlist(id, track_ids, now_ms()))
            })
            .await?;
        self.playlist(id)
    }

    /// Keeps the playlist's entries at `places` (from `PlaylistEntry`), in
    /// that order: what's left out is removed. Entries the library no
    /// longer has are kept at the end, so they come back with their song.
    pub async fn arrange_playlist(
        &self,
        id: u64,
        places: Vec<u32>,
    ) -> Result<PlaylistDetail, SoundscapeError> {
        let library = self.library();
        self.rt
            .command("playlist-arrange", move |state| {
                state.change_collection(|c| {
                    let stored = c.playlist_mut(id)?;
                    let mut keep = places;
                    for (place, track) in stored.track_ids.iter().enumerate() {
                        let place = place as u32;
                        if !keep.contains(&place) && !library.tracks.iter().any(|t| &t.id == track)
                        {
                            keep.push(place);
                        }
                    }
                    c.arrange_playlist(id, keep, now_ms())
                })
            })
            .await?;
        self.playlist(id)
    }

    pub fn playlist(&self, id: u64) -> Result<PlaylistDetail, SoundscapeError> {
        collection::playlist_detail(&self.collection.get(), &self.library(), id)
    }

    /// Notes that a song was played (see `counts_as_played`).
    pub async fn record_play(
        &self,
        track_id: String,
        recording_id: String,
    ) -> Result<(), SoundscapeError> {
        self.rt
            .command("collection-play", move |state| {
                state.change_collection(|c| {
                    c.record_play(&track_id, &recording_id, now_ms());
                    Ok(())
                })
            })
            .await
    }

    pub async fn clear_history(&self) -> Result<MusicCollection, SoundscapeError> {
        self.rt
            .command("collection-clear-history", move |state| {
                state.change_collection(|c| {
                    c.history.clear();
                    Ok(())
                })
            })
            .await?;
        Ok(self.collection())
    }

    // MARK: Offline

    /// What the person chose for the phone and how far along it is. Never
    /// waits for a command.
    pub fn offline(&self) -> OfflineSummary {
        let progress = self.progress().clone();
        offline::summary(
            &self.offline.get(),
            &progress,
            &self.kept_library(),
            &self.download_url(),
        )
    }

    /// A song's download state.
    pub fn download_state(&self, track_id: String) -> DownloadState {
        let progress = self.progress().clone();
        self.kept_library()
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .map_or(DownloadState::NotDownloaded, |t| {
                offline::state_of(&self.offline.get(), &progress, t)
            })
    }

    /// Chooses songs for the phone, or takes them off. Call
    /// `sync_downloads` after, to fetch or remove the files.
    pub async fn set_offline(
        &self,
        track_ids: Vec<String>,
        on: bool,
    ) -> Result<OfflineSummary, SoundscapeError> {
        self.rt
            .command("offline-set", move |state| {
                state.change_offline(|o| o.set_wanted(&track_ids, on, now_ms()))
            })
            .await?;
        Ok(self.offline())
    }

    /// Takes every song off the phone and deletes the files.
    pub async fn remove_all_downloads(&self) -> Result<OfflineSummary, SoundscapeError> {
        self.rt
            .command("offline-clear", move |state| {
                state.change_offline(|o| {
                    o.wanted.clear();
                    o.files.clear();
                    Ok(())
                })
            })
            .await?;
        Ok(self.offline())
    }

    /// Downloads what the person chose and removes what nothing wants, one
    /// file at a time, until everything chosen is on the phone or has
    /// failed on this pass. A failed file doesn't stop the others; it is
    /// tried again on the next pass. If a pass is already running this
    /// returns at once. On-screen work: it stops when the app is closed and
    /// carries on with the next pass.
    pub async fn sync_downloads(
        &self,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<OfflineSummary, SoundscapeError> {
        if self.syncing.swap(true, Ordering::SeqCst) {
            return Ok(self.offline());
        }
        let guard = SyncGuard(Arc::clone(&self.syncing), Arc::clone(&self.progress));
        {
            let mut progress = self.progress();
            progress.syncing = true;
            progress.failures.clear();
        }
        loop {
            let plan = offline::plan(&self.offline.get(), &self.kept_library());
            self.remove_unwanted(plan.remove).await?;
            let next = {
                let progress = self.progress();
                plan.fetch
                    .into_iter()
                    .find(|f| !progress.failures.contains_key(&f.key))
            };
            let Some(fetch) = next else {
                break;
            };
            self.progress().current = Some(fetch.key.clone());
            let partial = FileStore::new(self.downloads.clone()).partial_path()?;
            let http = Arc::clone(&self.http);
            let work = self.rt.foreground(cancellation.as_deref());
            let job = fetch.clone();
            let key = fetch.key.clone();
            let target = partial.clone();
            let result = self
                .rt
                .run("offline-download", work, move |cancel| {
                    offline::download(http.as_ref(), &job, &target, cancel)
                })
                .await;
            self.progress().current = None;
            let bytes = match result {
                Ok(bytes) => bytes,
                Err(err @ SoundscapeError::Stopped { .. }) => {
                    let _ = std::fs::remove_file(&partial);
                    return Err(err);
                }
                Err(err) => {
                    let _ = std::fs::remove_file(&partial);
                    self.progress()
                        .failures
                        .insert(fetch.key.clone(), err.to_string());
                    continue;
                }
            };
            let kept = self
                .rt
                .command("offline-keep", move |state| {
                    let name =
                        offline::file_name(state.offline.value().next_file + 1, &fetch.extension);
                    state.downloads.accept(&partial, &name)?;
                    state.change_offline(|o| {
                        o.next_file += 1;
                        o.files.insert(
                            fetch.key.clone(),
                            offline::Download {
                                file: name.clone(),
                                track_id: fetch.track_id.clone(),
                                recording_id: fetch.recording_id.clone(),
                                bytes,
                                at_ms: now_ms(),
                            },
                        );
                        Ok(())
                    })
                })
                .await;
            if let Err(err) = kept {
                self.progress().failures.insert(key, err.to_string());
            }
        }
        drop(guard);
        Ok(self.offline())
    }

    // MARK: Your own files

    /// Copies music files the person picked into Soundscape, with the tags
    /// the phone read from them. A file already imported is counted, not
    /// added again; one that isn't audio or can't be read is refused and
    /// the rest still come in.
    pub async fn import_music(
        &self,
        files: Vec<MusicFile>,
    ) -> Result<ImportResult, SoundscapeError> {
        if files.len() > library::MAX_IMPORT_FILES {
            return Err(SoundscapeError::range("files"));
        }
        self.rt
            .command("library-import", move |state| {
                let mut result = ImportResult::default();
                let mut adding: Vec<(String, library::ImportedFile)> = Vec::new();
                let mut stored_names = Vec::new();
                for file in &files {
                    let Some(extension) = library::music_extension(&file.file_name) else {
                        result
                            .refused
                            .push(format!("{}: not a music file", file.file_name));
                        continue;
                    };
                    let stored = match state
                        .imported
                        .import_hashed_with_extension(Path::new(&file.path), Some(&extension))
                    {
                        Ok(stored) => stored,
                        Err(err) => {
                            result.refused.push(format!("{}: {err}", file.file_name));
                            continue;
                        }
                    };
                    let hash = stored
                        .name
                        .split('.')
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    let id = library::imported_id(&hash);
                    if state.library.value().imported.contains_key(&id)
                        || adding.iter().any(|(a, _)| a == &id)
                    {
                        result.already += 1;
                        continue;
                    }
                    let artwork = file
                        .artwork_path
                        .as_deref()
                        .and_then(|p| keep_artwork(&state.artwork, Path::new(p)).ok());
                    stored_names.push(stored.name.clone());
                    adding.push((
                        id.clone(),
                        library::imported_file(file, stored.name, artwork, now_ms()),
                    ));
                    result.added.push(id);
                }
                let changed = state.change_library(|lib| {
                    lib.imported.extend(adding);
                    Ok(())
                });
                if let Err(err) = changed {
                    let named = state.library.value().music_files();
                    for name in stored_names.iter().filter(|n| !named.contains(*n)) {
                        state.imported.remove(name);
                    }
                    return Err(err);
                }
                Ok(result)
            })
            .await
    }

    /// Removes an imported file from the library and deletes Soundscape's
    /// copy (the original, wherever it was, is untouched).
    pub async fn remove_imported(&self, track_id: String) -> Result<Library, SoundscapeError> {
        self.rt
            .command("library-remove-imported", move |state| {
                if !state.library.value().imported.contains_key(&track_id) {
                    return Err(SoundscapeError::NotInLibrary { id: track_id });
                }
                state.change_library(|lib| {
                    lib.imported.remove(&track_id);
                    lib.edits.remove(&track_id);
                    lib.artwork.remove(&track_id);
                    Ok(())
                })
            })
            .await?;
        Ok(self.library())
    }

    // MARK: Library

    /// The library as kept on the phone: artists, albums and tracks, each
    /// track with its preferred recording. Empty until the first refresh.
    pub fn library(&self) -> Library {
        let mut library = self.kept_library();
        let offline = self.offline.get();
        let url = self.download_url();
        for track in &mut library.tracks {
            offline::localize_track(track, &offline, &url);
        }
        library
    }

    /// Asks the take server for its catalogue and keeps it. `fresh` makes
    /// the server look for new takes now. An older server without a
    /// catalogue is asked song by song. On-screen work.
    pub async fn refresh_library(
        &self,
        fresh: bool,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Library, SoundscapeError> {
        let server = self.server()?;
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        let snapshot = self
            .rt
            .run("library-refresh", work, move |cancel| {
                let now = now_ms();
                if let Some(snapshot) =
                    listen::fetch_catalog(http.as_ref(), &server, fresh, now, cancel)?
                {
                    return Ok(snapshot);
                }
                let songs = listen::fetch_library(http.as_ref(), &server, fresh, cancel)?;
                let mut both = Vec::with_capacity(songs.len());
                for summary in songs {
                    match listen::fetch_song(http.as_ref(), &server, &summary.slug, cancel) {
                        Ok(detail) => both.push((summary, detail)),
                        Err(SoundscapeError::Server { .. }) => continue,
                        Err(other) => return Err(other),
                    }
                }
                Ok(library::snapshot_from_songs(&server, now, &both))
            })
            .await?;
        self.rt
            .command("library-keep", move |state| {
                state.change_library(|lib| {
                    lib.snapshot = Some(snapshot);
                    Ok(())
                })
            })
            .await?;
        Ok(self.library())
    }

    /// Artists, albums and tracks matching `query`.
    pub fn search_library(&self, query: String) -> LibrarySearch {
        library::search(&self.library(), &query)
    }

    /// An album's tracks in order.
    pub fn album_tracks(&self, album_id: String) -> Vec<LibraryTrack> {
        library::album_tracks(&self.library(), &album_id)
    }

    /// An artist's albums, newest first.
    pub fn artist_albums(&self, artist_id: String) -> Vec<LibraryAlbum> {
        library::artist_albums(&self.library(), &artist_id)
    }

    /// An artist's tracks, by title.
    pub fn artist_tracks(&self, artist_id: String) -> Vec<LibraryTrack> {
        self.library()
            .tracks
            .into_iter()
            .filter(|t| t.artist_id.as_deref() == Some(artist_id.as_str()))
            .collect()
    }

    /// A track with every recording of it.
    pub fn library_track(&self, id: String) -> Result<TrackDetail, SoundscapeError> {
        let artwork = self.artwork_url();
        let music = self.imported_url();
        let mut detail = library::build_track(
            &self.library.get(),
            &library::Locate {
                artwork: &artwork,
                music: &music,
            },
            &id,
        )
        .ok_or(SoundscapeError::NotInLibrary { id })?;
        let offline = self.offline.get();
        let url = self.download_url();
        offline::localize_track(&mut detail.track, &offline, &url);
        for recording in &mut detail.recordings {
            offline::localize(&detail.track.id, recording, &offline, &url);
        }
        Ok(detail)
    }

    /// What the queue plays for `track_ids`, in that order: each track's
    /// preferred recording. Tracks that are gone are left out.
    pub fn queue_tracks(&self, track_ids: Vec<String>) -> Vec<QueueTrack> {
        let library = self.library();
        track_ids
            .iter()
            .filter_map(|id| library.tracks.iter().find(|t| &t.id == id))
            .filter_map(library::queue_track)
            .collect()
    }

    /// One recording of a track, to play instead of the preferred one.
    pub fn queue_recording(
        &self,
        track_id: String,
        recording_id: String,
    ) -> Result<QueueTrack, SoundscapeError> {
        let detail = self.library_track(track_id.clone())?;
        let recording = detail
            .recordings
            .into_iter()
            .find(|r| r.id == recording_id)
            .ok_or(SoundscapeError::NotInLibrary { id: recording_id })?;
        Ok(library::queue_track_with(&detail.track, recording))
    }

    /// Changes a track's title, artist, album, number or genre on the
    /// phone. An empty text goes back to what the server says.
    pub async fn edit_track(
        &self,
        id: String,
        edit: TrackEdit,
    ) -> Result<TrackDetail, SoundscapeError> {
        let key = id.clone();
        self.rt
            .command("library-edit", move |state| {
                if !state.library.value().has_track(&key) {
                    return Err(SoundscapeError::NotInLibrary { id: key });
                }
                state.change_library(|lib| {
                    let entry = lib.edits.entry(key.clone()).or_default();
                    entry.merge(edit)?;
                    if entry.is_empty() {
                        lib.edits.remove(&key);
                    }
                    Ok(())
                })
            })
            .await?;
        self.library_track(id)
    }

    /// Uses the picture at `source_path` (JPEG or PNG) as an artist's,
    /// album's or track's artwork. The file is copied into Soundscape.
    pub async fn set_artwork(
        &self,
        target: ArtworkTarget,
        source_path: String,
    ) -> Result<Library, SoundscapeError> {
        self.rt
            .command("library-artwork", move |state| {
                let name = keep_artwork(&state.artwork, Path::new(&source_path))?;
                let id = target.id().to_string();
                let result = state.change_library(|lib| {
                    lib.artwork.insert(id, name.clone());
                    Ok(())
                });
                if result.is_err() && !state.library.value().artwork_files().contains(&name) {
                    state.artwork.remove(&name);
                }
                result
            })
            .await?;
        Ok(self.library())
    }

    /// Goes back to the server's artwork (or the drawn stand-in).
    pub async fn clear_artwork(&self, target: ArtworkTarget) -> Result<Library, SoundscapeError> {
        self.rt
            .command("library-artwork-clear", move |state| {
                state.change_library(|lib| {
                    lib.artwork.remove(target.id());
                    Ok(())
                })
            })
            .await?;
        Ok(self.library())
    }

    /// Makes `recording_id` the one that plays for the track. For a take
    /// on the server that is the server's pick, so JewGrass's picks file
    /// stays the record. Finishing work.
    pub async fn choose_recording(
        &self,
        track_id: String,
        recording_id: String,
    ) -> Result<TrackDetail, SoundscapeError> {
        let slug = library::slug_of(&track_id)
            .ok_or_else(|| SoundscapeError::NotInLibrary {
                id: track_id.clone(),
            })?
            .to_string();
        self.mark_take(
            slug,
            recording_id,
            TakeMark {
                picked: Some(true),
                stars: None,
                note: None,
            },
        )
        .await?;
        self.library_track(track_id)
    }

    /// A take from the takes screen as a queue track, with the library's
    /// title, artist, album and artwork when the library has its song.
    pub fn take_queue_track(&self, song: SongDetail, take: Take) -> QueueTrack {
        let id = library::track_id(&song.slug);
        let known = self.library().tracks.into_iter().find(|t| t.id == id);
        let mut recording = Recording {
            id: take.id.clone(),
            label: take.label.clone(),
            kind: RecordingKind::Generated,
            source: AudioSource::TakeServer {
                server: self.listen_server().unwrap_or_default(),
            },
            audio_url: take.audio_url.clone(),
            made_ms: take.made_ms,
            duration_seconds: None,
            lufs: take.lufs,
            stars: take.stars,
            note: take.note.clone(),
            preferred: take.picked,
            generation: Some(Generation {
                seed: take.seed,
                model: take.model.clone(),
                bpm: take.bpm,
                key: take.key.clone(),
            }),
            lyrics: take.lyrics.clone(),
        };
        offline::localize(
            &id,
            &mut recording,
            &self.offline.get(),
            &self.download_url(),
        );
        match known {
            Some(track) => library::queue_track_with(&track, recording),
            None => QueueTrack {
                track_id: id,
                title: song.title.clone(),
                artist: song
                    .act
                    .clone()
                    .unwrap_or_else(|| library::UNKNOWN_ARTIST.into()),
                album: song.act.clone(),
                artwork_url: None,
                recording,
            },
        }
    }
}

impl SoundscapeCore {
    /// A core whose Listen requests go through `http`, for tests.
    pub fn open_with_http(
        data_directory: &str,
        caches_directory: &str,
        http: Arc<dyn HttpClient>,
    ) -> Result<Arc<Self>, SoundscapeError> {
        Self::open(data_directory, caches_directory, false, http)
    }

    /// Turns a stored artwork file name into a `file://` URL.
    fn artwork_url(&self) -> impl Fn(&str) -> Option<String> {
        stored_url(self.artwork.clone())
    }

    fn imported_url(&self) -> impl Fn(&str) -> Option<String> {
        stored_url(self.imported.clone())
    }

    fn download_url(&self) -> impl Fn(&str) -> Option<String> {
        stored_url(self.downloads.clone())
    }

    /// The library as kept, before downloads are swapped in: what the
    /// offline rules compare against.
    fn kept_library(&self) -> Library {
        let artwork = self.artwork_url();
        let music = self.imported_url();
        library::build_library(
            &self.library.get(),
            &library::Locate {
                artwork: &artwork,
                music: &music,
            },
        )
    }

    fn progress(&self) -> std::sync::MutexGuard<'_, Progress> {
        self.progress.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Removes downloads nothing wants.
    async fn remove_unwanted(&self, keys: Vec<String>) -> Result<(), SoundscapeError> {
        if keys.is_empty() {
            return Ok(());
        }
        self.rt
            .command("offline-remove", move |state| {
                state.change_offline(|o| {
                    for key in &keys {
                        o.files.remove(key);
                    }
                    Ok(())
                })
            })
            .await
    }

    fn server(&self) -> Result<String, SoundscapeError> {
        self.listen
            .get()
            .server
            .clone()
            .ok_or_else(|| SoundscapeError::BadAddress {
                reason: "set the take server's address first".into(),
            })
    }

    fn open(
        data_directory: &str,
        caches_directory: &str,
        fresh: bool,
        http: Arc<dyn HttpClient>,
    ) -> Result<Arc<Self>, SoundscapeError> {
        let directory = DataDirectory::open(data_directory)?;
        let path = directory.path().join(SAVE_FILE);
        let projects = if fresh {
            let (doc, kept) =
                Document::<Projects>::start_fresh(&path, project::VERSION, None, now_ms())?;
            if let Some(kept) = kept {
                forge_runtime::logging::warn!(
                    "unreadable projects set aside as {}",
                    kept.display()
                );
            }
            doc
        } else {
            Document::<Projects>::open(&path, project::VERSION)?
        };
        let recordings =
            FileStore::new(directory.path().join("recordings")).with_max_bytes(MAX_CLIP_BYTES);
        // A crash between keeping a recording and saving the project leaves
        // a file nothing names; at launch, before any command, it can go.
        let saved: HashSet<String> = projects.value().files();
        recordings.sweep(|name| saved.contains(name));
        let published = Arc::new(Published::new(projects.value().clone()));
        let root = recordings.root().to_path_buf();
        // Only an address: an unreadable file is set aside rather than
        // keeping the projects from opening.
        let (listen, kept) = Document::<ListenSettings>::start_fresh(
            directory.path().join(LISTEN_FILE),
            listen::SETTINGS_VERSION,
            None,
            now_ms(),
        )?;
        if let Some(kept) = kept {
            forge_runtime::logging::warn!(
                "unreadable Listen settings set aside as {}",
                kept.display()
            );
        }
        let listen_published = Arc::new(Published::new(listen.value().clone()));
        // The library is a copy of the server's catalogue plus edits: an
        // unreadable one is set aside and starts again empty.
        let (library_doc, kept) = Document::<LibraryFile>::start_fresh(
            directory.path().join(LIBRARY_FILE),
            library::LIBRARY_VERSION,
            None,
            now_ms(),
        )?;
        if let Some(kept) = kept {
            forge_runtime::logging::warn!("unreadable library set aside as {}", kept.display());
        }
        let artwork =
            FileStore::new(directory.path().join("artwork")).with_max_bytes(MAX_ARTWORK_BYTES);
        let named = library_doc.value().artwork_files();
        artwork.sweep(|name| named.contains(name));
        let artwork_root = artwork.root().to_path_buf();
        let library_published = Arc::new(Published::new(library_doc.value().clone()));
        // Playlists are the person's own work: an unreadable file is set
        // aside next to it (kept, not deleted) and the collection starts
        // again rather than keeping the app from opening.
        let (collection_doc, kept) = Document::<CollectionFile>::start_fresh(
            directory.path().join(COLLECTION_FILE),
            collection::COLLECTION_VERSION,
            None,
            now_ms(),
        )?;
        if let Some(kept) = kept {
            forge_runtime::logging::warn!("unreadable collection set aside as {}", kept.display());
        }
        let collection_published = Arc::new(Published::new(collection_doc.value().clone()));
        let imported =
            FileStore::new(directory.path().join("imported")).with_max_bytes(MAX_IMPORT_BYTES);
        let named = library_doc.value().music_files();
        imported.sweep(|name| named.contains(name));
        let imported_root = imported.root().to_path_buf();
        // Only copies of what the server has: an unreadable file is set
        // aside and the downloads start again.
        let (offline_doc, kept) = Document::<OfflineFile>::start_fresh(
            directory.path().join(OFFLINE_FILE),
            offline::OFFLINE_VERSION,
            None,
            now_ms(),
        )?;
        if let Some(kept) = kept {
            forge_runtime::logging::warn!(
                "unreadable downloads list set aside as {}",
                kept.display()
            );
        }
        let downloads = FileStore::new(directory.path().join("downloads"))
            .with_max_bytes(offline::MAX_DOWNLOAD_BYTES);
        let named = offline_doc.value().file_names();
        downloads.sweep(|name| named.contains(name));
        let downloads_root = downloads.root().to_path_buf();
        let offline_published = Arc::new(Published::new(offline_doc.value().clone()));
        let state = State {
            projects,
            recordings,
            published: Arc::clone(&published),
            listen,
            listen_published: Arc::clone(&listen_published),
            library: library_doc,
            library_published: Arc::clone(&library_published),
            artwork,
            collection: collection_doc,
            collection_published: Arc::clone(&collection_published),
            imported,
            offline: offline_doc,
            offline_published: Arc::clone(&offline_published),
            downloads,
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("soundscape", directory, state),
            projects: published,
            recordings: root,
            caches: PathBuf::from(caches_directory),
            listen: listen_published,
            library: library_published,
            artwork: artwork_root,
            collection: collection_published,
            imported: imported_root,
            offline: offline_published,
            downloads: downloads_root,
            progress: Arc::default(),
            syncing: Arc::default(),
            http,
        }))
    }

    async fn edit_project(
        &self,
        name: &'static str,
        id: u64,
        change: impl FnOnce(&mut Project) -> Result<(), SoundscapeError> + Send + 'static,
    ) -> Result<ProjectDetail, SoundscapeError> {
        self.rt
            .command(name, move |state| {
                state.change(|all| all.edit(id, now_ms(), change))
            })
            .await?;
        self.project(id)
    }

    fn detail(&self, p: &Project) -> ProjectDetail {
        ProjectDetail {
            summary: summary(p),
            loop_seconds: p.loop_seconds(),
            tracks: p
                .tracks
                .iter()
                .map(|t| {
                    let audible = p.audible(t);
                    Track {
                        id: t.id,
                        name: t.name.clone(),
                        clip: t.clip.as_ref().map(|c| Clip {
                            path: self.recordings.join(&c.file).to_string_lossy().into_owned(),
                            sample_rate: c.sample_rate,
                            channels: c.channels,
                            frames: c.frames,
                            duration_ms: (c.frames as i64 * 1000) / i64::from(c.sample_rate.max(1)),
                        }),
                        settings: t.settings,
                        audible,
                        mix: project::mix(&t.settings, audible, p.tempo_bpm),
                    }
                })
                .collect(),
        }
    }
}

fn summary(p: &Project) -> ProjectSummary {
    ProjectSummary {
        id: p.id,
        title: p.title.clone(),
        created_ms: p.created_ms,
        modified_ms: p.modified_ms,
        tempo_bpm: p.tempo_bpm,
        bars: p.bars,
        metronome: p.metronome,
        track_count: p.tracks.len() as u32,
    }
}

/// Turns a file name in the store at `root` into a `file://` URL, or `None`
/// when the file has gone.
fn stored_url(root: PathBuf) -> impl Fn(&str) -> Option<String> {
    move |name: &str| {
        let path = root.join(name);
        path.exists().then(|| library::file_url(&path))
    }
}

/// Copies a JPEG, PNG or HEIC picture into the artwork store and returns
/// its name there.
fn keep_artwork(store: &FileStore, source: &Path) -> Result<String, SoundscapeError> {
    let extension = match source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "jpg",
        Some("png") => "png",
        Some("heic") => "heic",
        _ => return Err(SoundscapeError::range("artwork")),
    };
    Ok(store
        .import_hashed_with_extension(source, Some(extension))?
        .name)
}

/// Clears the syncing flag however `sync_downloads` ends.
struct SyncGuard(Arc<AtomicBool>, Arc<Mutex<Progress>>);

impl Drop for SyncGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
        let mut progress = self.1.lock().unwrap_or_else(|e| e.into_inner());
        progress.syncing = false;
        progress.current = None;
    }
}

/// URLSession once the host has installed its services (on iOS, just after
/// the core is made); ureq before that and in Linux tests.
fn default_http() -> Arc<dyn HttpClient> {
    Arc::new(native::http::HttpService::prefer_installed(
        Arc::new(UreqClient::with_user_agent("Soundscape/0.1 (ixOS)")),
        |client| client.max_body_bytes(MAX_ANSWER_BYTES),
    ))
}

fn stopped() -> SoundscapeError {
    SoundscapeError::Stopped {
        kind: ErrorKind::Cancelled,
    }
}

forge_app::export_runtime!(SoundscapeCore, rt);
