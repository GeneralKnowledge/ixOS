//! `forge check swift`: compiles the app's Swift that needs no Apple UI
//! framework, and runs the tests that need none, with the plain Swift
//! toolchain on Linux. No iOS SDK, no Mac, no phone (dev-loop gap DL1).
//!
//! The view model and the UniFFI bindings import only Foundation and
//! Observation, which every Swift toolchain has, so a mistake at the
//! Swift–Rust boundary (a renamed method, a changed record) shows up here in
//! seconds instead of after an xtool build or macOS CI. SwiftUI views are left
//! out; they still need `forge build ios`.
//!
//! How: build the app's Rust crate for this computer, then lay out a small
//! SwiftPM package in `.forge/swift-check/` whose targets are links to the
//! app's own files (the ones that import nothing Apple-only), and run
//! `swift test` (or `swift build` when no test qualifies) in it. Compiler
//! messages are shown with the app's paths, not the links'.

use std::fs;
use std::path::{Path, PathBuf};

use crate::runner::{Cmd, CommandRunner};

/// Modules every Swift toolchain ships, plus the app's own. A file that
/// imports anything else (SwiftUI, UIKit, Security, …) outside an `#if` is
/// left out.
const PORTABLE_MODULES: &[&str] = &[
    "Foundation",
    "FoundationEssentials",
    "FoundationNetworking",
    "Observation",
    "Dispatch",
    "Swift",
    "_Concurrency",
    "Synchronization",
    "RegexBuilder",
    "XCTest",
    "Testing",
    "Glibc",
    "Musl",
    "ForgeFFI",
    "forge_ffiFFI",
    "AppHost",
];

/// The UniFFI bindings. The other files in `Generated/ForgeFFI` are the
/// native services' Swift adapters, which wrap Apple frameworks.
const BINDINGS_FILE: &str = "forge_ffi.swift";

/// Portable helpers copied next to the bindings when the app's crate opts
/// in (`forge_runtime::log_export!()` copies `ForgeLog.swift`).
const PORTABLE_HELPERS: &[&str] = &["ForgeLog.swift"];

const STAGE: &str = ".forge/swift-check";

/// The modules a Swift file imports outside `#if` blocks.
pub fn unconditional_imports(source: &str) -> Vec<String> {
    let mut depth = 0usize;
    let mut modules = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("#if") {
            depth += 1;
            continue;
        }
        if line.starts_with("#endif") {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth > 0 {
            continue;
        }
        // `@preconcurrency import X`, `@testable import X`, `import struct X.Y`.
        let mut words = line.split_whitespace().skip_while(|w| w.starts_with('@'));
        if words.next() != Some("import") {
            continue;
        }
        let mut name = words.next().unwrap_or("");
        if matches!(
            name,
            "struct" | "class" | "enum" | "protocol" | "typealias" | "func" | "var" | "let"
        ) {
            name = words.next().unwrap_or("");
        }
        let module = name.split('.').next().unwrap_or("");
        if !module.is_empty() {
            modules.push(module.to_string());
        }
    }
    modules
}

/// Whether a Swift file can build without Apple's frameworks.
pub fn is_portable(source: &str) -> bool {
    unconditional_imports(source)
        .iter()
        .all(|m| PORTABLE_MODULES.contains(&m.as_str()))
}

/// The `.swift` files directly in `dir` that [`is_portable`], sorted.
fn portable_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "swift"))
        .filter(|p| fs::read_to_string(p).is_ok_and(|s| is_portable(&s)))
        .collect();
    files.sort();
    files
}

/// What goes into the staging package.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub sources: Vec<PathBuf>,
    pub tests: Vec<PathBuf>,
    /// Swift files left out because they import an Apple-only module.
    pub skipped: Vec<PathBuf>,
}

pub fn select(app: &Path) -> Selection {
    let mut sel = Selection::default();
    for (dir, into_tests) in [
        (app.join("Sources/AppHost"), false),
        (app.join("Tests/AppHostTests"), true),
    ] {
        let portable = portable_files(&dir);
        if let Ok(entries) = fs::read_dir(&dir) {
            let mut skipped: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "swift"))
                .filter(|p| !portable.contains(p))
                .collect();
            skipped.sort();
            sel.skipped.extend(skipped);
        }
        if into_tests {
            sel.tests = portable;
        } else {
            sel.sources = portable;
        }
    }
    sel
}

