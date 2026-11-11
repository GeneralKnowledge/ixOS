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

use crate::evidence;
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
    // The app's own modules (bindings, C module, views and models) are
    // added from its forge.toml: see `Modules`.
    // The ixOS runtime's Swift package (forgekit/); it builds on Linux.
    "ForgeKit",
    "ForgeApp",
    // Its design system and application patterns: their model values
    // import only Foundation and Observation, and their SwiftUI is behind
    // `#if canImport(SwiftUI)`.
    "ForgeUI",
    "ForgeAppKit",
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

/// The app's own Swift module names: its bindings, their C module, and its
/// views and models. Forge's fixed names unless forge.toml's `[swift]` names
/// its own (an app another app hosts, docs/app-crate.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modules {
    pub bindings: String,
    pub ffi: String,
    pub app: String,
}

impl Default for Modules {
    fn default() -> Self {
        Modules {
            bindings: crate::config::DEFAULT_BINDINGS_MODULE.into(),
            ffi: "forge_ffiFFI".into(),
            app: crate::config::DEFAULT_APP_MODULE.into(),
        }
    }
}

impl Modules {
    /// The names in `cfg`, and the C module's from the app's generated
    /// module map (`module <name> {`).
    pub fn of(app: &Path, cfg: &crate::config::ForgeConfig) -> Self {
        let map = fs::read_to_string(app.join("Generated/forge_ffiFFI/module.modulemap"))
            .unwrap_or_default();
        let ffi = map
            .lines()
            .find_map(|l| l.trim().strip_prefix("module ")?.split_whitespace().next())
            .unwrap_or("forge_ffiFFI")
            .to_string();
        Modules {
            bindings: cfg.bindings_module().into(),
            ffi,
            app: cfg.app_module().into(),
        }
    }

    fn is_own(&self, module: &str) -> bool {
        [&self.bindings, &self.ffi, &self.app]
            .iter()
            .any(|m| m.as_str() == module)
    }
}

/// Whether a Swift file can build without Apple's frameworks.
pub fn is_portable(source: &str, modules: &Modules) -> bool {
    unconditional_imports(source)
        .iter()
        .all(|m| PORTABLE_MODULES.contains(&m.as_str()) || modules.is_own(m))
}

/// The `.swift` files directly in `dir` that [`is_portable`], sorted.
fn portable_files(dir: &Path, modules: &Modules) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "swift"))
        .filter(|p| fs::read_to_string(p).is_ok_and(|s| is_portable(&s, modules)))
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

pub fn select(app: &Path, modules: &Modules) -> Selection {
    let mut sel = Selection::default();
    for (dir, into_tests) in [
        (app.join("Sources/AppHost"), false),
        (app.join("Tests/AppHostTests"), true),
    ] {
        let portable = portable_files(&dir, modules);
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

/// The app's ForgeKit dependency, as its Package.swift declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeKit {
    /// `.package(path: …)`, made absolute: the staging package is elsewhere,
    /// and Docker must mount it.
    Path(PathBuf),
    /// `.package(url: …, exact: …)` (or any other requirement), copied as is.
    Remote(String),
}

impl ForgeKit {
    fn package_line(&self) -> String {
        match self {
            ForgeKit::Path(p) => format!(".package(path: \"{}\")", p.display()),
            ForgeKit::Remote(line) => line.clone(),
        }
    }
}

/// Finds `.package(path: ".../forgekit")` or `.package(url: ".../ForgeKit.git", …)`
/// in the app's Package.swift. `None` for an app that predates ForgeKit.
pub fn forgekit_dependency(app: &Path) -> Option<ForgeKit> {
    let text = fs::read_to_string(app.join("Package.swift")).ok()?;
    for line in text.lines() {
        let code = line.trim();
        if code.starts_with("//") {
            continue;
        }
        let Some(start) = code.find(".package(") else {
            continue;
        };
        let code = &code[start..];
        let quoted = code.split('"').nth(1)?;
        let named = quoted
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .rsplit('/')
            .next()
            .is_some_and(|n| n.eq_ignore_ascii_case("forgekit"));
        if !named {
            continue;
        }
        if code.starts_with(".package(path:") {
            let p = app.join(quoted);
            return Some(ForgeKit::Path(p.canonicalize().unwrap_or(p)));
        }
        let end = code.rfind(')')?;
        return Some(ForgeKit::Remote(code[..=end].to_string()));
    }
    None
}

/// The ForgeKit package's products the app's Package.swift uses. Only those
/// go into the staging package: an app pinned to a release without ForgeUI
/// still checks, and an app on ForgeUI alone (no runtime) links no ForgeApp
/// bindings its Rust library doesn't implement.
pub fn forgekit_products(app: &Path) -> Vec<&'static str> {
    let manifest = fs::read_to_string(app.join("Package.swift")).unwrap_or_default();
    ["ForgeApp", "ForgeKit", "ForgeUI", "ForgeAppKit"]
        .into_iter()
        .filter(|p| manifest.contains(&format!("\"{p}\"")))
        .collect()
}

