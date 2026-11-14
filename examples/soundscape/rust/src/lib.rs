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

pub mod dsp;
mod model;
pub mod project;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_app::{Cancellation, DataDirectory, Document, ErrorKind, Published, Runtime};
use forge_runtime::now_ms;
use forge_storage::FileStore;

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

const SAVE_FILE: &str = "soundscape.json";
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
        Self::open(&data_directory, &caches_directory, false)
    }

    /// Opens with no projects in place of a file that can't be read, which
    /// is kept next to it. Refused for data from a newer version.
    #[uniffi::constructor]
    pub fn start_fresh(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, SoundscapeError> {
        forge_runtime::logging::init();
        Self::open(&data_directory, &caches_directory, true)
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
        let path = self
            .caches
            .join("mixdowns")
            .join(format!("{}.wav", file_name(&project.title)));
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
}

impl SoundscapeCore {
    fn open(
        data_directory: &str,
        caches_directory: &str,
        fresh: bool,
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
        let state = State {
            projects,
            recordings,
            published: Arc::clone(&published),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("soundscape", directory, state),
            projects: published,
            recordings: root,
            caches: PathBuf::from(caches_directory),
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
        track_count: p.tracks.len() as u32,
    }
}

fn stopped() -> SoundscapeError {
    SoundscapeError::Stopped {
        kind: ErrorKind::Cancelled,
    }
}

/// A file name from a title: letters, digits, spaces and dashes only.
fn file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        "Mixdown".into()
    } else {
        name
    }
}

forge_app::export_runtime!(SoundscapeCore, rt);
