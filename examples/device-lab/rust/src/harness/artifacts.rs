//! The images a project's scripts made: what Python Lab's Artifacts
//! section and the harness's `ProducesImage` check look at
//! (docs/visual-labs-spec.md, P1). Python makes the files; this only lists
//! them and reads a PNG's size from its header.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// An image file in a project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectArtifact {
    /// Relative to the project, like "charts/sales.png".
    pub path: String,
    /// "png", "jpeg", "gif" or "svg".
    pub kind: String,
    pub bytes: u64,
    pub modified_ms: i64,
    /// From the PNG header; none for other kinds.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The kind of an image file by its extension, or none.
pub fn kind_of(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    let extension = lower.rsplit_once('.')?.1;
    match extension {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpeg"),
        "gif" => Some("gif"),
        "svg" => Some("svg"),
        _ => None,
    }
}

/// Width and height from a PNG's IHDR chunk (bytes 16 to 24), when the
/// file starts like a PNG.
pub fn png_size(head: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if head.len() < 24 || head[..8] != SIGNATURE || &head[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(head[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(head[20..24].try_into().ok()?);
    Some((width, height))
}

/// Every image under `root` (hidden folders such as `.harness` skipped),
/// newest first.
pub fn list(root: &Path) -> Vec<ProjectArtifact> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.path.cmp(&b.path))
    });
    out
}

/// Path to (bytes, modified) for each image: what a run is compared with.
pub fn stamps(root: &Path) -> BTreeMap<String, (u64, i64)> {
    list(root)
        .into_iter()
        .map(|a| (a.path, (a.bytes, a.modified_ms)))
        .collect()
}

/// The images that are new or changed since `before`.
pub fn changed(root: &Path, before: &BTreeMap<String, (u64, i64)>) -> Vec<ProjectArtifact> {
    list(root)
        .into_iter()
        .filter(|a| before.get(&a.path) != Some(&(a.bytes, a.modified_ms)))
        .collect()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<ProjectArtifact>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(root, &path, out);
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let Some(kind) = kind_of(&relative) else {
            continue;
        };
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64);
        let (width, height) = if kind == "png" {
            let mut head = [0u8; 24];
            let read = fs::File::open(&path)
                .and_then(|mut f| f.read(&mut head))
                .unwrap_or(0);
            png_size(&head[..read]).map_or((None, None), |(w, h)| (Some(w), Some(h)))
        } else {
            (None, None)
        };
        out.push(ProjectArtifact {
            path: relative,
            kind: kind.into(),
            bytes: meta.len(),
            modified_ms,
            width,
            height,
        });
    }
}

/// The images in the project folder `project`, newest first, for Python
/// Lab's Artifacts section and Bonsai Harness's project files.
#[uniffi::export]
pub fn project_artifacts(project: String) -> Vec<ProjectArtifact> {
    list(Path::new(&project))
}
