//! Python Lab's workspace: the person's projects and their files
//! (docs/python-lab-workspace-plan.md, PL1a).
//!
//! A project is a plain folder under the projects folder (iOS:
//! `Documents/Python Lab`, shown in the Files app). Bonsai Harness works in
//! the same folders, so a file either of them writes is the other's to read.
//! The harness's own folders (`.ladder`, `.runs`) and each project's hidden
//! folders (`.harness`, `.workspace`) are never listed here.
//!
//! Every path the screen passes is checked: relative, no `..`, no hidden
//! parts, never through a symbolic link, so nothing here can reach outside
//! the projects folder. Limits refuse with the limit; nothing is cut.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::harness::{move_old_ladder_projects, SCRATCH_PROJECT};

/// Longest project name, in characters.
pub const PROJECT_NAME_MAX: usize = 60;
/// Longest file or folder name, in characters.
pub const NAME_MAX: usize = 100;
/// Longest path inside a project, in characters.
pub const PATH_MAX: usize = 255;
/// Largest text file the editor saves.
pub const TEXT_MAX_BYTES: usize = 1024 * 1024;
/// Most files and folders a project may hold.
pub const ENTRIES_MAX: usize = 500;

/// The hidden folder in each project where Python Lab keeps its state.
const STATE_FOLDER: &str = ".workspace";
const STATE_FILE: &str = "state.json";

#[derive(uniffi::Error, thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    /// A name or path the person gave that can't be used, with why.
    #[error("{reason}")]
    Invalid { reason: String },
    #[error("There is no {what} called {name}.")]
    NotFound { what: String, name: String },
    #[error("{name} already exists.")]
    Exists { name: String },
    #[error("{reason}")]
    TooLarge { reason: String },
    #[error("The scratch project is Bonsai's default, so it can't be renamed or deleted.")]
    Protected,
    #[error("{reason}")]
    Storage { reason: String },
}

fn storage(err: std::io::Error) -> WorkspaceError {
    WorkspaceError::Storage {
        reason: err.to_string(),
    }
}

/// A project in the list.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceProject {
    pub name: String,
    /// Files (not folders), hidden ones left out.
    pub files: u32,
    /// The newest change to anything in it, in ms since 1970.
    pub modified_ms: i64,
    /// Bonsai's default project, which can't be renamed or deleted.
    pub is_scratch: bool,
}

/// A file or folder in a project.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    /// Relative to the project, like "lib/helper.py".
    pub path: String,
    pub is_folder: bool,
    pub bytes: u64,
    pub modified_ms: i64,
}

/// What Python Lab had open in a project, to reopen it.
#[derive(uniffi::Record, Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceState {
    /// The file Run starts; none runs the file on screen.
    #[serde(default)]
    pub run_file: Option<String>,
    /// The file on screen.
    #[serde(default)]
    pub current_file: Option<String>,
    /// Files open, in order.
    #[serde(default)]
    pub open_files: Vec<String>,
}

/// The person's projects. Thread-safe: every call goes to the disk.
#[derive(uniffi::Object)]
pub struct Workspace {
    root: PathBuf,
}

#[uniffi::export]
impl Workspace {
    /// Opens the projects folder, creating it and the scratch project if
    /// needed, and moving ladder projects left there by older builds.
    #[uniffi::constructor]
    pub fn new(projects_folder: String) -> Result<std::sync::Arc<Self>, WorkspaceError> {
        let root = PathBuf::from(projects_folder);
        fs::create_dir_all(root.join(SCRATCH_PROJECT)).map_err(storage)?;
        move_old_ladder_projects(&root);
        Ok(std::sync::Arc::new(Self { root }))
    }

