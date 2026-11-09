//! `forge inspect`: what an app folder holds, read from its files. For a
//! person opening an app for the first time, and for scripts and agents
//! (`--json`; `forge mcp`'s `forge_inspect_app` runs it).
//!
//! It reports facts it can point at, and labels the rest:
//!
//! - Swift files with their `import` lines, and whether they import a UI
//!   framework (SwiftUI, UIKit). It does not call a file a "view" or a
//!   "model": that would be a guess from the imports.
//! - Accessibility identifiers that are string literals passed to
//!   `.accessibilityIdentifier("…")`, with the file. An interpolated one
//!   (`"row.\(id)"`) is listed as its literal prefix; identifiers built any
//!   other way are not found, and the output says so.
//! - UI test files, baselines, `requirements.toml`, `Generated/` and the
//!   optional agent files, by path.
//! - Convention violations it can state exactly: a `*Model.swift` file that
//!   imports a UI framework (AGENTS.md rule 3: models compile on Linux).
//!
//! It runs nothing and changes nothing.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::config::ForgeConfig;

/// A file importing one of these needs the iOS SDK to compile.
const UI_FRAMEWORKS: &[&str] = &["SwiftUI", "UIKit"];

/// Files that configure AI coding agents. Optional: nothing in Forge reads
/// them to build, test or run an app (`forge agent setup` writes them).
pub const AGENT_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", ".mcp.json"];

const IDENTIFIERS_NOTE: &str = "Only string literals passed to .accessibilityIdentifier(\"…\") are found; `prefix` marks an interpolated one (\"row.\\(id)\"). Identifiers built at run time, or set through a helper, are not listed: `forge ui tree` on the phone shows the real ones.";

/// The app folder's facts as JSON (`data` of `forge --json inspect`).
pub fn scan(app: &Path) -> Result<Value, String> {
    let forge_toml = app.join("forge.toml");
    let text = std::fs::read_to_string(&forge_toml)
        .map_err(|e| format!("{}: {e} (pass --project <app>)", forge_toml.display()))?;
    let cfg = ForgeConfig::parse(&text)?;

    let mut swift = Vec::new();
    let mut identifiers = Vec::new();
    let mut warnings = Vec::new();
    for path in files_under(&app.join("Sources"), "swift") {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = rel(app, &path);
        let imports: Vec<String> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("import "))
            .map(|m| m.trim().to_string())
            .collect();
        let ui = imports.iter().any(|m| UI_FRAMEWORKS.contains(&m.as_str()));
        if ui && rel.ends_with("Model.swift") {
            warnings.push(format!(
                "{rel} imports a UI framework, so it can't be checked on Linux: keep models to Foundation, Observation, ForgeKit and ForgeFFI"
            ));
        }
        for (id, prefix) in crate::focus::identifiers(&text) {
            identifiers.push(json!({"id": id, "prefix": prefix, "file": rel}));
        }
        swift.push(json!({"path": rel, "imports": imports, "imports_ui_framework": ui}));
    }

    let ui_tests: Vec<String> = files_under(&app.join("uitests"), "toml")
        .iter()
        .map(|p| rel(app, p))
        .collect();
    let baselines = files_under(&app.join("uitests").join("baselines"), "png").len();
    let generated: Vec<String> = all_files(&app.join("Generated"))
        .iter()
        .map(|p| rel(app, p))
        .collect();
    let rust_dir = cfg.rust_crate_dir(app);
    let agent_files: Vec<&str> = AGENT_FILES
        .iter()
        .copied()
        .filter(|f| app.join(f).exists())
        .collect();
    let ios = if cfg.ios_enabled() {
        crate::app_config::summary_json(&cfg, app).unwrap_or_else(|e| json!({"error": e}))
    } else {
        Value::Null
    };

    Ok(json!({
        "app": {
            "name": cfg.app.name,
            "bundle_id": cfg.app.bundle_id,
            "path": app.display().to_string(),
            "ios": cfg.ios_enabled(),
            "android": cfg.android_enabled(),
        },
        "rust_crate": {
            "path": rel(app, &rust_dir),
            "exists": rust_dir.join("Cargo.toml").is_file(),
        },
        "ios_config": ios,
        "swift_files": swift,
        "accessibility_identifiers": identifiers,
        "identifiers_note": IDENTIFIERS_NOTE,
        "ui_tests": ui_tests,
        "screenshot_baselines": baselines,
        "requirements": app.join("requirements.toml").is_file().then_some("requirements.toml"),
        "generated": {
            "files": generated,
            "note": "Written by `forge bindings generate` and `forge config sync`; never edit by hand.",
        },
        "agent_files": {
            "present": agent_files,
            "note": "Optional: no Forge command needs them. `forge agent setup` adds them.",
        },
        "convention_warnings": warnings,
    }))
}

