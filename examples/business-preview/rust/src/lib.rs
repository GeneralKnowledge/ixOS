//! Business Preview: a local business's own app, made from its verified
//! profile, to show the owner (docs/research/business-to-app.md, M1).
//!
//! BusinessForge's extractor turns the business's website into a profile
//! pack. This crate imports packs ([`pack`]), keeps them on the phone (the
//! prospects list), and says what the preview may show ([`view`]): only
//! verified prices, the business's own booking page, and the concept label
//! on every screen until the business agrees. SwiftUI lays it out.
//!
//! Each prospect's files are kept in `packs/<id>/` with their SHA-256 in
//! `prospects.json`, and checked again every time the preview opens, so a
//! damaged copy is never shown as the business.

pub mod pack;
pub mod view;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use business_profile::{Category, PreviewStatus, Profile};
use forge_app::{DataDirectory, Document, ErrorKind, Published, Runtime};
use serde::{Deserialize, Serialize};

use pack::PackError;
pub use view::{
    BookKind, BookView, BusinessView, ContactKind, ContactView, DayHours, ServiceView, Words,
    CONCEPT_LABEL,
};

uniffi::setup_scaffolding!();

/// Version of `prospects.json`. Bump it (and migrate) when [`Saved`] changes shape.
const SAVE_VERSION: u32 = 1;
const SAVE_FILE: &str = "prospects.json";
const PACKS: &str = "packs";

