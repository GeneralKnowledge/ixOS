use std::fs;
use std::path::{Path, PathBuf};

use crate::app_config;
use crate::config::{self, ForgeConfig};
use crate::diagnose;
use crate::ios_builder::{IosBuilder, IosConfiguration, Purpose, SKIP_PREFLIGHT};
use crate::runner::{Cmd, CommandRunner};

const PLACEHOLDER_PACKAGE: &str = "FORGE_PACKAGE_NAME";
const PLACEHOLDER_APP_NAME: &str = "FORGE_APP_NAME";
const PLACEHOLDER_BUNDLE: &str = "FORGE_BUNDLE_ID";
const PLACEHOLDER_SUPPORT_DIR: &str = "FORGE_APP_SUPPORT_DIR";
const PLACEHOLDER_JAVA_PACKAGE: &str = "FORGE_JAVA_PACKAGE";
/// The starter Rust crate every new app gets as `rust/`.
const RUST_TEMPLATE: &str = "templates/app-rust";

/// Absolute path to an app package directory (SwiftPM or Gradle + forge.toml).
///
/// A relative path is taken from the current directory when an app is there
/// (`cd ~/projects && forge build ios --project MyApp`), otherwise from the
/// workspace root (`--project examples/counter` from anywhere in the checkout).
pub fn resolve_project_dir(root: &Path, project: &Path) -> PathBuf {
    if project.is_absolute() {
        return project.to_path_buf();
    }
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(project);
        if candidate.join("forge.toml").is_file() {
            return candidate;
        }
    }
    root.join(project)
}

/// Points the build scripts at the app's own Rust crate when it has one
/// (`[rust] path` in forge.toml); otherwise they build the workspace's
/// shared forge-ffi.
fn with_app_crate(cmd: Cmd, cfg: &ForgeConfig, project_dir: &Path) -> Cmd {
    match cfg.rust_crate_dir(project_dir) {
        Some(crate_dir) => cmd
            .env("FORGE_APP_DIR", project_dir)
            .env("FORGE_RUST_DIR", crate_dir),
        None => cmd,
    }
}

pub fn generated_dir_for_project(root: &Path, project: &Path) -> PathBuf {
    resolve_project_dir(root, project).join("Generated")
}

fn generated_dir_arg(root: &Path, generated: &Path) -> PathBuf {
    generated
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| generated.to_path_buf())
}

pub(crate) fn is_forge_workspace(dir: &Path) -> bool {
    dir.join("scripts/generate-bindings.sh").is_file()
        && dir.join("crates/forge-ffi/Cargo.toml").is_file()
}

/// Finds the Forge workspace: `FORGE_WORKSPACE`, else the nearest ancestor of
/// the current directory that is one, else the checkout this binary was built
/// from (if it still exists), else the current directory.
pub fn workspace_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("FORGE_WORKSPACE") {
        return PathBuf::from(dir);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(found) = cwd.ancestors().find(|d| is_forge_workspace(d)) {
        return found.to_path_buf();
    }
    let built_from = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if is_forge_workspace(&built_from) {
        if let Ok(path) = built_from.canonicalize() {
            return path;
        }
    }
    cwd
}

pub(crate) fn require_workspace(root: &Path) -> Result<(), String> {
    if is_forge_workspace(root) {
        Ok(())
    } else {
        Err(format!(
            "{} is not a Forge workspace: run forge from inside the Forge checkout, or set FORGE_WORKSPACE to its path",
            root.display()
        ))
    }
}

/// Runs a command attached to the terminal and fails if it exits non-zero.
pub(crate) fn run_checked(runner: &dyn CommandRunner, cmd: Cmd) -> Result<(), String> {
    let status = runner.stream(&cmd)?;
    if !status.success() {
        return Err(format!("`{}` failed ({status})", cmd.display()));
    }
    Ok(())
}

/// Runs a build step attached to the terminal; if it fails, explains why
/// from its output ([`diagnose`]): Rust, Swift, signing or device problems.
pub(crate) fn run_explained(runner: &dyn CommandRunner, cmd: Cmd) -> Result<(), String> {
    let (status, tail) = runner.stream_tail(&cmd)?;
    if status.success() {
        Ok(())
    } else {
        Err(diagnose::explain(&cmd.display(), status, &tail))
    }
}

pub(crate) fn script(root: &Path, name: &str) -> Cmd {
    Cmd::new("bash")
        .arg(root.join("scripts").join(name))
        .cwd(root)
}

fn load_config(project_dir: &Path) -> Result<ForgeConfig, String> {
    let forge_toml = project_dir.join("forge.toml");
    let content = fs::read_to_string(&forge_toml)
        .map_err(|e| format!("cannot read {}: {e}", forge_toml.display()))?;
    ForgeConfig::parse(&content)
}