    /// The projects, scratch first, then by name.
    pub fn projects(&self) -> Vec<WorkspaceProject> {
        let mut out = Vec::new();
        let Ok(read) = fs::read_dir(&self.root) else {
            return out;
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if name.starts_with('.') || !meta.is_dir() {
                continue;
            }
            let mut files = 0;
            let mut newest = modified_ms(&meta);
            let mut entries = Vec::new();
            // A project that can't be read is listed with what could be.
            let _ = walk(&entry.path(), &entry.path(), &mut entries, usize::MAX);
            for e in &entries {
                if !e.is_folder {
                    files += 1;
                }
                newest = newest.max(e.modified_ms);
            }
            out.push(WorkspaceProject {
                is_scratch: name == SCRATCH_PROJECT,
                name,
                files,
                modified_ms: newest,
            });
        }
        out.sort_by(|a, b| {
            b.is_scratch
                .cmp(&a.is_scratch)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    /// The folder of a project, for running its files and showing images.
    pub fn project_folder(&self, project: String) -> Result<String, WorkspaceError> {
        Ok(self.project(&project)?.display().to_string())
    }

    pub fn create_project(&self, name: String) -> Result<(), WorkspaceError> {
        let name = project_name(&name)?;
        let path = self.root.join(&name);
        if exists_any_case(&self.root, &name)? {
            return Err(WorkspaceError::Exists { name });
        }
        fs::create_dir(path).map_err(storage)
    }

    pub fn rename_project(&self, from: String, to: String) -> Result<(), WorkspaceError> {
        let old = self.project(&from)?;
        if is_scratch(&from) {
            return Err(WorkspaceError::Protected);
        }
        let to = project_name(&to)?;
        if to == from {
            return Ok(());
        }
        // A change of case only is allowed (the disk may not tell them apart).
        if to.to_lowercase() != from.to_lowercase() && exists_any_case(&self.root, &to)? {
            return Err(WorkspaceError::Exists { name: to });
        }
        fs::rename(old, self.root.join(to)).map_err(storage)
    }

    pub fn delete_project(&self, name: String) -> Result<(), WorkspaceError> {
        let path = self.project(&name)?;
        if is_scratch(&name) {
            return Err(WorkspaceError::Protected);
        }
        fs::remove_dir_all(path).map_err(storage)
    }

    /// The project's files and folders, folders before files at each level,
    /// hidden ones left out.
    pub fn entries(&self, project: String) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        let root = self.project(&project)?;
        let mut out = Vec::new();
        walk(&root, &root, &mut out, usize::MAX).map_err(storage)?;
        out.sort_by(|a, b| tree_order(&a.path, a.is_folder, &b.path, b.is_folder));
        Ok(out)
    }

    /// Creates an empty file; its folder must exist.
    pub fn create_file(&self, project: String, path: String) -> Result<(), WorkspaceError> {
        let full = self.new_path(&project, &path)?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(full)
            .map(|_| ())
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::AlreadyExists => WorkspaceError::Exists { name: path },
                _ => storage(err),
            })
    }

    /// Creates a folder; its parent must exist.
    pub fn create_folder(&self, project: String, path: String) -> Result<(), WorkspaceError> {
        let full = self.new_path(&project, &path)?;
        fs::create_dir(full).map_err(storage)
    }

    /// A text file's contents.
    pub fn read_text(&self, project: String, path: String) -> Result<String, WorkspaceError> {
        let full = self.existing(&project, &path)?;
        let meta = fs::metadata(&full).map_err(storage)?;
        if meta.is_dir() {
            return Err(WorkspaceError::Invalid {
                reason: format!("{path} is a folder."),
            });
        }
        if meta.len() > TEXT_MAX_BYTES as u64 {
            return Err(WorkspaceError::TooLarge {
                reason: format!(
                    "{path} is {} KB; the editor opens files up to {} KB.",
                    meta.len() / 1024,
                    TEXT_MAX_BYTES / 1024
                ),
            });
        }
        let bytes = fs::read(&full).map_err(storage)?;
        String::from_utf8(bytes).map_err(|_| WorkspaceError::Invalid {
            reason: format!("{path} isn't text (UTF-8), so the editor can't open it."),
        })
    }

