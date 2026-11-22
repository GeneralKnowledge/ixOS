//! Soundscape: a personal music player. One library across the person's
//! own files, free catalogues (Jamendo, the Internet Archive) and a take
//! server on their computer; the player doesn't care where a song comes
//! from (`docs/architecture.md`).
//!
//! The app's whole Rust side, built on the application runtime
//! (`forge_app`): [`library`] keeps tracks apart from their recordings and
//! where each recording's audio lives; [`collection`] the favourites,
//! playlists and history; [`offline`] what is kept on the phone; [`queue`]
//! what plays next and [`playback`] what was playing; [`jamendo`] and
//! [`archive`] the catalogues; [`listen`] the take server. AVPlayer plays;
//! Rust owns the state and the rules. Commands save before they answer;
//! queries read a published copy and never wait.

pub mod archive;
pub mod catalogue;
pub mod collection;
pub mod jamendo;
pub mod library;
pub mod listen;
pub mod lyrics;
pub mod merge;
mod model;
pub mod musicbrainz;
pub mod offline;
pub mod playback;
pub mod queue;
pub mod search;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use forge_app::{Cancellation, DataDirectory, Document, Published, Runtime};
use forge_net::{HttpClient, UreqClient};
use forge_runtime::now_ms;
use forge_storage::FileStore;

pub use archive::ArchiveItem;
pub use catalogue::{CatalogueTrack, Rights};
use collection::CollectionFile;
pub use collection::{
    counts_as_played, MusicCollection, Playlist, PlaylistDetail, PlaylistEntry, RecentPlay,
};
pub use jamendo::{JamendoGenre, JamendoMore, JamendoMoreTracks};
use library::LibraryFile;
pub use library::{
    ArtworkTarget, AudioSource, Generation, ImportResult, Library, LibraryAlbum, LibraryArtist,
    LibrarySearch, LibraryTrack, MusicFile, Recording, RecordingKind, RecordingOrder, TrackDetail,
    TrackEdit,
};
pub use library::{DuplicateReason, DuplicateSongs, MergedSong};
pub use listen::{
    group_songs, loudness_match, ListenSettings, LoudnessMatch, SongDetail, SongGroup, SongSummary,
    Take, TakeMark,
};
pub use lyrics::{lyric_line_at, lyric_lines, lyric_word_at, LyricKind, LyricLine, LyricWord};
pub use model::{soundscape_error_kind, SoundscapeError, MAX_NAME_CHARS};
pub use musicbrainz::MusicBrainzMatch;
pub use offline::{DownloadState, OfflineSummary, OfflineTrack};
use offline::{OfflineFile, Progress};
use playback::PlaybackFile;
pub use playback::SavedPlayback;
pub use queue::{PlayQueue, QueueEntry, QueueTrack, RepeatMode};
pub use search::CatalogueSearch;

uniffi::setup_scaffolding!();

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes to the same
// log as Rust.
forge_runtime::log_export!();

// Native services the host implements, as `crate::native`; `http` sends
// Listen's requests through URLSession on iOS, so they follow the phone's
// VPN (Tailscale) and network settings.
forge_services::native_services!(http);

/// Listen's settings (the take server's address), apart from the projects.
const LISTEN_FILE: &str = "listen.json";
/// The music library (the server's last catalogue, edits, artwork names).
const LIBRARY_FILE: &str = "library.json";
/// Favourites, playlists and history.
const COLLECTION_FILE: &str = "collection.json";
/// What is kept on the phone to play offline.
const OFFLINE_FILE: &str = "offline.json";
/// What was playing, to carry on after the app closes.
const PLAYBACK_FILE: &str = "playback.json";
/// The largest music file imported.
const MAX_IMPORT_BYTES: u64 = 1024 * 1024 * 1024;
/// The largest picture accepted as artwork.
const MAX_ARTWORK_BYTES: u64 = 20 * 1024 * 1024;
/// The largest answer the take server may give (a library of a few hundred
/// songs is well under this).
const MAX_ANSWER_BYTES: u64 = 4 * 1024 * 1024;

/// What the command thread owns.
struct State {
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
    playback: Document<PlaybackFile>,
}

