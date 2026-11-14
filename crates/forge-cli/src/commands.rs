use std::fs;
use std::path::{Path, PathBuf};

use crate::app_config;
use crate::config::{self, ForgeConfig};
use crate::diagnose::{self, Stage};
use crate::ios_builder::{IosBuilder, IosConfiguration, Purpose, SKIP_PREFLIGHT};
use crate::ios_env;
use crate::runner::{Cmd, CommandRunner};

const PLACEHOLDER_PACKAGE: &str = "FORGE_PACKAGE_NAME";
const PLACEHOLDER_APP_NAME: &str = "FORGE_APP_NAME";
const PLACEHOLDER_BUNDLE: &str = "FORGE_BUNDLE_ID";
const PLACEHOLDER_SUPPORT_DIR: &str = "FORGE_APP_SUPPORT_DIR";
const PLACEHOLDER_JAVA_PACKAGE: &str = "FORGE_JAVA_PACKAGE";
/// The app's name as a Swift module prefix (`MyApp` in `MyAppFFI`, `MyAppApp`).
const PLACEHOLDER_MODULE: &str = "FORGE_MODULE_NAME";
/// The Rust library's name (`my_app_core`).
const PLACEHOLDER_LIB: &str = "FORGE_LIB_NAME";
/// The starter Rust crate every new iOS app gets as `rust/`: a core on the
/// forge-app runtime, which ForgeKit hosts.
const RUST_TEMPLATE: &str = "templates/app-rust";
/// Android's starter crate, from before the runtime. Android is work in
/// progress and gets no new features (docs/roadmap.md).
const ANDROID_RUST_TEMPLATE: &str = "templates/android-rust";
/// The ixOS repository the Forge crates come from in a pinned app.
const IXOS_GIT: &str = "https://github.com/GeneralKnowledge/ixOS";
/// ForgeKit's release repository (scripts/forgekit.sh).
const FORGEKIT_GIT: &str = "https://github.com/GeneralKnowledge/ForgeKit.git";

fn rust_template(platform: &str) -> &'static str {
    if platform == "android" {
        ANDROID_RUST_TEMPLATE
    } else {
        RUST_TEMPLATE
    }
}

/// The framework release in `forge-release.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Release {
    version: String,
    ixos_rev: String,
}

/// The release a new app outside the checkout pins to: `forge-release.toml`,
/// but only while the checkout's forge-app is that version. Otherwise the
/// checkout has framework changes no release has, and the app uses them.
fn framework_release(root: &Path) -> Option<Release> {
    let text = fs::read_to_string(root.join("forge-release.toml")).ok()?;
    let doc: toml::Table = text.parse().ok()?;
    let fw = doc.get("framework")?.as_table()?;
    let release = Release {
        version: fw.get("version")?.as_str()?.to_string(),
        ixos_rev: fw.get("ixos_rev")?.as_str()?.to_string(),
    };
    let app_crate: toml::Table = fs::read_to_string(root.join("crates/forge-app/Cargo.toml"))
        .ok()?
        .parse()
        .ok()?;
    let checkout = app_crate.get("package")?.get("version")?.as_str()?;
    (checkout == release.version).then_some(release)
}
/// `forge init --minimal`: laid over templates/ios-app (and app-rust as
/// `rust/`), after removing [`MINIMAL_REMOVES`].
const MINIMAL_TEMPLATE: &str = "templates/ios-minimal";
/// Files of the full iOS starter that the minimal one doesn't have: the notes
/// screens and model, and native services (the minimal crate doesn't export
/// them). Both keep AppHost.swift, the ForgeKit host.
const MINIMAL_REMOVES: &[&str] = &[
    "Sources/AppHost/AppServices.swift",
    "Sources/AppHost/ComposeNoteView.swift",
    "Sources/AppHost/Note+Display.swift",
    "Sources/AppHost/NoteDetailView.swift",
    "Sources/AppHost/NoteListView.swift",
    "Sources/AppHost/NotesModel.swift",
    "Sources/AppHost/SettingsView.swift",
    "Tests/AppHostTests/NativeServicesTests.swift",
    "Tests/AppHostTests/NotesModelTests.swift",
];

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

