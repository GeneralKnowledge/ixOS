//! Attachments on this device: files a message carries (photos, documents).
//!
//! Every attachment is kept once, under the SHA-256 of its bytes (lowercase
//! hex), which is also its id on the server. Sending copies the user's file
//! into the store first, so the outbox can upload it later even if the
//! original goes away; receiving downloads it on demand and checks the
//! bytes against the id before keeping them.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::types::ChatError;

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
    id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The SHA-256 of everything `reader` yields, as lowercase hex, and the
/// number of bytes, copying them to `sink` on the way.
fn hash_copy(reader: &mut impl Read, sink: &mut impl Write) -> std::io::Result<(String, u64)> {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        hasher.update(&buffer[..n]);
        sink.write_all(&buffer[..n])?;
        total += n as u64;
        if total > MAX_ATTACHMENT_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "attachment too large",
            ));
        }
    }
    let hex = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((hex, total))
}

/// The SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    hash_copy(&mut file, &mut std::io::sink()).map(|(hex, _)| hex)
}

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// The folder of attachment files.
#[derive(Debug, Clone)]
pub struct AttachmentStore {
    directory: PathBuf,
}

impl AttachmentStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
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
        &self.directory
    }

    /// Where attachment `id` is kept (whether or not it is there yet).
    pub fn file_path(&self, id: &str) -> PathBuf {
        self.directory.join(id)
    }

    /// The local file of attachment `id`, if it is on this device.
    pub fn local_file(&self, id: &str) -> Option<PathBuf> {
        if !is_attachment_id(id) {
            return None;
        }
        let path = self.file_path(id);
        path.is_file().then_some(path)
    }

    /// Copies `file` into the store and describes it.
    pub fn import(&self, file: &OutgoingFile) -> Result<Attachment, ChatError> {
        let failed = |err: std::io::Error| {
            if err.kind() == std::io::ErrorKind::FileTooLarge {
                ChatError::invalid(format!(
                    "{} is larger than {} MB",
                    file.path.display(),
                    MAX_ATTACHMENT_BYTES / (1024 * 1024)
                ))
            } else {
                ChatError::invalid(format!("cannot read {}: {err}", file.path.display()))
            }
        };
        std::fs::create_dir_all(&self.directory).map_err(|err| ChatError::UnsupportedOnHost {
            detail: format!("cannot create {}: {err}", self.directory.display()),
        })?;
        let mut source = std::fs::File::open(&file.path).map_err(failed)?;
        let partial = self.directory.join(format!(
            ".import-{}-{}",
            std::process::id(),
            TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut sink = std::fs::File::create(&partial)?;
            let (id, size) = hash_copy(&mut source, &mut sink)?;
            sink.sync_all()?;
            if size == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "the file is empty",
                ));
            }
            std::fs::rename(&partial, self.file_path(&id))?;
            Ok((id, size))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        let (id, size) = result.map_err(failed)?;
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
            id,
            name,
            mime,
            size,
        })
    }

    /// Checks a downloaded file against its id and keeps it, or deletes it.
    pub fn accept_download(&self, id: &str, downloaded: &Path) -> Result<PathBuf, ChatError> {
        let actual = sha256_file(downloaded).map_err(|err| ChatError::UnsupportedOnHost {
            detail: format!("cannot read {}: {err}", downloaded.display()),
        })?;
        if actual != id {
            let _ = std::fs::remove_file(downloaded);
            return Err(ChatError::protocol(format!(
                "attachment {id} arrived with different contents ({actual})"
            )));
        }
        let destination = self.file_path(id);
        if downloaded != destination {
            std::fs::rename(downloaded, &destination).map_err(|err| {
                ChatError::UnsupportedOnHost {
                    detail: format!("cannot keep {}: {err}", destination.display()),
                }
            })?;
        }
        Ok(destination)
    }

    /// Where a download of `id` goes before it is checked.
    pub fn download_path(&self, id: &str) -> PathBuf {
        self.directory.join(format!(".download-{id}"))
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
