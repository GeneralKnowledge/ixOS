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
//!   never wait for a command. `trip` and `track` read the file through a
//!   second, read-only connection, as on-screen work off the command
//!   thread: a track can be long, and a command would raise the revision.
//! - **Export** (`export_trip`, GPX or GeoJSON) is finishing work: it keeps
//!   going if the app leaves the screen, while iOS allows.
//! - **Recording survives the process.** A trip has no end until
//!   `stop_trip`, so if iOS ends the app mid-walk, the next launch finds it
//!   still recording and the app can carry on or stop it.
//! - Swift owns Core Location and the camera and microphone, and passes the
//!   results in: fixes as [`LocationSample`]s, photos and recordings as
//!   files, which FieldKit copies into its own store.

mod export;
mod model;
mod store;
pub mod track;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use forge_app::{Cancellation, DataDirectory, ErrorKind, Published, Runtime};
use forge_runtime::{lock, now_ms};
use forge_storage::rusqlite::Connection;
use forge_storage::{FileStore, Opened};

pub use export::ExportFormat;
pub use model::{
    fieldkit_error_kind, FieldKitError, Note, NoteKind, Position, Recorded, TripDetail,
    TripSummary, MAX_NOTE_CHARS, MAX_TITLE_CHARS,
};
use store::{NewNote, Store};
pub use track::{LocationSample, TrackPoint};
use track::{Skip, Totals};

uniffi::setup_scaffolding!();

// Native services the host implements: Keychain, directories, permissions
// and the rest of the defaults, plus location (docs/native-services.md).
forge_services::native_services!(location);

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes to the same
// log as Rust.
forge_runtime::log_export!();

use forge_services::Watch;
use native::location::{
    LocationAccuracy, LocationEvent, LocationFix, LocationOptions, LocationService,
};
use native::{NativePermission, NativeServiceError, NativeServices, PermissionStatus};

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

    /// Keeps the good fixes among `samples` in the recording trip.
    fn record(&mut self, samples: &[LocationSample]) -> Result<Recorded, FieldKitError> {
        let recording = self.recording.as_ref().ok_or(FieldKitError::NotRecording)?;
        let mut totals = recording.totals;
        let mut last = recording.last;
        let first_seq = totals.point_count;
        let mut kept = Vec::new();
        let mut result = Recorded::default();
        for sample in samples {
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
            self.store.append(trip_id, first_seq, &kept, &totals)?;
            self.recording = Some(Recording {
                trip_id,
                totals,
                last,
            });
            self.publish()?;
        }
        Ok(result)
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
    /// Read-only connection for queries off the command thread.
    reader: Arc<Mutex<Connection>>,
    media_root: PathBuf,
    caches: PathBuf,
    /// The location service, when the host installed one.
    location: Option<LocationService>,
    /// Following the location service while a trip records.
    watch: Mutex<Option<Watch>>,
    /// Why following location stopped or never started, for the screen.
    location_problem: Mutex<Option<String>>,
    /// This core, for the location listener (which must not keep it alive).
    me: Weak<FieldKitCore>,
}

