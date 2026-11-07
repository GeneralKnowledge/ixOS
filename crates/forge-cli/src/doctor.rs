//! `forge doctor`: what this machine can build, and what to install for the
//! rest. `--json` prints the same report as one JSON object for CI and
//! scripts; without it the report is text with one fix per missing piece.

use std::path::Path;

use serde::Serialize;

use crate::commands::is_forge_workspace;
use crate::runner::{Cmd, CommandRunner};

/// Bumped when a field is removed or changes meaning; adding fields is not a
/// breaking change.
pub const SCHEMA_VERSION: u32 = 1;

pub const IOS_TARGET: &str = "aarch64-apple-ios";
pub const ANDROID_TARGETS: [&str; 2] = ["aarch64-linux-android", "x86_64-linux-android"];

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub forge_version: &'static str,
    pub host: HostInfo,
    pub workspace: Workspace,
    pub tools: Vec<Tool>,
    pub rust_targets: Vec<String>,
    pub swift_sdks: Vec<String>,
    pub env: Vec<EnvVar>,
    pub ready: Ready,
}

#[derive(Debug, Serialize)]
pub struct HostInfo {
    pub os: &'static str,
    pub arch: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Workspace {
    pub path: String,
    pub valid: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Ran and exited 0.
    Ok,
    /// Not installed or not on PATH.
    Missing,
    /// Installed, but the version probe exited non-zero.
    Error,
}

#[derive(Debug, Serialize)]
pub struct Tool {
    pub name: &'static str,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EnvVar {
    pub name: &'static str,
    pub value: Option<String>,
}

/// Whether each workflow has what it needs. Device pairing, signing and SDK
/// licences are not checked.
#[derive(Debug, Serialize)]
pub struct Ready {
    /// Workspace tests, bindings, `forge check` core steps.
    pub rust: bool,
    /// `forge build ios` from Linux via xtool (Swift, xtool, Darwin SDK, iOS target).
    pub ios_xtool: bool,
    /// `forge build android` (JDK, SDK, NDK, cargo-ndk, Android targets).
    pub android: bool,
}

const TOOLS: [(&str, &str, &[&str]); 9] = [
    ("rustc", "rustc", &["--version"]),
    ("cargo", "cargo", &["--version"]),
    ("rustup", "rustup", &["--version"]),
    ("swift", "swift", &["--version"]),
    ("xtool", "xtool", &["--version"]),
    ("usbmuxd", "usbmuxd", &["--version"]),
    ("java", "java", &["-version"]),
    ("adb", "adb", &["version"]),
    ("cargo-ndk", "cargo", &["ndk", "--version"]),
];

const ENV_VARS: [&str; 5] = [
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "ANDROID_NDK_HOME",
    "NDK_HOME",
    "FORGE_WORKSPACE",
];

fn first_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        // The JVM announces JAVA_TOOL_OPTIONS before `java -version` output.
        .find(|l| !l.is_empty() && !l.starts_with("Picked up "))
        .map(str::to_string)
}

fn probe(runner: &dyn CommandRunner, name: &'static str, program: &str, args: &[&str]) -> Tool {
    match runner.capture(&Cmd::new(program).args(args)) {
        // `java -version` prints to stderr.
        Ok(out) if out.status.success() => Tool {
            name,
            status: Status::Ok,
            version: first_line(&out.stdout).or_else(|| first_line(&out.stderr)),
            detail: None,
        },
        // A cargo subcommand that is not installed.
        Ok(out) if String::from_utf8_lossy(&out.stderr).contains("no such command") => Tool {
            name,
            status: Status::Missing,
            version: None,
            detail: first_line(&out.stderr),
        },
        Ok(out) => Tool {
            name,
            status: Status::Error,
            version: None,
            detail: first_line(&out.stderr)
                .or_else(|| first_line(&out.stdout))
                .or_else(|| Some(out.status.to_string())),
        },
        Err(e) => Tool {
            name,
            status: Status::Missing,
            version: None,
            detail: Some(e),
        },
    }
}

