//! `forge doctor --json`: the same readiness facts as `forge doctor`, as one
//! JSON object on stdout for CI and scripts.

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

#[cfg(test)]
mod tests {
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