/// `forge inspect`: prints the scan, or sets it as `data` with `--json`.
pub fn run(app: &Path) -> Result<(), String> {
    let data = scan(app)?;
    if crate::report::json_mode() {
        crate::report::set_data(data);
        return Ok(());
    }
    print!("{}", show(&data));
    Ok(())
}

fn show(d: &Value) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let _ = writeln!(
        out,
        "{} ({})\n  folder: {}",
        s(&d["app"]["name"]),
        s(&d["app"]["bundle_id"]),
        s(&d["app"]["path"])
    );
    let _ = writeln!(
        out,
        "  Rust crate: {}{}",
        s(&d["rust_crate"]["path"]),
        if d["rust_crate"]["exists"] == json!(true) {
            ""
        } else {
            " (missing)"
        }
    );
    let _ = writeln!(out, "\nSwift files (imports):");
    for f in d["swift_files"].as_array().into_iter().flatten() {
        let imports: Vec<String> = f["imports"]
            .as_array()
            .into_iter()
            .flatten()
            .map(&s)
            .collect();
        let sdk = if f["imports_ui_framework"] == json!(true) {
            "  [needs the iOS SDK]"
        } else {
            ""
        };
        let _ = writeln!(out, "  {}: {}{sdk}", s(&f["path"]), imports.join(", "));
    }
    let ids = d["accessibility_identifiers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let _ = writeln!(out, "\nAccessibility identifiers ({}):", ids.len());
    for i in &ids {
        let star = if i["prefix"] == json!(true) { "*" } else { "" };
        let _ = writeln!(out, "  {}{star}  {}", s(&i["id"]), s(&i["file"]));
    }
    let _ = writeln!(out, "  ({})", s(&d["identifiers_note"]));
    let tests: Vec<String> = d["ui_tests"]
        .as_array()
        .into_iter()
        .flatten()
        .map(&s)
        .collect();
    let _ = writeln!(
        out,
        "\nUI tests: {}\nScreenshot baselines: {}\nrequirements.toml: {}",
        if tests.is_empty() {
            "none".into()
        } else {
            tests.join(", ")
        },
        d["screenshot_baselines"],
        if d["requirements"].is_null() {
            "none"
        } else {
            "yes"
        }
    );
    let _ = writeln!(
        out,
        "Generated files: {} ({})",
        d["generated"]["files"].as_array().map_or(0, Vec::len),
        s(&d["generated"]["note"])
    );
    let agents: Vec<String> = d["agent_files"]["present"]
        .as_array()
        .into_iter()
        .flatten()
        .map(&s)
        .collect();
    let _ = writeln!(
        out,
        "Agent files: {} ({})",
        if agents.is_empty() {
            "none".into()
        } else {
            agents.join(", ")
        },
        s(&d["agent_files"]["note"])
    );
    let warnings = d["convention_warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if !warnings.is_empty() {
        let _ = writeln!(out, "\nConvention warnings:");
        for w in &warnings {
            let _ = writeln!(out, "  - {}", s(w));
        }
    }
    if !d["ios_config"].is_null() {
        let _ = writeln!(
            out,
            "\nThe iOS settings (bundle, permissions, entitlements, signing): forge config show"
        );
    }
    out
}

