//! `forge crash`: reads `.ips` crash reports on this computer, symbolicated
//! against the app's local build (the `forge-crash` crate). `forge device
//! crashes --pull` uses the same code on the reports it copies.

use std::io::Write;
use std::path::{Path, PathBuf};

use forge_crash::{Analysis, Options, ToolchainSwift};
use serde_json::{json, Value};

/// Frames per thread in text output; JSON has them all.
pub const TEXT_FRAMES: usize = 40;
/// Frames per thread in the short summary `device crashes --pull` prints.
const SUMMARY_FRAMES: usize = 8;

/// Where to look for the build: the app folder (its `.build` and `xtool`
/// output) and any binaries or dSYMs given by hand.
pub fn options(project: Option<&Path>, binaries: &[PathBuf], all_threads: bool) -> Options {
    Options {
        roots: project.map(|p| vec![p.to_path_buf()]).unwrap_or_default(),
        binaries: binaries.to_vec(),
        all_threads,
    }
}

pub fn analyze_file(file: &Path, options: &Options) -> Result<Analysis, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("read {}: {e}", file.display()))?;
    forge_crash::analyze(&bytes, options, &ToolchainSwift::default()).map_err(|e| e.to_string())
}

/// One entry of `data.crashes`: the analysis, or why there is none.
pub fn entry(file: &Path, analysis: &Result<Analysis, String>) -> Value {
    let mut v = match analysis {
        Ok(a) => serde_json::to_value(a).unwrap_or_else(|e| json!({ "error": e.to_string() })),
        Err(e) => json!({ "error": e }),
    };
    v["file"] = json!(file.display().to_string());
    v
}

/// `forge crash FILE...`
pub fn show(
    files: &[PathBuf],
    options: &Options,
    frames: usize,
    out: &mut dyn Write,
) -> Result<(), String> {
    let mut failed = Vec::new();
    for (i, file) in files.iter().enumerate() {
        if i > 0 {
            writeln!(out).map_err(|e| e.to_string())?;
        }
        match analyze_file(file, options) {
            Ok(a) => {
                if files.len() > 1 {
                    writeln!(out, "== {}", file.display()).map_err(|e| e.to_string())?;
                }
                write!(out, "{}", forge_crash::render(&a, frames)).map_err(|e| e.to_string())?;
            }
            Err(e) => failed.push(format!("{}: {e}", file.display())),
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("\n"))
    }
}

/// `forge crash FILE... --json`: `{"crashes": [...]}`.
pub fn show_data(files: &[PathBuf], options: &Options) -> Result<Value, String> {
    let entries: Vec<Value> = files
        .iter()
        .map(|f| entry(f, &analyze_file(f, options)))
        .collect();
    if entries.iter().all(|e| e.get("error").is_some()) {
        let errors: Vec<String> = entries
            .iter()
            .map(|e| format!("{}: {}", e["file"].as_str().unwrap_or(""), e["error"]))
            .collect();
        return Err(errors.join("\n"));
    }
    Ok(json!({ "crashes": entries }))
}

/// After `device crashes --pull`: analyses each `.ips` file, writes the full
/// text next to it (`<report>.txt`), and returns the analyses.
pub fn analyze_pulled(
    files: &[PathBuf],
    options: &Options,
) -> Vec<(PathBuf, Result<Analysis, String>)> {
    files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "ips"))
        .map(|f| {
            let a = analyze_file(f, options);
            if let Ok(a) = &a {
                let mut txt = f.clone().into_os_string();
                txt.push(".txt");
                let _ = std::fs::write(txt, forge_crash::render(a, TEXT_FRAMES * 2));
            }
            (f.clone(), a)
        })
        .collect()
}

/// The few lines `device crashes --pull` prints under each report.
pub fn summary(a: &Analysis) -> String {
    let full = forge_crash::render(a, SUMMARY_FRAMES);
    // Header, cause, hint and the crashed thread; other threads are in the
    // .txt file.
    let mut lines = Vec::new();
    let mut threads = 0;
    for line in full.lines() {
        if line.starts_with("Thread ") || line.starts_with("Exception thrown at:") {
            threads += 1;
            if threads > 1 {
                break;
            }
        }
        lines.push(format!("  {line}"));
    }
    lines.join("\n").trim_end().to_string() + "\n"
}
