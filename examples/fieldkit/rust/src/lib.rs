//! FieldKit: record a walk, ride or site visit as a GPS track, and note
//! things along the way (text, photos, voice memos).
//!
//! The app's whole Rust side. What a good track is lives in [`track`]; the
//! SQLite file in `store`; this file is the API the SwiftUI app calls, built
//! on the application runtime (`forge_app`):
//!
//! - **Commands** (`start_trip`, `record`, `stop_trip`, notes, `rename_trip`,
//!   `delete_trip`) run one at a time on the runtime's command thread. Each
//!   is one SQLite transaction or statement; the runtime then raises the
//!   revision and announces `Changed`.
//! - **Queries** (`trips`, `recording`) read a [`Published`] snapshot and
//!   never wait for a command. `trip` and `track` read the file on the
//!   command thread, because a track can be long.
//! - **Recording survives the process.** A trip has no end until
//!   `stop_trip`, so if iOS ends the app mid-walk, the next launch finds it
//!   still recording and the app can carry on or stop it.
//! - Swift owns Core Location and the camera and microphone, and passes the
//!   results in: fixes as [`LocationSample`]s, photos and recordings as
//!   files, which FieldKit copies into its own store.

mod model;
mod store;
pub mod track;

use std::path::Path;
use std::sync::Arc;

use forge_app::{DataDirectory, Published, Runtime};
use forge_runtime::now_ms;
use forge_storage::{FileStore, Opened};

pub use model::{
    fieldkit_error_kind, FieldKitError, Note, NoteKind, Position, Recorded, TripDetail,
    TripSummary, MAX_NOTE_CHARS, MAX_TITLE_CHARS,
};
use store::{NewNote, Store};
pub use track::{LocationSample, TrackPoint};
use track::{Skip, Totals};

uniffi::setup_scaffolding!();

/// Photos and voice memos larger than this are refused (a long 4K video is
/// not a note).
const MAX_MEDIA_BYTES: u64 = 100 * 1024 * 1024;

/// The trip being recorded, as the command thread keeps it.
struct Recording {
    trip_id: i64,
    totals: Totals,
    last: Option<TrackPoint>,
}

/// What the command thread owns.
struct State {
    store: Store,
    media: FileStore,
    recording: Option<Recording>,
    published: Arc<Published<Vec<TripSummary>>>,
}

impl State {
    fn publish(&self) -> Result<(), FieldKitError> {
        self.published.set(self.store.trips()?);
        Ok(())
    }

    fn summary(&self, trip_id: i64) -> Option<TripSummary> {
        self.published
            .get()
            .iter()
            .find(|t| t.id == trip_id)
            .cloned()
    }

    /// Copies a photo or recording into the media store, named by the
    /// SHA-256 of its contents, so the same file added twice is kept once.
    fn import_media(&self, source: &str) -> Result<String, FieldKitError> {
        Ok(self.media.import_hashed(Path::new(source))?.name)
    }

    /// Removes media files no note uses any more.
    fn drop_media(&self, names: &[String]) -> Result<(), FieldKitError> {
        for name in names {
            if !self.store.file_in_use(name)? {
                self.media.remove(name);
            }
        }
        Ok(())
    }

    /// Where a new note on `trip_id` was taken: `at` if given, else the
    /// last kept point while that trip is recording.
    fn note_position(&self, trip_id: i64, at: Option<Position>) -> Option<Position> {
        at.or_else(|| {
            let recording = self.recording.as_ref()?;
            let last = recording.last.as_ref()?;
            (recording.trip_id == trip_id).then_some(Position {
                latitude: last.latitude,
                longitude: last.longitude,
            })
        })
    }

    fn add_note(
        &mut self,
        trip_id: i64,
        at: Option<Position>,
        note: NewNote,
    ) -> Result<i64, FieldKitError> {
        if !self.store.exists(trip_id)? {
            return Err(FieldKitError::NotFound { id: trip_id });
        }
        let at = self.note_position(trip_id, at);
        let id = self.store.add_note(trip_id, now_ms(), at, &note)?;
        self.publish()?;
        Ok(id)
    }
}

