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
//!   streams the audio.

pub mod dsp;
pub mod listen;
mod model;
pub mod project;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_app::{Cancellation, DataDirectory, Document, ErrorKind, Published, Runtime};
use forge_net::{HttpClient, UreqClient};
use forge_runtime::now_ms;
use forge_storage::{files, FileStore};

pub use listen::{
    group_songs, loudness_match, ListenSettings, LoudnessMatch, SongDetail, SongGroup, SongSummary,
    Take, TakeMark,
};
pub use model::{
    soundscape_error_kind, Clip, Echo, Mixdown, ProjectDetail, ProjectSummary, SoundscapeError,
    Track, TrackMix, TrackSettings, MAX_BARS, MAX_GAIN_DB, MAX_NAME_CHARS, MAX_TEMPO, MAX_TRACKS,
    MIN_GAIN_DB, MIN_TEMPO,
};
use project::{Loaded, Project, Projects, SavedClip};

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
}

/// The app's core. The Swift app opens one per process and shares it.
#[derive(uniffi::Object)]
pub struct SoundscapeCore {
    rt: Runtime<State>,
    projects: Arc<Published<Projects>>,
    recordings: PathBuf,
    caches: PathBuf,
    listen: Arc<Published<ListenSettings>>,
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
        self.rt
            .run("mark-take", work, move |cancel| {
                listen::send_mark(http.as_ref(), &server, &slug, &take_id, &mark, cancel)?;
                listen::fetch_song(http.as_ref(), &server, &slug, cancel)
            })
            .await
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
        let state = State {
            projects,
            recordings,
            published: Arc::clone(&published),
            listen,
            listen_published: Arc::clone(&listen_published),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("soundscape", directory, state),
            projects: published,
            recordings: root,
            caches: PathBuf::from(caches_directory),
            listen: listen_published,
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