/// Points the build scripts at the app and its own Rust crate (`[rust] path`
/// in forge.toml).
fn with_app_crate(cmd: Cmd, cfg: &ForgeConfig, project_dir: &Path) -> Cmd {
    cmd.env("FORGE_APP_DIR", project_dir)
        .env("FORGE_RUST_DIR", cfg.rust_crate_dir(project_dir))
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
        && dir.join("crates/forge-runtime/Cargo.toml").is_file()
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

pub(crate) fn load_config(project_dir: &Path) -> Result<ForgeConfig, String> {
    let forge_toml = project_dir.join("forge.toml");
    let content = fs::read_to_string(&forge_toml)
        .map_err(|e| format!("cannot read {}: {e}", forge_toml.display()))?;
    let mut cfg = ForgeConfig::parse(&content)?;
    cfg.merge_hosted(project_dir)?;
    Ok(cfg)
}

#[cfg(test)]
pub fn init(
    runner: &dyn CommandRunner,
    root: &Path,
    name: &str,
    bundle_id: &str,
    platform: &str,
    path: Option<&Path>,
) -> Result<(), String> {
    init_with(runner, root, name, bundle_id, platform, path, false).map(|_| ())
}

/// `forge init`, with `minimal` for the one-screen starter (iOS only).
pub fn init_with(
    runner: &dyn CommandRunner,
    root: &Path,
    name: &str,
    bundle_id: &str,
    platform: &str,
    path: Option<&Path>,
    minimal: bool,
) -> Result<PathBuf, String> {
    if minimal && platform != "ios" {
        return Err("--minimal is for iOS apps only (Android is work in progress)".into());
    }
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
    let rust_template = rust_template(platform);
    if !root.join(rust_template).join("Cargo.toml").is_file() {
        return Err(format!("template missing at {rust_template}"));
    }
    if minimal
        && !root
            .join(MINIMAL_TEMPLATE)
            .join("rust/src/lib.rs")
            .is_file()
    {
        return Err(format!("template missing at {MINIMAL_TEMPLATE}"));
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
    // An iOS app outside the checkout pins to the framework release; one
    // inside it (like the examples) and Android use the checkout's.
    let release = match &workspace {
        WorkspaceRef::Absolute(_) if platform == "ios" => framework_release(root),
        _ => None,
    };
    let result = populate_app(
        runner,
        root,
        &template,
        &staging,
        name,
        bundle_id,
        platform,
        &workspace,
        release.as_ref(),
        minimal,
    )
    .and_then(|()| fs::rename(&staging, &dest).map_err(|e| e.to_string()));
    if let Err(err) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    println!("Created Forge app at {}", dest.display());
    crate::report::set_data(serde_json::json!({
        "path": dest.display().to_string(),
        "platform": platform,
    }));
    match (&workspace, &release) {
        (WorkspaceRef::Relative(_), _) => {}
        (WorkspaceRef::Absolute(ws), Some(r)) => println!(
            "This app uses framework release {} (the Forge crates from git, ForgeKit {}). Its build scripts come from the checkout at {ws}; if that moves, update REPO_ROOT in the Xcode project's \"Build Rust FFI\" phase.",
            r.version, r.version
        ),
        (WorkspaceRef::Absolute(ws), None) => println!(
            "Note: this app uses the Forge crates (and on iOS, ForgeKit) at {ws}, because that checkout has framework changes no release has yet. If it moves, update the paths in rust/Cargo.toml, Package.swift and REPO_ROOT in the Xcode project's \"Build Rust FFI\" phase."
        ),
    }
    print!(
        "{}",
        next_steps(&dest, platform, std::env::current_dir().ok().as_deref())
    );
    if minimal {
        println!("This is the minimal starter: rust/src/lib.rs, Sources/AppHost/ItemsModel.swift and ContentView.swift are the whole app. docs/first-app.md adds a feature to it.");
    }
    Ok(dest)
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
             \x20 forge run ios    # builds the Rust and Swift, signs and installs on the iPhone\n\
             \x20 forge test       # your Rust tests (and the XCTest suite on macOS)\n\
             \nYour logic is in rust/src/lib.rs, the screens in Sources/AppHost/ (ContentView.swift is the root).\n\
             README.md in the app explains the layout. On a Mac you can also open Xcode/ (docs/ios-xcode.md).\n",
        );
    } else {
        out.push_str(
            "\nNote: Android is work in progress. It builds, but gets no new features while Forge\n\
             focuses on iOS (docs/roadmap.md#android--work-in-progress).\n\n\
             \x20 forge doctor               # checks the JDK, Android SDK, NDK and cargo-ndk\n\
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
    release: Option<&Release>,
    minimal: bool,
) -> Result<(), String> {
    copy_template(template, dir)?;
    // The app's own Rust crate, with the workspace's lock file and toolchain
    // pin so it starts on exactly the versions Forge is tested with.
    let rust = dir.join("rust");
    copy_template(&root.join(rust_template(platform)), &rust)?;
    if minimal {
        for rel in MINIMAL_REMOVES {
            let file = dir.join(rel);
            if file.is_file() {
                fs::remove_file(&file).map_err(|e| e.to_string())?;
            }
        }
        copy_template(&root.join(MINIMAL_TEMPLATE), dir)?;
    }
    for file in ["Cargo.lock", "rust-toolchain.toml"] {
        let src = root.join(file);
        if src.is_file() {
            fs::copy(&src, rust.join(file)).map_err(|e| e.to_string())?;
        }
    }
    // rustup looks for the toolchain file from the current directory up,
    // not from --manifest-path, so `cargo test --manifest-path
    // rust/Cargo.toml` run in the app folder needs one there too. Without
    // it, a machine with no rustup default can't run cargo in the app
    // (seen on Reuben's laptop, 2026-11-10: only the pinned toolchain
    // installed, no default).
    let toolchain = root.join("rust-toolchain.toml");
    if toolchain.is_file() {
        fs::copy(&toolchain, dir.join("rust-toolchain.toml")).map_err(|e| e.to_string())?;
    }
    substitute_placeholders(dir, name, bundle_id)?;
    rewrite_workspace_paths(dir, platform, workspace, release)?;
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

/// Points the app's build files at the framework: the Forge crates and
/// ForgeKit at `release` when given, else at the workspace's own. The scripts
/// (the Xcode build phase) always come from the workspace.
fn rewrite_workspace_paths(
    dir: &Path,
    platform: &str,
    workspace: &WorkspaceRef,
    release: Option<&Release>,
) -> Result<(), String> {
    let mut edits: Vec<(&str, String, String)> = Vec::new();
    match release {
        // rust/Cargo.toml: the Forge crates from git at the release's commit.
        Some(r) => {
            let cargo_path = dir.join("rust/Cargo.toml");
            let text = fs::read_to_string(&cargo_path)
                .map_err(|e| format!("cannot read {}: {e}", cargo_path.display()))?;
            let pinned = pin_forge_crates(&text, &r.ixos_rev);
            if pinned.contains("FORGE_WORKSPACE") {
                return Err(format!(
                    "{} has a FORGE_WORKSPACE path outside crates/; update pin_forge_crates in forge-cli",
                    cargo_path.display()
                ));
            }
            fs::write(&cargo_path, pinned).map_err(|e| e.to_string())?;
        }
        // rust/Cargo.toml: path dependencies on the Forge crates.
        None => edits.push((
            "rust/Cargo.toml",
            r#"path = "FORGE_WORKSPACE/"#.into(),
            format!(r#"path = "{}/"#, workspace.seen_from("..", 0)),
        )),
    }
    if platform == "ios" {
        // Package.swift: ForgeKit, released or the checkout's forgekit/.
        let forgekit = match (release, workspace) {
            (Some(r), _) => format!(r#".package(url: "{FORGEKIT_GIT}", exact: "{}")"#, r.version),
            (None, WorkspaceRef::Relative(rel)) => format!(r#".package(path: "{rel}/forgekit")"#),
            (None, WorkspaceRef::Absolute(abs)) => format!(r#".package(path: "{abs}/forgekit")"#),
        };
        edits.push((
            "Package.swift",
            r#".package(path: "FORGE_WORKSPACE/forgekit")"#.into(),
            forgekit,
        ));
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

/// `forge-x = { path = "FORGE_WORKSPACE/crates/forge-x", … }` becomes
/// `forge-x = { git = "<ixOS>", rev = "<rev>", … }`.
fn pin_forge_crates(cargo_toml: &str, rev: &str) -> String {
    const PREFIX: &str = r#"path = "FORGE_WORKSPACE/crates/"#;
    cargo_toml
        .lines()
        .map(|line| match line.find(PREFIX) {
            Some(start) => {
                let rest = &line[start + PREFIX.len()..];
                let end = rest.find('"').map_or(rest.len(), |i| i + 1);
                format!(
                    r#"{}git = "{IXOS_GIT}", rev = "{rev}"{}"#,
                    &line[..start],
                    &rest[end..]
                )
            }
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
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
                .replace(PLACEHOLDER_MODULE, &module_name(package_name))
                .replace(PLACEHOLDER_LIB, &library_name(package_name))
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

/// `name` as a Swift module name: every character that isn't a letter,
/// digit or underscore becomes `_` (`my-app` → `my_app`).
fn module_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// The app's Rust library name: `name` in snake case plus `_core`
/// (`MyApp` → `my_app_core`). Named after the app, so another app can link
/// it (docs/app-crate.md).
fn library_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower = true;
        } else {
            if !out.ends_with('_') {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    format!("{}_core", out.trim_matches('_'))
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
/// - An app: its own crate's tests, then the app's host tests. The
///   workspace's own tests run only with `workspace` (`--workspace`), since
///   they test Forge, not the app.
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
            if !dir.join("forge.toml").is_file()
                || (!dir.join("gradlew").is_file() && !dir.join("Package.swift").is_file())
            {
                return Err(format!(
                    "{} is not a Forge app (no forge.toml, or no gradlew or Package.swift)",
                    dir.display()
                ));
            }
            Some(dir)
        }
        None => None,
    };

    // The app's own crate: run its Rust tests, and give the host tests that
    // crate's library.
    let app_crate = match &project_dir {
        Some(dir) => Some(load_config(dir)?.rust_crate_dir(dir)),
        None => None,
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
            // Regenerates the Kotlin bindings from the app's current Rust
            // (building its host library on the way), so the tests never run
            // stale bindings against a newer library.
            let mut host_lib = script(root, "generate-bindings.sh")
                .arg(dir.join("Generated"))
                .arg("kotlin")
                .env("FORGE_APP_DIR", &dir);
            if let Some(crate_dir) = &app_crate {
                host_lib = host_lib.env("FORGE_RUST_DIR", crate_dir);
            }
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

/// Regenerates an app's bindings into `<app>/Generated` from its own Rust
/// crate: Swift for an iOS app (`Package.swift`),
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
    println!(
        "[forge] Note: Android is work in progress; Forge is focusing on iOS (docs/roadmap.md#android--work-in-progress)"
    );
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
pub(crate) fn ios_project(root: &Path, project: &Path) -> Result<(PathBuf, ForgeConfig), String> {
    require_workspace(root)?;
    let project_dir = resolve_project_dir(root, project);
    let cfg = load_config(&project_dir)?;
    if !cfg.ios_enabled() {
        return Err("forge.toml does not enable iOS builds ([build] ios_device = true)".into());
    }
    Ok((project_dir, cfg))
}

/// Checks the toolchain and sign-in before the (slow) Rust build.
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
        Purpose::Run | Purpose::Install => "forge run ios",
    };
    builder.preflight(runner, purpose).into_result(what)
}

/// Syncs the app's configuration, then builds the Rust static library and
/// Swift bindings for an iOS app.
///
/// With `devtools`, the app crate is built with its `devtools` feature: the
/// live connection to `forge device` (docs/forge-device.md). Never on
/// unless asked, so an ordinary build contains none of it.
fn package_ios(
    runner: &dyn CommandRunner,
    root: &Path,
    project_dir: &Path,
    cfg: &ForgeConfig,
    devtools: bool,
) -> Result<(), String> {
    sync_app_config(project_dir, cfg)?;
    let generated = project_dir.join("Generated");
    let package = script(root, "package-ios-library.sh")
        .arg(generated_dir_arg(root, &generated))
        // The app's name on the live connection (forge device pair/connect).
        .env("FORGE_APP_ID", &cfg.app.bundle_id)
        .env(
            "FORGE_RUST_FEATURES",
            if devtools { "devtools" } else { "" },
        );
    run_explained(runner, with_app_crate(package, cfg, project_dir))
}

pub fn build_ios(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    root: &Path,
    project: &Path,
    package_only: bool,
    configuration: IosConfiguration,
    devtools: bool,
) -> Result<(), String> {
    let (project_dir, cfg) = ios_project(root, project)?;
    if !package_only {
        ios_preflight(runner, builder, Purpose::Build)?;
    }
    package_ios(runner, root, &project_dir, &cfg, devtools)?;
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
        "[forge] Built {}. Install it on your iPhone with `forge run ios`.",
        project_dir.display()
    );
    Ok(())
}

/// Builds and installs on a connected device. xtool does not launch the
/// app (and `xtool launch` does not work on iOS 17+), so the user opens it.
#[cfg(test)]
pub fn run_ios(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    root: &Path,
    project: &Path,
    configuration: IosConfiguration,
    devtools: bool,
) -> Result<(), String> {
    run_ios_with(
        runner,
        builder,
        root,
        project,
        configuration,
        devtools,
        None,
    )
}

/// How often `--wait-for-device` looks for the phone.
const DEVICE_POLL: std::time::Duration = std::time::Duration::from_secs(5);

/// How long `--wait-for-device` goes without seeing a phone before it first
/// restarts usbmuxd (ios_env::UsbmuxdRecovery spaces out the later ones).
const USBMUXD_RESTART_AFTER: std::time::Duration = std::time::Duration::from_secs(20);

/// `forge run ios`. Builds and packages the app (Rust and Swift) first,
/// whether or not the iPhone is there, and looks for the phone only once
/// the build is done: a missing phone never holds up compiling, and a
/// compile error shows up without anyone fetching the phone. Then the
/// built app is signed and installed (`IosBuilder::install_build`), which
/// is the only step that needs the phone.
///
/// Without a phone it stops with "built, connect the iPhone and run
/// again"; the next run rebuilds only what changed (Cargo and SwiftPM are
/// incremental, so seconds) and installs. With `wait_for_device` it waits
/// up to that long for the phone instead: ask the person for the phone
/// only once it says `waiting_for_device` (Good Dog pain point 17). Every
/// run writes `<app>/.forge/status/run-ios.json` (status.rs), so the
/// requester can see it started (pain point 18).
pub fn run_ios_with(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    root: &Path,
    project: &Path,
    configuration: IosConfiguration,
    devtools: bool,
    wait_for_device: Option<std::time::Duration>,
) -> Result<(), String> {
    let (project_dir, cfg) = ios_project(root, project)?;
    let status = crate::status::Status::start(&project_dir, "run ios");
    let result = run_ios_steps(
        runner,
        builder,
        root,
        &project_dir,
        &cfg,
        configuration,
        devtools,
        wait_for_device,
        &status,
    );
    status.finish(&result);
    result
}

#[allow(clippy::too_many_arguments)]
fn run_ios_steps(
    runner: &dyn CommandRunner,
    builder: &dyn IosBuilder,
    root: &Path,
    project_dir: &Path,
    cfg: &ForgeConfig,
    configuration: IosConfiguration,
    devtools: bool,
    wait_for_device: Option<std::time::Duration>,
    status: &crate::status::Status,
) -> Result<(), String> {
    let project_dir = project_dir.to_path_buf();
    // Setup problems (no xtool, no SDK, not signed in) still stop us before
    // the slow build; the iPhone is not one of them.
    ios_preflight(runner, builder, Purpose::Run)?;
    // Only xtool signs here; other builders (Xcode) choose their own team.
    let team = if builder.name() == "xtool" {
        check_signing_team(runner, cfg)?
    } else {
        None
    };
    status.phase("building");
    package_ios(runner, root, &project_dir, cfg, devtools)?;
    println!(
        "[forge] Building the Swift app ({}) before looking for the iPhone",
        configuration.name()
    );
    builder.build(runner, &project_dir, configuration)?;
    let product = crate::app_config::xtool_product(cfg);
    status.phase("waiting_for_device");
    if let Some(problem) = phone_missing(runner, builder) {
        let Some(limit) = wait_for_device else {
            return Err(built_without_phone(&project_dir, configuration, &problem));
        };
        println!(
            "[forge] Built. Waiting up to {} min for the iPhone: connect it by USB and unlock it.",
            limit.as_secs().div_ceil(60)
        );
        let started = std::time::Instant::now();
        let deadline = started + limit;
        let env = |k: &str| std::env::var(k).ok();
        let mut recovery = crate::ios_env::UsbmuxdRecovery::new(&env, USBMUXD_RESTART_AFTER);
        loop {
            let listed = crate::ios_env::ios_devices(runner);
            match listed {
                Some(ref devices) if !devices.is_empty() => break,
                _ if std::time::Instant::now() < deadline
                    && recovery
                        .tick(runner, started.elapsed())
                        .map(|note| println!("[forge] {note}"))
                        .is_some() => {}
                _ if std::time::Instant::now() >= deadline => {
                    // Say which: no phone, or phones couldn't be listed.
                    let why = if listed.is_none() {
                        "the phones couldn't be listed (`xtool devices --no-wait` failed or timed out: usbmuxd may be stuck; `sudo systemctl restart usbmuxd`)".to_string()
                    } else {
                        format!(
                            "no iPhone was connected within {} min",
                            limit.as_secs().div_ceil(60)
                        )
                    };
                    return Err(built_without_phone(&project_dir, configuration, &why));
                }
                _ => std::thread::sleep(DEVICE_POLL),
            }
        }
    }
    status.phase("installing");
    println!(
        "[forge] Signing and installing the built app ({})",
        configuration.name()
    );
    builder
        .install_build(runner, &project_dir, &product)
        .map_err(|err| explain_team_limit(err, team_type(cfg), team.as_ref()))?;
    // xtool installs but cannot launch on iOS 17+: someone opens the app.
    crate::report::set_data(serde_json::json!({
        "configuration": configuration.name(),
        "devtools": devtools,
        "installed": true,
        // false: xtool hung after [Installing] 100% and Forge stopped it.
        "verified": !crate::ios_builder::INSTALL_UNVERIFIED
            .load(std::sync::atomic::Ordering::Relaxed),
        "launched": false,
        "signing_team": team,
    }));
    let verified =
        !crate::ios_builder::INSTALL_UNVERIFIED.load(std::sync::atomic::Ordering::Relaxed);
    let first_install = record_install(&project_dir, &cfg.app.bundle_id);
    print!(
        "{}",
        install_summary(&InstallSummary {
            app: cfg.app.display_name.as_deref().unwrap_or(&cfg.app.name),
            configuration: configuration.name(),
            team: team.as_ref().map(|t| t.describe()),
            verified,
            process: &crate::app_config::xtool_product(cfg),
            first_install,
            devtools,
        })
    );
    Ok(())
}

/// Why the iPhone can't be installed on right now, or `None` when one is
/// connected. Uses the builder's install checks, which restart a stuck
/// usbmuxd once (#180) and look again, so a phone that dropped off USB
/// while plugged in is still found. `FORGE_SKIP_PREFLIGHT=1` skips it.
fn phone_missing(runner: &dyn CommandRunner, builder: &dyn IosBuilder) -> Option<String> {
    if std::env::var_os(SKIP_PREFLIGHT).is_some_and(|v| v == "1") {
        return None;
    }
    let pf = builder.preflight(runner, Purpose::Install);
    for w in &pf.warnings {
        eprintln!("warning: {w}");
    }
    let mut problems = pf.problems.into_iter();
    let first = problems.next()?;
    Some(
        std::iter::once(first)
            .chain(problems)
            .map(|p| format!("{} {}", p.problem, p.fix))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The end of a `forge run ios` that built the app but found no iPhone:
/// the build is done, and running the same command again installs it.
fn built_without_phone(project_dir: &Path, configuration: IosConfiguration, why: &str) -> String {
    let again = format!("forge run ios --project {}", project_dir.display());
    let again = match configuration {
        IosConfiguration::Debug => again,
        IosConfiguration::Release => format!("{again} --release"),
    };
    crate::report::set_data(serde_json::json!({
        "configuration": configuration.name(),
        "built": true,
        "installed": false,
    }));
    crate::report::record_failure(
        "forge run ios",
        vec![crate::report::JsonHint::new(
            Stage::Device,
            &format!("The app is built, but it can't be installed yet: {why}"),
            &format!("Connect the iPhone by USB, unlock it, and run `{again}` again: it rebuilds only what changed and installs."),
        )],
        "",
    );
    format!(
        "[forge] Built {} ({}), ready to install.\nThe iPhone isn't available: {why}\nConnect it by USB, unlock it, and run `{again}` again. Only what changed is rebuilt, then the app is signed and installed.",
        project_dir.display(),
        configuration.name()
    )
}

/// What `forge run ios` says after a successful install.
struct InstallSummary<'a> {
    /// The name on the home screen.
    app: &'a str,
    configuration: &'a str,
    team: Option<String>,
    /// The app's process on the phone (`[ios] xtool_product`).
    process: &'a str,
    /// false: xtool hung after `[Installing] 100%` and Forge stopped it.
    verified: bool,
    /// No earlier install of this bundle id is recorded for this app.
    first_install: bool,
    devtools: bool,
}

/// The end of a successful `forge run ios`: what was done, then the one
/// thing the person has to do next, which xtool can't do for them (it
/// doesn't launch apps on iOS 17 and later). The profile-trust step is
/// spelled out on the app's first install and shortened after that.
fn install_summary(s: &InstallSummary) -> String {
    let mut out = String::from("\n[forge] Done:\n");
    out.push_str("  ✓ Rust library built for iOS\n");
    out.push_str("  ✓ Swift bindings generated\n");
    out.push_str(&format!("  ✓ App built ({})\n", s.configuration));
    match &s.team {
        Some(team) => out.push_str(&format!("  ✓ Signed with {team}\n")),
        None => out.push_str("  ✓ Signed\n"),
    }
    if s.verified {
        out.push_str("  ✓ Installed on the iPhone\n");
    } else {
        out.push_str(
            "  ! Install probably finished: xtool reached 100% but didn't exit, so Forge stopped it. If the app isn't on the home screen, run `forge run ios` again.\n",
        );
    }
    out.push_str(&format!(
        "\nNext: open {} on your iPhone. Forge can't open it for you (xtool can't launch apps on iOS 17 and later).\n",
        s.app
    ));
    if s.first_install {
        out.push_str(
            "\nFirst install of this app: iOS won't open it until you trust your developer profile.\n  On the iPhone: Settings > General > VPN & Device Management > your Apple ID > Trust.\n  You do this once per Apple ID on this phone.\n",
        );
    } else {
        out.push_str(
            "If iOS says \"Untrusted Developer\": Settings > General > VPN & Device Management > your Apple ID > Trust.\n",
        );
    }
    out.push_str(&format!(
        "If it opens to a blank screen or closes at once: run `forge diagnose` in the app's folder and open the app when it asks, or `forge device logs --process {} --for 30s` (docs/troubleshooting.md).\n",
        s.process
    ));
    if s.devtools {
        out.push_str(
            "Development build: with the app open, run `forge device pair` once, then `forge device connect`.\n",
        );
    }
    out
}

/// Notes a successful install in `<app>/.forge/installed` (one bundle id
/// per line). Returns true when this bundle id wasn't there yet. A file
/// that can't be read or written counts as a first install, which only
/// makes the trust step longer.
fn record_install(project_dir: &Path, bundle_id: &str) -> bool {
    let path = project_dir.join(".forge/installed");
    let seen = std::fs::read_to_string(&path).unwrap_or_default();
    if seen.lines().any(|l| l.trim() == bundle_id) {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, format!("{seen}{bundle_id}\n"));
    true
}

fn team_type(cfg: &ForgeConfig) -> Option<&str> {
    cfg.ios.as_ref()?.signing.team_type.as_deref()
}

/// Finds the team xtool will sign with and warns when `forge.toml` says
/// otherwise. Stops before the build when the team is free and `forge.toml`
/// asks for paid-only entitlements, which xtool would silently leave out.
fn check_signing_team(
    runner: &dyn CommandRunner,
    cfg: &ForgeConfig,
) -> Result<Option<ios_env::SigningTeam>, String> {
    if std::env::var_os(SKIP_PREFLIGHT).is_some_and(|v| v == "1") {
        return Ok(None);
    }
    let Some(team) = ios_env::signing_team(runner) else {
        return Ok(None);
    };
    println!("[forge] Signing with {}", team.describe());
    if let Some(warning) = ios_env::team_mismatch(team_type(cfg), &team) {
        eprintln!("[forge] warning: {warning}");
    }
    let paid = app_config::paid_only_entitlements(cfg);
    if team.kind == ios_env::TeamKind::Free && !paid.is_empty() {
        let problem = format!(
            "The app asks for {}, which Apple only provisions for paid teams, but xtool signs with {}.",
            paid.join(", "),
            team.describe()
        );
        let fix = "Sign in with the paid team (`xtool auth logout`, then `xtool auth login` and pick it), or remove the entitlement from forge.toml.";
        crate::report::record_failure(
            "forge run ios",
            vec![crate::report::JsonHint::new(Stage::Signing, &problem, fix)],
            "",
        );
        return Err(format!(
            "forge run ios cannot start yet:\n  - {problem}\n    Fix: {fix}"
        ));
    }
    Ok(Some(team))
}

/// A free-team limit (3 apps, 10 App IDs a week) while `forge.toml` or xtool
/// says the team is paid means xtool is signed in with the wrong team.
fn explain_team_limit(
    err: String,
    configured: Option<&str>,
    team: Option<&ios_env::SigningTeam>,
) -> String {
    let free_limit = err.contains("A free Apple account");
    let thinks_paid =
        configured == Some("paid") || team.is_some_and(|t| t.kind == ios_env::TeamKind::Paid);
    if !free_limit || !thinks_paid {
        return err;
    }
    let problem = "This limit applies only to free teams, but this app is meant to be signed by a paid team: xtool is probably signed in with the Apple ID's free personal team.";
    let fix = "`xtool ds teams list` shows the teams; `xtool auth logout`, then `xtool auth login` and pick the paid one. `forge doctor` shows which team xtool uses.";
    crate::report::add_hint(crate::report::JsonHint::new(Stage::Signing, problem, fix));
    format!("{err}\n\n{problem}\nFix: {fix}")
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
            if crate::report::json_mode() {
                crate::report::set_data(app_config::summary_json(&cfg, &project_dir)?);
                return Ok(());
            }
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
        let ios = label.starts_with("iOS");
        let mut result = runner.capture(&cmd);
        let no_phone = |r: &Result<std::process::Output, String>| {
            !r.as_ref()
                .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
        };
        if ios && no_phone(&result) {
            // The phone sometimes drops off usbmuxd while still plugged in.
            let env = |k: &str| std::env::var(k).ok();
            let mut recovery =
                crate::ios_env::UsbmuxdRecovery::new(&env, std::time::Duration::ZERO);
            if let Some(note) = recovery.tick(runner, std::time::Duration::ZERO) {
                println!("[forge] {note}");
                result = runner.capture(&cmd);
            }
        }
        match result {
            Ok(output) => {
                let listing = String::from_utf8_lossy(&output.stdout);
                print!("{listing}");
                if output.status.success() && listing.trim().is_empty() && ios {
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
path = "rust"

[build]
android = true
"#;

    const IOS_TOML: &str = r#"
[app]
name = "Demo"
bundle_id = "org.example.demo"
minimum_ios = "17.0"

[rust]
path = "rust"

[build]
ios_device = true
"#;

    /// A minimal fake workspace with one app at `apps/demo`.
    fn workspace(forge_toml: &str, files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        for f in [
            "scripts/generate-bindings.sh",
            "crates/forge-runtime/Cargo.toml",
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
            false,
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
            false,
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
            false,
        )
        .unwrap();
        let lines = mock.call_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("package-ios-library.sh apps/demo/Generated"));
        let calls = mock.calls();
        assert_eq!(env_of(&calls[0], "FORGE_RUST_FEATURES"), Some("".into()));
    }

    #[test]
    fn devtools_builds_turn_on_the_app_crates_feature() {
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
            true,
        )
        .unwrap();
        let calls = mock.calls();
        assert_eq!(
            env_of(&calls[0], "FORGE_RUST_FEATURES"),
            Some("devtools".into())
        );
        assert!(env_of(&calls[0], "FORGE_APP_ID").is_some());
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
            false,
        )
        .unwrap();
        assert_eq!(mock.call_lines().last().unwrap(), "xtool dev build");
        // The build wrote the app's iOS configuration from forge.toml first.
        let xtool = read(root.join("apps/demo/xtool.yml"));
        assert!(xtool.contains("bundleID: org.example.demo\n"), "{xtool}");
        assert!(read(root.join("apps/demo/Info.plist")).contains("<string>Demo</string>"));

        // The mock xtool builds nothing, so put the app where xtool would.
        let app = fake_built_app(&root);
        let mock = ready_ios_host();
        run_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
            false,
        )
        .unwrap();
        let calls = mock.call_lines();
        let build = calls.iter().position(|l| l == "xtool dev build");
        assert!(build.is_some(), "{calls:?}");
        assert_eq!(
            calls.last().unwrap(),
            &format!("xtool install {}", app.display())
        );
        assert!(calls.iter().all(|l| !l.contains("dev run")), "{calls:?}");
        assert_eq!(
            mock.calls().last().unwrap().cwd.as_deref(),
            Some(root.join("apps/demo").as_path())
        );
    }

    /// Puts an empty `xtool/AppHostXtool.app` where `xtool dev build` would.
    fn fake_built_app(root: &Path) -> PathBuf {
        let app = crate::ios_builder::built_app(
            &root.join("apps/demo"),
            crate::app_config::DEFAULT_XTOOL_PRODUCT,
        );
        std::fs::create_dir_all(&app).unwrap();
        app
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
    fn run_ios_stops_when_a_free_team_would_drop_paid_entitlements() {
        let toml =
            format!("{IOS_TOML}\n[ios.entitlements]\npush_notifications = \"development\"\n");
        let (_tmp, root) = workspace(&toml, &[]);
        let mock = ready_ios_host();
        mock.respond(
            "xtool auth status",
            0,
            "Logged in.\n- Team ID: ZZZZZ99999\n",
        )
        .respond(
            "xtool ds teams list",
            0,
            "Someone [active]: ZZZZZ99999\n- Xcode Free Provisioning Program (ios)\n",
        );
        let err = run_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
            false,
        )
        .unwrap_err();
        assert!(
            err.contains("push notifications") || err.contains("Push"),
            "{err}"
        );
        assert!(err.contains("free personal team"), "{err}");
        assert!(mock
            .call_lines()
            .iter()
            .all(|l| !l.contains("package-ios-library.sh")));
    }

    #[test]
    fn free_team_limits_on_a_paid_app_point_at_the_team_choice() {
        let err = "What went wrong\nA free Apple account can have at most 3 sideloaded apps on a device at once.".to_string();
        let out = explain_team_limit(err.clone(), Some("paid"), None);
        assert!(
            out.contains("free personal team") && out.contains("xtool auth login"),
            "{out}"
        );
        assert_eq!(explain_team_limit(err.clone(), Some("free"), None), err);
        assert_eq!(
            explain_team_limit("other".into(), Some("paid"), None),
            "other"
        );
    }

    fn summary(first_install: bool, verified: bool) -> String {
        install_summary(&InstallSummary {
            app: "My App",
            configuration: "debug",
            team: Some("Someone (ZZZZZ99999), free personal team".into()),
            process: "MyAppXtool",
            verified,
            first_install,
            devtools: false,
        })
    }

    #[test]
    fn install_summary_names_the_app_and_the_next_step() {
        let out = summary(true, true);
        assert!(out.contains("✓ Installed on the iPhone"), "{out}");
        assert!(out.contains("Signed with Someone (ZZZZZ99999)"), "{out}");
        assert!(out.contains("Next: open My App on your iPhone"), "{out}");
        assert!(out.contains("First install of this app"), "{out}");
        assert!(
            out.contains("forge device logs --process MyAppXtool"),
            "{out}"
        );
    }

    #[test]
    fn install_summary_shortens_the_trust_step_after_the_first_install() {
        let out = summary(false, true);
        assert!(!out.contains("First install"), "{out}");
        assert!(out.contains("Untrusted Developer"), "{out}");
    }

    #[test]
    fn install_summary_says_when_the_install_was_not_confirmed() {
        let out = summary(false, false);
        assert!(!out.contains("✓ Installed"), "{out}");
        assert!(out.contains("Install probably finished"), "{out}");
    }

    #[test]
    fn record_install_is_true_only_the_first_time() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(record_install(tmp.path(), "org.example.a"));
        assert!(!record_install(tmp.path(), "org.example.a"));
        assert!(record_install(tmp.path(), "org.example.b"));
        assert!(!record_install(tmp.path(), "org.example.a"));
    }

    #[test]
    fn run_ios_builds_before_looking_for_the_iphone_and_stops_when_none_is_connected() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = ready_ios_host();
        mock.respond("xtool devices --no-wait", 0, "\n")
            .respond("sudo", 1, "");
        let err = run_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
            false,
        )
        .unwrap_err();
        assert!(err.contains("ready to install"), "{err}");
        assert!(err.contains("No iPhone is connected"), "{err}");
        assert!(
            err.contains("run `forge run ios --project"),
            "says how to install: {err}"
        );
        let calls = mock.call_lines();
        let rust = calls
            .iter()
            .position(|l| l.contains("package-ios-library.sh"))
            .expect("Rust built");
        let swift = calls
            .iter()
            .position(|l| l == "xtool dev build")
            .expect("Swift built");
        let looked = calls
            .iter()
            .position(|l| l == "xtool devices --no-wait")
            .expect("looked for the phone");
        assert!(rust < swift && swift < looked, "{calls:?}");
        assert!(
            calls.iter().all(|l| !l.contains("xtool install")),
            "{calls:?}"
        );
        assert!(calls.iter().all(|l| !l.contains("dev run")), "{calls:?}");
    }

    #[test]
    fn run_ios_still_stops_before_the_build_when_xtool_is_not_signed_in() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = ready_ios_host();
        mock.respond("xtool auth status", 0, "Logged out\n");
        let err = run_ios(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
            false,
        )
        .unwrap_err();
        assert!(err.contains("xtool auth login"), "{err}");
        assert!(
            mock.call_lines()
                .iter()
                .all(|l| !l.contains("package-ios-library.sh")),
            "{:?}",
            mock.call_lines()
        );
    }

    #[test]
    fn wait_for_device_builds_before_looking_for_the_phone() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = ready_ios_host();
        // No phone at all: the preflight doesn't ask, the build still runs,
        // and the wait ends with a message saying the build is ready.
        mock.respond("xtool devices --no-wait", 0, "\n");
        let err = run_ios_with(
            &mock,
            &XtoolBuilder,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
            false,
            Some(std::time::Duration::from_secs(0)),
        )
        .unwrap_err();
        assert!(err.contains("ready to install"), "{err}");
        assert!(err.contains("no iPhone was connected within"), "{err}");
        let calls = mock.call_lines();
        let build = calls
            .iter()
            .position(|l| l.contains("dev build"))
            .expect("built");
        assert!(
            calls.iter().all(|l| !l.contains("xtool install")),
            "{calls:?}"
        );
        assert!(calls[..build]
            .iter()
            .any(|l| l.contains("package-ios-library.sh")));
        let status: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("apps/demo/.forge/status/run-ios.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(status["phase"], "failed");
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
            false,
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
        fn install_build(
            &self,
            _: &dyn CommandRunner,
            dir: &Path,
            product: &str,
        ) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push(format!("install_build {} {product}", dir.display()));
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
            false,
        )
        .unwrap();
        run_ios(
            &mock,
            &fake,
            &root,
            Path::new("apps/demo"),
            IosConfiguration::Debug,
            false,
        )
        .unwrap();
        install_ios(&mock, &fake, Path::new("App.ipa")).unwrap();

        let dir = root.join("apps/demo");
        assert_eq!(
            *fake.0.lock().unwrap(),
            [
                format!("build {}", dir.display()),
                format!("build {}", dir.display()),
                format!("install_build {} AppHostXtool", dir.display()),
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
            false,
        )
        .unwrap_err();
        assert!(err.contains("--package-only"), "{err}");
    }

    #[test]
    fn app_owned_crate_is_tested_and_its_bindings_regenerated() {
        let (_tmp, root) = workspace(ANDROID_TOML, &["gradlew"]);
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
        let (_tmp, root) = workspace(IOS_TOML, &["Package.swift"]);
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
    fn minimal_starter_removes_only_files_the_full_template_has() {
        let root = workspace_root();
        for rel in MINIMAL_REMOVES {
            assert!(
                root.join("templates/ios-app").join(rel).is_file(),
                "{rel} is not in templates/ios-app: update MINIMAL_REMOVES"
            );
        }
        // The overlay must not bring back a file it removes.
        for rel in MINIMAL_REMOVES {
            assert!(!root.join(MINIMAL_TEMPLATE).join(rel).exists(), "{rel}");
        }
    }

    /// The minimal starter needs only the runtime and ForgeKit: ForgeUI and
    /// ForgeAppKit are optional, and only the full starter uses them.
    #[test]
    fn minimal_starter_needs_only_the_runtime_and_forgekit() {
        let root = workspace_root();
        let full = root.join("templates/ios-app");
        let minimal = root.join(MINIMAL_TEMPLATE);
        let manifest = fs::read_to_string(minimal.join("Package.swift")).unwrap();
        for product in ["ForgeUI", "ForgeAppKit"] {
            assert!(!manifest.contains(&format!("\"{product}\"")), "{product}");
        }
        assert!(manifest.contains(r#""ForgeKit""#), "{manifest}");
        // Every Swift file the minimal app ends up with: the overlay's, and
        // the full starter's that it keeps.
        fn swift_files(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).into_iter().flatten() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    swift_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "swift") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        for base in [&full, &minimal] {
            for dir in ["Sources", "Tests"] {
                let mut found = Vec::new();
                swift_files(&base.join(dir), &mut found);
                for path in found {
                    {
                        let rel = path.strip_prefix(base).unwrap().to_path_buf();
                        let kept = !MINIMAL_REMOVES.iter().any(|r| Path::new(r) == rel);
                        let overlaid = base == &full && minimal.join(&rel).exists();
                        if kept && !overlaid {
                            files.push(path);
                        }
                    }
                }
            }
        }
        assert!(files.len() > 3, "{files:?}");
        for path in files {
            let text = fs::read_to_string(&path).unwrap();
            for module in ["ForgeUI", "ForgeAppKit"] {
                assert!(
                    !text.lines().any(|l| l.trim_start().starts_with("import")
                        && l.split_whitespace().any(|w| w == module)),
                    "{} imports {module}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn minimal_is_ios_only() {
        let err = init_with(
            &crate::runner::MockRunner::default(),
            &workspace_root(),
            "MiniDroid",
            "org.example.minidroid",
            "android",
            None,
            true,
        )
        .unwrap_err();
        assert!(err.contains("iOS"), "{err}");
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
        // The pinned toolchain in the app folder too, so cargo works there
        // on a machine with no rustup default.
        assert!(dest.join("rust-toolchain.toml").is_file());
        assert!(dest.join("rust/rust-toolchain.toml").is_file());
        // No agent configuration unless asked for (forge init --agent,
        // forge agent setup): a new app is complete without it.
        for file in crate::inspect::AGENT_FILES {
            assert!(!dest.join(file).exists(), "{file}");
        }
        // Info.plist, App.entitlements and xtool.yml are generated from forge.toml.
        let plist = fs::read_to_string(dest.join("Info.plist")).unwrap();
        assert!(plist.contains("<string>InitTestApp</string>"));
        assert!(plist.contains("Generated by Forge"));
        let xtool = fs::read_to_string(dest.join("xtool.yml")).unwrap();
        assert!(
            xtool.contains("bundleID: org.example.inittest\n"),
            "{xtool}"
        );
        assert!(xtool.contains("product: InitTestAppXtool\n"), "{xtool}");
        assert!(pkg.contains(".library(name: \"InitTestAppXtool\""), "{pkg}");
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
        // ForgeKit hosts the core; its data folder is named after the app.
        let host = fs::read_to_string(dest.join("Sources/AppHost/AppHost.swift")).unwrap();
        assert!(host.contains(r#"ForgeHost(app: "InitTestApp")"#), "{host}");
        let pkg_deps = fs::read_to_string(dest.join("Package.swift")).unwrap();
        assert!(
            pkg_deps.contains(r#".package(path: "../../forgekit")"#),
            "{pkg_deps}"
        );
        assert!(dest
            .join("Tests/AppHostTests/NotesModelTests.swift")
            .is_file());
        // The app owns its Rust crate, with the workspace's lock file and toolchain.
        assert!(dest.join("rust/src/lib.rs").is_file());
        assert!(dest.join("rust/Cargo.lock").is_file());
        let config = crate::config::ForgeConfig::parse(&read(dest.join("forge.toml"))).unwrap();
        assert_eq!(config.rust_crate_dir(&dest), dest.join("rust"));
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
        // The scripts come from the workspace.
        assert!(pbx.contains(&format!(r#"REPO_ROOT=\"$(cd \"{ws}\" && pwd)\""#)));
        assert!(!pbx.contains("SRCROOT}/../../.."));
        // The framework: the release in forge-release.toml, or the workspace
        // while it has unreleased framework changes.
        let cargo = read(dest.join("rust/Cargo.toml"));
        match framework_release(&root) {
            Some(r) => {
                assert!(
                    cargo.contains(&format!(
                        r#"forge-runtime = {{ git = "{IXOS_GIT}", rev = "{}" }}"#,
                        r.ixos_rev
                    )),
                    "{cargo}"
                );
                assert!(
                    pkg.contains(&format!(r#"exact: "{}")"#, r.version)),
                    "{pkg}"
                );
            }
            None => {
                assert!(
                    cargo.contains(&format!(r#"path = "{ws}/crates/forge-runtime""#)),
                    "{cargo}"
                );
                assert!(
                    pkg.contains(&format!(r#".package(path: "{ws}/forgekit")"#)),
                    "{pkg}"
                );
            }
        }
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
    fn a_release_pins_the_forge_crates_and_forgekit() {
        let cargo = "[dependencies]\nforge-app = { path = \"FORGE_WORKSPACE/crates/forge-app\" }\nforge-net = { path = \"FORGE_WORKSPACE/crates/forge-net\", features = [\"ureq\"] }\nserde = \"1\"\n";
        assert_eq!(
            pin_forge_crates(cargo, "abc123"),
            format!(
                "[dependencies]\nforge-app = {{ git = \"{IXOS_GIT}\", rev = \"abc123\" }}\nforge-net = {{ git = \"{IXOS_GIT}\", rev = \"abc123\", features = [\"ureq\"] }}\nserde = \"1\"\n"
            )
        );

        // The real templates, as `forge init` lays them out.
        let root = workspace_root();
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        copy_template(&root.join("templates/ios-app"), dir).unwrap();
        copy_template(&root.join(RUST_TEMPLATE), &dir.join("rust")).unwrap();
        let release = Release {
            version: "9.8.7".into(),
            ixos_rev: "abc123".into(),
        };
        let ws = WorkspaceRef::Absolute("/opt/ixos".into());
        rewrite_workspace_paths(dir, "ios", &ws, Some(&release)).unwrap();
        let cargo = fs::read_to_string(dir.join("rust/Cargo.toml")).unwrap();
        assert!(!cargo.contains("FORGE_WORKSPACE"), "{cargo}");
        assert!(
            cargo.contains(&format!(
                r#"forge-app = {{ git = "{IXOS_GIT}", rev = "abc123" }}"#
            )),
            "{cargo}"
        );
        let pkg = fs::read_to_string(dir.join("Package.swift")).unwrap();
        assert!(
            pkg.contains(&format!(
                r#".package(url: "{FORGEKIT_GIT}", exact: "9.8.7")"#
            )),
            "{pkg}"
        );
        // The scripts still come from the checkout.
        let pbx = fs::read_to_string(find_pbxproj(dir).unwrap()).unwrap();
        assert!(pbx.contains(r#"REPO_ROOT=\"$(cd \"/opt/ixos\" && pwd)\""#));
    }

    #[test]
    fn unreleased_framework_changes_mean_the_checkout() {
        let root = workspace_root();
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path();
        fs::create_dir_all(fake.join("crates/forge-app")).unwrap();
        let release = "[framework]\nversion = \"1.2.0\"\nixos_rev = \"abc\"\n";
        fs::write(fake.join("forge-release.toml"), release).unwrap();
        let crate_at = |v: &str| {
            fs::write(
                fake.join("crates/forge-app/Cargo.toml"),
                format!("[package]\nname = \"forge-app\"\nversion = \"{v}\"\n"),
            )
            .unwrap()
        };
        crate_at("1.2.0");
        assert_eq!(
            framework_release(fake),
            Some(Release {
                version: "1.2.0".into(),
                ixos_rev: "abc".into()
            })
        );
        crate_at("1.3.0");
        assert_eq!(framework_release(fake), None);

        // In path mode the iOS template points at the checkout's forgekit/.
        let dir = tmp.path().join("app");
        copy_template(&root.join("templates/ios-app"), &dir).unwrap();
        copy_template(&root.join(RUST_TEMPLATE), &dir.join("rust")).unwrap();
        rewrite_workspace_paths(&dir, "ios", &WorkspaceRef::Relative("../..".into()), None)
            .unwrap();
        let pkg = fs::read_to_string(dir.join("Package.swift")).unwrap();
        assert!(pkg.contains(r#".package(path: "../../forgekit")"#), "{pkg}");
        let cargo = fs::read_to_string(dir.join("rust/Cargo.toml")).unwrap();
        assert!(
            cargo.contains(r#"path = "../../../crates/forge-app""#),
            "{cargo}"
        );
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
    fn an_apps_module_and_library_names() {
        assert_eq!(module_name("MyApp"), "MyApp");
        assert_eq!(module_name("my-app"), "my_app");
        assert_eq!(library_name("MyApp"), "my_app_core");
        assert_eq!(library_name("FreshIos"), "fresh_ios_core");
        assert_eq!(library_name("my-app"), "my_app_core");
        assert_eq!(library_name("notes2"), "notes2_core");
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
                .replace("FORGE_MODULE_NAME", "RelDefaultApp")
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
