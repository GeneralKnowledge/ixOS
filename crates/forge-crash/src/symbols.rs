//! Finding the local binary that a crashed image came from, and reading its
//! symbols and line tables.
//!
//! A report names each image by its UUID (`LC_UUID`), which the linker sets
//! and signing keeps. A binary or dSYM on this computer with the same UUID
//! is the exact build that crashed; nothing else is trusted, because an
//! offset into a different build points at the wrong code.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use object::{Object, ObjectSegment};

/// Folders under an app that never hold the app's binaries but can be large
/// (package checkouts, caches), so the search skips them.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "checkouts",
    "repositories",
    "ModuleCache",
    "index",
    "node_modules",
    "Sources",
    "Tests",
    "Generated",
    "target",
    "jniLibs",
];
const MAX_DEPTH: usize = 9;
/// Binaries larger than this are not app builds (or not ones worth mapping).
const MAX_SIZE: u64 = 2 << 30;

/// A local file whose UUID matched an image in the report.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    /// The file is a dSYM's DWARF file rather than the binary.
    pub dsym: bool,
}

/// What the search saw for one image name: the UUIDs of same-named binaries
/// that did not match (an older or newer build of the same app).
#[derive(Debug, Default)]
pub struct Search {
    pub found: HashMap<String, Found>,
    pub other_builds: HashMap<String, Vec<(String, PathBuf)>>,
}

/// Reads a linked Mach-O file's UUID as the report writes it (lower-case,
/// dashed): executables, dylibs and dSYM companions. Object files, archives
/// and universal (fat) files give `None`. Only the header and load commands
/// are read, so scanning a build folder full of object files stays cheap.
pub fn file_uuid(path: &Path) -> Option<String> {
    use std::io::Read;
    const MH_MAGIC_64: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
    const MH_EXECUTE: u32 = 2;
    const MH_DYLIB: u32 = 6;
    const MH_BUNDLE: u32 = 8;
    const MH_DSYM: u32 = 10;
    const LC_UUID: u32 = 0x1b;

    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() < 32 || meta.len() > MAX_SIZE {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 32];
    file.read_exact(&mut header).ok()?;
    if header[0..4] != MH_MAGIC_64 {
        return None;
    }
    let u32_at = |b: &[u8], at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
    };
    let filetype = u32_at(&header, 12)?;
    if ![MH_EXECUTE, MH_DYLIB, MH_BUNDLE, MH_DSYM].contains(&filetype) {
        return None;
    }
    let ncmds = u32_at(&header, 16)? as usize;
    let sizeofcmds = u32_at(&header, 20)? as usize;
    if sizeofcmds > 4 << 20 || sizeofcmds as u64 + 32 > meta.len() {
        return None;
    }
    let mut cmds = vec![0u8; sizeofcmds];
    file.read_exact(&mut cmds).ok()?;
    let mut at = 0usize;
    for _ in 0..ncmds {
        let cmd = u32_at(&cmds, at)?;
        let size = u32_at(&cmds, at + 4)? as usize;
        if cmd == LC_UUID {
            let bytes: [u8; 16] = cmds.get(at + 8..at + 24)?.try_into().ok()?;
            return Some(format_uuid(bytes));
        }
        if size < 8 {
            return None;
        }
        at += size;
    }
    None
}