    /// Saves a text file, creating it if needed. The old contents are
    /// replaced only once the new ones are safely written.
    pub fn write_text(
        &self,
        project: String,
        path: String,
        text: String,
    ) -> Result<(), WorkspaceError> {
        if text.len() > TEXT_MAX_BYTES {
            return Err(WorkspaceError::TooLarge {
                reason: format!(
                    "{path} would be {} KB; a file can be at most {} KB.",
                    text.len() / 1024,
                    TEXT_MAX_BYTES / 1024
                ),
            });
        }
        let root = self.project(&project)?;
        check_path(&path)?;
        let full = safe_join(&root, &path)?;
        if full.is_dir() {
            return Err(WorkspaceError::Invalid {
                reason: format!("{path} is a folder."),
            });
        }
        if !full.exists() {
            self.check_room(&root)?;
            if !full.parent().is_some_and(Path::is_dir) {
                return Err(WorkspaceError::NotFound {
                    what: "folder".into(),
                    name: parent_of(&path),
                });
            }
        }
        write_atomic(&full, text.as_bytes()).map_err(storage)
    }

    /// Renames or moves a file or folder inside the project.
    pub fn rename_entry(
        &self,
        project: String,
        from: String,
        to: String,
    ) -> Result<(), WorkspaceError> {
        let old = self.existing(&project, &from)?;
        let root = self.project(&project)?;
        check_path(&to)?;
        let new = safe_join(&root, &to)?;
        if to == from {
            return Ok(());
        }
        if to.starts_with(&format!("{from}/")) {
            return Err(WorkspaceError::Invalid {
                reason: format!("{from} can't move inside itself."),
            });
        }
        if new.exists() && to.to_lowercase() != from.to_lowercase() {
            return Err(WorkspaceError::Exists { name: to });
        }
        if !new.parent().is_some_and(Path::is_dir) {
            return Err(WorkspaceError::NotFound {
                what: "folder".into(),
                name: parent_of(&to),
            });
        }
        fs::rename(old, new).map_err(storage)
    }

    /// Deletes a file, or a folder with everything in it.
    pub fn delete_entry(&self, project: String, path: String) -> Result<(), WorkspaceError> {
        let full = self.existing(&project, &path)?;
        if fs::symlink_metadata(&full).map_err(storage)?.is_dir() {
            fs::remove_dir_all(full).map_err(storage)
        } else {
            fs::remove_file(full).map_err(storage)
        }
    }