/// The app's core. The Swift app opens one per process and shares it.
#[derive(uniffi::Object)]
pub struct FieldKitCore {
    rt: Runtime<State>,
    trips: Arc<Published<Vec<TripSummary>>>,
}

#[uniffi::export]
impl FieldKitCore {
    /// Opens FieldKit's data in `data_directory` (created if missing),
    /// holding the directory so no second core can write to it. A damaged
    /// database is set aside (kept) and a fresh one started.
    #[uniffi::constructor]
    pub fn new(data_directory: String) -> Result<Arc<Self>, FieldKitError> {
        let directory = DataDirectory::open(&data_directory)?;
        let (store, opened) = Store::open(directory.path())?;
        if let Opened::Recovered { reason, .. } = &opened {
            forge_runtime::logging::warn!("trips were damaged and set aside: {reason}");
        }
        let media = FileStore::new(directory.path().join("media")).with_max_bytes(MAX_MEDIA_BYTES);
        media.sweep_temporaries();
        let recording = store.recording()?.map(|(trip_id, totals, last)| Recording {
            trip_id,
            totals,
            last,
        });
        let trips = Arc::new(Published::new(store.trips()?));
        let state = State {
            store,
            media,
            recording,
            published: Arc::clone(&trips),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("fieldkit", directory, state),
            trips,
        }))
    }

    /// Every trip, newest first. Never waits for a command.
    pub fn trips(&self) -> Vec<TripSummary> {
        self.trips.get().as_ref().clone()
    }

    /// The trip being recorded, if any. After a relaunch this is the trip
    /// that was recording when the app ended.
    pub fn recording(&self) -> Option<TripSummary> {
        self.trips.get().iter().find(|t| t.recording).cloned()
    }

    /// Starts recording a new trip; returns it. Only one trip records at a
    /// time.
    pub async fn start_trip(&self, title: String) -> Result<TripSummary, FieldKitError> {
        self.rt
            .command("start-trip", move |state| {
                let title = model::clean_title(&title)?;
                if state.recording.is_some() {
                    return Err(FieldKitError::AlreadyRecording);
                }
                let trip_id = state.store.start_trip(&title, now_ms())?;
                state.recording = Some(Recording {
                    trip_id,
                    totals: Totals::default(),
                    last: None,
                });
                state.publish()?;
                state
                    .summary(trip_id)
                    .ok_or(FieldKitError::NotFound { id: trip_id })
            })
            .await
    }

    /// Adds location fixes to the recording trip, keeping the good ones
    /// (see [`track::check`]). Pass fixes in the order Core Location gave
    /// them; batching a few seconds' worth saves battery.
    pub async fn record(&self, samples: Vec<LocationSample>) -> Result<Recorded, FieldKitError> {
        self.rt
            .command("record", move |state| {
                let recording = state
                    .recording
                    .as_ref()
                    .ok_or(FieldKitError::NotRecording)?;
                let mut totals = recording.totals;
                let mut last = recording.last;
                let first_seq = totals.point_count;
                let mut kept = Vec::new();
                let mut result = Recorded::default();
                for sample in &samples {
                    match track::check(last.as_ref(), sample) {
                        Ok(()) => {
                            let point = TrackPoint::from(*sample);
                            totals.add(last.as_ref(), &point);
                            last = Some(point);
                            kept.push(point);
                        }
                        Err(Skip::Invalid | Skip::Inaccurate) => result.inaccurate += 1,
                        Err(_) => result.skipped += 1,
                    }
                }
                result.kept = kept.len() as u32;
                if !kept.is_empty() {
                    let trip_id = recording.trip_id;
                    state.store.append(trip_id, first_seq, &kept, &totals)?;
                    state.recording = Some(Recording {
                        trip_id,
                        totals,
                        last,
                    });
                    state.publish()?;
                }
                Ok(result)
            })
            .await
    }

    /// Stops recording; returns the finished trip.
    pub async fn stop_trip(&self) -> Result<TripSummary, FieldKitError> {
        self.rt
            .command("stop-trip", |state| {
                let trip_id = state
                    .recording
                    .as_ref()
                    .ok_or(FieldKitError::NotRecording)?
                    .trip_id;
                state.store.end_trip(trip_id, now_ms())?;
                state.recording = None;
                state.publish()?;
                state
                    .summary(trip_id)
                    .ok_or(FieldKitError::NotFound { id: trip_id })
            })
            .await
    }

    pub async fn rename_trip(&self, trip_id: i64, title: String) -> Result<(), FieldKitError> {
        self.rt
            .command("rename-trip", move |state| {
                let title = model::clean_title(&title)?;
                state.store.rename_trip(trip_id, &title)?;
                state.publish()
            })
            .await
    }

    /// Deletes a trip, its track, its notes and their files. Deleting the
    /// recording trip stops the recording.
    pub async fn delete_trip(&self, trip_id: i64) -> Result<(), FieldKitError> {
        self.rt
            .command("delete-trip", move |state| {
                let files = state.store.delete_trip(trip_id)?;
                if state.recording.as_ref().map(|r| r.trip_id) == Some(trip_id) {
                    state.recording = None;
                }
                state.drop_media(&files)?;
                state.publish()
            })
            .await
    }

    /// A trip with its notes.
    pub async fn trip(&self, trip_id: i64) -> Result<TripDetail, FieldKitError> {
        self.rt
            .command("trip", move |state| {
                let summary = state
                    .summary(trip_id)
                    .ok_or(FieldKitError::NotFound { id: trip_id })?;
                let media = &state.media;
                let notes = state.store.notes(trip_id, |name| {
                    media.root().join(name).to_string_lossy().into_owned()
                })?;
                Ok(TripDetail { summary, notes })
            })
            .await
    }

    /// A trip's kept track, in order.
    pub async fn track(&self, trip_id: i64) -> Result<Vec<TrackPoint>, FieldKitError> {
        self.rt
            .command("track", move |state| {
                if !state.store.exists(trip_id)? {
                    return Err(FieldKitError::NotFound { id: trip_id });
                }
                state.store.track(trip_id)
            })
            .await
    }

    /// Adds a text note. `at` is where it applies; leave it out to use the
    /// current position of a recording trip.
    pub async fn add_text_note(
        &self,
        trip_id: i64,
        body: String,
        at: Option<Position>,
    ) -> Result<i64, FieldKitError> {
        self.rt
            .command("add-text-note", move |state| {
                let body = model::clean_note(&body, true)?;
                state.add_note(trip_id, at, NewNote::Text { body })
            })
            .await
    }

    /// Adds a photo, copied from `source_path` (the caller may delete its
    /// copy afterwards).
    pub async fn add_photo_note(
        &self,
        trip_id: i64,
        source_path: String,
        caption: String,
        at: Option<Position>,
    ) -> Result<i64, FieldKitError> {
        self.rt
            .command("add-photo-note", move |state| {
                let caption = model::clean_note(&caption, false)?;
                let file = state.import_media(&source_path)?;
                let result = state.add_note(
                    trip_id,
                    at,
                    NewNote::Photo {
                        file: file.clone(),
                        caption,
                    },
                );
                if result.is_err() {
                    state.drop_media(&[file])?;
                }
                result
            })
            .await
    }

    /// Adds a voice memo, copied from `source_path`.
    pub async fn add_voice_note(
        &self,
        trip_id: i64,
        source_path: String,
        duration_ms: i64,
        at: Option<Position>,
    ) -> Result<i64, FieldKitError> {
        self.rt
            .command("add-voice-note", move |state| {
                let file = state.import_media(&source_path)?;
                let result = state.add_note(
                    trip_id,
                    at,
                    NewNote::Voice {
                        file: file.clone(),
                        duration_ms,
                    },
                );
                if result.is_err() {
                    state.drop_media(&[file])?;
                }
                result
            })
            .await
    }

    /// Removes a note and, if no other note uses it, its file.
    pub async fn remove_note(&self, note_id: i64) -> Result<(), FieldKitError> {
        self.rt
            .command("remove-note", move |state| {
                if let Some(file) = state.store.remove_note(note_id)? {
                    state.drop_media(&[file])?;
                }
                state.publish()
            })
            .await
    }
}

forge_app::export_runtime!(FieldKitCore, rt);