/// What can go wrong. Arrives in Swift as a thrown `BusinessPreviewError`.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq)]
pub enum BusinessPreviewError {
    /// Not a pack the preview can read (no profile.json, not a zip).
    #[error("not a profile pack: {reason}")]
    NotAPack { reason: String },
    /// The profile breaks the schema's rules.
    #[error("the profile isn't valid: {}", problems.join("; "))]
    Invalid { problems: Vec<String> },
    #[error("{what} is over {max_bytes} bytes")]
    TooLarge { what: String, max_bytes: u64 },
    #[error("{path}: {reason}")]
    BadImage { path: String, reason: String },
    /// A file doesn't match the hash the pack lists for it.
    #[error("{path} doesn't match its hash")]
    HashMismatch { path: String },
    /// Packs are fetched over https only.
    #[error("not a pack address: {reason}")]
    BadAddress { reason: String },
    #[error("no prospect with id {id}")]
    NotFound { id: String },
    /// The copy on the phone has changed since it was imported.
    #[error("the saved copy of {id} is damaged")]
    Damaged { id: String },
    #[error("the app is open in another copy of the app")]
    InUse,
    #[error("the saved prospects can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("the prospects were saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

forge_app::app_error_conversions!(BusinessPreviewError: Stopped, OpenError, DocumentError);

impl From<PackError> for BusinessPreviewError {
    fn from(err: PackError) -> Self {
        match err {
            PackError::NotAPack(reason) => Self::NotAPack { reason },
            PackError::Invalid(problems) => Self::Invalid { problems },
            PackError::TooLarge { what, max_bytes } => Self::TooLarge { what, max_bytes },
            PackError::BadImage { path, reason } => Self::BadImage { path, reason },
            PackError::HashMismatch { path } => Self::HashMismatch { path },
            PackError::Io(reason) => Self::Storage { reason },
        }
    }
}

fn io(err: std::io::Error) -> BusinessPreviewError {
    BusinessPreviewError::Storage {
        reason: err.to_string(),
    }
}

/// The framework kind of an error, so ForgeKit can present any of them.
#[uniffi::export]
pub fn business_preview_error_kind(error: BusinessPreviewError) -> ErrorKind {
    use BusinessPreviewError::*;
    match error {
        NotAPack { .. }
        | Invalid { .. }
        | TooLarge { .. }
        | BadImage { .. }
        | HashMismatch { .. }
        | BadAddress { .. } => ErrorKind::Invalid,
        NotFound { .. } => ErrorKind::NotFound,
        Damaged { .. } | Corrupt { .. } => ErrorKind::Corrupt,
        InUse => ErrorKind::Unavailable,
        NewerData => ErrorKind::NewerData,
        Storage { .. } => ErrorKind::Internal,
        Stopped { kind } => kind,
    }
}

/// Checks an address typed to download a pack from, and returns it as the
/// download should use it. Packs come over https only.
#[uniffi::export]
pub fn business_preview_pack_address(text: String) -> Result<String, BusinessPreviewError> {
    let bad = |reason: &str| BusinessPreviewError::BadAddress {
        reason: reason.into(),
    };
    let url = url::Url::parse(text.trim())
        .map_err(|_| bad("type the whole address, starting https://"))?;
    if url.scheme() != "https" {
        return Err(bad("packs are downloaded over https only"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(bad("the address has no host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad("the address mustn't hold a user name or password"));
    }
    Ok(url.to_string())
}

/// The concept label, for screens that show it before a business loads.
#[uniffi::export]
pub fn business_preview_concept_label() -> String {
    CONCEPT_LABEL.to_string()
}

/// One business on the prospects list.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Prospect {
    pub id: String,
    pub name: String,
    /// The kind of business, in words.
    pub category: String,
    /// True until the business agrees: the preview carries the concept label.
    pub is_concept: bool,
    /// Values on the site no person has checked yet.
    pub unverified: u32,
    /// When it was imported, in milliseconds since 1970.
    pub imported_ms: i64,
}

/// What an import did.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub prospect: Prospect,
    /// True when it replaced a pack of the same business.
    pub replaced: bool,
}

/// What `prospects.json` holds.
#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    prospects: Vec<Stored>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Stored {
    id: String,
    name: String,
    category: Category,
    status: PreviewStatus,
    unverified: u32,
    imported_ms: i64,
    /// Each file kept, by path in the pack, with its SHA-256.
    files: BTreeMap<String, String>,
}

impl Stored {
    fn prospect(&self) -> Prospect {
        Prospect {
            id: self.id.clone(),
            name: self.name.clone(),
            category: view::category_name(self.category).into(),
            is_concept: self.status == PreviewStatus::Concept,
            unverified: self.unverified,
            imported_ms: self.imported_ms,
        }
    }
}

fn list(saved: &Saved) -> Vec<Prospect> {
    let mut out: Vec<Prospect> = saved.prospects.iter().map(Stored::prospect).collect();
    // Newest first: the business about to be shown is usually the last one sent.
    out.sort_by(|a, b| b.imported_ms.cmp(&a.imported_ms).then(a.name.cmp(&b.name)));
    out
}

/// What the command thread owns.
struct State {
    saved: Document<Saved>,
    packs: PathBuf,
    published: Arc<Published<Vec<Prospect>>>,
}

/// Writes `files` into a new folder next to `dest`, then swaps it in, so a
/// failed write never leaves half a pack where the old one was.
fn write_pack(dest: &Path, files: &BTreeMap<String, &[u8]>) -> std::io::Result<()> {
    let parent = dest.parent().expect("a pack folder has a parent");
    let name = dest
        .file_name()
        .expect("a pack folder has a name")
        .to_string_lossy();
    let fresh = parent.join(format!(".{name}.new"));
    let old = parent.join(format!(".{name}.old"));
    let _ = std::fs::remove_dir_all(&fresh);
    for (path, bytes) in files {
        let full = fresh.join(path);
        if let Some(dir) = full.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&full, bytes)?;
    }
    let _ = std::fs::remove_dir_all(&old);
    if dest.exists() {
        std::fs::rename(dest, &old)?;
    }
    std::fs::rename(&fresh, dest)?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Reads a kept prospect's profile, checking every file against its hash.
fn read_kept(folder: &Path, stored: &Stored) -> Result<Profile, BusinessPreviewError> {
    let damaged = || BusinessPreviewError::Damaged {
        id: stored.id.clone(),
    };
    let mut profile_json = None;
    for (path, hash) in &stored.files {
        let bytes = std::fs::read(folder.join(path)).map_err(|_| damaged())?;
        if pack::sha256(&bytes) != *hash {
            return Err(damaged());
        }
        if path == "profile.json" {
            profile_json = Some(bytes);
        }
    }
    let text = String::from_utf8(profile_json.ok_or_else(damaged)?).map_err(|_| damaged())?;
    Profile::from_json(&text).map_err(|_| damaged())
}

/// The app. ForgeKit's `ForgeHost` opens one per process and keeps it.
#[derive(uniffi::Object)]
pub struct BusinessPreviewCore {
    rt: Runtime<State>,
    prospects: Arc<Published<Vec<Prospect>>>,
}

#[uniffi::export]
impl BusinessPreviewCore {
    /// Opens the prospects kept in `storage_directory`, or none.
    /// Fails with `InUse` if another core has that folder open.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, BusinessPreviewError> {
        forge_runtime::logging::init();
        let directory = DataDirectory::open(&storage_directory)?;
        let saved = Document::<Saved>::open(directory.path().join(SAVE_FILE), SAVE_VERSION)?;
        let packs = directory.path().join(PACKS);
        std::fs::create_dir_all(&packs).map_err(io)?;
        // Folders left by an import that stopped half way.
        if let Ok(entries) = std::fs::read_dir(&packs) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with('.') {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
        let published = Arc::new(Published::new(list(saved.value())));
        let state = State {
            saved,
            packs,
            published: Arc::clone(&published),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("business-preview", directory, state),
            prospects: published,
        }))
    }

