use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{self, ForgeConfig};
use crate::runner::{Cmd, CommandRunner};

const PLACEHOLDER_PACKAGE: &str = "FORGE_PACKAGE_NAME";
const PLACEHOLDER_APP_NAME: &str = "FORGE_APP_NAME";
const PLACEHOLDER_BUNDLE: &str = "FORGE_BUNDLE_ID";
const PLACEHOLDER_SUPPORT_DIR: &str = "FORGE_APP_SUPPORT_DIR";
const PLACEHOLDER_JAVA_PACKAGE: &str = "FORGE_JAVA_PACKAGE";

/// Absolute path to an app package directory (SwiftPM or Gradle + forge.toml).
pub fn resolve_project_dir(root: &Path, project: &Path) -> PathBuf {
    if project.is_absolute() {
        project.to_path_buf()
    } else {
        root.join(project)
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

fn is_forge_workspace(dir: &Path) -> bool {
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

fn require_workspace(root: &Path) -> Result<(), String> {
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
fn run_checked(runner: &dyn CommandRunner, cmd: Cmd) -> Result<(), String> {
    let status = runner.stream(&cmd)?;
    if !status.success() {
        return Err(format!("`{}` failed ({status})", cmd.display()));
    }
    Ok(())
}

fn script(root: &Path, name: &str) -> Cmd {
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

pub fn doctor(runner: &dyn CommandRunner, root: &Path) -> Result<(), String> {
    let validate = root.join("scripts/validate-toolchain.sh");
    if validate.exists() {
        run_checked(runner, script(root, "validate-toolchain.sh"))?;
    } else {
        println!("Forge workspace root: {}", root.display());
        for (program, args) in [
            ("rustc", &["--version"][..]),
            ("cargo", &["--version"]),
            ("swift", &["--version"]),
            ("xtool", &["--version"]),
        ] {
            try_version(runner, program, args);
        }
    }
    Ok(())
}

fn try_version(runner: &dyn CommandRunner, program: &str, args: &[&str]) {
    match runner.capture(&Cmd::new(program).args(args)) {
        Ok(out) if out.status.success() => print!("{}", String::from_utf8_lossy(&out.stdout)),
        Ok(out) => println!("{program}: {}", String::from_utf8_lossy(&out.stderr).trim()),
        Err(e) => println!("{program}: not available ({e})"),
    }
}

pub fn init(
    runner: &dyn CommandRunner,
    root: &Path,
    name: &str,
    bundle_id: &str,
    platform: &str,
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
core_crate = "forge-core"
ffi_crate = "forge-ffi"
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
core_crate = "forge-core"
ffi_crate = "forge-ffi"

[build]
android = true
"#,
        )
    };
    ForgeConfig::parse(&probe)?;

    require_workspace(root)?;
    let examples = root.join("examples");
    let dest = examples.join(name);
    if dest.exists() {
        return Err(format!("destination already exists: {}", dest.display()));
    }

    let template = root.join(format!("templates/{platform}-app"));
    if !template.is_dir() {
        return Err(format!("template missing at templates/{platform}-app"));
    }
    if platform == "ios" {
        validate_ios_template(&template)?;
    } else {
        validate_android_template(&template)?;
    }

    // Build the app in a hidden staging folder next to its destination and move
    // it into place only when every step succeeded, so a failure never leaves a
    // half-made app behind that blocks the next attempt.
    let staging = examples.join(format!(".{name}.forge-init"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    let result = populate_app(runner, root, &template, &staging, name, bundle_id, platform)
        .and_then(|()| fs::rename(&staging, &dest).map_err(|e| e.to_string()));
    if let Err(err) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    println!("Created Forge app at {}", dest.display());
    println!("Note: apps generated by forge init are workspace-local in v0.1.");
    let rel = dest.strip_prefix(root).unwrap_or(&dest).to_string_lossy();
    if platform == "ios" {
        println!("Linux/xtool: forge build ios --project {rel}");
        println!(
            "macOS/Xcode: open {}/Xcode/{}.xcodeproj (see docs/ios-xcode.md)",
            dest.display(),
            name
        );
    } else {
        println!("Build from repo root: forge build android --project {rel}");
    }
    Ok(())
}

fn populate_app(
    runner: &dyn CommandRunner,
    root: &Path,
    template: &Path,
    dir: &Path,
    name: &str,
    bundle_id: &str,
    platform: &str,
) -> Result<(), String> {
    copy_template(template, dir)?;
    substitute_placeholders(dir, name, bundle_id)?;
    if platform == "ios" {
        relocate_ios_xcode_host(dir, name)?;
    } else {
        relocate_android_package(dir, "org.forgemobile.templatehost", bundle_id)?;
    }
    let generated = dir.join("Generated");
    fs::create_dir_all(&generated).map_err(|e| e.to_string())?;
    let language = if platform == "ios" { "swift" } else { "kotlin" };
    bindings_generate(runner, root, &generated, language)
}

fn validate_ios_template(template: &Path) -> Result<(), String> {
    for rel in [
        "Package.swift",
        "forge.toml",
        "xtool.yml",
        "Sources/AppHostLauncher/AppMain.swift",
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
        if name == "Generated" {
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
        Some("swift" | "toml" | "yml" | "yaml" | "md" | "kts" | "kt" | "xml" | "properties")
    ) || path
        .file_name()
        .is_some_and(|n| n == "Package.swift" || n == "project.pbxproj")
}

/// Runs Rust tests and format checks; with `project`, also the app's host tests
/// (JVM unit tests for an Android app, XCTest on macOS for an iOS app).
pub fn test(runner: &dyn CommandRunner, root: &Path, project: Option<&Path>) -> Result<(), String> {
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

    match project_dir {
        Some(dir) if dir.join("gradlew").is_file() => {
            run_checked(
                runner,
                Cmd::new("cargo")
                    .args(["build", "-p", "forge-ffi"])
                    .cwd(root),
            )?;
            run_checked(
                runner,
                Cmd::new(dir.join("gradlew"))
                    .arg(":app:testDebugUnitTest")
                    .cwd(&dir),
            )
        }
        Some(dir) => run_checked(runner, script(root, "test-swift-host.sh").arg(&dir)),
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
    run_checked(
        runner,
        script(root, "package-android-library.sh")
            .arg(generated_dir_arg(root, &generated))
            .env("FORGE_ANDROID_ABIS", android.abis.join(" "))
            .env(
                "FORGE_ANDROID_JNI_LIBS",
                project_dir.join("app/src/main/jniLibs"),
            ),
    )?;

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

/// Builds the Rust static library and Swift bindings for an iOS app.
fn package_ios(runner: &dyn CommandRunner, root: &Path, project: &Path) -> Result<PathBuf, String> {
    require_workspace(root)?;
    let project_dir = resolve_project_dir(root, project);
    let cfg = load_config(&project_dir)?;
    if !cfg.ios_enabled() {
        return Err("forge.toml does not enable iOS builds ([build] ios_device = true)".into());
    }
    let generated = generated_dir_for_project(root, project);
    run_checked(
        runner,
        script(root, "package-ios-library.sh").arg(generated_dir_arg(root, &generated)),
    )?;
    Ok(project_dir)
}

pub fn build_ios(
    runner: &dyn CommandRunner,
    root: &Path,
    project: &Path,
    package_only: bool,
) -> Result<(), String> {
    let project_dir = package_ios(runner, root, project)?;
    if package_only {
        println!(
            "Rust static library and Swift bindings are ready for {} (open Xcode/ or run xtool dev).",
            project_dir.display()
        );
        return Ok(());
    }
    require_xtool(runner)?;
    run_checked(
        runner,
        Cmd::new("xtool").args(["dev", "build"]).cwd(&project_dir),
    )
}

/// Builds, installs and launches on a connected device (`xtool dev`).
pub fn run_ios(runner: &dyn CommandRunner, root: &Path, project: &Path) -> Result<(), String> {
    let project_dir = package_ios(runner, root, project)?;
    require_xtool(runner)?;
    run_checked(runner, Cmd::new("xtool").arg("dev").cwd(&project_dir))
}

fn require_xtool(runner: &dyn CommandRunner) -> Result<(), String> {
    if which_program(runner, "xtool") {
        Ok(())
    } else {
        Err("xtool not available: use --package-only for macOS/Xcode builds, or install xtool + Darwin SDK (see docs/linux-setup.md)".into())
    }
}

pub fn devices(runner: &dyn CommandRunner) -> Result<(), String> {
    let mut listed = false;
    let mut found_tool = false;
    let mut errors: Vec<String> = Vec::new();

    for (label, program, args) in [
        ("Android (adb)", "adb", &["devices", "-l"][..]),
        ("iOS (xtool)", "xtool", &["devices"]),
    ] {
        if !which_program(runner, program) {
            continue;
        }
        found_tool = true;
        println!("--- {label} ---");
        match runner.capture(&Cmd::new(program).args(args)) {
            Ok(output) => {
                print!("{}", String::from_utf8_lossy(&output.stdout));
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
        Err("no device tools found (install adb and/or xtool)".into())
    }
}

pub fn install_ios(runner: &dyn CommandRunner, ipa: &Path) -> Result<(), String> {
    if !which_program(runner, "xtool") {
        return Err("xtool not installed".into());
    }
    run_checked(runner, Cmd::new("xtool").arg("install").arg(ipa))
}

fn which_program(runner: &dyn CommandRunner, program: &str) -> bool {
    runner
        .capture(&Cmd::new(program).arg("--version"))
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let err = build_ios(&mock, &root, Path::new("apps/demo"), true).unwrap_err();
        assert!(err.contains("does not enable iOS"), "{err}");

        fs::remove_file(root.join("apps/demo/forge.toml")).unwrap();
        let err = build_ios(&mock, &root, Path::new("apps/demo"), true).unwrap_err();
        assert!(err.contains("forge.toml"), "{err}");
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn build_ios_package_only_skips_xtool() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        build_ios(&mock, &root, Path::new("apps/demo"), true).unwrap();
        let lines = mock.call_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("package-ios-library.sh apps/demo/Generated"));
    }

    #[test]
    fn build_ios_builds_and_run_ios_launches() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "").respond("xtool", 0, "");
        build_ios(&mock, &root, Path::new("apps/demo"), false).unwrap();
        assert_eq!(mock.call_lines().last().unwrap(), "xtool dev build");

        let mock = MockRunner::default();
        mock.respond("bash", 0, "").respond("xtool", 0, "");
        run_ios(&mock, &root, Path::new("apps/demo")).unwrap();
        assert_eq!(mock.call_lines().last().unwrap(), "xtool dev");
        assert_eq!(
            mock.calls().last().unwrap().cwd.as_deref(),
            Some(root.join("apps/demo").as_path())
        );
    }

    #[test]
    fn build_ios_without_xtool_explains_package_only() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        let err = build_ios(&mock, &root, Path::new("apps/demo"), false).unwrap_err();
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
        test(&mock, &root, Some(Path::new("apps/demo"))).unwrap();
        let lines = mock.call_lines();
        assert_eq!(lines[0], "cargo test --workspace");
        assert!(lines.contains(&"cargo build -p forge-ffi".to_string()));
        assert!(lines
            .last()
            .unwrap()
            .ends_with("gradlew :app:testDebugUnitTest"));
    }

    #[test]
    fn test_runs_swift_host_tests_for_ios_project() {
        let (_tmp, root) = workspace(IOS_TOML, &["Package.swift"]);
        let mock = MockRunner::default();
        mock.respond("cargo", 0, "").respond("bash", 0, "");
        test(&mock, &root, Some(Path::new("apps/demo"))).unwrap();
        assert!(mock
            .call_lines()
            .last()
            .unwrap()
            .contains("test-swift-host.sh"));
    }

    #[test]
    fn test_rejects_unknown_project_before_running_anything() {
        let (_tmp, root) = workspace(IOS_TOML, &[]);
        let mock = MockRunner::default();
        let err = test(&mock, &root, Some(Path::new("apps/demo"))).unwrap_err();
        assert!(err.contains("not a Forge app"), "{err}");
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn devices_reports_missing_tools() {
        let mock = MockRunner::default();
        let err = devices(&mock).unwrap_err();
        assert!(err.contains("no device tools"));

        let mock = MockRunner::default();
        mock.respond("adb", 0, "List of devices attached\n");
        devices(&mock).unwrap();
        assert!(mock.call_lines().contains(&"adb devices -l".to_string()));
    }

    #[test]
    fn doctor_runs_the_toolchain_script() {
        let root = workspace_root();
        let mock = MockRunner::default();
        mock.respond("bash", 0, "");
        doctor(&mock, &root).unwrap();
        assert!(mock.call_lines()[0].ends_with("scripts/validate-toolchain.sh"));
    }

    #[test]
    fn init_rejects_android_bundle_ids_that_are_not_kotlin_packages() {
        let mock = MockRunner::default();
        for bad in ["org.my-app.demo", "org.example.fun", "org.example.class"] {
            let err = init(&mock, &workspace_root(), "BadPkg", bad, "android").unwrap_err();
            assert!(err.contains("Kotlin package"), "{bad}: {err}");
        }
    }

    #[test]
    fn init_failure_leaves_nothing_behind() {
        let root = workspace_root();
        let name = "InitFailureApp";
        // The mock knows no commands, so binding generation fails after the copy.
        let mock = MockRunner::default();
        let err = init(&mock, &root, name, "org.example.initfail", "ios").unwrap_err();
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
        init(&RealRunner, &root, name, "org.example.inittest", "ios").expect("init");
        let pkg = fs::read_to_string(dest.join("Package.swift")).unwrap();
        assert!(pkg.contains("name: \"InitTestApp\""));
        assert!(!pkg.contains("FORGE_PACKAGE_NAME"));
        assert!(dest.join("Generated/ForgeFFI/forge_ffi.swift").is_file());
        assert!(dest
            .join("Generated/forge_ffiFFI/module.modulemap")
            .is_file());
        assert!(dest
            .join("Xcode/InitTestApp.xcodeproj/project.pbxproj")
            .is_file());
        let vm = fs::read_to_string(dest.join("Sources/AppHost/CounterViewModel.swift")).unwrap();
        assert!(vm.contains("InitTestApp"));
        assert!(dest
            .join("Tests/AppHostTests/CounterViewModelTests.swift")
            .is_file());
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
        )
        .unwrap_err();
        assert!(err.contains("already exists"));
        assert!(dest.exists());
    }
}
