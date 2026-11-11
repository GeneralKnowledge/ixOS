//! Copies of the user's data that Forge keeps beside a store: a file set
//! aside because it could not be read, and the file as it was before a
//! schema migration.
//!
//! - **Set aside** (`library.unreadable-<ms>.json`, `app.db.corrupt-<ms>`):
//!   made when the app starts again from an unreadable file. Only the newest
//!   [`KEPT_COPIES`] of each file are kept, so a file that keeps failing
//!   cannot fill the disk (foundation review E2).
//! - **Before a migration** (`library.v1.json`, `app.v3.db`): made once per
//!   old schema version, just before the first write in the new one, so an
//!   upgrade that goes wrong can be undone by hand or by the next build.
//!   One per version the app has shipped, so they are not capped.

use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic::sync_dir;
use crate::StorageError;

/// How many set-aside copies of one file are kept.
pub const KEPT_COPIES: usize = 3;

/// Renames the file at `path` to `<stem>.unreadable-<now_ms>.<ext>` next to
/// it, so an app can start again without destroying it, and deletes all but
/// the newest [`KEPT_COPIES`] such copies. Returns the copy's path, or
/// `None` when there was no file.
pub fn set_aside(path: &Path, now_ms: i64) -> Result<Option<PathBuf>, StorageError> {
    if !path.exists() {
        return Ok(None);
    }
    let (stem, ext) = stem_and_extension(path);
    let kept = path.with_file_name(match &ext {
        Some(ext) => format!("{stem}.unreadable-{now_ms}.{ext}"),
        None => format!("{stem}.unreadable-{now_ms}"),
    });
    fs::rename(path, &kept)
        .map_err(|e| StorageError::write(&format!("move {} aside", path.display()), e))?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = sync_dir(dir);
        let prefix = format!("{stem}.unreadable-");
        let suffix = ext.map(|e| format!(".{e}")).unwrap_or_default();
        prune(dir, &prefix, &suffix, &[]);
    }
    Ok(Some(kept))
}

/// Where the copy of `path` taken before migrating away from schema
/// `version` goes: `library.json` becomes `library.v1.json`, `app.db`
/// becomes `app.v3.db`, and a file with no extension gets `.v<version>`.
pub fn version_copy_path(path: &Path, version: u32) -> PathBuf {
    let (stem, ext) = stem_and_extension(path);
    path.with_file_name(match ext {
        Some(ext) => format!("{stem}.v{version}.{ext}"),
        None => format!("{stem}.v{version}"),
    })
}

/// Deletes all but the newest [`KEPT_COPIES`] files in `dir` named
/// `<prefix><number><suffix>`, and each deleted copy's `<name><side>` files.
pub(crate) fn prune(dir: &Path, prefix: &str, suffix: &str, sides: &[&str]) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut copies: Vec<(u128, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let number = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
            let stamp = number
                .bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| number.parse().ok())??;
            Some((stamp, name))
        })
        .collect();
    if copies.len() <= KEPT_COPIES {
        return;
    }
    copies.sort();
    let doomed = copies.len() - KEPT_COPIES;
    for (_, name) in copies.into_iter().take(doomed) {
        let _ = fs::remove_file(dir.join(&name));
        for side in sides {
            let _ = fs::remove_file(dir.join(format!("{name}{side}")));
        }
    }
    let _ = sync_dir(dir);
}

fn stem_and_extension(path: &Path) -> (String, Option<String>) {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data".into());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    (stem, ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_aside_keeps_the_newest_three() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        assert_eq!(set_aside(&path, 1).unwrap(), None);
        for ms in 1..=5 {
            fs::write(&path, format!("bad {ms}")).unwrap();
            let kept = set_aside(&path, ms).unwrap().unwrap();
            assert_eq!(
                kept.file_name().unwrap().to_str().unwrap(),
                format!("library.unreadable-{ms}.json")
            );
            assert!(!path.exists());
        }
        // An unrelated file with a similar name is never touched.
        fs::write(dir.path().join("library.unreadable-old.json"), b"x").unwrap();
        fs::write(&path, b"bad 6").unwrap();
        set_aside(&path, 6).unwrap();
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "library.unreadable-4.json",
                "library.unreadable-5.json",
                "library.unreadable-6.json",
                "library.unreadable-old.json",
            ]
        );
    }

    #[test]
    fn version_copies_sit_beside_the_file() {
        let p = Path::new("/data/library.json");
        assert_eq!(version_copy_path(p, 1), Path::new("/data/library.v1.json"));
        assert_eq!(
            version_copy_path(Path::new("/data/notes"), 2),
            Path::new("/data/notes.v2")
        );
    }
}