#[uniffi::export]
impl FieldKitCore {
    /// Opens FieldKit's data in `data_directory` (created if missing),
    /// holding the directory so no second core can write to it. A damaged
    /// database is set aside (kept) and a fresh one started.
    /// `caches_directory` is for files the app can lose (exports).
    #[uniffi::constructor]
    pub fn new(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, FieldKitError> {
        forge_runtime::logging::init();
        Self::open(
            &data_directory,
            &caches_directory,
            LocationService::installed().ok(),
        )
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

    /// Starts recording a new trip and following the location service;
    /// returns the trip. Only one trip records at a time. Ask for location
    /// permission first (`request_location_permission`).
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
            .inspect(|_| self.follow_location())
    }

    /// Adds location fixes to the recording trip, keeping the good ones
    /// (see [`track::check`]), in the order Core Location gave them. The
    /// core follows the location service itself while a trip records; this
    /// is for fixes from elsewhere (an imported track, tests).
    pub async fn record(&self, samples: Vec<LocationSample>) -> Result<Recorded, FieldKitError> {
        self.rt
            .command("record", move |state| state.record(&samples))
            .await
    }

    /// Stops recording; returns the finished trip.
    pub async fn stop_trip(&self) -> Result<TripSummary, FieldKitError> {
        self.stop_following();
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
        if self.recording().map(|t| t.id) == Some(trip_id) {
            self.stop_following();
        }
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

    /// Starts following the location service again for the recording trip,
    /// if it isn't already: after location permission was granted, or when
    /// the app comes back to the screen. Does nothing when no trip records.
    pub fn resume_recording(&self) {
        if self.recording().is_some() && lock(&self.watch).is_none() {
            self.follow_location();
        }
    }

    /// Whether fixes are reaching the recording trip.
    pub fn is_following_location(&self) -> bool {
        lock(&self.watch).is_some()
    }

    /// Why location isn't reaching the recording trip, in words for the
    /// screen, or `None`.
    pub fn location_problem(&self) -> Option<String> {
        lock(&self.location_problem).clone()
    }

    /// Whether FieldKit may use location.
    pub async fn location_permission(&self) -> PermissionStatus {
        match NativeServices::installed() {
            Ok(native) => native.permission_status(NativePermission::Location).await,
            Err(_) => PermissionStatus::Unsupported,
        }
    }

    /// Shows the system prompt (if never asked); call it from a button the
    /// person tapped. Resumes a recording trip if it is now allowed.
    pub async fn request_location_permission(&self) -> PermissionStatus {
        let status = match NativeServices::installed() {
            Ok(native) => native.request_permission(NativePermission::Location).await,
            Err(_) => PermissionStatus::Unsupported,
        };
        if status.allows_use() {
            self.resume_recording();
        }
        status
    }

    /// A trip with its notes. On-screen work: stopped by `cancellation`,
    /// the background and shutdown.
    pub async fn trip(
        &self,
        trip_id: i64,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<TripDetail, FieldKitError> {
        let summary = self.summary(trip_id)?;
        let reader = Arc::clone(&self.reader);
        let media = self.media_root.clone();
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("trip", work, move |_| {
                let notes = store::notes(&lock(&reader), trip_id, |name| {
                    media.join(name).to_string_lossy().into_owned()
                })?;
                Ok(TripDetail { summary, notes })
            })
            .await
    }

    /// A trip's kept track, in order. On-screen work, like `trip`.
    pub async fn track(
        &self,
        trip_id: i64,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<TrackPoint>, FieldKitError> {
        self.summary(trip_id)?;
        let reader = Arc::clone(&self.reader);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("track", work, move |_| {
                store::track(&lock(&reader), trip_id)
            })
            .await
    }

    /// Writes a trip as GPX or GeoJSON for sharing; returns the file's
    /// path. Finishing work: it keeps going when the app leaves the screen,
    /// for as long as iOS gives the app; stopped by `cancellation` and
    /// shutdown.
    pub async fn export_trip(
        &self,
        trip_id: i64,
        format: ExportFormat,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<String, FieldKitError> {
        let trip = self.summary(trip_id)?;
        let reader = Arc::clone(&self.reader);
        let media = self.media_root.clone();
        let path = self.caches.join(format!(
            "{}.{}",
            export_name(&trip.title),
            format.extension()
        ));
        let work = self.rt.finishing(cancellation.as_deref());
        self.rt
            .run("export", work, move |cancel| {
                let (points, notes) = {
                    let c = lock(&reader);
                    let points = store::track(&c, trip_id)?;
                    let notes = store::notes(&c, trip_id, |name| {
                        media.join(name).to_string_lossy().into_owned()
                    })?;
                    (points, notes)
                };
                if cancel.is_cancelled() {
                    return Err(FieldKitError::Stopped {
                        kind: ErrorKind::Cancelled,
                    });
                }
                let text = match format {
                    ExportFormat::Gpx => export::gpx(&trip, &points, &notes),
                    ExportFormat::GeoJson => export::geojson(&trip, &points, &notes),
                };
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|err| FieldKitError::Storage {
                        reason: format!("create the export folder: {err}"),
                    })?;
                }
                forge_storage::write_atomic(&path, text.as_bytes())?;
                Ok(path.to_string_lossy().into_owned())
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

impl FieldKitCore {
    /// Opens the core with an explicit location service (tests pass a fake).
    pub fn open(
        data_directory: &str,
        caches_directory: &str,
        location: Option<LocationService>,
    ) -> Result<Arc<Self>, FieldKitError> {
        let directory = DataDirectory::open(data_directory)?;
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
        let reader = Arc::new(Mutex::new(store.reader()?));
        let media_root = media.root().to_path_buf();
        let state = State {
            store,
            media,
            recording,
            published: Arc::clone(&trips),
        };
        let core = Arc::new_cyclic(|me| Self {
            rt: Runtime::with_directory("fieldkit", directory, state),
            trips,
            reader,
            media_root,
            caches: PathBuf::from(caches_directory),
            location,
            watch: Mutex::new(None),
            location_problem: Mutex::new(None),
            me: me.clone(),
        });
        // A trip that was recording when the app ended carries on.
        core.resume_recording();
        Ok(core)
    }

    /// Follows the location service, sending each fix to the recording
    /// trip as a command. The listener holds the core weakly, so the core
    /// still closes when the app lets go of it.
    fn follow_location(&self) {
        let mut watch = lock(&self.watch);
        if watch.is_some() {
            return;
        }
        let Some(service) = &self.location else {
            *lock(&self.location_problem) = Some("Location isn't available in this build.".into());
            return;
        };
        let options = LocationOptions {
            accuracy: LocationAccuracy::Best,
            // Closer fixes are dropped anyway (track::MIN_STEP_M).
            distance_filter_m: Some(2.0),
            include_heading: false,
        };
        let me = self.me.clone();
        let started = service.watch(options, move |event| {
            let Some(core) = me.upgrade() else { return };
            match event {
                LocationEvent::Fix { fix } => {
                    *lock(&core.location_problem) = None;
                    let sample = sample_from(fix);
                    // Queued now; nobody waits for the answer. A fix that
                    // arrives after the trip stopped is refused, harmlessly.
                    drop(
                        core.rt
                            .command("record-fix", move |state| state.record(&[sample])),
                    );
                }
                LocationEvent::Failed {
                    permission_denied, ..
                } => {
                    *lock(&core.location_problem) = Some(if *permission_denied {
                        "Location is turned off for FieldKit. Allow it in Settings to keep recording.".into()
                    } else {
                        "Location isn't available right now.".into()
                    });
                }
                LocationEvent::Heading { .. } => {}
            }
        });
        match started {
            Ok(w) => {
                *watch = Some(w);
                *lock(&self.location_problem) = None;
            }
            Err(err) => {
                forge_runtime::logging::warn!("location watch failed: {err}");
                *lock(&self.location_problem) = Some(match err {
                    NativeServiceError::PermissionDenied { .. } => {
                        "Allow location so FieldKit can record your track.".into()
                    }
                    _ => "Location isn't available right now.".into(),
                });
            }
        }
    }

    fn stop_following(&self) {
        // Dropping the watch stops the platform updates.
        lock(&self.watch).take();
    }

    fn summary(&self, trip_id: i64) -> Result<TripSummary, FieldKitError> {
        self.trips
            .get()
            .iter()
            .find(|t| t.id == trip_id)
            .cloned()
            .ok_or(FieldKitError::NotFound { id: trip_id })
    }
}

fn sample_from(fix: &LocationFix) -> LocationSample {
    LocationSample {
        latitude: fix.latitude,
        longitude: fix.longitude,
        altitude_m: fix.altitude_m,
        horizontal_accuracy_m: fix.horizontal_accuracy_m,
        timestamp_ms: fix.timestamp_ms,
    }
}

/// A file name from a trip title: letters, digits, spaces and dashes only.
fn export_name(title: &str) -> String {
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
        "Trip".into()
    } else {
        name
    }
}

forge_app::export_runtime!(FieldKitCore, rt);

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(i: i64) -> LocationEvent {
        LocationEvent::Fix {
            fix: LocationFix {
                latitude: 51.5 + 0.0001 * i as f64,
                longitude: -0.12,
                horizontal_accuracy_m: 5.0,
                altitude_m: None,
                vertical_accuracy_m: None,
                speed_mps: None,
                speed_accuracy_mps: None,
                course_degrees: None,
                course_accuracy_degrees: None,
                floor: None,
                timestamp_ms: 1_000_000 + i * 5000,
            },
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        forge_runtime::block_on(future)
    }

    /// Waits for the queued fix commands: a command runs after the ones
    /// queued before it.
    fn settle(core: &FieldKitCore) {
        block_on(core.rt.command("settle", |_| Ok::<_, FieldKitError>(()))).unwrap();
    }

    fn open(dir: &tempfile::TempDir, location: Option<LocationService>) -> Arc<FieldKitCore> {
        FieldKitCore::open(
            dir.path().join("data").to_str().unwrap(),
            dir.path().join("caches").to_str().unwrap(),
            location,
        )
        .unwrap()
    }

    #[test]
    fn a_recording_trip_follows_location_until_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let (service, fake) = LocationService::for_tests();
        let core = open(&dir, Some(service.clone()));
        assert!(fake.options().is_none(), "nothing follows location yet");

        let trip = block_on(core.start_trip("Walk".into())).unwrap();
        assert!(core.is_following_location());
        assert_eq!(fake.options().unwrap().accuracy, LocationAccuracy::Best);
        for i in 0..5 {
            fake.send(fix(i));
        }
        settle(&core);
        assert_eq!(core.recording().unwrap().point_count, 5);

        block_on(core.stop_trip()).unwrap();
        assert!(!core.is_following_location());
        assert!(fake.options().is_none(), "the platform updates stopped");
        fake.send(fix(9));
        settle(&core);
        assert_eq!(core.trips()[0].point_count, 5);
        assert_eq!(core.trips()[0].id, trip.id);
    }

    #[test]
    fn a_trip_recording_at_launch_follows_location_again() {
        let dir = tempfile::tempdir().unwrap();
        let (service, fake) = LocationService::for_tests();
        let core = open(&dir, Some(service.clone()));
        block_on(core.start_trip("Walk".into())).unwrap();
        fake.send(fix(0));
        settle(&core);
        core.shutdown();
        drop(core);

        let core = open(&dir, Some(service));
        assert!(core.is_following_location());
        fake.send(fix(1));
        settle(&core);
        assert_eq!(core.recording().unwrap().point_count, 2);
    }

    #[test]
    fn denied_or_missing_location_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let (service, fake) = LocationService::for_tests();
        fake.denied.store(true, std::sync::atomic::Ordering::SeqCst);
        let core = open(&dir, Some(service));
        block_on(core.start_trip("Walk".into())).unwrap();
        assert!(!core.is_following_location());
        assert!(core.location_problem().unwrap().contains("Allow location"));

        // Allowed later: resume picks it up.
        fake.denied
            .store(false, std::sync::atomic::Ordering::SeqCst);
        core.resume_recording();
        assert!(core.is_following_location());
        assert_eq!(core.location_problem(), None);

        fake.send(LocationEvent::Failed {
            reason: "denied".into(),
            permission_denied: true,
        });
        assert!(core.location_problem().unwrap().contains("Settings"));

        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir, None);
        block_on(core.start_trip("Walk".into())).unwrap();
        assert!(core.location_problem().unwrap().contains("isn't available"));
    }
}