/// The staging package's manifest.
fn manifest(
    lib_dir: &Path,
    with_tests: bool,
    forgekit: Option<&ForgeKit>,
    products: &[&str],
    modules: &Modules,
) -> String {
    let Modules { bindings, ffi, app } = modules;
    let product = |name: &str| format!(r#", .product(name: "{name}", package: "forgekit")"#);
    let (deps, ffi_dep, kit_dep) = match forgekit {
        Some(kit) => {
            // The bindings use the runtime's contract types (ForgeApp); the
            // app's own code uses the rest.
            let ffi_dep = if products.contains(&"ForgeApp") {
                product("ForgeApp")
            } else {
                String::new()
            };
            let kit_dep: String = products
                .iter()
                .filter(|p| **p != "ForgeApp")
                .map(|p| product(p))
                .collect();
            (
                format!("\n    dependencies: [{}],", kit.package_line()),
                ffi_dep,
                kit_dep,
            )
        }
        None => (String::new(), String::new(), String::new()),
    };
    let tests = if with_tests {
        format!(
            r#"
        .testTarget(name: "AppHostTests", dependencies: ["{app}", "{bindings}"{kit_dep}], path: "Tests/AppHostTests"),"#
        )
    } else {
        String::new()
    };
    format!(
        r#"// swift-tools-version: 6.0
// Written by `forge check swift`; every source is a link to the app's own file.
import PackageDescription

let package = Package(
    name: "ForgeSwiftCheck",
    platforms: [.macOS(.v14)],{deps}
    targets: [
        .systemLibrary(name: "{ffi}", path: "Generated/forge_ffiFFI"),
        .target(
            name: "{bindings}",
            dependencies: ["{ffi}"{ffi_dep}],
            path: "Generated/ForgeFFI",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [.linkedLibrary("forge_ffi"), .unsafeFlags(["-L", "{lib}"])]
        ),
        .target(name: "{app}", dependencies: ["{bindings}"{kit_dep}], path: "Sources/AppHost"),{tests}
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
pub fn stage(
    app: &Path,
    sel: &Selection,
    host_lib: &Path,
    modules: &Modules,
) -> Result<PathBuf, String> {
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
        manifest(
            &st.join("lib"),
            !sel.tests.is_empty(),
            forgekit_dependency(app).as_ref(),
            &forgekit_products(app),
            modules,
        ),
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
            let mut cmd = Cmd::new("docker").args(["run", "--rm"]);
            for dir in docker_mounts(app) {
                cmd = cmd.arg("-v").arg(format!("{0}:{0}", dir.display()));
            }
            cmd.arg("-w").arg(st).arg(image).arg("swift").args(args)
        }
        _ => Cmd::new("swift").args(args).cwd(st),
    }
}

/// What the container sees: the app, and a ForgeKit outside it.
fn docker_mounts(app: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![app.to_path_buf()];
    if let Some(ForgeKit::Path(kit)) = forgekit_dependency(app) {
        if !kit.starts_with(app) {
            dirs.push(kit);
        }
    }
    dirs
}

/// `swiftc -parse` of `files` (syntax only, no SDK), on this computer or in
/// the `FORGE_SWIFT_DOCKER` image.
fn swiftc_parse(app: &Path, files: &[&PathBuf]) -> Cmd {
    let cmd = match std::env::var("FORGE_SWIFT_DOCKER") {
        Ok(image) if !image.is_empty() => {
            let mount = format!("{0}:{0}", app.display());
            Cmd::new("docker")
                .args(["run", "--rm", "-v", &mount])
                .arg(image)
                .arg("swiftc")
        }
        _ => Cmd::new("swiftc"),
    };
    let mut cmd = cmd.arg("-parse");
    for f in files {
        cmd = cmd.arg(f.as_path());
    }
    cmd
}

/// With `FORGE_SWIFT_DOCKER`, Docker must be running, or the check fails
/// with a message that doesn't say why (Good Dog pain point 10).
fn docker_running(runner: &dyn CommandRunner) -> Result<(), String> {
    let ok = runner
        .capture(&Cmd::new("docker").args(["info", "--format", "{{.ServerVersion}}"]))
        .is_ok_and(|o| o.status.success());
    if ok {
        return Ok(());
    }
    let problem = "FORGE_SWIFT_DOCKER is set, but Docker isn't running (or isn't installed).";
    let fix = "Start it (`sudo systemctl start docker`, or `sudo dockerd &` where there is no systemd), or install Swift 6.2 and unset FORGE_SWIFT_DOCKER.";
    crate::report::record_failure(
        "docker info",
        vec![crate::report::JsonHint {
            stage: "test",
            problem: problem.into(),
            fix: fix.into(),
        }],
        "",
    );
    Err(format!("{problem}\n  Fix: {fix}"))
}

/// The name of the app crate's library (`[lib] name`): `forge_ffi` in most
/// apps, the app's own (`weather_core`, ...) in one another app links. The
/// staged copy is always `libforge_ffi.a`.
fn library_name(metadata: &serde_json::Value) -> String {
    metadata["packages"]
        .as_array()
        .and_then(|packages| packages.first())
        .and_then(|package| package["targets"].as_array())
        .and_then(|targets| {
            targets.iter().find(|t| {
                t["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|k| k == "staticlib" || k == "lib"))
            })
        })
        .and_then(|t| t["name"].as_str())
        .map_or_else(|| "forge_ffi".to_string(), |name| name.replace('-', "_"))
}

/// The host build of the app's Rust library: `<target>/debug/lib<name>.a`.
fn host_library(runner: &dyn CommandRunner, app: &Path) -> Result<PathBuf, String> {
    let manifest = app.join("rust/Cargo.toml");
    // Run from the crate's folder: rustup picks the toolchain from the
    // working directory (rust/rust-toolchain.toml), not from --manifest-path,
    // so from the app folder it fails where no default toolchain is set.
    let crate_dir = app.join("rust");
    crate::commands::run_explained(
        runner,
        Cmd::new("cargo")
            .args(["build", "--lib", "--manifest-path"])
            .arg(&manifest)
            .cwd(&crate_dir),
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
            .arg(&manifest)
            .cwd(&crate_dir),
    )?;
    let meta = serde_json::from_slice::<serde_json::Value>(&meta.stdout).ok();
    let target = meta
        .as_ref()
        .and_then(|v| v["target_directory"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| app.join("rust/target"));
    let lib = target.join("debug").join(format!(
        "lib{}.a",
        meta.as_ref()
            .map_or_else(|| "forge_ffi".to_string(), library_name)
    ));
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
    let (app, cfg) = crate::commands::ios_project(root, project)?;
    let app = app.canonicalize().unwrap_or(app);
    // The bindings must match the Rust being checked. Generated first, so a
    // machine without Swift still leaves Generated/ matching the Rust API.
    crate::commands::bindings_generate_for_project(runner, root, &app)?;
    if std::env::var_os("FORGE_SWIFT_DOCKER").is_none()
        && !crate::commands::which_program(runner, "swift")
    {
        return Err(
            "Swift is not installed. Install Swift 6.2 for Linux (docs/linux-setup.md), or set FORGE_SWIFT_DOCKER=swift:6.2 to run it in Docker"
                .into(),
        );
    }
    if let Ok(image) = std::env::var("FORGE_SWIFT_DOCKER") {
        if !image.is_empty() {
            docker_running(runner)?;
        }
    }
    let lib = host_library(runner, &app)?;
    let modules = Modules::of(&app, &cfg);
    let sel = select(&app, &modules);
    if sel.sources.is_empty() {
        return Err("no Swift file in Sources/AppHost builds without SwiftUI or UIKit; keep the view model in its own file that imports only Foundation and Observation (docs/ios-ui.md)".into());
    }
    let reqs = evidence::load_requirements(&app)?;
    let st = stage(&app, &sel, &lib, &modules)?;
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
    let args: &[&str] = if sel.tests.is_empty() {
        &["build"]
    } else {
        &["test"]
    };
    let cmd = swift_cmd(&st, &app, args);
    let started = std::time::Instant::now();
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

    // The evidence: which tests ran, and what they prove (docs/evidence.md).
    let tests = evidence::parse_xctest(&text);
    let skipped_classes: Vec<String> = sel
        .skipped
        .iter()
        .filter(|p| p.starts_with(app.join("Tests")))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let verdicts = evidence::judge(&reqs, &tests, &skipped_classes);
    // No test ran (a compile error, or no portable test): nothing to record.
    let bundle = if tests.is_empty() {
        None
    } else {
        let run = evidence::Run {
            command: "check swift",
            kind: "linux-twin",
            app: &app,
            build: build_info(runner, &app),
            tests: &tests,
            requirements: &reqs,
            verdicts: &verdicts,
            raw_output: ("swift-test.log", &text),
            duration_ms: started.elapsed().as_millis() as u64,
        };
        Some(evidence::write_bundle(&run, &crate::report::timestamp())?)
    };
    let failed: Vec<&str> = tests
        .iter()
        .filter(|t| !t.passed)
        .map(|t| t.name.as_str())
        .collect();
    crate::report::set_data(serde_json::json!({
        "sources": sel.sources.iter().map(rel).collect::<Vec<_>>(),
        "tests": sel.tests.iter().map(rel).collect::<Vec<_>>(),
        "skipped": sel.skipped.iter().map(rel).collect::<Vec<_>>(),
        "results": { "passed": tests.len() - failed.len(), "failed": failed },
        "requirements": reqs.iter().zip(&verdicts).map(|(r, (v, _))| {
            serde_json::json!({ "id": r.id, "twin": v })
        }).collect::<Vec<_>>(),
        "evidence": bundle.as_ref().map(|b| b.join("evidence.json").display().to_string()),
    }));
    if !reqs.is_empty() {
        println!(
            "[forge] Requirements (headless twin on Linux; the iPhone column needs a device run):"
        );
        for (r, (v, notes)) in reqs.iter().zip(&verdicts) {
            let v = serde_json::to_value(v).unwrap_or_default();
            let mut line = format!(
                "  {:<8} {:<9} {}",
                r.id,
                v.as_str().unwrap_or(""),
                r.statement
            );
            if !notes.is_empty() {
                line.push_str(&format!(" ({})", notes.join("; ")));
            }
            println!("{line}");
        }
    }
    if let Some(b) = &bundle {
        println!("[forge] Evidence: {}", b.join("evidence.json").display());
    }

    if !failed.is_empty() {
        let msg = format!(
            "{} Swift test(s) failed: {}",
            failed.len(),
            failed.join(", ")
        );
        crate::report::record_failure(
            &format!("swift {}", args[0]),
            vec![crate::report::JsonHint {
                stage: "test",
                problem: msg.clone(),
                fix: "The test output above shows each failed assertion; the evidence bundle keeps it.".into(),
            }],
            &text,
        );
        return Err(msg);
    }
    if !out.status.success() {
        return Err(crate::diagnose::explain(
            &format!("swift {}", args[0]),
            out.status,
            &text,
        ));
    }
    // `swift test` that passed but whose results couldn't be read proved
    // nothing: never report that as a pass.
    if args[0] == "test" && tests.is_empty() {
        return Err(format!(
            "swift test finished, but no test result could be read from its output ({} test file(s) were compiled). Nothing was proved; the log has the output.",
            sel.tests.len()
        ));
    }
    // A cheap syntax check of the files Linux can't compile (SwiftUI views):
    // catches a stray brace now instead of 35 minutes later on macOS CI.
    // Type errors still need `forge build ios` (Good Dog pain point 9).
    let views: Vec<&PathBuf> = sel
        .skipped
        .iter()
        .filter(|p| p.starts_with(app.join("Sources")))
        .collect();
    if !views.is_empty() {
        let parse = swiftc_parse(&app, &views);
        let out = runner.capture(&parse)?;
        if !out.status.success() {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            crate::report::log_bytes(format!("$ {}\n{text}", parse.display()).as_bytes());
            print!("{text}");
            return Err(crate::diagnose::explain("swiftc -parse", out.status, &text));
        }
        println!(
            "[forge] Syntax-checked {} view file(s) (not type-checked: that needs the iOS SDK)",
            views.len()
        );
    }
    let missing: Vec<String> = reqs
        .iter()
        .zip(&verdicts)
        .filter(|(_, (v, _))| *v == evidence::Verdict::Missing)
        .map(|(r, (_, notes))| format!("{} ({})", r.id, notes.join("; ")))
        .collect();
    if !missing.is_empty() {
        let msg = format!(
            "{} names a test that did not run: {}",
            evidence::REQUIREMENTS_FILE,
            missing.join(", ")
        );
        crate::report::record_failure(
            "requirements",
            vec![crate::report::JsonHint {
                stage: "test",
                problem: msg.clone(),
                fix: "Point the requirement at the test's current `Class.method` name.".into(),
            }],
            "",
        );
        return Err(msg);
    }
    println!("[forge] Swift check passed (SwiftUI views still need `forge build ios`).");
    Ok(())
}

/// What the run was built from (evidence::provenance), plus the Swift that
/// ran the tests.
fn build_info(runner: &dyn CommandRunner, app: &Path) -> serde_json::Value {
    let mut build = evidence::provenance(runner, app);
    let swift = match std::env::var("FORGE_SWIFT_DOCKER") {
        Ok(image) if !image.is_empty() => Some(format!("docker {image}")),
        _ => runner
            .capture(&Cmd::new("swift").arg("--version"))
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .map(str::to_string)
            }),
    };
    build["swift"] = serde_json::json!(swift);
    build
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_docker_is_named() {
        let mock = crate::runner::MockRunner::default();
        mock.respond("docker info", 1, "");
        let err = docker_running(&mock).unwrap_err();
        assert!(err.contains("Docker isn't running"), "{err}");
        mock.respond("docker info", 0, "27.0.1\n");
        assert!(docker_running(&mock).is_ok());
    }

    #[test]
    fn view_files_are_parsed_not_compiled() {
        let a = PathBuf::from("/app/Sources/AppHost/TodayView.swift");
        let line = swiftc_parse(Path::new("/app"), &[&a]).display();
        assert!(
            line.contains("swiftc -parse /app/Sources/AppHost/TodayView.swift"),
            "{line}"
        );
    }

    #[test]
    fn imports_outside_if_blocks_decide() {
        let model = "import Foundation\nimport Observation\n@preconcurrency import ForgeFFI\n";
        assert!(is_portable(model, &Modules::default()));
        assert!(!is_portable(
            "import Foundation\nimport SwiftUI\n",
            &Modules::default()
        ));
        assert!(!is_portable(
            "@testable import AppHost\nimport UIKit\n",
            &Modules::default()
        ));
        // Guarded imports don't count.
        assert!(is_portable(
            "import Foundation\n#if canImport(UIKit)\nimport UIKit\n#endif\n",
            &Modules::default()
        ));
        // A whole file inside #if canImport(Security) is portable (it compiles to nothing).
        assert!(is_portable(
            "#if canImport(Security)\nimport Security\nimport XCTest\n#endif\n",
            &Modules::default()
        ));
        assert_eq!(
            unconditional_imports("import struct Foundation.URL\nimport os.log\n"),
            ["Foundation", "os"]
        );
    }

    #[test]
    fn the_apps_forgekit_dependency_reaches_the_staging_package() {
        let ws = tempfile::tempdir().unwrap();
        let ws = ws.path().canonicalize().unwrap();
        let app = ws.join("examples/App");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(ws.join("forgekit")).unwrap();
        let write = |text: &str| fs::write(app.join("Package.swift"), text).unwrap();

        write("// .package(url: \"x/ForgeKit.git\", exact: \"1.0.0\")\nlet p = 1\n");
        assert_eq!(forgekit_dependency(&app), None);

        write("    dependencies: [\n        .package(path: \"../../forgekit\"),\n    ],\n");
        let kit = forgekit_dependency(&app).unwrap();
        assert_eq!(kit, ForgeKit::Path(ws.join("forgekit")));
        assert_eq!(docker_mounts(&app), [app.clone(), ws.join("forgekit")]);

        let url =
            r#".package(url: "https://github.com/GeneralKnowledge/ForgeKit.git", exact: "0.3.0")"#;
        write(&format!("    dependencies: [{url}],\n"));
        let kit = forgekit_dependency(&app).unwrap();
        assert_eq!(kit, ForgeKit::Remote(url.into()));
        assert_eq!(docker_mounts(&app), vec![app.clone()]);

        let m = manifest(
            Path::new("/lib"),
            true,
            Some(&kit),
            &["ForgeApp", "ForgeKit"],
            &Modules::default(),
        );
        assert!(m.contains(&format!("dependencies: [{url}]")), "{m}");
        assert!(
            m.contains(r#"["forge_ffiFFI", .product(name: "ForgeApp", package: "forgekit")]"#),
            "{m}"
        );
        assert!(m.contains(r#"["ForgeFFI", .product(name: "ForgeKit", package: "forgekit")], path: "Sources/AppHost""#), "{m}");
        let plain = manifest(Path::new("/lib"), true, None, &[], &Modules::default());
        assert!(!plain.contains("forgekit"), "{plain}");

        // ForgeAppKit only when the app's own Package.swift uses it.
        assert!(!m.contains("ForgeAppKit"), "{m}");
        let with_kit = manifest(
            Path::new("/lib"),
            true,
            Some(&kit),
            &["ForgeUI", "ForgeAppKit"],
            &Modules::default(),
        );
        assert!(with_kit.contains(r#"["ForgeFFI", .product(name: "ForgeUI", package: "forgekit"), .product(name: "ForgeAppKit", package: "forgekit")], path: "Sources/AppHost""#), "{with_kit}");
        fs::write(
            app.join("Package.swift"),
            r#".product(name: "ForgeAppKit", package: "forgekit")"#,
        )
        .unwrap();
        assert_eq!(forgekit_products(&app), ["ForgeAppKit"]);
        // An app on ForgeUI alone gets no runtime bindings to link.
        let ui_only = manifest(
            Path::new("/lib"),
            true,
            Some(&kit),
            &["ForgeUI"],
            &Modules::default(),
        );
        assert!(
            ui_only.contains(r#"dependencies: ["forge_ffiFFI"]"#),
            "{ui_only}"
        );
        assert!(!ui_only.contains("ForgeApp\""), "{ui_only}");
    }

    #[test]
    fn an_apps_own_module_names_reach_the_staging_package() {
        let own = Modules {
            bindings: "ChatFFI".into(),
            ffi: "chat_appFFI".into(),
            app: "ChatApp".into(),
        };
        assert!(is_portable(
            "import Foundation\n@preconcurrency import ChatFFI\n",
            &own
        ));
        assert!(is_portable("@testable import ChatApp\n", &own));
        assert!(!is_portable("import ChatFFI\n", &Modules::default()));
        let m = manifest(Path::new("/lib"), true, None, &[], &own);
        assert!(
            m.contains(r#".systemLibrary(name: "chat_appFFI", path: "Generated/forge_ffiFFI")"#),
            "{m}"
        );
        assert!(
            m.contains(
                r#"name: "ChatFFI",
            dependencies: ["chat_appFFI"]"#
            ),
            "{m}"
        );
        assert!(
            m.contains(
                r#".target(name: "ChatApp", dependencies: ["ChatFFI"], path: "Sources/AppHost")"#
            ),
            "{m}"
        );
        assert!(
            m.contains(r#"dependencies: ["ChatApp", "ChatFFI"], path: "Tests/AppHostTests""#),
            "{m}"
        );
    }

    #[test]
    fn library_name_comes_from_the_crate() {
        let meta = |kind: &str, name: &str| {
            serde_json::json!({ "packages": [{ "targets": [
                { "kind": ["bin"], "name": "tool" },
                { "kind": [kind, "rlib"], "name": name },
            ] }] })
        };
        assert_eq!(
            library_name(&meta("staticlib", "weather_core")),
            "weather_core"
        );
        assert_eq!(library_name(&meta("lib", "forge-ffi")), "forge_ffi");
        assert_eq!(library_name(&serde_json::json!({})), "forge_ffi");
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
        let sel = select(app, &Modules::default());
        assert_eq!(sel.sources, [app.join("Sources/AppHost/NotesModel.swift")]);
        assert_eq!(
            sel.tests,
            [app.join("Tests/AppHostTests/NotesModelTests.swift")]
        );
        assert_eq!(sel.skipped.len(), 2);

        let st = stage(
            app,
            &sel,
            &app.join("lib/libforge_ffi.a"),
            &Modules::default(),
        )
        .unwrap();
        assert!(st.join("Sources/AppHost/NotesModel.swift").is_file());
        assert!(!st.join("Sources/AppHost/ContentView.swift").exists());
        // Only the bindings, not the Apple-only service adapters.
        assert!(st.join("Generated/ForgeFFI/forge_ffi.swift").is_file());
        assert!(!st.join("Generated/ForgeFFI/ForgeServices.swift").exists());
        assert!(st.join("Generated/forge_ffiFFI/module.modulemap").is_file());
        let manifest = fs::read_to_string(st.join("Package.swift")).unwrap();
        assert!(manifest.contains("testTarget"), "{manifest}");
        // Staging again replaces it cleanly.
        stage(
            app,
            &sel,
            &app.join("lib/libforge_ffi.a"),
            &Modules::default(),
        )
        .unwrap();

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
