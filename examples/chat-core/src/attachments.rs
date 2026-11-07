//! Attachments on this device: files a message carries (photos, documents).
//!
//! Every attachment is kept once, under the SHA-256 of its bytes (lowercase
//! hex), which is also its id on the server. Sending copies the user's file
//! into the store first, so the outbox can upload it later even if the
//! original goes away; receiving downloads it on demand and checks the
//! bytes against the id before keeping them.
//!
//! The file handling is Forge's [`FileStore`] (`forge-storage`, feature
//! `hash`); this module adds Chat's rules: the size limit, the id format and
//! how failures read to the user.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_storage::files::{FileStore, FileStoreError};
use serde::{Deserialize, Serialize};

use crate::types::ChatError;

pub use forge_storage::files::sha256_file;

/// Largest attachment, in bytes (25 MiB; the server's limit too).
pub const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

/// Most attachments one message may carry (the server's limit too).
pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;

/// A file a message carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Attachment {
    /// The SHA-256 of its bytes, lowercase hex: its name in the store and
    /// on the server.
    pub id: String,
    /// The file name as the sender had it, e.g. `IMG_0042.jpg`.
    pub name: String,
    /// Its media type, e.g. `image/jpeg` or `application/pdf`.
    pub mime: String,
    pub size: u64,
}

impl Attachment {
    /// A photo or other picture the app can show inline.
    pub fn is_image(&self) -> bool {
        self.mime.starts_with("image/")
    }
}

/// A file the user picked to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingFile {
    /// Where it is now. It is copied into the store when sent.
    pub path: PathBuf,
    /// The name to show; empty takes the path's file name.
    pub name: String,
    /// Its media type; empty means `application/octet-stream`.
    pub mime: String,
}

/// An attachment id: 64 lowercase hex characters.
pub fn is_attachment_id(id: &str) -> bool {
    forge_storage::files::is_sha256_hex(id)
}

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// The folder of attachment files.
#[derive(Debug, Clone)]
pub struct AttachmentStore {
    files: FileStore,
}

impl AttachmentStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            files: FileStore::new(directory).with_max_bytes(MAX_ATTACHMENT_BYTES),
        }
    }

    /// A store in a fresh folder under the system's temporary directory
    /// (for in-memory chat stores: tests, `chat-cli follow`).
    pub fn temporary() -> Self {
        let n = TEMPORARY.fetch_add(1, Ordering::Relaxed);
        Self::new(
            std::env::temp_dir().join(format!("forge-chat-attachments-{}-{n}", std::process::id())),
        )
    }

    pub fn directory(&self) -> &Path {
        self.files.root()
    }

    /// Where attachment `id` is kept (whether or not it is there yet).
    pub fn file_path(&self, id: &str) -> PathBuf {
        self.directory().join(id)
    }

    /// The local file of attachment `id`, if it is on this device.
    pub fn local_file(&self, id: &str) -> Option<PathBuf> {
        if !is_attachment_id(id) {
            return None;
        }
        self.files.get(id)
    }

    /// Copies `file` into the store and describes it.
    pub fn import(&self, file: &OutgoingFile) -> Result<Attachment, ChatError> {
        let stored = self
            .files
            .import_hashed(&file.path)
            .map_err(|err| match err {
                FileStoreError::TooLarge { .. } => ChatError::invalid(format!(
                    "{} is larger than {} MB",
                    file.path.display(),
                    MAX_ATTACHMENT_BYTES / (1024 * 1024)
                )),
                FileStoreError::Empty => ChatError::invalid(format!(
                    "cannot read {}: the file is empty",
                    file.path.display()
                )),
                other => {
                    ChatError::invalid(format!("cannot read {}: {other}", file.path.display()))
                }
            })?;
        let name = if file.name.trim().is_empty() {
            file.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into())
        } else {
            file.name.trim().to_string()
        };
        let mime = if file.mime.trim().is_empty() {
            "application/octet-stream".to_string()
        } else {
            file.mime.trim().to_ascii_lowercase()
        };
        Ok(Attachment {
            id: stored.name,
            name,
            mime,
            size: stored.size,
        })
    }

    /// Checks a downloaded file against its id and keeps it, or deletes it.
    pub fn accept_download(&self, id: &str, downloaded: &Path) -> Result<PathBuf, ChatError> {
        match self.files.accept_verified(downloaded, id) {
            Ok(stored) => Ok(stored.path),
            Err(FileStoreError::Mismatch { actual, .. }) => Err(ChatError::protocol(format!(
                "attachment {id} arrived with different contents ({actual})"
            ))),
            Err(FileStoreError::TooLarge { .. }) => Err(ChatError::protocol(format!(
                "attachment {id} is larger than {} MB",
                MAX_ATTACHMENT_BYTES / (1024 * 1024)
            ))),
            Err(FileStoreError::Empty) => Err(ChatError::protocol(format!(
                "attachment {id} arrived empty"
            ))),
            Err(err) => Err(ChatError::UnsupportedOnHost {
                detail: format!("cannot keep attachment {id}: {err}"),
            }),
        }
    }

    /// Where a download of `id` goes before it is checked.
    pub fn download_path(&self, id: &str) -> PathBuf {
        self.directory().join(format!(".download-{id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_by_content_and_checks_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(dir.path().join("attachments"));
        let original = dir.path().join("Report.pdf");
        std::fs::write(&original, b"hello").unwrap();
        let attachment = store
            .import(&OutgoingFile {
                path: original.clone(),
                name: String::new(),
                mime: "Application/PDF".into(),
            })
            .unwrap();
        assert_eq!(
            attachment,
            Attachment {
                id: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
                name: "Report.pdf".into(),
                mime: "application/pdf".into(),
                size: 5,
            }
        );
        assert!(!attachment.is_image());
        // The copy outlives the original.
        std::fs::remove_file(&original).unwrap();
        let kept = store.local_file(&attachment.id).unwrap();
        assert_eq!(std::fs::read(kept).unwrap(), b"hello");
        assert_eq!(store.local_file("../Report.pdf"), None);

        // Empty or missing files are refused.
        let empty = dir.path().join("empty");
        std::fs::write(&empty, b"").unwrap();
        let file = |path: PathBuf| OutgoingFile {
            path,
            name: String::new(),
            mime: String::new(),
        };
        assert!(store.import(&file(empty)).is_err());
        assert!(store.import(&file(dir.path().join("missing"))).is_err());

        // A download is kept only if it matches its id.
        let id = sha256_file(&dir.path().join("attachments").join(&attachment.id)).unwrap();
        let other = "0".repeat(64);
        let wrong = store.download_path(&other);
        std::fs::write(&wrong, b"not it").unwrap();
        assert!(matches!(
            store.accept_download(&other, &wrong),
            Err(ChatError::Protocol { .. })
        ));
        assert!(!wrong.exists());
        let right = store.download_path(&id);
        std::fs::write(&right, b"hello").unwrap();
        assert_eq!(
            store.accept_download(&id, &right).unwrap(),
            store.file_path(&id)
        );
        // No temporary files are left behind.
        let names: Vec<String> = std::fs::read_dir(store.directory())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [id]);
    }
}