    /// What Python Lab had open in the project (empty if nothing saved, or
    /// the saved state is unreadable). Files that no longer exist are left
    /// out.
    pub fn state(&self, project: String) -> Result<WorkspaceState, WorkspaceError> {
        let root = self.project(&project)?;
        let mut state: WorkspaceState = fs::read(root.join(STATE_FOLDER).join(STATE_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let present = |p: &String| check_path(p).is_ok() && root.join(p).is_file();
        state.open_files.retain(present);
        state.current_file = state.current_file.filter(present);
        state.run_file = state.run_file.filter(present);
        Ok(state)
    }

    pub fn save_state(&self, project: String, state: WorkspaceState) -> Result<(), WorkspaceError> {
        let root = self.project(&project)?;
        let folder = root.join(STATE_FOLDER);
        fs::create_dir_all(&folder).map_err(storage)?;
        let json = serde_json::to_vec_pretty(&state).map_err(|err| WorkspaceError::Storage {
            reason: err.to_string(),
        })?;
        write_atomic(&folder.join(STATE_FILE), &json).map_err(storage)
    }
}

impl Workspace {
    /// An existing project's folder.
    fn project(&self, name: &str) -> Result<PathBuf, WorkspaceError> {
        project_name(name)?;
        let path = self.root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => Ok(path),
            _ => Err(WorkspaceError::NotFound {
                what: "project".into(),
                name: name.into(),
            }),
        }
    }

    /// An existing file or folder in a project.
    fn existing(&self, project: &str, path: &str) -> Result<PathBuf, WorkspaceError> {
        let root = self.project(project)?;
        check_path(path)?;
        let full = safe_join(&root, path)?;
        if fs::symlink_metadata(&full).is_err() {
            return Err(WorkspaceError::NotFound {
                what: "file or folder".into(),
                name: path.into(),
            });
        }
        Ok(full)
    }

    /// Where a new file or folder goes: checked, free, room for it, and
    /// its folder exists.
    fn new_path(&self, project: &str, path: &str) -> Result<PathBuf, WorkspaceError> {
        let root = self.project(project)?;
        check_path(path)?;
        let full = safe_join(&root, path)?;
        if full.exists() {
            return Err(WorkspaceError::Exists { name: path.into() });
        }
        self.check_room(&root)?;
        if !full.parent().is_some_and(Path::is_dir) {
            return Err(WorkspaceError::NotFound {
                what: "folder".into(),
                name: parent_of(path),
            });
        }
        Ok(full)
    }

    fn check_room(&self, root: &Path) -> Result<(), WorkspaceError> {
        let mut entries = Vec::new();
        walk(root, root, &mut entries, ENTRIES_MAX).map_err(storage)?;
        if entries.len() >= ENTRIES_MAX {
            return Err(WorkspaceError::TooLarge {
                reason: format!("A project can hold at most {ENTRIES_MAX} files and folders."),
            });
        }
        Ok(())
    }
}

/// Moves the projects from the folder older builds used (`Application
/// Support/Python Lab/Projects`) to `new`, once: only when `new` doesn't
/// exist yet or holds nothing but an empty scratch project. Returns whether
/// it moved anything. If the rename fails (different volumes), it copies
/// and then deletes the old folder.
#[uniffi::export]
pub fn workspace_move_projects(old: String, new: String) -> Result<bool, WorkspaceError> {
    let old = PathBuf::from(old);
    let new = PathBuf::from(new);
    if !old.is_dir() {
        return Ok(false);
    }
    if new.exists() {
        let scratch = new.join(SCRATCH_PROJECT);
        // Fail closed: anything unreadable counts as work, never as empty.
        let mut only_scratch = true;
        for entry in fs::read_dir(&new).map_err(storage)? {
            if entry.map_err(storage)?.path() != scratch {
                only_scratch = false;
            }
        }
        let only_empty_scratch = only_scratch
            && match fs::read_dir(&scratch) {
                Ok(mut d) => d.next().is_none(),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
                Err(err) => return Err(storage(err)),
            };
        if !only_empty_scratch {
            return Ok(false);
        }
        fs::remove_dir_all(&new).map_err(storage)?;
    }
    if let Some(parent) = new.parent() {
        fs::create_dir_all(parent).map_err(storage)?;
    }
    if fs::rename(&old, &new).is_err() {
        copy_tree(&old, &new).map_err(storage)?;
        fs::remove_dir_all(&old).map_err(storage)?;
    }
    Ok(true)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A project name: no slashes, not hidden, no control characters, at most
/// [`PROJECT_NAME_MAX`] characters. Returned trimmed.
fn project_name(name: &str) -> Result<String, WorkspaceError> {
    let trimmed = name.trim();
    let count = trimmed.chars().count();
    if trimmed.is_empty() {
        return Err(WorkspaceError::Invalid {
            reason: "A project needs a name.".into(),
        });
    }
    if count > PROJECT_NAME_MAX {
        return Err(WorkspaceError::Invalid {
            reason: format!(
                "That name is {count} characters; a project name can be at most {PROJECT_NAME_MAX}."
            ),
        });
    }
    name_part(trimmed, "A project name")?;
    // A folder with a ladder task's name would be taken for an old ladder
    // project, moved into the hidden `.ladder` and wiped by the next run.
    if crate::harness::is_ladder_task(trimmed) {
        return Err(WorkspaceError::Invalid {
            reason: format!("{trimmed} is the name of one of Bonsai's test tasks; choose another."),
        });
    }
    Ok(trimmed.to_string())
}

/// One file, folder or project name: not empty, not `.` or `..`, not
/// hidden, no `/`, `\`, `:` or control characters, no space at either end.
fn name_part(part: &str, what: &str) -> Result<(), WorkspaceError> {
    let bad = |reason: String| Err(WorkspaceError::Invalid { reason });
    if part.is_empty() || part == "." || part == ".." {
        return bad(format!("{what} can't be empty, \".\" or \"..\"."));
    }
    if part.starts_with('.') {
        return bad(format!("{what} can't start with a dot (that hides it)."));
    }
    if part != part.trim() {
        return bad(format!("{what} can't start or end with a space."));
    }
    if part.chars().any(|c| c.is_control() || "/\\:".contains(c)) {
        return bad(format!(
            "{what} can't contain /, \\, : or control characters."
        ));
    }
    if part.chars().count() > NAME_MAX {
        return bad(format!("{what} can be at most {NAME_MAX} characters."));
    }
    Ok(())
}

/// A path inside a project, like "lib/helper.py".
fn check_path(path: &str) -> Result<(), WorkspaceError> {
    if path.chars().count() > PATH_MAX {
        return Err(WorkspaceError::Invalid {
            reason: format!("A path can be at most {PATH_MAX} characters."),
        });
    }
    if path.starts_with('/') {
        return Err(WorkspaceError::Invalid {
            reason: "A path must be inside the project.".into(),
        });
    }
    for part in path.split('/') {
        name_part(part, "A file or folder name")?;
    }
    Ok(())
}

/// `root/path`, refusing any part that is a symbolic link (one could point
/// outside the project).
fn safe_join(root: &Path, path: &str) -> Result<PathBuf, WorkspaceError> {
    let mut current = root.to_path_buf();
    for part in path.split('/') {
        current.push(part);
        if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(WorkspaceError::Invalid {
                reason: format!("{path} passes through a symbolic link."),
            });
        }
    }
    Ok(current)
}

fn parent_of(path: &str) -> String {
    path.rsplit_once('/')
        .map_or(String::new(), |(p, _)| p.to_string())
}

fn exists_any_case(dir: &Path, name: &str) -> Result<bool, WorkspaceError> {
    let lower = name.to_lowercase();
    for entry in fs::read_dir(dir).map_err(storage)? {
        if entry
            .map_err(storage)?
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            == lower
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_scratch(name: &str) -> bool {
    name.trim().to_lowercase() == SCRATCH_PROJECT
}

fn modified_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64)
}

/// Files and folders under `dir`, hidden ones and links skipped, stopping
/// once `out` holds `limit`.
fn walk(
    root: &Path,
    dir: &Path,
    out: &mut Vec<WorkspaceEntry>,
    limit: usize,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if out.len() >= limit {
            return Ok(());
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        out.push(WorkspaceEntry {
            path: relative,
            is_folder: meta.is_dir(),
            bytes: if meta.is_dir() { 0 } else { meta.len() },
            modified_ms: modified_ms(&meta),
        });
        if meta.is_dir() {
            walk(root, &path, out, limit)?;
        }
    }
    Ok(())
}

/// Tree order: by path, part by part, with folders before files at the
/// same level and names compared without case.
fn tree_order(a: &str, a_folder: bool, b: &str, b_folder: bool) -> std::cmp::Ordering {
    let a_parts: Vec<&str> = a.split('/').collect();
    let b_parts: Vec<&str> = b.split('/').collect();
    for (i, (x, y)) in a_parts.iter().zip(&b_parts).enumerate() {
        if x == y {
            continue;
        }
        // At the first difference: is each side a folder at this level?
        let x_folder = i + 1 < a_parts.len() || a_folder;
        let y_folder = i + 1 < b_parts.len() || b_folder;
        return y_folder
            .cmp(&x_folder)
            .then_with(|| x.to_lowercase().cmp(&y.to_lowercase()))
            .then_with(|| x.cmp(y));
    }
    a_parts.len().cmp(&b_parts.len())
}

/// Writes to a temporary file beside `path`, then renames it over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A name of its own for each save, so two saves at once never share a
    // temporary file; the last rename wins with complete contents.
    static SAVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SAVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = path.with_file_name(format!(".{name}.{}.{n}.saving", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, std::sync::Arc<Workspace>) {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path().display().to_string()).unwrap();
        (dir, ws)
    }