pub fn format_uuid(b: [u8; 16]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Looks for the binaries (or dSYMs) of `wanted` (UUID → image name) in
/// `roots` and in the explicit `binaries` (files or `.dSYM` bundles).
pub fn search(roots: &[PathBuf], binaries: &[PathBuf], wanted: &HashMap<String, String>) -> Search {
    let names: HashSet<&str> = wanted.values().map(String::as_str).collect();
    let mut out = Search::default();
    // Every linked Mach-O file is checked, whatever its name: the build's
    // copy of the app's executable may be named differently from the image.
    // A dSYM beats a binary of the same name, which beats any other copy.
    let rank = |path: &Path, name: &str, image: &str| -> u8 {
        if is_dsym_dwarf(path) {
            2
        } else {
            u8::from(name == image)
        }
    };
    let mut consider = |path: &Path| {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(uuid) = file_uuid(path) else {
            return;
        };
        match wanted.get(&uuid) {
            Some(image) => {
                let new_rank = rank(path, name, image);
                let better = out.found.get(&uuid).is_none_or(|f| {
                    let old = f.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    new_rank > rank(&f.path, old, image)
                });
                if better {
                    out.found.insert(
                        uuid,
                        Found {
                            path: path.to_path_buf(),
                            dsym: is_dsym_dwarf(path),
                        },
                    );
                }
            }
            None if names.contains(name) => out
                .other_builds
                .entry(name.to_string())
                .or_default()
                .push((uuid, path.to_path_buf())),
            None => {}
        }
    };
    for b in binaries {
        if b.is_dir() {
            walk(b, 0, &mut consider);
        } else {
            consider(b);
        }
    }
    for root in roots {
        walk(root, 0, &mut consider);
    }
    out
}

fn is_dsym_dwarf(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains(".dSYM/Contents/Resources/DWARF/")
}

fn walk(dir: &Path, depth: usize, f: &mut dyn FnMut(&Path)) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(kind) = e.file_type() else { continue };
        let path = e.path();
        if kind.is_dir() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) || name.ends_with(".swiftmodule") {
                continue;
            }
            walk(&path, depth + 1, f);
        } else if kind.is_file() {
            f(&path);
        }
    }
}

/// One function at an address. Inlined calls give several, innermost first.
#[derive(Debug, Clone)]
pub struct Located {
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub inlined: bool,
}

/// Symbols and line tables of one local binary or dSYM.
pub struct Symbols {
    loader: addr2line::Loader,
    text_vmaddr: u64,
}

impl Symbols {
    pub fn open(path: &Path) -> Option<Symbols> {
        let data = std::fs::read(path).ok()?;
        let file = object::File::parse(&*data).ok()?;
        let text_vmaddr = file
            .segments()
            .find(|s| s.name().ok().flatten() == Some("__TEXT"))
            .map(|s| s.address())
            .unwrap_or(0);
        let loader = addr2line::Loader::new(path).ok()?;
        Some(Symbols {
            loader,
            text_vmaddr,
        })
    }

    /// What is at `offset` from the image's start. `return_address` frames
    /// point just after their call, so the lookup steps back one byte to land
    /// on the call's line.
    pub fn lookup(&self, offset: u64, return_address: bool) -> Vec<Located> {
        let probe = self.text_vmaddr + offset - u64::from(return_address && offset > 0);
        let mut out = Vec::new();
        if let Ok(mut frames) = self.loader.find_frames(probe) {
            while let Ok(Some(frame)) = frames.next() {
                let function = frame
                    .function
                    .as_ref()
                    .and_then(|f| f.raw_name().ok().map(|n| n.to_string()));
                let (file, line, column) = match &frame.location {
                    Some(l) => (l.file.map(str::to_string), l.line, l.column),
                    None => (None, None, None),
                };
                out.push(Located {
                    function,
                    file,
                    line,
                    column,
                    inlined: false,
                });
            }
        }
        let n = out.len();
        for (i, l) in out.iter_mut().enumerate() {
            l.inlined = i + 1 < n;
        }
        // No line table entry (no debug info for that code): the symbol
        // table still names the function.
        let symbol = self
            .loader
            .find_symbol(probe)
            .map(|n| crate::demangle::from_symbol_table(n).to_string());
        match out.last_mut() {
            Some(last) if last.function.is_none() => last.function = symbol,
            None if symbol.is_some() => out.push(Located {
                function: symbol,
                file: None,
                line: None,
                column: None,
                inlined: false,
            }),
            _ => {}
        }
        out
    }

    /// Offset of `probe`'s containing symbol start, for `symbol + N` display.
    pub fn symbol_offset(&self, offset: u64) -> Option<u64> {
        let probe = self.text_vmaddr + offset;
        self.loader
            .find_symbol_info(probe)
            .map(|s| probe.saturating_sub(s.address()))
    }
}