    /// The prospects list, newest first. Quick: it never waits for a command.
    pub fn prospects(&self) -> Vec<Prospect> {
        self.prospects.get().as_ref().clone()
    }

    /// Imports the pack at `path` (a folder, a zip or a lone profile.json),
    /// replacing an earlier pack of the same business. The file at `path`
    /// is only read; the caller deletes its copy.
    pub async fn import_pack(&self, path: String) -> Result<Imported, BusinessPreviewError> {
        self.rt
            .command("import_pack", move |state| {
                let pack = pack::read(Path::new(&path))?;
                let profile = &pack.profile;
                let files = pack.files();
                write_pack(&state.packs.join(&profile.id), &files).map_err(io)?;
                let stored = Stored {
                    id: profile.id.clone(),
                    name: profile.name.value.clone(),
                    category: profile.category,
                    status: profile.preview.status,
                    unverified: profile.unverified().len() as u32,
                    imported_ms: forge_runtime::now_ms(),
                    files: files
                        .iter()
                        .map(|(path, bytes)| (path.clone(), pack::sha256(bytes)))
                        .collect(),
                };
                let prospect = stored.prospect();
                let replaced = state.saved.change(|saved| {
                    let before = saved.prospects.len();
                    saved.prospects.retain(|p| p.id != stored.id);
                    let replaced = saved.prospects.len() != before;
                    saved.prospects.push(stored);
                    Ok::<_, BusinessPreviewError>(replaced)
                })?;
                state.published.set(list(state.saved.value()));
                forge_runtime::logging::info!(
                    "imported a pack: {} services, {} images, replaced {replaced}",
                    profile.services.len(),
                    pack.images.len()
                );
                Ok(Imported { prospect, replaced })
            })
            .await
    }

    /// Deletes a prospect and its files.
    pub async fn delete(&self, id: String) -> Result<(), BusinessPreviewError> {
        self.rt
            .command("delete", move |state| {
                state.saved.change(|saved| {
                    let before = saved.prospects.len();
                    saved.prospects.retain(|p| p.id != id);
                    if saved.prospects.len() == before {
                        return Err(BusinessPreviewError::NotFound { id: id.clone() });
                    }
                    Ok(())
                })?;
                // The list no longer names it, so a folder left behind is
                // only wasted space: removed again at the next import of it.
                if let Err(err) = std::fs::remove_dir_all(state.packs.join(&id)) {
                    forge_runtime::logging::warn!("a deleted pack's folder stayed: {err}");
                }
                state.published.set(list(state.saved.value()));
                Ok(())
            })
            .await
    }

    /// The business, ready to show. Every file is checked against the hash
    /// taken at import first.
    pub async fn business(&self, id: String) -> Result<BusinessView, BusinessPreviewError> {
        self.rt
            .command("business", move |state| {
                let stored = state
                    .saved
                    .value()
                    .prospects
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| BusinessPreviewError::NotFound { id: id.clone() })?;
                let folder = state.packs.join(&stored.id);
                let profile = read_kept(&folder, stored)?;
                Ok(view::business(&profile, &folder))
            })
            .await
    }
}

// events(), set_phase(), phase(), revision() and shutdown(), for ForgeKit.
forge_app::export_runtime!(BusinessPreviewCore, rt);
