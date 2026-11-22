//! The files a project's scripts made. Images are what the harness's
//! `ProducesImage` check and Bonsai look at (docs/visual-labs-spec.md, P1);
//! Python Lab's artifact browser (PL2e) lists every output, images, tables,
//! text and anything else, and previews tables and text from here. Python
//! makes the files; this only lists and reads them.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// An output file in a project (an image, for `project_artifacts`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectArtifact {
    /// Relative to the project, like "charts/sales.png".
    pub path: String,
    /// "png", "jpeg", "gif" or "svg"; with `project_outputs` also "table"
    /// (CSV, TSV), "json", "text", "html", "pdf" or "file".
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

/// The kind of any output file by its extension: an image kind, or
/// "table", "json", "text", "html", "pdf" or "file". Python source and
/// compiled files aren't outputs.
pub fn output_kind_of(path: &str) -> Option<&'static str> {
    if let Some(image) = kind_of(path) {
        return Some(image);
    }
    let lower = path.to_ascii_lowercase();
    let extension = lower.rsplit_once('.').map_or("", |(_, e)| e);
    Some(match extension {
        "py" | "pyc" | "pyi" => return None,
        "csv" | "tsv" => "table",
        "json" => "json",
        "txt" | "md" | "log" | "yaml" | "yml" | "toml" | "xml" | "ini" | "cfg" => "text",
        "html" | "htm" => "html",
        "pdf" => "pdf",
        _ => "file",
    })
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
    list_kinds(root, kind_of)
}

/// Every output file under `root` (PL2e), newest first.
pub fn list_outputs(root: &Path) -> Vec<ProjectArtifact> {
    list_kinds(root, output_kind_of)
}

fn list_kinds(root: &Path, kind: fn(&str) -> Option<&'static str>) -> Vec<ProjectArtifact> {
    let mut out = Vec::new();
    walk(root, root, kind, &mut out);
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

fn walk(
    root: &Path,
    dir: &Path,
    kind_of: fn(&str) -> Option<&'static str>,
    out: &mut Vec<ProjectArtifact>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "__pycache__" {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(root, &path, kind_of, out);
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

/// Every output file in the project folder `project` (PL2e): images,
/// tables, text and the rest, newest first. Python files aren't outputs.
#[uniffi::export]
pub fn project_outputs(project: String) -> Vec<ProjectArtifact> {
    list_outputs(Path::new(&project))
}

/// The most of a file a preview reads.
pub const PREVIEW_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Rows, columns and characters per cell a table preview keeps.
pub const TABLE_MAX_ROWS: usize = 500;
pub const TABLE_MAX_COLUMNS: usize = 50;
pub const CELL_MAX_CHARS: usize = 200;
/// The most text a text preview shows.
pub const TEXT_MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error, thiserror::Error)]
pub enum ArtifactError {
    #[error("{reason}")]
    Unreadable { reason: String },
}

/// A CSV or TSV file's first rows, the first being its header.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArtifactTable {
    pub rows: Vec<Vec<String>>,
    /// Rows in the part of the file read, header included; more than
    /// `rows.len()` when the preview stops early.
    pub total_rows: u64,
    /// Some rows had more columns, or cells more characters, than shown.
    pub cut: bool,
    /// The file is longer than the part read.
    pub partial: bool,
}

/// A text or JSON file to show; JSON is indented when it parses.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArtifactText {
    pub text: String,
    pub truncated: bool,
}

fn read_head(file: &str) -> Result<(Vec<u8>, bool), ArtifactError> {
    let unreadable = |e: std::io::Error| ArtifactError::Unreadable {
        reason: format!("Couldn't read the file: {e}"),
    };
    let handle = fs::File::open(file).map_err(unreadable)?;
    let size = handle.metadata().map_err(unreadable)?.len();
    let mut bytes = Vec::new();
    handle
        .take(PREVIEW_MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    Ok((bytes, size > PREVIEW_MAX_BYTES))
}

/// The first rows of the CSV (or, for `.tsv`, tab-separated) file at
/// `file`. Quoted cells may hold the separator, quotes (doubled) and
/// newlines, as Python's `csv` module writes them.
#[uniffi::export]
pub fn artifact_table(file: String) -> Result<ArtifactTable, ArtifactError> {
    let (bytes, partial) = read_head(&file)?;
    let text = String::from_utf8_lossy(&bytes);
    let separator = if file.to_ascii_lowercase().ends_with(".tsv") {
        '\t'
    } else {
        ','
    };
    Ok(parse_table(&text, separator, partial))
}

pub fn parse_table(text: &str, separator: char, partial: bool) -> ArtifactTable {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut total = 0u64;
    let mut cut = false;
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut finish_row = |row: &mut Vec<String>, rows: &mut Vec<Vec<String>>, cut: &mut bool| {
        total += 1;
        if rows.len() < TABLE_MAX_ROWS {
            if row.len() > TABLE_MAX_COLUMNS {
                row.truncate(TABLE_MAX_COLUMNS);
                *cut = true;
            }
            rows.push(std::mem::take(row));
        } else {
            row.clear();
        }
    };
    let push_cell = |cell: &mut String, row: &mut Vec<String>, cut: &mut bool| {
        // Cells past the last column shown are counted out, never kept.
        if row.len() >= TABLE_MAX_COLUMNS {
            cell.clear();
            *cut = true;
            return;
        }
        if cell.chars().count() > CELL_MAX_CHARS {
            // cut: a preview cell, ending in "…"; `cut` makes the screen say so
            *cell = cell.chars().take(CELL_MAX_CHARS).collect::<String>() + "…";
            *cut = true;
        }
        row.push(std::mem::take(cell));
    };
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                push_cell(&mut cell, &mut row, &mut cut);
                finish_row(&mut row, &mut rows, &mut cut);
                any = false;
            }
            c if c == separator => push_cell(&mut cell, &mut row, &mut cut),
            c => cell.push(c),
        }
    }
    if any {
        push_cell(&mut cell, &mut row, &mut cut);
        finish_row(&mut row, &mut rows, &mut cut);
    }
    ArtifactTable {
        rows,
        total_rows: total,
        cut,
        partial,
    }
}

/// The text of the file at `file` (the first 256 KB), JSON indented.
#[uniffi::export]
pub fn artifact_text(file: String) -> Result<ArtifactText, ArtifactError> {
    let (bytes, partial) = read_head(&file)?;
    if !partial && file.to_ascii_lowercase().ends_with(".json") {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Ok(pretty) = serde_json::to_string_pretty(&value) {
                return Ok(limit_text(pretty, false));
            }
        }
    }
    Ok(limit_text(
        String::from_utf8_lossy(&bytes).into_owned(),
        partial,
    ))
}

fn limit_text(mut text: String, truncated: bool) -> ArtifactText {
    if text.len() <= TEXT_MAX_BYTES {
        return ArtifactText { text, truncated };
    }
    let mut end = TEXT_MAX_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    ArtifactText {
        text,
        truncated: true,
    }
}