/// The staging package's manifest.
fn manifest(lib_dir: &Path, with_tests: bool) -> String {
    let tests = if with_tests {
        r#"
        .testTarget(name: "AppHostTests", dependencies: ["AppHost", "ForgeFFI"], path: "Tests/AppHostTests"),"#
    } else {
        ""
    };
    format!(
        r#"// swift-tools-version: 6.0
// Written by `forge check swift`; every source is a link to the app's own file.
import PackageDescription

let package = Package(
    name: "ForgeSwiftCheck",
    targets: [
        .systemLibrary(name: "forge_ffiFFI", path: "Generated/forge_ffiFFI"),
        .target(
            name: "ForgeFFI",
            dependencies: ["forge_ffiFFI"],
            path: "Generated/ForgeFFI",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [.linkedLibrary("forge_ffi"), .unsafeFlags(["-L", "{lib}"])]
        ),
        .target(name: "AppHost", dependencies: ["ForgeFFI"], path: "Sources/AppHost"),{tests}
    ]
)
"#,
        lib = lib_dir.display(),
    )
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(not(unix))]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::copy(from, to).map(|_| ())
}

fn io<'a>(what: &'a str, path: &'a Path) -> impl FnOnce(std::io::Error) -> String + 'a {
    move |e| format!("{what} {}: {e}", path.display())
}

/// Writes `.forge/swift-check/` for `app`: links to the selected files and
/// the C header, a copy (hard link where possible) of the host library, and
/// the manifest. Returns the staging directory.
pub fn stage(app: &Path, sel: &Selection, host_lib: &Path) -> Result<PathBuf, String> {
    let st = app.join(STAGE);
    if st.exists() {
        // Keep SwiftPM's .build cache between runs; replace everything else.
        for entry in fs::read_dir(&st).map_err(io("read", &st))?.flatten() {
            if entry.file_name() == ".build" {
                continue;
            }
            let p = entry.path();
            if p.is_dir() && !p.is_symlink() {
                fs::remove_dir_all(&p).map_err(io("remove", &p))?;
            } else {
                fs::remove_file(&p).map_err(io("remove", &p))?;
            }
        }
    }
    let gen = st.join("Generated");
    for dir in [
        gen.join("ForgeFFI"),
        st.join("Sources/AppHost"),
        st.join("Tests/AppHostTests"),
        st.join("lib"),
    ] {
        fs::create_dir_all(&dir).map_err(io("create", &dir))?;
    }
    link(
        &app.join("Generated/forge_ffiFFI"),
        &gen.join("forge_ffiFFI"),
    )
    .map_err(io("link", &gen))?;
    let bindings = app.join("Generated/ForgeFFI").join(BINDINGS_FILE);
    if !bindings.is_file() {
        return Err(format!(
            "{} is missing: run `forge bindings generate`",
            bindings.display()
        ));
    }
    link(&bindings, &gen.join("ForgeFFI").join(BINDINGS_FILE)).map_err(io("link", &bindings))?;
    for name in PORTABLE_HELPERS {
        let helper = app.join("Generated/ForgeFFI").join(name);
        if helper.is_file() {
            link(&helper, &gen.join("ForgeFFI").join(name)).map_err(io("link", &helper))?;
        }
    }
    for (files, dir) in [
        (&sel.sources, st.join("Sources/AppHost")),
        (&sel.tests, st.join("Tests/AppHostTests")),
    ] {
        for f in files {
            let name = f.file_name().unwrap_or_default();
            link(f, &dir.join(name)).map_err(io("link", f))?;
        }
    }
    let lib = st.join("lib").join("libforge_ffi.a");
    if fs::hard_link(host_lib, &lib).is_err() {
        fs::copy(host_lib, &lib).map_err(io("copy", host_lib))?;
    }
    fs::write(
        st.join("Package.swift"),
        manifest(&st.join("lib"), !sel.tests.is_empty()),
    )
    .map_err(io("write", &st))?;
    Ok(st)
}