fn rel(app: &Path, path: &Path) -> String {
    path.strip_prefix(app).unwrap_or(path).display().to_string()
}

fn files_under(dir: &Path, ext: &str) -> Vec<PathBuf> {
    all_files(dir)
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect()
}

fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            out.extend(all_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORGE_TOML: &str = "[app]\nname = \"Tipster\"\nbundle_id = \"org.example.tipster\"\nminimum_ios = \"18.0\"\n\n[rust]\npath = \"rust\"\n\n[build]\nios_device = true\n";

    fn app() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Sources/AppHost");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(tmp.path().join("forge.toml"), FORGE_TOML).unwrap();
        std::fs::write(
            src.join("TipModel.swift"),
            "import Foundation\nimport Observation\nimport ForgeFFI\n",
        )
        .unwrap();
        std::fs::write(
            src.join("TipView.swift"),
            "import SwiftUI\nText(\"x\").accessibilityIdentifier(\"tip.total\")\nButton {}.accessibilityIdentifier(\"tip.15\")\nRow().accessibilityIdentifier(\"tip.row.\\(id)\")\n",
        )
        .unwrap();
        std::fs::write(src.join("BadModel.swift"), "import SwiftUI\n").unwrap();
        std::fs::create_dir_all(tmp.path().join("uitests")).unwrap();
        std::fs::write(tmp.path().join("uitests/smoke.toml"), "").unwrap();
        std::fs::create_dir_all(tmp.path().join("Generated")).unwrap();
        std::fs::write(tmp.path().join("Generated/core.swift"), "").unwrap();
        tmp
    }

    #[test]
    fn reports_facts_not_guesses() {
        let tmp = app();
        let d = scan(tmp.path()).unwrap();
        // Imports are reported; no file is called a view or a model.
        let files = d["swift_files"].as_array().unwrap();
        assert!(files.iter().all(|f| f.get("kind").is_none()));
        let view = files
            .iter()
            .find(|f| f["path"] == "Sources/AppHost/TipView.swift")
            .unwrap();
        assert_eq!(view["imports_ui_framework"], json!(true));
        let model = files
            .iter()
            .find(|f| f["path"] == "Sources/AppHost/TipModel.swift")
            .unwrap();
        assert_eq!(model["imports_ui_framework"], json!(false));
        // Literal identifiers, and an interpolated one as its prefix.
        let ids: Vec<(String, bool)> = d["accessibility_identifiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| (i["id"].as_str().unwrap().into(), i["prefix"] == json!(true)))
            .collect();
        assert!(ids.contains(&("tip.total".into(), false)));
        assert!(ids.contains(&("tip.15".into(), false)));
        assert!(ids.contains(&("tip.row.".into(), true)));
        assert!(d["identifiers_note"]
            .as_str()
            .unwrap()
            .contains("not listed"));
        assert_eq!(d["ui_tests"], json!(["uitests/smoke.toml"]));
        assert_eq!(d["generated"]["files"], json!(["Generated/core.swift"]));
        assert_eq!(d["convention_warnings"].as_array().unwrap().len(), 1);
        assert_eq!(d["agent_files"]["present"], json!([]));
        assert_eq!(d["rust_crate"]["path"], "rust");
    }

    #[test]
    fn lists_agent_files_as_optional() {
        let tmp = app();
        std::fs::write(tmp.path().join("AGENTS.md"), "").unwrap();
        let d = scan(tmp.path()).unwrap();
        assert_eq!(d["agent_files"]["present"], json!(["AGENTS.md"]));
        let text = show(&d);
        assert!(text.contains("Agent files: AGENTS.md (Optional"), "{text}");
        assert!(text.contains("tip.row.*"), "{text}");
        assert!(text.contains("[needs the iOS SDK]"), "{text}");
    }

    #[test]
    fn needs_a_forge_app() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(scan(tmp.path()).unwrap_err().contains("--project"));
    }
}