fn lines_of(runner: &dyn CommandRunner, cmd: Cmd) -> Vec<String> {
    match runner.capture(&cmd) {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub fn report(
    runner: &dyn CommandRunner,
    root: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Report {
    let tools: Vec<Tool> = TOOLS
        .iter()
        .map(|(name, program, args)| probe(runner, name, program, args))
        .collect();
    let ok = |n: &str| tools.iter().any(|t| t.name == n && t.status == Status::Ok);

    let rust_targets = if ok("rustup") {
        lines_of(
            runner,
            Cmd::new("rustup").args(["target", "list", "--installed"]),
        )
    } else {
        Vec::new()
    };
    let swift_sdks = if ok("swift") {
        lines_of(runner, Cmd::new("swift").args(["sdk", "list"]))
    } else {
        Vec::new()
    };
    let env: Vec<EnvVar> = ENV_VARS
        .iter()
        .map(|&name| EnvVar {
            name,
            value: env(name).filter(|v| !v.is_empty()),
        })
        .collect();
    let set = |n: &str| env.iter().any(|e| e.name == n && e.value.is_some());
    let has_target = |t: &str| rust_targets.iter().any(|x| x == t);

    let ready = Ready {
        rust: ok("rustc") && ok("cargo"),
        ios_xtool: ok("swift")
            && ok("xtool")
            && has_target(IOS_TARGET)
            && swift_sdks
                .iter()
                .any(|s| s.to_lowercase().contains("darwin")),
        android: ok("java")
            && ok("cargo-ndk")
            && (set("ANDROID_HOME") || set("ANDROID_SDK_ROOT"))
            && (set("ANDROID_NDK_HOME") || set("NDK_HOME"))
            && ANDROID_TARGETS.iter().any(|t| has_target(t)),
    };

    Report {
        schema_version: SCHEMA_VERSION,
        forge_version: env!("CARGO_PKG_VERSION"),
        host: HostInfo {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        },
        workspace: Workspace {
            path: root.to_string_lossy().into_owned(),
            valid: is_forge_workspace(root),
        },
        tools,
        rust_targets,
        swift_sdks,
        env,
        ready,
    }
}

pub fn print_json(runner: &dyn CommandRunner, root: &Path) -> Result<(), String> {
    let report = report(runner, root, &|k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    });
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

/// One workflow's readiness, with a fix for each missing piece.
#[derive(Debug, PartialEq, Eq)]
pub struct Workflow {
    pub name: &'static str,
    pub ready: bool,
    pub fixes: Vec<String>,
}

/// What each workflow still needs, in the order to fix it.
pub fn workflows(r: &Report) -> Vec<Workflow> {
    let ok = |n: &str| {
        r.tools
            .iter()
            .any(|t| t.name == n && t.status == Status::Ok)
    };
    let set = |n: &str| r.env.iter().any(|e| e.name == n && e.value.is_some());
    let has_target = |t: &str| r.rust_targets.iter().any(|x| x == t);

    let mut rust = Vec::new();
    if !ok("rustc") || !ok("cargo") {
        rust.push("install Rust with rustup: https://rustup.rs".to_string());
    }
    if !r.workspace.valid {
        rust.push(format!(
            "{} is not a Forge checkout: run forge inside the checkout or set FORGE_WORKSPACE",
            r.workspace.path
        ));
    }

    let mut ios = Vec::new();
    if !ok("swift") {
        ios.push("install Swift 6 (docs/linux-setup.md)".to_string());
    }
    if !ok("xtool") {
        ios.push("install xtool (docs/linux-setup.md)".to_string());
    }
    if !has_target(IOS_TARGET) {
        ios.push(format!("rustup target add {IOS_TARGET}"));
    }
    if !r
        .swift_sdks
        .iter()
        .any(|s| s.to_lowercase().contains("darwin"))
    {
        ios.push(
            "xtool setup with an Xcode.xip, to install the Darwin SDK (docs/linux-setup.md)"
                .to_string(),
        );
    }
    // Installing on a device from Linux; not needed to build.
    if r.host.os == "linux" && !ok("usbmuxd") {
        ios.push("to install on an iPhone: install usbmuxd (docs/linux-setup.md)".to_string());
    }

    let mut android = Vec::new();
    if !ok("java") {
        android.push("install JDK 17 (docs/android-setup.md)".to_string());
    }
    if !(set("ANDROID_HOME") || set("ANDROID_SDK_ROOT")) {
        android.push(
            "install the Android SDK and set ANDROID_HOME (docs/android-setup.md)".to_string(),
        );
    }
    if !(set("ANDROID_NDK_HOME") || set("NDK_HOME")) {
        android
            .push("install the NDK and set ANDROID_NDK_HOME (docs/android-setup.md)".to_string());
    }
    if !ok("cargo-ndk") {
        android.push("cargo install cargo-ndk".to_string());
    }
    if !ANDROID_TARGETS.iter().any(|t| has_target(t)) {
        android.push(format!("rustup target add {}", ANDROID_TARGETS.join(" ")));
    }
    if !ok("adb") {
        android
            .push("to install on a phone: put the SDK's platform-tools (adb) on PATH".to_string());
    }

    vec![
        Workflow {
            name: "Rust core, tests and bindings",
            ready: r.ready.rust && r.workspace.valid,
            fixes: rust,
        },
        Workflow {
            name: "iOS from Linux (xtool, device builds)",
            ready: r.ready.ios_xtool,
            fixes: ios,
        },
        Workflow {
            name: "Android (Gradle, debug APK)",
            ready: r.ready.android,
            fixes: android,
        },
    ]
}

/// The text report: tool versions, then each workflow with its fixes.
pub fn render_text(r: &Report) -> String {
    let mut out = format!(
        "Forge {} on {}/{}\nWorkspace: {}{}\n\nTools:\n",
        r.forge_version,
        r.host.os,
        r.host.arch,
        r.workspace.path,
        if r.workspace.valid {
            ""
        } else {
            " (not a Forge checkout)"
        }
    );
    for t in &r.tools {
        let state = match t.status {
            Status::Ok => t.version.clone().unwrap_or_else(|| "ok".into()),
            Status::Missing => "missing".into(),
            Status::Error => format!("error: {}", t.detail.clone().unwrap_or_default()),
        };
        out.push_str(&format!("  {:<10} {state}\n", t.name));
    }
    out.push('\n');
    for w in workflows(r) {
        let mark = if w.ready { "ready" } else { "not ready" };
        out.push_str(&format!("{}: {mark}\n", w.name));
        for fix in &w.fixes {
            out.push_str(&format!("  - {fix}\n"));
        }
    }
    if r.host.os == "macos" {
        out.push_str("\nOn macOS you can also build iOS apps with Xcode instead of xtool (docs/ios-xcode.md).\n");
    }
    out.push_str("Signing, device pairing and SDK licences are not checked. Tested versions: docs/compatibility.md\n");
    out
}

pub fn print_text(runner: &dyn CommandRunner, root: &Path) -> Result<(), String> {
    let report = report(runner, root, &|k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    });
    print!("{}", render_text(&report));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_report_names_a_fix_for_every_missing_piece() {
        let mock = MockRunner::default();
        let r = report(&mock, &workspace_root(), &|_| None);
        let flows = workflows(&r);
        assert!(flows.iter().all(|w| !w.ready));
        let ios = &flows[1];
        assert!(ios
            .fixes
            .iter()
            .any(|f| f == "rustup target add aarch64-apple-ios"));
        assert!(ios.fixes.iter().any(|f| f.contains("xtool setup")));
        let android = &flows[2];
        assert!(android.fixes.iter().any(|f| f == "cargo install cargo-ndk"));
        assert!(android.fixes.iter().any(|f| f.contains("ANDROID_NDK_HOME")));
        let text = render_text(&r);
        assert!(
            text.contains("Android (Gradle, debug APK): not ready"),
            "{text}"
        );
        assert!(text.contains("  xtool      missing"), "{text}");
    }

    use super::*;
    use crate::commands::workspace_root;
    use crate::runner::MockRunner;
    use serde_json::Value;

    fn to_value(r: &Report) -> Value {
        serde_json::to_value(r).unwrap()
    }

    fn tool<'a>(v: &'a Value, name: &str) -> &'a Value {
        v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no tool {name}"))
    }

    #[test]
    fn bare_host_reports_missing_tools_and_nothing_ready() {
        let mock = MockRunner::default();
        let v = to_value(&report(&mock, &workspace_root(), &|_| None));
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["workspace"]["valid"], true);
        assert_eq!(tool(&v, "xtool")["status"], "missing");
        assert_eq!(v["ready"]["rust"], false);
        assert_eq!(v["ready"]["ios_xtool"], false);
        assert_eq!(v["ready"]["android"], false);
        assert_eq!(v["rust_targets"], Value::Array(vec![]));
        assert_eq!(v["env"][0]["name"], "ANDROID_HOME");
        assert_eq!(v["env"][0]["value"], Value::Null);
    }

    #[test]
    fn ready_flags_follow_tools_targets_and_env() {
        let mock = MockRunner::default();
        mock.respond("rustc", 0, "rustc 1.97.0 (2d8144b78 2026-07-07)\n")
            .respond("cargo --version", 0, "cargo 1.97.0\n")
            .respond("cargo ndk", 0, "cargo-ndk 4.1.2\n")
            .respond("rustup --version", 0, "rustup 1.28.2\n")
            .respond(
                "rustup target list",
                0,
                "aarch64-apple-ios\naarch64-linux-android\nx86_64-unknown-linux-gnu\n",
            )
            .respond("swift --version", 0, "Swift version 6.2\n")
            .respond("swift sdk list", 0, "darwin\n")
            .respond("xtool", 0, "xtool 1.16.1\n")
            .respond(
                "java",
                0,
                "Picked up JAVA_TOOL_OPTIONS: -Dx=y\nopenjdk version \"17.0.12\"\n",
            );
        let env = |k: &str| matches!(k, "ANDROID_HOME" | "NDK_HOME").then(|| format!("/opt/{k}"));
        let v = to_value(&report(&mock, &workspace_root(), &env));
        assert_eq!(
            tool(&v, "rustc")["version"],
            "rustc 1.97.0 (2d8144b78 2026-07-07)"
        );
        assert_eq!(tool(&v, "cargo-ndk")["status"], "ok");
        assert_eq!(tool(&v, "java")["version"], "openjdk version \"17.0.12\"");
        assert_eq!(v["ready"]["rust"], true);
        assert_eq!(v["ready"]["ios_xtool"], true);
        assert_eq!(v["ready"]["android"], true);
        assert_eq!(v["swift_sdks"][0], "darwin");

        // Without the NDK variable Android is not ready.
        let env = |k: &str| (k == "ANDROID_HOME").then(|| "/opt/sdk".to_string());
        let v = to_value(&report(&mock, &workspace_root(), &env));
        assert_eq!(v["ready"]["android"], false);
    }

    #[test]
    fn failing_probe_is_an_error_not_missing() {
        let mock = MockRunner::default();
        mock.respond("xtool", 2, "");
        let v = to_value(&report(&mock, &workspace_root(), &|_| None));
        assert_eq!(tool(&v, "xtool")["status"], "error");
        assert!(tool(&v, "xtool").get("version").is_none());
    }
}
