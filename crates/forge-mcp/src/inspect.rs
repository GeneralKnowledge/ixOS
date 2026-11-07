//! The file side of `forge_inspect_app`: which Swift files are model and
//! which are views, which accessibility identifiers the app uses, and its UI
//! test files. Reading only.

use std::path::Path;

use serde_json::{json, Map, Value};

/// UI frameworks: a file importing one of these is a view file and does not
/// compile on Linux.
const UI_FRAMEWORKS: &[&str] = &["SwiftUI", "UIKit"];

pub fn scan(app_dir: &Path) -> Map<String, Value> {
    let mut swift = Vec::new();
    let mut identifiers = Vec::new();
    let mut misplaced = Vec::new();
    walk(&app_dir.join("Sources"), &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("swift") {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let imports: Vec<String> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("import "))
            .map(|m| m.trim().to_string())
            .collect();
        let view = imports.iter().any(|m| UI_FRAMEWORKS.contains(&m.as_str()));
        let rel = path
            .strip_prefix(app_dir)
            .unwrap_or(path)
            .display()
            .to_string();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if view && name.ends_with("Model.swift") {
            misplaced.push(format!(
                "{rel} imports a UI framework, so it can't be checked on Linux: keep models to Foundation, Observation and ForgeFFI"
            ));
        }
        for id in identifiers_in(&text) {
            identifiers.push(json!({"id": id, "file": rel}));
        }
        swift.push(
            json!({"path": rel, "kind": if view { "view" } else { "model" }, "imports": imports}),
        );
    });
    let mut tests = Vec::new();
    walk(&app_dir.join("uitests"), &mut |path| {
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            tests.push(json!(path
                .strip_prefix(app_dir)
                .unwrap_or(path)
                .display()
                .to_string()));
        }
    });
    let mut out = Map::new();
    out.insert("swift_files".into(), json!(swift));
    out.insert("accessibility_identifiers".into(), json!(identifiers));
    out.insert("ui_tests".into(), json!(tests));
    out.insert("convention_warnings".into(), json!(misplaced));
    out
}

/// The string literals passed to `.accessibilityIdentifier(…)`.
fn identifiers_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let needle = ".accessibilityIdentifier(\"";
    let mut rest = text;
    while let Some(i) = rest.find(needle) {
        rest = &rest[i + needle.len()..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        }
    }
    out
}

fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swift_files_identifiers_and_tests() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Sources/AppHost");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("TipModel.swift"),
            "import Foundation\nimport Observation\nimport ForgeFFI\n",
        )
        .unwrap();
        std::fs::write(
            src.join("TipView.swift"),
            "import SwiftUI\nText(\"x\").accessibilityIdentifier(\"tip.total\")\nButton {}.accessibilityIdentifier(\"tip.15\")\n",
        )
        .unwrap();
        std::fs::write(src.join("BadModel.swift"), "import SwiftUI\n").unwrap();
        std::fs::create_dir_all(tmp.path().join("uitests")).unwrap();
        std::fs::write(tmp.path().join("uitests/smoke.toml"), "").unwrap();
        let out = scan(tmp.path());
        let kinds: Vec<_> = out["swift_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_string(),
                    f["kind"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert!(kinds.contains(&("Sources/AppHost/TipModel.swift".into(), "model".into())));
        assert!(kinds.contains(&("Sources/AppHost/TipView.swift".into(), "view".into())));
        let ids: Vec<_> = out["accessibility_identifiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, vec!["tip.total", "tip.15"]);
        assert_eq!(out["ui_tests"], json!(["uitests/smoke.toml"]));
        assert_eq!(out["convention_warnings"].as_array().unwrap().len(), 1);
    }
}