impl State {
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
    listen: Arc<Published<ListenSettings>>,
    library: Arc<Published<LibraryFile>>,
    artwork: PathBuf,
    collection: Arc<Published<CollectionFile>>,
    imported: PathBuf,
    offline: Arc<Published<OfflineFile>>,
    downloads: PathBuf,
    progress: Arc<Mutex<Progress>>,
    playback: Arc<Published<PlaybackFile>>,
    /// True while `sync_downloads` runs, so a second one returns at once.
    syncing: Arc<AtomicBool>,
    http: Arc<dyn HttpClient>,
}

#[uniffi::export]
impl SoundscapeCore {
    /// Opens the library saved in `data_directory` (created if missing),
    /// holding the directory so no second core can write to it. A file that
    /// can't be read is set aside and starts again empty.
    /// `caches_directory` is for files the app can lose.
    #[uniffi::constructor]
    pub fn new(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, SoundscapeError> {
        forge_runtime::logging::init();
        Self::open(&data_directory, &caches_directory, false, default_http())
    }

    /// The same as `new`: kept for the host's recovery screen, which offers
    /// a fresh start when opening fails.
    #[uniffi::constructor]
    pub fn start_fresh(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, SoundscapeError> {
        forge_runtime::logging::init();
        Self::open(&data_directory, &caches_directory, true, default_http())
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

    /// Removes a song the person added: an imported file (deleting
    /// Soundscape's copy; the original, wherever it was, is untouched) or a
    /// catalogue track. Its download goes on the next pass.
    pub async fn remove_imported(&self, track_id: String) -> Result<Library, SoundscapeError> {
        self.rt
            .command("library-remove-imported", move |state| {
                let lib = state.library.value();
                if !lib.imported.contains_key(&track_id) && !lib.catalogue.contains_key(&track_id) {
                    return Err(SoundscapeError::NotInLibrary { id: track_id });
                }
                state.change_library(|lib| {
                    lib.imported.remove(&track_id);
                    lib.catalogue.remove(&track_id);
                    lib.edits.remove(&track_id);
                    lib.artwork.remove(&track_id);
                    lib.musicbrainz.remove(&track_id);
                    Ok(())
                })
            })
            .await?;
        Ok(self.library())
    }

    // MARK: Catalogues

    /// The person's Jamendo client ID, or `None` before they set one.
    pub fn jamendo_client_id(&self) -> Option<String> {
        self.listen.get().jamendo_client_id.clone()
    }

    /// Saves the Jamendo client ID (from developer.jamendo.com). Empty
    /// forgets it; Jamendo tracks already in the library stay. Returns
    /// what was saved.
    pub async fn set_jamendo_client_id(
        &self,
        client_id: String,
    ) -> Result<Option<String>, SoundscapeError> {
        let client_id = jamendo::clean_client_id(&client_id)?;
        self.rt
            .command("set-jamendo-client-id", move |state| {
                state.listen.change(|s| {
                    s.jamendo_client_id = client_id.clone();
                    Ok::<_, SoundscapeError>(())
                })?;
                state.listen_published.set(state.listen.value().clone());
                Ok(client_id)
            })
            .await
    }

    /// How many seconds songs overlap as one ends and the next starts; 0
    /// is gapless.
    pub fn crossfade_seconds(&self) -> u32 {
        self.listen.get().crossfade_seconds
    }

    /// Sets the crossfade, up to `MAX_CROSSFADE_SECONDS`; 0 plays songs
    /// back to back with no gap. Returns what was saved.
    pub async fn set_crossfade_seconds(&self, seconds: u32) -> Result<u32, SoundscapeError> {
        let seconds = seconds.min(listen::MAX_CROSSFADE_SECONDS);
        self.rt
            .command("set-crossfade", move |state| {
                state.listen.change(|s| {
                    s.crossfade_seconds = seconds;
                    Ok::<_, SoundscapeError>(())
                })?;
                state.listen_published.set(state.listen.value().clone());
                Ok(seconds)
            })
            .await
    }

    /// Jamendo tracks matching `query`, or this week's most popular when it
    /// is empty. On-screen work.
    pub async fn search_jamendo(
        &self,
        query: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
        let client_id = self.needs_jamendo_client_id()?;
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("jamendo-search", work, move |cancel| {
                jamendo::search(http.as_ref(), &client_id, &query, cancel)
            })
            .await
    }

    /// The genres Discover offers for Jamendo, in the order shown.
    pub fn jamendo_genres(&self) -> Vec<JamendoGenre> {
        jamendo::genres()
    }

    /// Jamendo tracks in the genre `tag` (such as `jazz`), this week's
    /// most popular first. On-screen work.
    pub async fn jamendo_genre(
        &self,
        tag: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
        let client_id = self.needs_jamendo_client_id()?;
        let tag = jamendo::clean_tag(&tag)?;
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("jamendo-genre", work, move |cancel| {
                jamendo::tracks(http.as_ref(), &client_id, &jamendo::Ask::Genre(tag), cancel)
            })
            .await
    }

    /// More from Jamendo for the Jamendo track `track_id`
    /// (`jamendo:<id>`): its artist's other tracks, or others in its
    /// genre. On-screen work.
    pub async fn more_from_jamendo(
        &self,
        track_id: String,
        by: JamendoMore,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<JamendoMoreTracks, SoundscapeError> {
        let client_id = self.needs_jamendo_client_id()?;
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("jamendo-more", work, move |cancel| {
                jamendo::more(http.as_ref(), &client_id, &track_id, by, cancel)
            })
            .await
    }

    /// Internet Archive items (albums, concerts, recordings) matching
    /// `query`, most downloaded first; this week's most downloaded
    /// net-label releases when it is empty. On-screen work.
    pub async fn search_archive(
        &self,
        query: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<ArchiveItem>, SoundscapeError> {
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("archive-search", work, move |cancel| {
                archive::search(http.as_ref(), &query, cancel)
            })
            .await
    }

    /// One search for everything, the catalogues' part: Jamendo and the
    /// Internet Archive asked together for `query`, each with its own
    /// results or its own problem, leaving out Jamendo tracks already in
    /// the library (`search_library` shows those). Nothing for an empty
    /// query. On-screen work.
    pub async fn search_catalogues(
        &self,
        query: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<CatalogueSearch, SoundscapeError> {
        let client_id = self.jamendo_client_id();
        let library = Arc::clone(&self.library);
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("search-catalogues", work, move |cancel| {
                let file = library.get();
                search::search_catalogues(
                    http.as_ref(),
                    client_id.as_deref(),
                    &query,
                    &|id| file.has_track(id),
                    cancel,
                )
            })
            .await
    }

    /// An Internet Archive item's playable tracks, in order, each with the
    /// item's licence. On-screen work.
    pub async fn archive_item_tracks(
        &self,
        identifier: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<CatalogueTrack>, SoundscapeError> {
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("archive-item", work, move |cancel| {
                archive::item_tracks(http.as_ref(), &identifier, cancel)
            })
            .await
    }

    // MARK: MusicBrainz

    /// Recordings MusicBrainz knows that might be this song, best first,
    /// ranked by its title, artist and length. Changes nothing. On-screen
    /// work.
    pub async fn identify_track(
        &self,
        track_id: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<MusicBrainzMatch>, SoundscapeError> {
        let track = self.library_track(track_id)?.track;
        let wanted = musicbrainz::Wanted {
            title: track.title,
            artist: (track.artist != library::UNKNOWN_ARTIST).then_some(track.artist),
            duration_seconds: track.duration_seconds,
        };
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("musicbrainz-identify", work, move |cancel| {
                musicbrainz::identify(http.as_ref(), &wanted, cancel)
            })
            .await
    }

    /// Links a song to the MusicBrainz recording the person picked. With
    /// `use_details`, the song also takes its title, artist, album and
    /// track number (as the person's own changes, so Reset brings back the
    /// source's); its cover shows when the song has no picture of its own.
    pub async fn link_musicbrainz(
        &self,
        track_id: String,
        found: MusicBrainzMatch,
        use_details: bool,
    ) -> Result<TrackDetail, SoundscapeError> {
        if !musicbrainz::is_mbid(&found.recording_id)
            || found
                .release_id
                .as_deref()
                .is_some_and(|r| !musicbrainz::is_mbid(r))
            || found
                .release_group_id
                .as_deref()
                .is_some_and(|r| !musicbrainz::is_mbid(r))
        {
            return Err(SoundscapeError::Server {
                reason: "that isn't a MusicBrainz recording".into(),
            });
        }
        let key = track_id.clone();
        self.rt
            .command("musicbrainz-link", move |state| {
                if !state.library.value().has_track(&key) {
                    return Err(SoundscapeError::NotInLibrary { id: key });
                }
                let edit = use_details.then(|| TrackEdit {
                    title: Some(found.title.clone()),
                    artist: found.artist.clone(),
                    album: found.album.clone(),
                    number: found.number,
                    genre: None,
                });
                state.change_library(|lib| {
                    if let Some(edit) = edit {
                        let entry = lib.edits.entry(key.clone()).or_default();
                        let mut merged = entry.clone();
                        merged.merge(edit)?;
                        *entry = merged;
                    }
                    lib.musicbrainz.insert(
                        key.clone(),
                        library::MusicBrainzLink {
                            recording_id: found.recording_id.clone(),
                            release_id: found.release_id.clone(),
                            artwork_url: found
                                .release_group_id
                                .as_deref()
                                .map(musicbrainz::cover_url),
                        },
                    );
                    Ok(())
                })
            })
            .await?;
        self.library_track(track_id)
    }

    /// Forgets a song's MusicBrainz link. Details it took stay, as the
    /// person's changes, until they reset them.
    pub async fn unlink_musicbrainz(
        &self,
        track_id: String,
    ) -> Result<TrackDetail, SoundscapeError> {
        let key = track_id.clone();
        self.rt
            .command("musicbrainz-unlink", move |state| {
                if !state.library.value().musicbrainz.contains_key(&key) {
                    return Ok(());
                }
                state.change_library(|lib| {
                    lib.musicbrainz.remove(&key);
                    Ok(())
                })
            })
            .await?;
        self.library_track(track_id)
    }

    // MARK: One song, several sources

    /// Songs that look like one song from several sources, to offer
    /// merging: linked to the same MusicBrainz recording, or the same
    /// title, artist and length. Groups the person kept apart are left out.
    pub fn duplicate_songs(&self) -> Vec<DuplicateSongs> {
        merge::duplicates(&self.library.get(), &self.kept_library())
    }

    /// Merges the songs `ids` into the song `into`: one song whose
    /// recordings come from every source. Favourites, playlists, history
    /// and songs wanted offline move to `into`. Returns the merged song.
    pub async fn merge_songs(
        &self,
        into: String,
        ids: Vec<String>,
    ) -> Result<TrackDetail, SoundscapeError> {
        merge::check(&self.library.get(), &self.kept_library(), &into, &ids)?;
        let target = into.clone();
        self.rt
            .command("songs-merge", move |state| {
                // Checked again here: a removal queued before this command
                // may have run since the check above.
                let lib = state.library.value();
                if !lib.has_track(&target) || lib.merged.contains_key(&target) {
                    return Err(SoundscapeError::NotInLibrary { id: target.clone() });
                }
                let ids: Vec<String> = ids.into_iter().filter(|id| *id != target).collect();
                for id in &ids {
                    if !state.library.value().has_track(id) {
                        return Err(SoundscapeError::NotInLibrary { id: id.clone() });
                    }
                }
                // The library first: if a save after it fails, opening the
                // app finishes the move (merge::repair_collection).
                state.change_library(|lib| {
                    merge::merge(lib, &target, &ids);
                    Ok(())
                })?;
                state.change_collection(|c| {
                    for id in &ids {
                        merge::move_collection(c, id, &target);
                    }
                    Ok(())
                })?;
                state.change_offline(|o| {
                    for id in &ids {
                        merge::move_offline(o, id, &target);
                    }
                    Ok(())
                })
            })
            .await?;
        self.library_track(into)
    }

    /// Brings a merged song back as a song of its own, as it was. Returns
    /// the library.
    pub async fn separate_song(&self, id: String) -> Result<Library, SoundscapeError> {
        self.rt
            .command("song-separate", move |state| {
                if !state.library.value().merged.contains_key(&id) {
                    return Ok(());
                }
                state.change_library(|lib| {
                    merge::separate(lib, &id);
                    Ok(())
                })
            })
            .await?;
        Ok(self.library())
    }

    /// Remembers that these songs aren't one song, so they aren't offered
    /// for merging again.
    pub async fn keep_songs_apart(&self, ids: Vec<String>) -> Result<(), SoundscapeError> {
        if ids.len() < 2 || ids.len() > merge::MAX_MERGE {
            return Err(SoundscapeError::range("songs"));
        }
        let key = merge::group_key(&ids);
        self.rt
            .command("songs-keep-apart", move |state| {
                state.change_library(|lib| {
                    lib.distinct.insert(key);
                    Ok(())
                })
            })
            .await
    }

    /// Adds catalogue tracks to the library, keeping their rights as they
    /// are now. A track already there keeps what was saved. Returns the
    /// library.
    pub async fn add_catalogue_tracks(
        &self,
        tracks: Vec<CatalogueTrack>,
    ) -> Result<Library, SoundscapeError> {
        if tracks.len() > library::MAX_IMPORT_FILES {
            return Err(SoundscapeError::range("tracks"));
        }
        if let Some(bad) = tracks
            .iter()
            .find(|t| catalogue::provider_of(&t.id).is_none() || !t.rights.may_stream)
        {
            return Err(SoundscapeError::Server {
                reason: format!("{} can't be added from a catalogue", bad.title),
            });
        }
        self.rt
            .command("library-add-catalogue", move |state| {
                let now = now_ms();
                state.change_library(|lib| {
                    for (i, mut track) in tracks.into_iter().enumerate() {
                        track.added_ms = now + i as i64;
                        lib.catalogue.entry(track.id.clone()).or_insert(track);
                    }
                    Ok(())
                })
            })
            .await?;
        Ok(self.library())
    }

    /// Whether a track (from a catalogue or anywhere) is in the library.
    pub fn in_library(&self, track_id: String) -> bool {
        self.library.get().has_track(&track_id)
    }

    /// What the queue plays for catalogue tracks, in order: the library's
    /// copy (with the person's edits and any download) when the track is
    /// in the library, else the catalogue's stream.
    pub fn catalogue_queue_tracks(&self, tracks: Vec<CatalogueTrack>) -> Vec<QueueTrack> {
        let library = self.library();
        tracks
            .iter()
            .filter(|t| t.rights.may_stream)
            .map(|t| {
                library
                    .tracks
                    .iter()
                    .find(|l| l.id == t.id)
                    .and_then(library::queue_track)
                    .unwrap_or_else(|| QueueTrack {
                        track_id: t.id.clone(),
                        title: t.title.clone(),
                        artist: t
                            .artist
                            .clone()
                            .unwrap_or_else(|| library::UNKNOWN_ARTIST.into()),
                        album: t.album.clone(),
                        artwork_url: t.artwork_url.clone(),
                        recording: library::catalogue_recording(t),
                    })
            })
            .collect()
    }

    // MARK: Playback

    /// Keeps what is playing (the queue and how far into the song), or
    /// forgets it (`None`, when playback stops), so the app opens where it
    /// was left.
    pub async fn save_playback(&self, saved: Option<SavedPlayback>) -> Result<(), SoundscapeError> {
        let saved = saved.and_then(playback::tidy);
        let published = Arc::clone(&self.playback);
        self.rt
            .command("playback-save", move |state| {
                state.playback.change(|p| {
                    p.saved = saved;
                    Ok::<_, SoundscapeError>(())
                })?;
                published.set(state.playback.value().clone());
                Ok(())
            })
            .await
    }

    /// What was playing when the app closed, brought up to date: each
    /// library song as the library has it now (edits, a download since, or
    /// a file that moved), songs no longer playable left out. `None` when
    /// nothing was playing.
    pub fn saved_playback(&self) -> Option<SavedPlayback> {
        let saved = self.playback.get().saved.clone()?;
        let library = self.library();
        playback::refresh(saved, |queued| {
            match library.tracks.iter().find(|t| t.id == queued.track_id) {
                Some(track)
                    if track.preferred.as_ref().map(|r| &r.id) == Some(&queued.recording.id) =>
                {
                    library::queue_track(track)
                }
                Some(_) => self
                    .queue_recording(queued.track_id.clone(), queued.recording.id.clone())
                    .ok(),
                // Not in the library: a stream (catalogue, take server)
                // still plays; a file on the phone that nothing names is gone.
                None => (!queued.recording.audio_url.starts_with("file:")).then(|| queued.clone()),
            }
        })
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
            let owner = recording
                .merged_from
                .clone()
                .unwrap_or_else(|| detail.track.id.clone());
            offline::localize(&owner, recording, &offline, &url);
        }
        Ok(detail)
    }

    /// Songs waiting for a pick: two or more takes and none picked, most
    /// recently changed first.
    pub fn tracks_to_pick(&self) -> Vec<LibraryTrack> {
        library::to_pick(&self.library())
    }

    /// A track with its recordings in `order`.
    pub fn library_track_sorted(
        &self,
        id: String,
        order: RecordingOrder,
    ) -> Result<TrackDetail, SoundscapeError> {
        let mut detail = self.library_track(id)?;
        detail.recordings = library::sort_recordings(detail.recordings, order);
        Ok(detail)
    }

    /// Rates, notes or picks one of a track's take-server recordings, on the
    /// server (JewGrass's picks file stays the record), and returns the
    /// track as it now is. Finishing work.
    pub async fn mark_recording(
        &self,
        track_id: String,
        recording_id: String,
        mark: TakeMark,
    ) -> Result<TrackDetail, SoundscapeError> {
        let owner = self.recording_owner(&track_id, &recording_id);
        let slug = library::slug_of(&owner)
            .ok_or_else(|| SoundscapeError::NotInLibrary {
                id: track_id.clone(),
            })?
            .to_string();
        self.mark_take(slug, recording_id, mark).await?;
        self.library_track(track_id)
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
        let owner = self.recording_owner(&track_id, &recording_id);
        let slug = library::slug_of(&owner)
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
        // On a merged song, the chosen recording's song is the one to play.
        let kept = track_id.clone();
        self.rt
            .command("song-prefer", move |state| {
                let lib = state.library.value();
                let now = lib.prefer.get(&kept);
                let want = (owner != kept
                    && library::merged_root(lib, &owner).as_ref() == Some(&kept))
                .then_some(&owner);
                if now == want {
                    return Ok(());
                }
                state.change_library(|lib| {
                    match want {
                        Some(owner) => lib.prefer.insert(kept.clone(), owner.clone()),
                        None => lib.prefer.remove(&kept),
                    };
                    Ok(())
                })
            })
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
            rights: None,
            merged_from: None,
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
    /// The saved Jamendo client ID, or the error that asks for one.
    fn needs_jamendo_client_id(&self) -> Result<String, SoundscapeError> {
        self.jamendo_client_id()
            .ok_or_else(|| SoundscapeError::BadAddress {
                reason: "add your Jamendo client ID first".into(),
            })
    }

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
    /// The song a recording of `track_id` belongs to: the song merged into
    /// it that brought it, else `track_id` itself.
    fn recording_owner(&self, track_id: &str, recording_id: &str) -> String {
        self.library_track(track_id.to_string())
            .ok()
            .and_then(|d| {
                d.recordings
                    .into_iter()
                    .find(|r| r.id == recording_id)
                    .and_then(|r| r.merged_from)
            })
            .unwrap_or_else(|| track_id.to_string())
    }

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
        // Studio (loop projects) was removed on 2026-11-18. Its files
        // (soundscape.json, recordings/) are left where they are.
        let _ = (caches_directory, fresh);
        let directory = DataDirectory::open(data_directory)?;
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
        let mut collection_doc = collection_doc;
        let mut repaired = collection_doc.value().clone();
        if merge::repair_collection(library_doc.value(), &mut repaired) {
            if let Err(err) = collection_doc.replace(repaired) {
                forge_runtime::logging::warn!("merged songs' favourites not moved: {err}");
            }
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
        let mut offline_doc = offline_doc;
        let mut repaired = offline_doc.value().clone();
        if merge::repair_offline(library_doc.value(), &mut repaired) {
            if let Err(err) = offline_doc.replace(repaired) {
                forge_runtime::logging::warn!("merged songs' downloads not moved: {err}");
            }
        }
        let offline_published = Arc::new(Published::new(offline_doc.value().clone()));
        // Losing what was playing is a small loss: an unreadable file is
        // set aside and playback starts empty.
        let (playback_doc, kept) = Document::<PlaybackFile>::start_fresh(
            directory.path().join(PLAYBACK_FILE),
            playback::PLAYBACK_VERSION,
            None,
            now_ms(),
        )?;
        if let Some(kept) = kept {
            forge_runtime::logging::warn!(
                "unreadable playback state set aside as {}",
                kept.display()
            );
        }
        let playback_published = Arc::new(Published::new(playback_doc.value().clone()));
        let state = State {
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
            playback: playback_doc,
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("soundscape", directory, state),
            listen: listen_published,
            library: library_published,
            artwork: artwork_root,
            collection: collection_published,
            imported: imported_root,
            offline: offline_published,
            downloads: downloads_root,
            playback: playback_published,
            progress: Arc::default(),
            syncing: Arc::default(),
            http,
        }))
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

forge_app::export_runtime!(SoundscapeCore, rt);
