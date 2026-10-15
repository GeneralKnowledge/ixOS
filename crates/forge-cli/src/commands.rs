use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{self, ForgeConfig};
use crate::runner::CommandRunner;

pub fn workspace_root() -> PathBuf {
    if let Ok(dir) = std::env::var("FORGE_WORKSPACE") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn run_checked(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<(), String> {
    let output = runner.run(program, args, cwd)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "{program} exited with {}:\n{stdout}{stderr}",
            output.status
        ));
    }
    Ok(())
}

pub fn doctor(runner: &dyn CommandRunner) -> Result<(), String> {
    let root = workspace_root();
    let script = root.join("scripts/validate-toolchain.sh");
    if script.exists() {
        run_checked(runner, "bash", &[script.to_str().unwrap()], Some(&root))?;
    } else {
        println!("Forge workspace root: {}", root.display());
        let _ = try_version(runner, "rustc", &["--version"]);
        let _ = try_version(runner, "cargo", &["--version"]);
        let _ = try_version(runner, "swift", &["--version"]);
        let _ = try_version(runner, "xtool", &["--version"]);
    }
    Ok(())
}

fn try_version(runner: &dyn CommandRunner, program: &str, args: &[&str]) -> Result<(), String> {
    match runner.run(program, args, None) {
        Ok(out) if out.status.success() => {
            print!("{}", String::from_utf8_lossy(&out.stdout));
            Ok(())
        }
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).into()),
        Err(e) => {
            println!("{program}: not available ({e})");
            Ok(())
        }
    }
}

pub fn init(name: &str, bundle_id: &str) -> Result<(), String> {
    if !config::is_valid_project_name(name) {
        return Err(
            "invalid project name: use ASCII letters, digits, hyphen, underscore; must start with a letter".into(),
        );
    }
    let probe = format!(
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
    );
    ForgeConfig::parse(&probe)?;

    let root = workspace_root();
    let dest = root.join("examples").join(name);
    if dest.exists() {
        return Err(format!("destination already exists: {}", dest.display()));
    }

    let template = root.join("templates/ios-app");
    if !template.exists() {
        return Err("template missing at templates/ios-app".into());
    }
    copy_dir_recursive(&template, &dest)?;

    let forge_toml = dest.join("forge.toml");
    let mut content = fs::read_to_string(&forge_toml).map_err(|e| e.to_string())?;
    content = content.replace("Counter", name);
    content = content.replace("org.forgemobile.counter", bundle_id);
    fs::write(&forge_toml, content).map_err(|e| e.to_string())?;

    let xtool_yml = dest.join("xtool.yml");
    let xtool = fs::read_to_string(&xtool_yml).map_err(|e| e.to_string())?;
    let xtool = xtool.replace("org.forgemobile.counter", bundle_id);
    fs::write(&xtool_yml, xtool).map_err(|e| e.to_string())?;

    println!("Created Forge app at {}", dest.display());
    println!("Next: cd {} && forge build ios", dest.display());
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn test(runner: &dyn CommandRunner, _project: Option<&Path>) -> Result<(), String> {
    let root = workspace_root();
    run_checked(runner, "cargo", &["test", "--workspace"], Some(&root))?;
    run_checked(
        runner,
        "cargo",
        &["fmt", "--all", "--", "--check"],
        Some(&root),
    )?;
    Ok(())
}

pub fn bindings_generate(runner: &dyn CommandRunner, out: &Path) -> Result<(), String> {
    let root = workspace_root();
    let script = root.join("scripts/generate-bindings.sh");
    let out_str = out.to_str().ok_or("invalid output path")?;
    run_checked(
        runner,
        "bash",
        &[script.to_str().unwrap(), out_str],
        Some(&root),
    )
}

pub fn build_ios(runner: &dyn CommandRunner, project: &Path) -> Result<(), String> {
    let root = workspace_root();
    let forge_toml = project.join("forge.toml");
    if forge_toml.exists() {
        let content = fs::read_to_string(&forge_toml).map_err(|e| e.to_string())?;
        ForgeConfig::parse(&content)?;
    }

    run_checked(
        runner,
        "bash",
        &[root
            .join("scripts/package-ios-library.sh")
            .to_str()
            .unwrap()],
        Some(&root),
    )?;

    if which_xtool(runner)? {
        run_checked(runner, "xtool", &["dev"], Some(project))?;
    } else {
        return Err(
            "xtool not available: Rust library packaged, but iOS app build requires xtool + Darwin SDK (see docs/linux-setup.md)".into(),
        );
    }
    Ok(())
}

pub fn devices(runner: &dyn CommandRunner) -> Result<(), String> {
    if !which_xtool(runner)? {
        return Err("xtool not installed".into());
    }
    let output = runner.run("xtool", &["devices"], None)?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    Ok(())
}

pub fn install_ios(runner: &dyn CommandRunner, ipa: &Path) -> Result<(), String> {
    if !which_xtool(runner)? {
        return Err("xtool not installed".into());
    }
    let ipa_str = ipa.to_str().ok_or("invalid ipa path")?;
    run_checked(runner, "xtool", &["install", ipa_str], None)
}

pub fn run_ios(runner: &dyn CommandRunner, project: &Path) -> Result<(), String> {
    build_ios(runner, project)?;
    if !which_xtool(runner)? {
        return Err("xtool not installed".into());
    }
    run_checked(runner, "xtool", &["dev"], Some(project))
}

fn which_xtool(runner: &dyn CommandRunner) -> Result<bool, String> {
    match runner.run("xtool", &["--version"], None) {
        Ok(o) if o.status.success() => Ok(true),
        Ok(_) => Ok(false),
        Err(_) => Ok(false),
    }
}