    fn s(text: &str) -> String {
        text.to_string()
    }

    #[test]
    fn a_new_workspace_has_only_scratch() {
        let (_dir, ws) = workspace();
        let projects = ws.projects();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "scratch");
        assert!(projects[0].is_scratch);
        assert_eq!(projects[0].files, 0);
    }

    #[test]
    fn projects_are_created_renamed_and_deleted() {
        let (dir, ws) = workspace();
        ws.create_project(s("  Expenses ")).unwrap();
        ws.create_project(s("Zoo")).unwrap();
        ws.create_project(s("árbol 2")).unwrap();
        let names: Vec<_> = ws.projects().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["scratch", "Expenses", "Zoo", "árbol 2"]);
        assert_eq!(
            ws.create_project(s("expenses")),
            Err(WorkspaceError::Exists {
                name: s("expenses")
            })
        );
        ws.rename_project(s("Expenses"), s("Money")).unwrap();
        assert!(dir.path().join("Money").is_dir());
        ws.rename_project(s("Money"), s("money")).unwrap();
        assert!(ws.projects().iter().any(|p| p.name == "money"));
        assert_eq!(
            ws.rename_project(s("money"), s("Zoo")),
            Err(WorkspaceError::Exists { name: s("Zoo") })
        );
        ws.delete_project(s("money")).unwrap();
        assert!(!ws.projects().iter().any(|p| p.name == "money"));
        assert!(matches!(
            ws.delete_project(s("money")),
            Err(WorkspaceError::NotFound { .. })
        ));
    }

    #[test]
    fn scratch_cannot_be_renamed_or_deleted() {
        let (_dir, ws) = workspace();
        // iOS's file system ignores case, so "Scratch" names the same folder.
        assert!(is_scratch(" Scratch "));
        assert!(is_scratch("SCRATCH"));
        for variant in ["Scratch", "SCRATCH"] {
            let result = ws.delete_project(s(variant));
            assert!(
                matches!(
                    result,
                    Err(WorkspaceError::Protected | WorkspaceError::NotFound { .. })
                ),
                "{variant}: {result:?}"
            );
        }
        assert_eq!(
            ws.delete_project(s("scratch")),
            Err(WorkspaceError::Protected)
        );
        assert_eq!(
            ws.rename_project(s("scratch"), s("mine")),
            Err(WorkspaceError::Protected)
        );
    }

    #[test]
    fn bad_names_are_refused_with_a_reason() {
        let (_dir, ws) = workspace();
        for bad in ["", "  ", ".hidden", "a/b", "..", "a:b", "a\nb", "a\\b"] {
            assert!(
                matches!(
                    ws.create_project(s(bad)),
                    Err(WorkspaceError::Invalid { .. })
                ),
                "{bad:?}"
            );
        }
        assert!(matches!(
            ws.create_project(s("t1-hello")),
            Err(WorkspaceError::Invalid { .. })
        ));
        ws.create_project(s("mine")).unwrap();
        assert!(matches!(
            ws.rename_project(s("mine"), s("T3-One-Fix")),
            Err(WorkspaceError::Invalid { .. })
        ));
        let long = "x".repeat(PROJECT_NAME_MAX + 1);
        let Err(WorkspaceError::Invalid { reason }) = ws.create_project(long) else {
            panic!("a long name was taken");
        };
        assert!(reason.contains("61 characters"), "{reason}");
        ws.create_project("x".repeat(PROJECT_NAME_MAX)).unwrap();
    }

    #[test]
    fn hidden_and_harness_folders_are_not_projects() {
        let (dir, ws) = workspace();
        fs::create_dir_all(dir.path().join(".ladder/t1-hello")).unwrap();
        fs::create_dir_all(dir.path().join(".runs")).unwrap();
        fs::write(dir.path().join("stray.txt"), "x").unwrap();
        let names: Vec<_> = ws.projects().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["scratch"]);
        assert!(ws.entries(s(".ladder")).is_err());
    }

    #[test]
    fn old_ladder_projects_are_moved_out_of_the_list() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("t1-hello")).unwrap();
        fs::write(dir.path().join("t1-hello/hello.py"), "print(1)").unwrap();
        // A person's project that happens to have another name stays.
        fs::create_dir_all(dir.path().join("mine")).unwrap();
        let ws = Workspace::new(dir.path().display().to_string()).unwrap();
        let names: Vec<_> = ws.projects().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["scratch", "mine"]);
        assert!(dir.path().join(".ladder/t1-hello/hello.py").is_file());
    }

    #[test]
    fn files_and_folders_round_trip() {
        let (_dir, ws) = workspace();
        let p = s("scratch");
        ws.create_folder(p.clone(), s("lib")).unwrap();
        ws.create_file(p.clone(), s("main.py")).unwrap();
        ws.write_text(
            p.clone(),
            s("lib/helper.py"),
            s("def two():\n    return 2\n"),
        )
        .unwrap();
        ws.write_text(
            p.clone(),
            s("main.py"),
            s("from lib.helper import two\nprint(two())\n"),
        )
        .unwrap();
        ws.write_text(p.clone(), s("data file.csv"), s("a,b\n"))
            .unwrap();
        assert_eq!(
            ws.read_text(p.clone(), s("lib/helper.py")).unwrap(),
            "def two():\n    return 2\n"
        );
        let paths: Vec<_> = ws
            .entries(p.clone())
            .unwrap()
            .into_iter()
            .map(|e| (e.path, e.is_folder))
            .collect();
        assert_eq!(
            paths,
            [
                (s("lib"), true),
                (s("lib/helper.py"), false),
                (s("data file.csv"), false),
                (s("main.py"), false),
            ]
        );
        assert_eq!(ws.projects()[0].files, 3);
        // No temporary file is left behind.
        let leftovers = fs::read_dir(_dir.path().join("scratch"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".saving"))
            .count();
        assert_eq!(leftovers, 0);

        ws.rename_entry(p.clone(), s("lib/helper.py"), s("helper.py"))
            .unwrap();
        ws.rename_entry(p.clone(), s("lib"), s("tools")).unwrap();
        assert_eq!(
            ws.rename_entry(p.clone(), s("tools"), s("tools/inner")),
            Err(WorkspaceError::Invalid {
                reason: s("tools can't move inside itself.")
            })
        );
        assert_eq!(
            ws.rename_entry(p.clone(), s("main.py"), s("helper.py")),
            Err(WorkspaceError::Exists {
                name: s("helper.py")
            })
        );
        ws.delete_entry(p.clone(), s("tools")).unwrap();
        ws.delete_entry(p.clone(), s("helper.py")).unwrap();
        let left: Vec<_> = ws.entries(p).unwrap().into_iter().map(|e| e.path).collect();
        assert_eq!(left, ["data file.csv", "main.py"]);
    }

    #[test]
    fn creating_needs_its_folder_and_a_free_name() {
        let (_dir, ws) = workspace();
        let p = s("scratch");
        assert_eq!(
            ws.create_file(p.clone(), s("missing/a.py")),
            Err(WorkspaceError::NotFound {
                what: s("folder"),
                name: s("missing")
            })
        );
        assert!(matches!(
            ws.write_text(p.clone(), s("missing/a.py"), s("")),
            Err(WorkspaceError::NotFound { .. })
        ));
        ws.create_file(p.clone(), s("a.py")).unwrap();
        assert_eq!(
            ws.create_file(p.clone(), s("a.py")),
            Err(WorkspaceError::Exists { name: s("a.py") })
        );
        assert!(matches!(
            ws.read_text(p, s("b.py")),
            Err(WorkspaceError::NotFound { .. })
        ));
    }

    #[test]
    fn paths_cannot_leave_the_project() {
        let (dir, ws) = workspace();
        let p = s("scratch");
        for bad in [
            "../x.py",
            "/etc/passwd",
            "a/../../x",
            ".harness/journal.jsonl",
            "a//b",
        ] {
            assert!(
                matches!(
                    ws.write_text(p.clone(), s(bad), s("x")),
                    Err(WorkspaceError::Invalid { .. })
                ),
                "{bad}"
            );
        }
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("scratch/link")).unwrap();
        assert!(matches!(
            ws.write_text(p.clone(), s("link/x.py"), s("x")),
            Err(WorkspaceError::Invalid { .. })
        ));
        assert!(matches!(
            ws.read_text(p.clone(), s("link")),
            Err(WorkspaceError::Invalid { .. })
        ));
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
        // The link isn't listed either.
        assert!(ws.entries(p).unwrap().is_empty());
    }

    #[test]
    fn limits_refuse_instead_of_cutting() {
        let (_dir, ws) = workspace();
        let p = s("scratch");
        let big = "x".repeat(TEXT_MAX_BYTES + 1);
        let Err(WorkspaceError::TooLarge { reason }) = ws.write_text(p.clone(), s("big.txt"), big)
        else {
            panic!("a large file was saved");
        };
        assert!(reason.contains("1024 KB"), "{reason}");
        assert!(!_dir.path().join("scratch/big.txt").exists());
        ws.write_text(p.clone(), s("ok.txt"), "x".repeat(TEXT_MAX_BYTES))
            .unwrap();

        for i in 0..ENTRIES_MAX - 1 {
            fs::write(_dir.path().join(format!("scratch/f{i}")), "").unwrap();
        }
        let Err(WorkspaceError::TooLarge { reason }) = ws.create_file(p.clone(), s("one-more"))
        else {
            panic!("the project grew past its limit");
        };
        assert!(reason.contains("500"), "{reason}");
        // An existing file can still be saved.
        ws.write_text(p, s("ok.txt"), s("changed")).unwrap();
    }

    #[test]
    fn binary_files_are_not_opened_as_text() {
        let (dir, ws) = workspace();
        fs::write(
            dir.path().join("scratch/plot.png"),
            [0x89, b'P', 0xff, 0xfe],
        )
        .unwrap();
        assert!(matches!(
            ws.read_text(s("scratch"), s("plot.png")),
            Err(WorkspaceError::Invalid { .. })
        ));
    }

    #[test]
    fn state_round_trips_and_forgets_missing_files() {
        let (_dir, ws) = workspace();
        let p = s("scratch");
        assert_eq!(ws.state(p.clone()).unwrap(), WorkspaceState::default());
        ws.write_text(p.clone(), s("main.py"), s("")).unwrap();
        ws.write_text(p.clone(), s("gone.py"), s("")).unwrap();
        let state = WorkspaceState {
            run_file: Some(s("main.py")),
            current_file: Some(s("gone.py")),
            open_files: vec![s("main.py"), s("gone.py")],
        };
        ws.save_state(p.clone(), state).unwrap();
        ws.delete_entry(p.clone(), s("gone.py")).unwrap();
        let back = ws.state(p.clone()).unwrap();
        assert_eq!(back.run_file.as_deref(), Some("main.py"));
        assert_eq!(back.current_file, None);
        assert_eq!(back.open_files, [s("main.py")]);
        // The state folder isn't a file of the project.
        assert!(ws
            .entries(p.clone())
            .unwrap()
            .iter()
            .all(|e| !e.path.starts_with('.')));
        // Unreadable state reads as nothing open.
        fs::write(
            _dir.path().join("scratch/.workspace/state.json"),
            "{not json",
        )
        .unwrap();
        assert_eq!(ws.state(p).unwrap(), WorkspaceState::default());
    }

    #[test]
    fn projects_move_once_to_the_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Application Support/Python Lab/Projects");
        let new = dir.path().join("Documents/Python Lab");
        fs::create_dir_all(old.join("scratch")).unwrap();
        fs::write(old.join("scratch/main.py"), "print(1)").unwrap();
        // The new folder was already opened once (an empty scratch).
        fs::create_dir_all(new.join("scratch")).unwrap();
        let moved = workspace_move_projects(old.display().to_string(), new.display().to_string());
        assert_eq!(moved, Ok(true));
        assert!(new.join("scratch/main.py").is_file());
        assert!(!old.exists());
        // Nothing left to move.
        assert_eq!(
            workspace_move_projects(old.display().to_string(), new.display().to_string()),
            Ok(false)
        );
        // The new folder is never overwritten once it has work in it.
        fs::create_dir_all(old.join("other")).unwrap();
        assert_eq!(
            workspace_move_projects(old.display().to_string(), new.display().to_string()),
            Ok(false)
        );
        assert!(new.join("scratch/main.py").is_file());
    }

    #[test]
    fn tree_order_puts_folders_first() {
        let mut paths = vec![
            ("b.py", false),
            ("A", true),
            ("A/z.py", false),
            ("A/sub", true),
            ("A/sub/x.py", false),
            ("a.py", false),
        ];
        paths.sort_by(|a, b| tree_order(a.0, a.1, b.0, b.1));
        let order: Vec<_> = paths.into_iter().map(|p| p.0).collect();
        assert_eq!(
            order,
            ["A", "A/sub", "A/sub/x.py", "A/z.py", "a.py", "b.py"]
        );
    }
}