/// Compiler output names the staging links; point it back at the app's files.
pub fn unstage_paths(output: &str, app: &Path) -> String {
    let st = app.join(STAGE);
    output
        .replace(
            &format!("{}/Sources/", st.display()),
            &format!("{}/Sources/", app.display()),
        )
        .replace(
            &format!("{}/Tests/", st.display()),
            &format!("{}/Tests/", app.display()),
        )
        .replace(
            &format!("{}/Generated/", st.display()),
            &format!("{}/Generated/", app.display()),
        )
}

/// The Swift command: `swift` on this computer, or the same inside a Docker
/// image when `FORGE_SWIFT_DOCKER` names one (for machines without Swift).
fn swift_cmd(st: &Path, app: &Path, args: &[&str]) -> Cmd {
    match std::env::var("FORGE_SWIFT_DOCKER") {
        Ok(image) if !image.is_empty() => {
            let mount = format!("{0}:{0}", app.display());
            Cmd::new("docker")
                .args(["run", "--rm", "-v", &mount, "-w"])
                .arg(st)
                .arg(image)
                .arg("swift")
                .args(args)
        }
        _ => Cmd::new("swift").args(args).cwd(st),
    }
}

/// The host build of the app's Rust library: `<target>/debug/libforge_ffi.a`.
fn host_library(runner: &dyn CommandRunner, app: &Path) -> Result<PathBuf, String> {
    let manifest = app.join("rust/Cargo.toml");
    crate::commands::run_explained(
        runner,
        Cmd::new("cargo")
            .args(["build", "--lib", "--manifest-path"])
            .arg(&manifest),
    )?;
    let meta = runner.capture(
        &Cmd::new("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(&manifest),
    )?;
    let target = serde_json::from_slice::<serde_json::Value>(&meta.stdout)
        .ok()
        .and_then(|v| v["target_directory"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| app.join("rust/target"));
    let lib = target.join("debug").join("libforge_ffi.a");
    if !lib.is_file() {
        return Err(format!(
            "{} was not built; the app's crate needs crate-type \"staticlib\"",
            lib.display()
        ));
    }
    Ok(lib)
}

/// `forge check swift`
pub fn check_swift(runner: &dyn CommandRunner, root: &Path, project: &Path) -> Result<(), String> {
    let (app, _cfg) = crate::commands::ios_project(root, project)?;
    let app = app.canonicalize().unwrap_or(app);
    if std::env::var_os("FORGE_SWIFT_DOCKER").is_none()
        && !crate::commands::which_program(runner, "swift")
    {
        return Err(
            "Swift is not installed. Install Swift 6.2 for Linux (docs/linux-setup.md), or set FORGE_SWIFT_DOCKER=swift:6.2 to run it in Docker"
                .into(),
        );
    }
    // The bindings must match the Rust being checked.
    crate::commands::bindings_generate_for_project(runner, root, &app)?;
    let lib = host_library(runner, &app)?;
    let sel = select(&app);
    if sel.sources.is_empty() {
        return Err("no Swift file in Sources/AppHost builds without SwiftUI or UIKit; keep the view model in its own file that imports only Foundation and Observation (docs/ios-ui.md)".into());
    }
    let st = stage(&app, &sel, &lib)?;
    let rel = |p: &PathBuf| p.strip_prefix(&app).unwrap_or(p).display().to_string();
    println!(
        "[forge] Checking {} Swift file(s) and {} test file(s) without the iOS SDK; left out (they need Apple's frameworks): {}",
        sel.sources.len(),
        sel.tests.len(),
        if sel.skipped.is_empty() {
            "none".to_string()
        } else {
            sel.skipped.iter().map(rel).collect::<Vec<_>>().join(", ")
        }
    );
    crate::report::set_data(serde_json::json!({
        "sources": sel.sources.iter().map(rel).collect::<Vec<_>>(),
        "tests": sel.tests.iter().map(rel).collect::<Vec<_>>(),
        "skipped": sel.skipped.iter().map(rel).collect::<Vec<_>>(),
    }));
    let args: &[&str] = if sel.tests.is_empty() {
        &["build"]
    } else {
        &["test"]
    };
    let cmd = swift_cmd(&st, &app, args);
    let out = runner.capture(&cmd)?;
    let text = unstage_paths(
        &format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        &app,
    );
    crate::report::log_bytes(format!("$ {}\n{text}", cmd.display()).as_bytes());
    print!("{text}");
    if out.status.success() {
        println!("[forge] Swift check passed (SwiftUI views still need `forge build ios`).");
        Ok(())
    } else {
        Err(crate::diagnose::explain(
            &format!("swift {}", args[0]),
            out.status,
            &text,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_outside_if_blocks_decide() {
        let model = "import Foundation\nimport Observation\n@preconcurrency import ForgeFFI\n";
        assert!(is_portable(model));
        assert!(!is_portable("import Foundation\nimport SwiftUI\n"));
        assert!(!is_portable("@testable import AppHost\nimport UIKit\n"));
        // Guarded imports don't count.
        assert!(is_portable(
            "import Foundation\n#if canImport(UIKit)\nimport UIKit\n#endif\n"
        ));
        // A whole file inside #if canImport(Security) is portable (it compiles to nothing).
        assert!(is_portable(
            "#if canImport(Security)\nimport Security\nimport XCTest\n#endif\n"
        ));
        assert_eq!(
            unconditional_imports("import struct Foundation.URL\nimport os.log\n"),
            ["Foundation", "os"]
        );
    }

    #[test]
    fn stages_links_and_points_messages_back_at_the_app() {
        let app = tempfile::tempdir().unwrap();
        let app = app.path();
        for (path, text) in [
            ("Sources/AppHost/NotesModel.swift", "import Foundation\n"),
            ("Sources/AppHost/ContentView.swift", "import SwiftUI\n"),
            (
                "Tests/AppHostTests/NotesModelTests.swift",
                "import XCTest\n",
            ),
            (
                "Tests/AppHostTests/LifecycleTests.swift",
                "import SwiftUI\n",
            ),
            ("Generated/ForgeFFI/forge_ffi.swift", "import Foundation\n"),
            (
                "Generated/ForgeFFI/ForgeServices.swift",
                "import Foundation\n",
            ),
            (
                "Generated/forge_ffiFFI/module.modulemap",
                "module forge_ffiFFI {}\n",
            ),
            ("lib/libforge_ffi.a", "!<arch>\n"),
        ] {
            let p = app.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }
        let sel = select(app);
        assert_eq!(sel.sources, [app.join("Sources/AppHost/NotesModel.swift")]);
        assert_eq!(
            sel.tests,
            [app.join("Tests/AppHostTests/NotesModelTests.swift")]
        );
        assert_eq!(sel.skipped.len(), 2);

        let st = stage(app, &sel, &app.join("lib/libforge_ffi.a")).unwrap();
        assert!(st.join("Sources/AppHost/NotesModel.swift").is_file());
        assert!(!st.join("Sources/AppHost/ContentView.swift").exists());
        // Only the bindings, not the Apple-only service adapters.
        assert!(st.join("Generated/ForgeFFI/forge_ffi.swift").is_file());
        assert!(!st.join("Generated/ForgeFFI/ForgeServices.swift").exists());
        assert!(st.join("Generated/forge_ffiFFI/module.modulemap").is_file());
        let manifest = fs::read_to_string(st.join("Package.swift")).unwrap();
        assert!(manifest.contains("testTarget"), "{manifest}");
        // Staging again replaces it cleanly.
        stage(app, &sel, &app.join("lib/libforge_ffi.a")).unwrap();

        let msg = format!(
            "{}/Sources/AppHost/NotesModel.swift:3:5: error: nope",
            st.display()
        );
        assert_eq!(
            unstage_paths(&msg, app),
            format!(
                "{}/Sources/AppHost/NotesModel.swift:3:5: error: nope",
                app.display()
            )
        );
    }
}