pub fn init(
    runner: &dyn CommandRunner,
    root: &Path,
    name: &str,
    bundle_id: &str,
    platform: &str,
    path: Option<&Path>,
) -> Result<(), String> {
    if !config::is_valid_project_name(name) {
        return Err(
            "invalid project name: use ASCII letters, digits, hyphen, underscore; must start with a letter".into(),
        );
    }
    match platform {
        "ios" | "android" => {}
        other => return Err(format!("unsupported platform '{other}'")),
    }
    if platform == "android" && !config::is_valid_kotlin_package(bundle_id) {
        return Err(format!(
            "invalid bundle id '{bundle_id}' for Android: it becomes the Kotlin package, so each segment must start with a letter, use only letters, digits and underscores, and not be a Java or Kotlin keyword"
        ));
    }

    let probe = if platform == "ios" {
        format!(
            r#"
[app]
name = "{name}"
bundle_id = "{bundle_id}"
minimum_ios = "17.0"
[rust]
path = "rust"
[build]
ios_device = true
ios_simulator = false
"#,
        )
    } else {
        format!(
            r#"
[app]
name = "{name}"
bundle_id = "{bundle_id}"

[android]
min_sdk = 26
abis = ["arm64-v8a", "x86_64"]

[rust]
path = "rust"

[build]
android = true
"#,
        )
    };
    ForgeConfig::parse(&probe)?;

    require_workspace(root)?;
    let dest = path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("examples").join(name));
    if dest.exists() {
        return Err(format!("destination already exists: {}", dest.display()));
    }
    let (parent, dest) = prepare_destination(&dest)?;
    let workspace = WorkspaceRef::from_app_dir(root, &dest)?;

    let template = root.join(format!("templates/{platform}-app"));
    if !template.is_dir() {
        return Err(format!("template missing at templates/{platform}-app"));
    }
    if !root.join(RUST_TEMPLATE).join("Cargo.toml").is_file() {
        return Err(format!("template missing at {RUST_TEMPLATE}"));
    }
    if platform == "ios" {
        validate_ios_template(&template)?;
    } else {
        validate_android_template(&template)?;
    }

    // Build the app in a hidden staging folder next to its destination and move
    // it into place only when every step succeeded, so a failure never leaves a
    // half-made app behind that blocks the next attempt.
    let dir_name = dest.file_name().unwrap_or_default().to_string_lossy();
    let staging = parent.join(format!(".{dir_name}.forge-init"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    let result = populate_app(
        runner, root, &template, &staging, name, bundle_id, platform, &workspace,
    )
    .and_then(|()| fs::rename(&staging, &dest).map_err(|e| e.to_string()));
    if let Err(err) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    println!("Created Forge app at {}", dest.display());
    match &workspace {
        WorkspaceRef::Relative(_) => {}
        WorkspaceRef::Absolute(ws) => println!(
            "Note: this app's Rust crate uses the Forge crates at {ws}. If that checkout moves, update the paths in rust/Cargo.toml (and REPO_ROOT in the Xcode project's \"Build Rust FFI\" phase on iOS)."
        ),
    }
    print!(
        "{}",
        next_steps(&dest, platform, std::env::current_dir().ok().as_deref())
    );
    Ok(())
}

/// What to type after `forge init`, starting with `cd` into the app (a
/// relative path when the app is below the current directory).
fn next_steps(dest: &Path, platform: &str, cwd: Option<&Path>) -> String {
    let cd = cwd
        .and_then(|c| c.canonicalize().ok())
        .and_then(|c| dest.strip_prefix(c).ok().map(Path::to_path_buf))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| dest.to_path_buf());
    let mut out = format!("\nNext steps:\n  cd {}\n", cd.display());
    if platform == "ios" {
        out.push_str(
            "  forge doctor     # checks xtool, the Darwin SDK, your Apple sign-in and the iPhone\n\
             \x20 forge run ios    # builds the Rust and Swift, installs and launches on the iPhone\n\
             \x20 forge test       # your Rust tests (and the XCTest suite on macOS)\n\
             \nYour logic is in rust/src/lib.rs, the screen in Sources/AppHost/ContentView.swift.\n\
             README.md in the app explains the layout. On a Mac you can also open Xcode/ (docs/ios-xcode.md).\n",
        );
    } else {
        out.push_str(
            "  forge doctor               # checks the JDK, Android SDK, NDK and cargo-ndk\n\
             \x20 forge build android --install   # debug APK on a connected phone or emulator\n\
             \x20 forge test                 # your Rust tests and the JVM unit tests\n\
             \nYour logic is in rust/src/lib.rs. README.md in the app explains the layout.\n",
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn populate_app(
    runner: &dyn CommandRunner,
    root: &Path,
    template: &Path,
    dir: &Path,
    name: &str,
    bundle_id: &str,
    platform: &str,
    workspace: &WorkspaceRef,
) -> Result<(), String> {
    copy_template(template, dir)?;
    // The app's own Rust crate, with the workspace's lock file and toolchain
    // pin so it starts on exactly the versions Forge is tested with.
    let rust = dir.join("rust");
    copy_template(&root.join(RUST_TEMPLATE), &rust)?;
    for file in ["Cargo.lock", "rust-toolchain.toml"] {
        let src = root.join(file);
        if src.is_file() {
            fs::copy(&src, rust.join(file)).map_err(|e| e.to_string())?;
        }
    }
    substitute_placeholders(dir, name, bundle_id)?;
    rewrite_workspace_paths(dir, platform, workspace)?;
    if platform == "ios" {
        relocate_ios_xcode_host(dir, name)?;
        // Info.plist, App.entitlements and xtool.yml come from forge.toml.
        let cfg = load_config(dir)?;
        app_config::sync(dir, &cfg, app_config::Mode::Write { force: false })?;
    } else {
        relocate_android_package(dir, "org.forgemobile.templatehost", bundle_id)?;
    }
    let generated = dir.join("Generated");
    fs::create_dir_all(&generated).map_err(|e| e.to_string())?;
    let language = if platform == "ios" { "swift" } else { "kotlin" };
    require_workspace(root)?;
    run_checked(
        runner,
        script(root, "generate-bindings.sh")
            .arg(&generated)
            .arg(language)
            .env("FORGE_APP_DIR", dir)
            .env("FORGE_RUST_DIR", &rust),
    )
}

/// Creates the destination's parent directory and returns it with the
/// destination itself, both absolute and free of symlinks and `..`.
fn prepare_destination(dest: &Path) -> Result<(PathBuf, PathBuf), String> {
    let dir_name = dest
        .file_name()
        .filter(|n| *n != "." && *n != "..")
        .ok_or_else(|| {
            format!(
                "--path must name the new app directory, got {}",
                dest.display()
            )
        })?
        .to_owned();
    let parent = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    fs::create_dir_all(&parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", parent.display()))?;
    let dest = parent.join(dir_name);
    if dest.exists() {
        return Err(format!("destination already exists: {}", dest.display()));
    }
    Ok((parent, dest))
}

fn workspace_canonical(root: &Path) -> Result<PathBuf, String> {
    root.canonicalize()
        .map_err(|e| format!("cannot resolve workspace {}: {e}", root.display()))
}

/// How a generated app's build files point back at the Forge workspace, which
/// holds the Rust crates, the scripts, and the built native libraries.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspaceRef {
    /// App inside the workspace: a relative path like `../..`, so the checkout
    /// can move as a whole.
    Relative(String),
    /// App outside the workspace: the workspace's absolute path.
    Absolute(String),
}

impl WorkspaceRef {
    fn from_app_dir(root: &Path, app_dir: &Path) -> Result<Self, String> {
        let root = workspace_canonical(root)?;
        if let Ok(inner) = app_dir.strip_prefix(&root) {
            let depth = inner.components().count();
            let rel = vec![".."; depth].join("/");
            return Ok(Self::Relative(rel));
        }
        let abs = root
            .to_str()
            .ok_or_else(|| format!("workspace path is not UTF-8: {}", root.display()))?;
        // The path is written into Swift, Kotlin and Xcode shell strings.
        if abs.contains(['"', '\\', '$', '`', '\n']) {
            return Err(format!(
                "workspace path {abs} contains a character (\" \\ $ ` or newline) that cannot be written into the app's build files; move the checkout or create the app inside it"
            ));
        }
        Ok(Self::Absolute(abs.to_string()))
    }

    /// The workspace as seen from `base`, a build-system variable naming a
    /// directory `extra_up` levels below the app directory.
    fn seen_from(&self, base: &str, extra_up: usize) -> String {
        match self {
            Self::Relative(rel) => {
                let mut parts = vec![base.to_string()];
                parts.extend(std::iter::repeat_n("..".to_string(), extra_up));
                parts.push(rel.clone());
                parts.join("/")
            }
            Self::Absolute(abs) => abs.clone(),
        }
    }
}

/// Points the app's build files at the workspace. The templates assume the
/// default `examples/<Name>` location (`../..`); every reference is rewritten so
/// apps created anywhere else (`forge init --path`) build too.
fn rewrite_workspace_paths(
    dir: &Path,
    platform: &str,
    workspace: &WorkspaceRef,
) -> Result<(), String> {
    // rust/Cargo.toml: path dependencies on the Forge crates.
    let mut edits: Vec<(&str, String, String)> = vec![(
        "rust/Cargo.toml",
        r#"path = "FORGE_WORKSPACE/"#.into(),
        format!(r#"path = "{}/"#, workspace.seen_from("..", 0)),
    )];
    if platform == "ios" {
        // The Xcode project's "Build Rust FFI" phase runs the workspace's
        // scripts. `SRCROOT` is the app's `Xcode/` directory.
        edits.push((
            "PBXPROJ",
            r#"REPO_ROOT=\"$(cd \"${SRCROOT}/../../..\" && pwd)\""#.into(),
            format!(
                r#"REPO_ROOT=\"$(cd \"{}\" && pwd)\""#,
                workspace.seen_from("${SRCROOT}", 1)
            ),
        ));
    }
    for (file, old, new) in edits {
        let path = if file == "PBXPROJ" {
            find_pbxproj(dir)?
        } else {
            dir.join(file)
        };
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if !content.contains(&old) {
            return Err(format!(
                "template {} no longer contains the workspace path `{old}`; update rewrite_workspace_paths in forge-cli",
                path.display()
            ));
        }
        fs::write(&path, content.replace(&old, &new)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn find_pbxproj(dir: &Path) -> Result<PathBuf, String> {
    let xcode = dir.join("Xcode");
    for entry in fs::read_dir(&xcode).map_err(|e| format!("{}: {e}", xcode.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "xcodeproj") {
            return Ok(path.join("project.pbxproj"));
        }
    }
    Err(format!("no .xcodeproj under {}", xcode.display()))
}

fn validate_ios_template(template: &Path) -> Result<(), String> {
    for rel in [
        "Package.swift",
        "forge.toml",
        "Sources/AppHostLauncher/AppMain.swift",
        "Sources/AppHostXtoolApp/AppMain.swift",
        "Xcode/FORGE_PACKAGE_NAME.xcodeproj/project.pbxproj",
    ] {
        let p = template.join(rel);
        if !p.is_file() {
            return Err(format!("incomplete template: missing {}", rel));
        }
    }
    Ok(())
}

fn relocate_ios_xcode_host(dest: &Path, name: &str) -> Result<(), String> {
    let xcode = dest.join("Xcode");
    let template_proj = xcode.join("FORGE_PACKAGE_NAME.xcodeproj");
    let template_host = xcode.join("FORGE_PACKAGE_NAME");
    let named_proj = xcode.join(format!("{name}.xcodeproj"));
    let named_host = xcode.join(name);
    if template_proj.is_dir() {
        if named_proj.exists() {
            return Err(format!(
                "Xcode project already exists: {}",
                named_proj.display()
            ));
        }
        fs::rename(&template_proj, &named_proj).map_err(|e| e.to_string())?;
    }
    if template_host.is_dir() {
        if named_host.exists() {
            return Err(format!(
                "Xcode host folder already exists: {}",
                named_host.display()
            ));
        }
        fs::rename(&template_host, &named_host).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn relocate_android_package(
    dest: &Path,
    template_package: &str,
    bundle_id: &str,
) -> Result<(), String> {
    if template_package == bundle_id {
        return Ok(());
    }
    // Main sources must exist; unit-test sources are optional.
    for (source_set, required) in [("main", true), ("test", false)] {
        let java_root = dest.join(format!("app/src/{source_set}/java"));
        let old_dir = java_root.join(template_package.replace('.', "/"));
        let new_dir = java_root.join(bundle_id.replace('.', "/"));
        if !old_dir.is_dir() {
            if required {
                return Err(format!(
                    "android template java package missing at {}",
                    old_dir.display()
                ));
            }
            continue;
        }
        if new_dir.exists() {
            return Err(format!(
                "java package path already exists: {}",
                new_dir.display()
            ));
        }
        fs::create_dir_all(new_dir.parent().ok_or("invalid android package path")?)
            .map_err(|e| e.to_string())?;
        fs::rename(&old_dir, &new_dir).map_err(|e| e.to_string())?;
        // Remove the template package's now-empty parents (`org/forgemobile`).
        let mut parent = old_dir.parent();
        while let Some(dir) = parent.filter(|d| *d != java_root) {
            if fs::remove_dir(dir).is_err() {
                break; // not empty: it holds the new package
            }
            parent = dir.parent();
        }
    }
    Ok(())
}

fn validate_android_template(template: &Path) -> Result<(), String> {
    for rel in [
        "forge.toml",
        "settings.gradle.kts",
        "app/build.gradle.kts",
        "app/src/main/AndroidManifest.xml",
    ] {
        let p = template.join(rel);
        if !p.is_file() {
            return Err(format!("incomplete template: missing {}", rel));
        }
    }
    Ok(())
}

fn copy_template(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        // Build output left behind by building a template in place.
        if ["Generated", "target", "build", ".build", ".gradle"]
            .iter()
            .any(|skip| name == *skip)
        {
            continue;
        }
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let target = dst.join(name);
        if ty.is_dir() {
            copy_template(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn substitute_placeholders(dest: &Path, name: &str, bundle_id: &str) -> Result<(), String> {
    let support_dir = name;
    walk_substitute(dest, name, bundle_id, support_dir, bundle_id)
}

fn walk_substitute(
    dir: &Path,
    package_name: &str,
    bundle_id: &str,
    support_dir: &str,
    java_package: &str,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == "Generated") {
            continue;
        }
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            walk_substitute(&path, package_name, bundle_id, support_dir, java_package)?;
        } else if is_text_template(&path) {
            let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let updated = content
                .replace(PLACEHOLDER_PACKAGE, package_name)
                .replace(PLACEHOLDER_APP_NAME, package_name)
                .replace(PLACEHOLDER_BUNDLE, bundle_id)
                .replace(PLACEHOLDER_SUPPORT_DIR, support_dir)
                .replace(PLACEHOLDER_JAVA_PACKAGE, java_package);
            fs::write(&path, updated).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn is_text_template(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(
            "swift"
                | "toml"
                | "yml"
                | "yaml"
                | "md"
                | "kts"
                | "kt"
                | "xml"
                | "properties"
                | "xcscheme"
                | "plist"
        )
    ) || path
        .file_name()
        .is_some_and(|n| n == "Package.swift" || n == "project.pbxproj")
}

/// `forge test`.
///
/// - No app: the Forge workspace's Rust tests and format check.
/// - An app with its own crate (every app from `forge init`): that crate's
///   tests, then the app's host tests. The workspace's own tests run only
///   with `workspace` (`--workspace`), since they test Forge, not the app.
/// - A bundled example (shares the workspace's forge-ffi): the workspace
///   tests, then the host tests.
///
/// Host tests are JVM unit tests for an Android app (any host) and XCTest for
/// an iOS app, which needs macOS; on Linux they are skipped with a note.
pub fn test(
    runner: &dyn CommandRunner,
    root: &Path,
    project: Option<&Path>,
    workspace: bool,
) -> Result<(), String> {
    test_on(runner, root, project, workspace, cfg!(target_os = "macos"))
}

fn test_on(
    runner: &dyn CommandRunner,
    root: &Path,
    project: Option<&Path>,
    workspace: bool,
    macos: bool,
) -> Result<(), String> {
    let project_dir = match project {
        Some(p) => {
            let dir = resolve_project_dir(root, p);
            if !dir.join("gradlew").is_file() && !dir.join("Package.swift").is_file() {
                return Err(format!(
                    "{} is not a Forge app (no gradlew or Package.swift)",
                    dir.display()
                ));
            }
            Some(dir)
        }
        None => None,
    };

    // An app with its own crate: run its Rust tests, and give the host tests
    // that crate's library.
    let app_crate = match &project_dir {
        Some(dir) if dir.join("forge.toml").is_file() => load_config(dir)?.rust_crate_dir(dir),
        _ => None,
    };

    if app_crate.is_none() || workspace {
        require_workspace(root)?;
        run_checked(
            runner,
            Cmd::new("cargo").args(["test", "--workspace"]).cwd(root),
        )?;
        run_checked(
            runner,
            Cmd::new("cargo")
                .args(["fmt", "--all", "--", "--check"])
                .cwd(root),
        )?;
    }
    if let Some(crate_dir) = &app_crate {
        let manifest = crate_dir.join("Cargo.toml");
        println!("[forge] Rust tests: {}", manifest.display());
        run_explained(
            runner,
            Cmd::new("cargo")
                .arg("test")
                .arg("--manifest-path")
                .arg(&manifest)
                .cwd(root),
        )?;
    }

    match project_dir {
        Some(dir) if dir.join("gradlew").is_file() => {
            let host_lib = match &app_crate {
                // Regenerates the Kotlin bindings from the app's current Rust
                // (building its host library on the way), so the tests never
                // run stale bindings against a newer library.
                Some(crate_dir) => script(root, "generate-bindings.sh")
                    .arg(dir.join("Generated"))
                    .arg("kotlin")
                    .env("FORGE_APP_DIR", &dir)
                    .env("FORGE_RUST_DIR", crate_dir),
                None => Cmd::new("cargo")
                    .args(["build", "-p", "forge-ffi"])
                    .cwd(root),
            };
            run_checked(runner, host_lib)?;
            run_checked(
                runner,
                Cmd::new(dir.join("gradlew"))
                    .arg(":app:testDebugUnitTest")
                    .cwd(&dir),
            )
        }
        Some(dir) if macos => run_checked(runner, script(root, "test-swift-host.sh").arg(&dir)),
        Some(_) => {
            println!(
                "[forge] Skipped the XCTest suite: SwiftUI host tests need macOS (CI runs them). Your Rust tests above ran on this machine; `forge run ios` tries the app on an iPhone."
            );
            Ok(())
        }
        None => Ok(()),
    }
}

pub fn bindings_generate(
    runner: &dyn CommandRunner,
    root: &Path,
    out: &Path,
    language: &str,
) -> Result<(), String> {
    match language {
        "swift" | "kotlin" => {}
        other => return Err(format!("unsupported binding language '{other}'")),
    }
    require_workspace(root)?;
    run_checked(
        runner,
        script(root, "generate-bindings.sh").arg(out).arg(language),
    )
}

/// Regenerates an app's bindings into `<app>/Generated` from its own Rust
/// crate (or the shared forge-ffi): Swift for an iOS app (`Package.swift`),
/// Kotlin for an Android app (`gradlew`). Run it after changing the Rust API.
pub fn bindings_generate_for_project(
    runner: &dyn CommandRunner,
    root: &Path,
    project: &Path,
) -> Result<(), String> {
    require_workspace(root)?;
    let project_dir = resolve_project_dir(root, project);
    let cfg = load_config(&project_dir)?;
    let language = if project_dir.join("Package.swift").is_file() {
        "swift"
    } else if project_dir.join("gradlew").is_file() {
        "kotlin"
    } else {
        return Err(format!(
            "{} is not a Forge app (no Package.swift or gradlew)",
            project_dir.display()
        ));
    };
    let generate = script(root, "generate-bindings.sh")
        .arg(project_dir.join("Generated"))
        .arg(language);
    run_checked(runner, with_app_crate(generate, &cfg, &project_dir))
}

pub fn build_android(
    runner: &dyn CommandRunner,
    root: &Path,
    project: &Path,
    install: bool,
) -> Result<(), String> {
    require_workspace(root)?;
    let project_dir = resolve_project_dir(root, project);
    let cfg = load_config(&project_dir)?;
    if !cfg.android_enabled() {
        return Err("forge.toml does not enable Android builds ([build] android = true)".into());
    }
    let android = cfg
        .android
        .as_ref()
        .ok_or("missing [android] in forge.toml")?;
    // Gradle reads application_id and min_sdk from forge.toml itself.
    println!(
        "[forge] Android app {} (minSdk {}), ABIs: {}",
        cfg.android_application_id()?,
        android.min_sdk,
        android.abis.join(" ")
    );

    let generated = generated_dir_for_project(root, project);
    let package = script(root, "package-android-library.sh")
        .arg(generated_dir_arg(root, &generated))
        .env("FORGE_ANDROID_ABIS", android.abis.join(" "))
        .env(
            "FORGE_ANDROID_JNI_LIBS",
            project_dir.join("app/src/main/jniLibs"),
        );
    run_checked(runner, with_app_crate(package, &cfg, &project_dir))?;

    let gradlew = project_dir.join("gradlew");
    if !gradlew.is_file() {
        return Err(format!("gradlew not found in {}", project_dir.display()));
    }
    let gradle_task = if install {
        ":app:installDebug"
    } else {
        ":app:assembleDebug"
    };
    run_checked(runner, Cmd::new(gradlew).arg(gradle_task).cwd(&project_dir))
}

/// Brings the generated iOS files (Info.plist, App.entitlements, xtool.yml,
/// Package.swift's platform) in line with forge.toml, and says what changed.
fn sync_app_config(project_dir: &Path, cfg: &ForgeConfig) -> Result<(), String> {
    let outcome = app_config::sync(project_dir, cfg, app_config::Mode::Write { force: false })?;
    if !outcome.changed.is_empty() {
        println!(
            "[forge] Updated {} from forge.toml",
            outcome.changed.join(", ")
        );
    }
    for warning in &outcome.warnings {
        eprintln!("[forge] warning: {warning}");
    }
    Ok(())
}

/// The app directory, once its forge.toml is readable and enables iOS.
fn ios_project(root: &Path, project: &Path) -> Result<(PathBuf, ForgeConfig), String> {
    require_workspace(root)?;
    let project_dir = resolve_project_dir(root, project);
    let cfg = load_config(&project_dir)?;
    if !cfg.ios_enabled() {
        return Err("forge.toml does not enable iOS builds ([build] ios_device = true)".into());
    }
    Ok((project_dir, cfg))
}

/// Checks the toolchain, sign-in and iPhone before the (slow) Rust build.
fn ios_preflight(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    purpose: Purpose,
) -> Result<(), String> {
    if std::env::var_os(SKIP_PREFLIGHT).is_some_and(|v| v == "1") {
        return Ok(());
    }
    let what = match purpose {
        Purpose::Build => "forge build ios",
        Purpose::Run => "forge run ios",
    };
    builder.preflight(runner, purpose).into_result(what)
}

/// Syncs the app's configuration, then builds the Rust static library and
/// Swift bindings for an iOS app.
fn package_ios(
    runner: &dyn CommandRunner,
    root: &Path,
    project_dir: &Path,
    cfg: &ForgeConfig,
) -> Result<(), String> {
    sync_app_config(project_dir, cfg)?;
    let generated = project_dir.join("Generated");
    let package = script(root, "package-ios-library.sh").arg(generated_dir_arg(root, &generated));
    run_explained(runner, with_app_crate(package, cfg, project_dir))
}

pub fn build_ios(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    root: &Path,
    project: &Path,
    package_only: bool,
    configuration: IosConfiguration,
) -> Result<(), String> {
    let (project_dir, cfg) = ios_project(root, project)?;
    if !package_only {
        ios_preflight(runner, builder, Purpose::Build)?;
    }
    package_ios(runner, root, &project_dir, &cfg)?;
    if package_only {
        println!(
            "Rust static library and Swift bindings are ready for {} (open Xcode/ or build with {}).",
            project_dir.display(),
            builder.name()
        );
        return Ok(());
    }
    println!("[forge] Building the Swift app ({})", configuration.name());
    builder.build(runner, &project_dir, configuration)?;
    println!(
        "[forge] Built {}. Install and launch it on your iPhone with `forge run ios`.",
        project_dir.display()
    );
    Ok(())
}

/// Builds, installs and launches on a connected device.
pub fn run_ios(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    root: &Path,
    project: &Path,
    configuration: IosConfiguration,
) -> Result<(), String> {
    let (project_dir, cfg) = ios_project(root, project)?;
    ios_preflight(runner, builder, Purpose::Run)?;
    package_ios(runner, root, &project_dir, &cfg)?;
    println!(
        "[forge] Building, installing and launching ({})",
        configuration.name()
    );
    builder.run(runner, &project_dir, configuration)?;
    println!(
        "[forge] Installed. If the app does not open, trust your developer profile on the iPhone: Settings > General > VPN & Device Management."
    );
    Ok(())
}

/// `forge config sync|check|show`: the app's iOS configuration.
pub fn config(root: &Path, project: &Path, action: ConfigAction) -> Result<(), String> {
    let project_dir = resolve_project_dir(root, project);
    let cfg = load_config(&project_dir)?;
    if !cfg.ios_enabled() {
        println!(
            "{} does not build for iOS; forge.toml [ios] settings only apply to iOS apps.",
            project_dir.display()
        );
        return Ok(());
    }
    match action {
        ConfigAction::Sync { force } => {
            let outcome = app_config::sync(&project_dir, &cfg, app_config::Mode::Write { force })?;
            if outcome.changed.is_empty() {
                println!("[forge] iOS configuration already matches forge.toml");
            } else {
                println!(
                    "[forge] Updated {} from forge.toml",
                    outcome.changed.join(", ")
                );
            }
            for warning in &outcome.warnings {
                eprintln!("[forge] warning: {warning}");
            }
        }
        ConfigAction::Check => {
            let outcome = app_config::sync(&project_dir, &cfg, app_config::Mode::Check)?;
            println!(
                "[forge] {}, {} and {} match forge.toml",
                app_config::INFO_PLIST,
                app_config::ENTITLEMENTS,
                app_config::XTOOL_YML
            );
            for warning in outcome
                .warnings
                .iter()
                .chain(&app_config::xcode_differences(&cfg, &project_dir))
            {
                eprintln!("[forge] warning: {warning}");
            }
        }
        ConfigAction::Show => {
            print!("{}", app_config::summary(&cfg)?);
            let notes: Vec<String> = app_config::warnings(&cfg, &project_dir)
                .into_iter()
                .chain(app_config::xcode_differences(&cfg, &project_dir))
                .collect();
            if !notes.is_empty() {
                println!(
                    "
Notes:"
                );
                for note in notes {
                    println!("  - {note}");
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    Sync { force: bool },
    Check,
    Show,
}

pub fn devices(runner: &dyn CommandRunner, ios: &dyn IosBuilder) -> Result<(), String> {
    let mut listed = false;
    let mut found_tool = false;
    let mut errors: Vec<String> = Vec::new();

    let mut sources = vec![(
        "Android (adb)".to_string(),
        Cmd::new("adb").args(["devices", "-l"]),
    )];
    if let Some(cmd) = ios.devices_command() {
        sources.push((format!("iOS ({})", ios.name()), cmd));
    }
    for (label, cmd) in sources {
        if !which_program(runner, &cmd.program.to_string_lossy()) {
            continue;
        }
        found_tool = true;
        println!("--- {label} ---");
        match runner.capture(&cmd) {
            Ok(output) => {
                let listing = String::from_utf8_lossy(&output.stdout);
                print!("{listing}");
                if output.status.success() && listing.trim().is_empty() && label.starts_with("iOS")
                {
                    println!("No iPhone found. Connect it by USB, unlock it and tap Trust. If it still does not show: sudo systemctl restart usbmuxd, then replug.");
                }
                if output.status.success() {
                    listed = true;
                } else {
                    errors.push(String::from_utf8_lossy(&output.stderr).into());
                }
            }
            Err(e) => errors.push(e),
        }
    }

    if listed {
        Ok(())
    } else if found_tool {
        Err(errors.join("\n"))
    } else {
        Err(format!(
            "no device tools found (install adb and/or {})",
            ios.name()
        ))
    }
}

pub fn install_ios(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    ipa: &Path,
) -> Result<(), String> {
    builder.install(runner, ipa)
}

pub(crate) fn which_program(runner: &dyn CommandRunner, program: &str) -> bool {
    runner
        .capture(&Cmd::new(program).arg("--version"))
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ios_builder::XtoolBuilder;
    use crate::runner::{MockRunner, RealRunner};
    use std::ffi::OsString;

    const ANDROID_TOML: &str = r#"
[app]
name = "Demo"
bundle_id = "org.example.demo"

[android]
min_sdk = 28
abis = ["x86_64"]

[rust]
core_crate = "forge-core"
ffi_crate = "forge-ffi"

[build]
android = true
"#;

    const IOS_TOML: &str = r#"
[app]
name = "Demo"
bundle_id = "org.example.demo"
minimum_ios = "17.0"

[rust]
core_crate = "forge-core"
ffi_crate = "forge-ffi"

[build]
ios_device = true
"#;

    /// A minimal fake workspace with one app at `apps/demo`.
    fn workspace(forge_toml: &str, files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        for f in [
            "scripts/generate-bindings.sh",
            "crates/forge-ffi/Cargo.toml",
        ] {
            fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
            fs::write(root.join(f), "").unwrap();
        }
        let app = root.join("apps/demo");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("forge.toml"), forge_toml).unwrap();
        for f in files {
            fs::write(app.join(f), "").unwrap();
        }
        (dir, root)
    }

    fn env_of(cmd: &Cmd, key: &str) -> Option<OsString> {
        cmd.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    #[test]
    fn generated_dir_for_counter_example() {
        let root = workspace_root();
        let gen = generated_dir_for_project(&root, Path::new("examples/counter"));
        assert_eq!(gen, root.join("examples/counter/Generated"));
    }

    #[test]
    fn workspace_root_is_found_from_the_crate_directory() {
        assert!(is_forge_workspace(&workspace_root()));
    }

    #[test]
    fn build_android_passes_config_per_command_and_assembles() {
        let (_tmp, root) = workspace(ANDROID_TOML, &["gradlew"]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "").respond(
            &root.join("apps/demo/gradlew").to_string_lossy(),
            0,
            "",
        );
        build_android(&mock, &root, Path::new("apps/demo"), false).unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 2, "{:?}", mock.call_lines());
        let package = &calls[0];
        assert!(package
            .display()
            .contains("package-android-library.sh apps/demo/Generated"));
        assert_eq!(env_of(package, "FORGE_ANDROID_ABIS").unwrap(), "x86_64");
        assert_eq!(
            env_of(package, "FORGE_ANDROID_JNI_LIBS").unwrap(),
            root.join("apps/demo/app/src/main/jniLibs").into_os_string()
        );
        assert!(calls[1].display().ends_with("gradlew :app:assembleDebug"));
        // Nothing leaks into this process's environment.
        assert!(std::env::var_os("FORGE_ANDROID_JNI_LIBS").is_none());
    }

    #[test]
    fn build_android_install_uses_install_task() {
        let (_tmp, root) = workspace(ANDROID_TOML, &["gradlew"]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "").respond(
            &root.join("apps/demo/gradlew").to_string_lossy(),
            0,
            "",
        );
        build_android(&mock, &root, Path::new("apps/demo"), true).unwrap();
        assert!(mock.call_lines()[1].ends_with(":app:installDebug"));
    }

    #[test]
    fn build_android_stops_when_packaging_fails() {
        let (_tmp, root) = workspace(ANDROID_TOML, &["gradlew"]);
        let mock = MockRunner::default();
        mock.respond("bash", 1, "");
        let err = build_android(&mock, &root, Path::new("apps/demo"), false).unwrap_err();
        assert!(err.contains("package-android-library.sh"), "{err}");
        assert_eq!(mock.calls().len(), 1);
    }

    #[test]
    fn build_android_refuses_when_android_disabled() {
        let toml =
            ANDROID_TOML.replace("android = true", "android = false\nios_device = true") + "\n";
        let toml = toml.replace(
            "bundle_id = \"org.example.demo\"",
            "bundle_id = \"org.example.demo\"\nminimum_ios = \"17.0\"",
        );
        let (_tmp, root) = workspace(&toml, &["gradlew"]);
        let mock = MockRunner::default();
        let err = build_android(&mock, &root, Path::new("apps/demo"), false).unwrap_err();
        assert!(err.contains("does not enable Android"), "{err}");
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn build_ios_requires_forge_toml_with_ios_enabled() {
        let (_tmp, root) = workspace(ANDROID_TOML, &[]);
        let mock = MockRunner::default();
        let err = build_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            true,
            IosConfiguration::Debug,
        )
        .unwrap_err();
        assert!(err.contains("does not enable iOS"), "{err}");

        fs::remove_file(root.join("apps/demo/forge.toml")).unwrap();
        let err = build_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            true,
            IosConfiguration::Debug,
        )
        .unwrap_err();
        assert!(err.contains("forge.toml"), "{err}");
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn build_ios_package_only_skips_xtool() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        build_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            true,
            IosConfiguration::Debug,
        )
        .unwrap();
        let lines = mock.call_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("package-ios-library.sh apps/demo/Generated"));
    }

    #[test]
    fn build_ios_builds_and_run_ios_launches() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = ready_ios_host();
        build_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            false,
            IosConfiguration::Debug,
        )
        .unwrap();
        assert_eq!(mock.call_lines().last().unwrap(), "xtool dev build");
        // The build wrote the app's iOS configuration from forge.toml first.
        let xtool = read(root.join("apps/demo/xtool.yml"));
        assert!(xtool.contains("bundleID: org.example.demo\n"), "{xtool}");
        assert!(read(root.join("apps/demo/Info.plist")).contains("<string>Demo</string>"));

        let mock = ready_ios_host();
        run_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
        )
        .unwrap();
        assert_eq!(mock.call_lines().last().unwrap(), "xtool dev run");
        assert_eq!(
            mock.calls().last().unwrap().cwd.as_deref(),
            Some(root.join("apps/demo").as_path())
        );
    }

    /// A Linux host where the xtool preflight passes and every command succeeds.
    fn ready_ios_host() -> MockRunner {
        let mock = MockRunner::default();
        mock.respond("bash", 0, "")
            .respond("xtool", 0, "")
            .respond("rustup target list", 0, "aarch64-apple-ios\n")
            .respond("swift sdk list", 0, "darwin\n")
            .respond("xtool auth status", 0, "Logged in.\n")
            .respond("xtool devices --no-wait", 0, "iPhone [usb]: 0000\n");
        mock
    }

    #[test]
    fn run_ios_stops_before_the_rust_build_when_no_iphone_is_connected() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = ready_ios_host();
        mock.respond("xtool devices --no-wait", 0, "\n");
        let err = run_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
        )
        .unwrap_err();
        assert!(err.contains("No iPhone is connected"), "{err}");
        assert!(
            mock.call_lines()
                .iter()
                .all(|l| !l.contains("package-ios-library.sh")),
            "{:?}",
            mock.call_lines()
        );
    }

    #[test]
    fn failed_rust_build_is_explained() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = ready_ios_host();
        mock.respond(
            "bash",
            101,
            "error[E0425]: cannot find value `x` in this scope\nerror: could not compile `demo-core` (lib)",
        );
        let err = run_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
        )
        .unwrap_err();
        assert!(
            err.contains("[Rust library] Your Rust code does not compile"),
            "{err}"
        );
    }

    /// A backend that records what it was asked to do, proving the iOS
    /// commands only reach the toolchain through `IosBuilder`.
    #[derive(Default)]
    struct RecordingBuilder(std::sync::Mutex<Vec<String>>);

    impl IosBuilder for RecordingBuilder {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn ensure_available(&self, _: &dyn CommandRunner) -> Result<(), String> {
            Ok(())
        }
        fn build(
            &self,
            _: &dyn CommandRunner,
            dir: &Path,
            _: IosConfiguration,
        ) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push(format!("build {}", dir.display()));
            Ok(())
        }
        fn run(
            &self,
            _: &dyn CommandRunner,
            dir: &Path,
            _: IosConfiguration,
        ) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push(format!("run {}", dir.display()));
            Ok(())
        }
        fn install(&self, _: &dyn CommandRunner, ipa: &Path) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push(format!("install {}", ipa.display()));
            Ok(())
        }
        fn devices_command(&self) -> Option<Cmd> {
            None
        }
    }

    #[test]
    fn ios_commands_delegate_to_the_selected_builder() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        let fake = RecordingBuilder::default();
        build_ios(
            &mock,
            &fake,
            &root,
            Path::new("apps/demo"),
            false,
            IosConfiguration::Debug,
        )
        .unwrap();
        run_ios(
            &mock,
            &fake,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
        )
        .unwrap();
        install_ios(&mock, &fake, Path::new("App.ipa")).unwrap();

        let dir = root.join("apps/demo");
        assert_eq!(
            *fake.0.lock().unwrap(),
            [
                format!("build {}", dir.display()),
                format!("run {}", dir.display()),
                "install App.ipa".to_string(),
            ]
        );
        // Only the backend-independent packaging script ran directly.
        assert!(mock.call_lines().iter().all(|l| !l.contains("xtool")));
    }

    #[test]
    fn build_ios_without_xtool_explains_package_only() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        let err = build_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            false,
            IosConfiguration::Debug,
        )
        .unwrap_err();
        assert!(err.contains("--package-only"), "{err}");
    }

    #[test]
    fn test_runs_android_host_tests_for_project() {
        let (_tmp, root) = workspace(ANDROID_TOML, &["gradlew"]);
        let mock = MockRunner::default();
        mock.respond("cargo", 0, "").respond(
            &root.join("apps/demo/gradlew").to_string_lossy(),
            0,
            "",
        );
        test_on(&mock, &root, Some(Path::new("apps/demo")), false, false).unwrap();
        let lines = mock.call_lines();
        assert_eq!(lines[0], "cargo test --workspace");
        assert!(lines.contains(&"cargo build -p forge-ffi".to_string()));
        assert!(lines
            .last()
            .unwrap()
            .ends_with("gradlew :app:testDebugUnitTest"));
    }

    #[test]
    fn app_owned_crate_is_tested_and_its_bindings_regenerated() {
        let toml = ANDROID_TOML.replace(
            "core_crate = \"forge-core\"\nffi_crate = \"forge-ffi\"",
            "path = \"rust\"",
        );
        let (_tmp, root) = workspace(&toml, &["gradlew"]);
        let app = root.join("apps/demo");
        let mock = MockRunner::default();
        mock.respond("cargo", 0, "").respond("bash", 0, "").respond(
            &app.join("gradlew").to_string_lossy(),
            0,
            "",
        );
        test_on(&mock, &root, Some(Path::new("apps/demo")), false, false).unwrap();
        let calls = mock.calls();
        let lines = mock.call_lines();
        let manifest = app.join("rust/Cargo.toml");
        // The app's tests, not Forge's own.
        assert!(
            !lines.contains(&"cargo test --workspace".to_string()),
            "{lines:?}"
        );
        assert!(lines.contains(&format!(
            "cargo test --manifest-path {}",
            manifest.display()
        )));
        // Kotlin bindings come from the app's crate, before the JVM tests.
        let gen = calls
            .iter()
            .position(|c| c.display().contains("generate-bindings.sh"))
            .expect("bindings regenerated");
        assert!(calls[gen].display().ends_with("apps/demo/Generated kotlin"));
        assert_eq!(
            env_of(&calls[gen], "FORGE_APP_DIR"),
            Some(app.clone().into())
        );
        assert_eq!(
            env_of(&calls[gen], "FORGE_RUST_DIR"),
            Some(app.join("rust").into())
        );
        assert!(!lines.contains(&"cargo build -p forge-ffi".to_string()));
        assert!(lines
            .last()
            .unwrap()
            .ends_with("gradlew :app:testDebugUnitTest"));

        // `forge bindings generate --project` does the same on its own.
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        bindings_generate_for_project(&mock, &root, Path::new("apps/demo")).unwrap();
        let call = &mock.calls()[0];
        assert!(call.display().ends_with("apps/demo/Generated kotlin"));
        assert_eq!(
            env_of(call, "FORGE_RUST_DIR"),
            Some(app.join("rust").into())
        );
    }

    #[test]
    fn test_runs_swift_host_tests_for_ios_project() {
        let (_tmp, root) = workspace(IOS_TOML, &["Package.swift"]);
        let mock = MockRunner::default();
        mock.respond("cargo", 0, "").respond("bash", 0, "");
        test_on(&mock, &root, Some(Path::new("apps/demo")), false, true).unwrap();
        assert!(mock
            .call_lines()
            .last()
            .unwrap()
            .contains("test-swift-host.sh"));

        // On Linux the XCTest suite is skipped, not a failure.
        let mock = MockRunner::default();
        mock.respond("cargo", 0, "");
        test_on(&mock, &root, Some(Path::new("apps/demo")), false, false).unwrap();
        assert!(mock
            .call_lines()
            .iter()
            .all(|l| !l.contains("test-swift-host.sh")));
    }

    #[test]
    fn workspace_flag_adds_forges_own_tests_to_an_apps() {
        let toml = IOS_TOML.replace(
            "core_crate = \"forge-core\"\nffi_crate = \"forge-ffi\"",
            "path = \"rust\"",
        );
        let (_tmp, root) = workspace(&toml, &["Package.swift"]);
        let mock = MockRunner::default();
        mock.respond("cargo", 0, "");
        test_on(&mock, &root, Some(Path::new("apps/demo")), true, false).unwrap();
        let lines = mock.call_lines();
        assert_eq!(lines[0], "cargo test --workspace");
        assert!(
            lines
                .last()
                .unwrap()
                .starts_with("cargo test --manifest-path"),
            "{lines:?}"
        );
    }

    #[test]
    fn test_rejects_unknown_project_before_running_anything() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        let err = test_on(&mock, &root, Some(Path::new("apps/demo")), false, true).unwrap_err();
        assert!(err.contains("not a Forge app"), "{err}");
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn devices_reports_missing_tools() {
        let mock = MockRunner::default();
        let err = devices(&mock, &XtoolBuilder).unwrap_err();
        assert!(err.contains("no device tools"));

        let mock = MockRunner::default();
        mock.respond("adb", 0, "List of devices attached\n");
        devices(&mock, &XtoolBuilder).unwrap();
        assert!(mock.call_lines().contains(&"adb devices -l".to_string()));
    }

    #[test]
    fn init_rejects_android_bundle_ids_that_are_not_kotlin_packages() {
        let mock = MockRunner::default();
        for bad in ["org.my-app.demo", "org.example.fun", "org.example.class"] {
            let err = init(&mock, &workspace_root(), "BadPkg", bad, "android", None).unwrap_err();
            assert!(err.contains("Kotlin package"), "{bad}: {err}");
        }
    }

    #[test]
    fn init_failure_leaves_nothing_behind() {
        let root = workspace_root();
        let name = "InitFailureApp";
        // The mock knows no commands, so binding generation fails after the copy.
        let mock = MockRunner::default();
        let err = init(&mock, &root, name, "org.example.initfail", "ios", None).unwrap_err();
        assert!(err.contains("generate-bindings.sh"), "{err}");
        assert!(!root.join("examples").join(name).exists());
        assert!(!root
            .join("examples")
            .join(format!(".{name}.forge-init"))
            .exists());
    }

    #[test]
    fn init_substitutes_placeholders_and_generates_bindings() {
        let root = workspace_root();
        let name = "InitTestApp";
        let dest = root.join("examples").join(name);
        let _ = fs::remove_dir_all(&dest);
        init(
            &RealRunner,
            &root,
            name,
            "org.example.inittest",
            "ios",
            None,
        )
        .expect("init");
        let pkg = fs::read_to_string(dest.join("Package.swift")).unwrap();
        assert!(pkg.contains("name: \"InitTestApp\""));
        assert!(!pkg.contains("FORGE_PACKAGE_NAME"));
        let readme = fs::read_to_string(dest.join("README.md")).unwrap();
        assert!(readme.starts_with("# InitTestApp\n"), "{readme}");
        // Info.plist, App.entitlements and xtool.yml are generated from forge.toml.
        let plist = fs::read_to_string(dest.join("Info.plist")).unwrap();
        assert!(plist.contains("<string>InitTestApp</string>"));
        assert!(plist.contains("Generated by Forge"));
        let xtool = fs::read_to_string(dest.join("xtool.yml")).unwrap();
        assert!(
            xtool.contains("bundleID: org.example.inittest\n"),
            "{xtool}"
        );
        assert!(xtool.contains("product: AppHostXtool\n"), "{xtool}");
        assert!(dest.join("App.entitlements").is_file());
        let cfg = crate::config::ForgeConfig::parse(&read(dest.join("forge.toml"))).unwrap();
        crate::app_config::sync(&dest, &cfg, crate::app_config::Mode::Check)
            .expect("a new app starts in sync");
        assert!(dest.join("Generated/ForgeFFI/forge_ffi.swift").is_file());
        assert!(dest
            .join("Generated/forge_ffiFFI/module.modulemap")
            .is_file());
        assert!(dest
            .join("Xcode/InitTestApp.xcodeproj/project.pbxproj")
            .is_file());
        let model = fs::read_to_string(dest.join("Sources/AppHost/NotesModel.swift")).unwrap();
        assert!(model.contains("InitTestApp"));
        assert!(dest
            .join("Tests/AppHostTests/NotesModelTests.swift")
            .is_file());
        // The app owns its Rust crate, with the workspace's lock file and toolchain.
        assert!(dest.join("rust/src/lib.rs").is_file());
        assert!(dest.join("rust/Cargo.lock").is_file());
        let config = crate::config::ForgeConfig::parse(&read(dest.join("forge.toml"))).unwrap();
        assert_eq!(config.rust_crate_dir(&dest), Some(dest.join("rust")));
        fs::remove_dir_all(dest).unwrap();
    }

    #[test]
    fn init_fails_when_destination_exists() {
        let root = workspace_root();
        let dest = root.join("examples").join("counter");
        let err = init(
            &MockRunner::default(),
            &root,
            "counter",
            "org.example.counter",
            "ios",
            None,
        )
        .unwrap_err();
        assert!(err.contains("already exists"));
        assert!(dest.exists());
    }

    fn read(path: impl AsRef<Path>) -> String {
        fs::read_to_string(path.as_ref())
            .unwrap_or_else(|e| panic!("{}: {e}", path.as_ref().display()))
    }

    #[test]
    fn init_with_path_outside_workspace_points_ios_build_files_at_workspace() {
        let root = workspace_root();
        let ws = root.canonicalize().unwrap();
        let ws = ws.to_str().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        // The parent directory does not exist yet; init creates it.
        let dest = tmp.path().join("projects/PathApp");
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        init(
            &mock,
            &root,
            "PathApp",
            "org.example.pathapp",
            "ios",
            Some(&dest),
        )
        .unwrap();

        // Native libraries are the app's own (built from rust/ into build/).
        let pkg = read(dest.join("Package.swift"));
        assert!(
            pkg.contains(r#"let iosLibSearch = "\(packageDir)/build/ios-device/lib""#),
            "{pkg}"
        );
        let pbx = read(dest.join("Xcode/PathApp.xcodeproj/project.pbxproj"));
        assert!(pbx.contains(r#""$(SRCROOT)/../build/ios-device/lib/libforge_ffi.a""#));
        // The scripts and Forge crates still come from the workspace.
        assert!(pbx.contains(&format!(r#"REPO_ROOT=\"$(cd \"{ws}\" && pwd)\""#)));
        assert!(!pbx.contains("SRCROOT}/../../.."));
        let cargo = read(dest.join("rust/Cargo.toml"));
        assert!(
            cargo.contains(&format!(r#"path = "{ws}/crates/forge-runtime""#)),
            "{cargo}"
        );
        assert!(!cargo.contains("FORGE_WORKSPACE"));

        // Bindings went to the app's own Generated/, and staging is gone.
        let gen = mock.call_lines()[0].clone();
        assert!(gen.contains("generate-bindings.sh"), "{gen}");
        assert!(gen.contains(".PathApp.forge-init/Generated swift"), "{gen}");
        let left: Vec<_> = fs::read_dir(tmp.path().join("projects"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("PathApp")]);
    }

    #[test]
    fn init_with_path_outside_workspace_points_gradle_at_workspace() {
        let root = workspace_root();
        let ws = root.canonicalize().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("DroidApp");
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        init(
            &mock,
            &root,
            "DroidApp",
            "org.example.droid",
            "android",
            Some(&dest),
        )
        .unwrap();
        let gradle = read(dest.join("app/build.gradle.kts"));
        assert!(
            gradle.contains(r#"rootProject.file("rust/target/debug")"#),
            "{gradle}"
        );
        let cargo = read(dest.join("rust/Cargo.toml"));
        assert!(
            cargo.contains(&format!(
                r#"path = "{}/crates/forge-runtime""#,
                ws.display()
            )),
            "{cargo}"
        );
        assert!(dest
            .join("app/src/main/java/org/example/droid/NotesViewModel.kt")
            .is_file());
        assert!(!dest.join("app/src/main/java/org/forgemobile").exists());
        let settings = read(dest.join("settings.gradle.kts"));
        assert!(settings.contains(r#"rootProject.name = "DroidApp""#));
    }

    #[test]
    fn init_inside_workspace_keeps_relative_paths() {
        let root = workspace_root();
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");

        // Default location: the template's own `../..` paths, unchanged.
        let default_dest = root.join("examples/RelDefaultApp");
        let _ = fs::remove_dir_all(&default_dest);
        init(
            &mock,
            &root,
            "RelDefaultApp",
            "org.example.rel",
            "ios",
            None,
        )
        .unwrap();
        let cargo = read(default_dest.join("rust/Cargo.toml"));
        let pbx = read(default_dest.join("Xcode/RelDefaultApp.xcodeproj/project.pbxproj"));
        let template_pbx =
            read(root.join("templates/ios-app/Xcode/FORGE_PACKAGE_NAME.xcodeproj/project.pbxproj"))
                .replace("FORGE_PACKAGE_NAME", "RelDefaultApp")
                .replace("FORGE_APP_NAME", "RelDefaultApp")
                .replace("FORGE_BUNDLE_ID", "org.example.rel");
        fs::remove_dir_all(&default_dest).unwrap();
        // rust/ is one level below the app, so three levels up to the workspace.
        assert!(
            cargo.contains(r#"path = "../../../crates/forge-runtime""#),
            "{cargo}"
        );
        assert!(pbx == template_pbx, "project.pbxproj paths changed");

        // One level deeper: one more `..`, still relative.
        let nested = root.join("examples/.forge-test-nested");
        let _ = fs::remove_dir_all(&nested);
        let dest = nested.join("RelNestedApp");
        init(
            &mock,
            &root,
            "RelNestedApp",
            "org.example.nested",
            "ios",
            Some(&dest),
        )
        .unwrap();
        let cargo = read(dest.join("rust/Cargo.toml"));
        let pbx = read(dest.join("Xcode/RelNestedApp.xcodeproj/project.pbxproj"));
        fs::remove_dir_all(&nested).unwrap();
        assert!(
            cargo.contains(r#"path = "../../../../crates/forge-runtime""#),
            "{cargo}"
        );
        assert!(pbx.contains(r#"REPO_ROOT=\"$(cd \"${SRCROOT}/../../../..\" && pwd)\""#));
    }

    #[test]
    fn init_with_path_rejects_existing_destination_and_bare_dots() {
        let root = workspace_root();
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockRunner::default();
        let err = init(
            &mock,
            &root,
            "Taken",
            "org.example.taken",
            "ios",
            Some(tmp.path()),
        )
        .unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        let err = init(
            &mock,
            &root,
            "Dots",
            "org.example.dots",
            "ios",
            Some(&tmp.path().join("..")),
        )
        .unwrap_err();
        assert!(
            err.contains("already exists") || err.contains("--path"),
            "{err}"
        );
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn workspace_ref_renders_paths_per_build_system() {
        let rel = WorkspaceRef::Relative("../..".into());
        assert_eq!(rel.seen_from("$(SRCROOT)", 1), "$(SRCROOT)/../../..");
        assert_eq!(rel.seen_from("..", 0), "../../..");
        let abs = WorkspaceRef::Absolute("/src/forge".into());
        assert_eq!(abs.seen_from("$(SRCROOT)", 1), "/src/forge");
        assert_eq!(abs.seen_from("..", 0), "/src/forge");
    }
}
