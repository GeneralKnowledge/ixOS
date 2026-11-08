//! `forge doctor`: what this machine can build, and what to install for the
//! rest. `--json` prints the same report as one JSON object for CI and
//! scripts; without it the report is text with one fix per missing piece.

use std::path::Path;

use serde::Serialize;

use crate::commands::is_forge_workspace;
use crate::ios_env::{self, Auth};
use crate::runner::{Cmd, CommandRunner};

/// Bumped when a field is removed or changes meaning; adding fields is not a
/// breaking change.
pub const SCHEMA_VERSION: u32 = 1;

pub const IOS_TARGET: &str = ios_env::IOS_TARGET;
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
    pub ios_device: IosDevice,
    /// The installed xtool against the pinned build in `toolchain/xtool.env`.
    pub xtool_pin: XtoolPin,
    pub ready: Ready,
}

/// The pinned xtool file, compiled in so `forge doctor` and
/// `scripts/install-xtool.sh` read the same version.
const XTOOL_PIN_FILE: &str = include_str!("../../../toolchain/xtool.env");

/// The xtool version Forge is tested with (`XTOOL_VERSION` in
/// `toolchain/xtool.env`).
pub fn pinned_xtool_version() -> &'static str {
    XTOOL_PIN_FILE
        .lines()
        .find_map(|l| l.trim().strip_prefix("XTOOL_VERSION="))
        .map(str::trim)
        .unwrap_or("unknown")
}

/// Whether the installed xtool is the pinned build. A different version is a
/// warning, not a blocker: stock xtool builds apps too, it just isn't what
/// the device results in docs/compatibility.md were recorded with.
#[derive(Debug, Serialize)]
pub struct XtoolPin {
    pub expected: &'static str,
    /// The version xtool reports (`xtool 1.24.0-ixos.1` gives `1.24.0-ixos.1`),
    /// `null` when xtool is missing.
    pub found: Option<String>,
    /// `null` when xtool is missing.
    pub matches: Option<bool>,
}

impl XtoolPin {
    fn check(version_line: Option<&str>) -> Self {
        let expected = pinned_xtool_version();
        let found = version_line.map(|l| {
            l.trim()
                .strip_prefix("xtool")
                .unwrap_or(l)
                .trim()
                .to_string()
        });
        // A local build of the pinned commit reports `<version>-dirty`.
        let matches = found.as_deref().map(|f| {
            f == expected
                || f.strip_prefix(expected)
                    .is_some_and(|rest| rest == "-dirty")
        });
        XtoolPin {
            expected,
            found,
            matches,
        }
    }
}

/// Signing and device state for installing on an iPhone with xtool. Each
/// field is `null` when it could not be checked (xtool missing, or too old to
/// answer).
#[derive(Debug, Default, Serialize)]
pub struct IosDevice {
    /// `"logged_in"` or `"logged_out"` (`xtool auth status`).
    pub apple_sign_in: Option<&'static str>,
    /// Linux: whether usbmuxd's socket exists (`null` elsewhere).
    pub usbmuxd_running: Option<bool>,
    /// Connected devices as xtool lists them (`Name [usb]: UDID`). Not listed
    /// (`null`) when usbmuxd is not running, since xtool would then wait.
    pub devices: Option<Vec<String>>,
    /// iOS version of the SDK in xtool's Darwin SDK, e.g. `"26.0"`.
    pub ios_sdk_version: Option<String>,
    /// The team xtool signs with: `id`, `name`, `kind` (`free`, `paid` or
    /// `unknown`) and `sign_in` (`apple_id` or `api_key`). `null` when signed
    /// out. Finding the kind asks Apple, so it is `unknown` offline.
    pub signing_team: Option<ios_env::SigningTeam>,
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
    /// `forge run ios`: `ios_xtool`, plus a signed-in xtool and a connected iPhone.
    pub ios_device: bool,
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

/// An AppImage (xtool ships as one) that can't mount itself prints this and
/// may still exit 0, so it would pass as a version string.
fn appimage_fuse_error(out: &std::process::Output) -> Option<String> {
    [&out.stdout, &out.stderr]
        .iter()
        .flat_map(|b| {
            String::from_utf8_lossy(b)
                .lines()
                .map(str::trim)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .find(|l| l.contains("fusermount") || l.contains("libfuse"))
}

/// The fix for an xtool that can't start because FUSE is missing.
const FUSE_FIX: &str = "xtool can't start because FUSE is missing (it ships as an AppImage): sudo apt-get install fuse3 (docs/linux-setup.md)";

fn probe(runner: &dyn CommandRunner, name: &'static str, program: &str, args: &[&str]) -> Tool {
    match runner.capture(&Cmd::new(program).args(args)) {
        Ok(out) if appimage_fuse_error(&out).is_some() => Tool {
            name,
            status: Status::Error,
            version: None,
            detail: appimage_fuse_error(&out),
        },
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
    env_fn: &dyn Fn(&str) -> Option<String>,
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
            value: env_fn(name).filter(|v| !v.is_empty()),
        })
        .collect();
    let set = |n: &str| env.iter().any(|e| e.name == n && e.value.is_some());
    let has_target = |t: &str| rust_targets.iter().any(|x| x == t);

    let ios_device = if ok("xtool") {
        IosDevice {
            apple_sign_in: ios_env::xtool_auth(runner).map(Auth::as_str),
            usbmuxd_running: ios_env::usbmuxd_running(runner, env_fn),
            devices: if ios_env::usbmuxd_running(runner, env_fn) == Some(false) {
                None
            } else {
                ios_env::ios_devices(runner)
            },
            ios_sdk_version: ios_env::darwin_ios_sdk_version(runner, env_fn),
            signing_team: ios_env::signing_team(runner),
        }
    } else {
        IosDevice::default()
    };

    let xtool_pin = XtoolPin::check(
        tools
            .iter()
            .find(|t| t.name == "xtool" && t.status == Status::Ok)
            .and_then(|t| t.version.as_deref()),
    );

    let ios_xtool = ok("swift")
        && ok("xtool")
        && has_target(IOS_TARGET)
        && swift_sdks
            .iter()
            .any(|s| s.to_lowercase().contains("darwin"));
    let ready = Ready {
        rust: ok("rustc") && ok("cargo"),
        ios_xtool,
        ios_device: ios_xtool
            && ios_device.apple_sign_in != Some(Auth::LoggedOut.as_str())
            && ios_device.devices.as_ref().is_some_and(|d| !d.is_empty()),
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
        ios_device,
        xtool_pin,
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
    let xtool_needs_fuse = r.tools.iter().any(|t| {
        t.name == "xtool"
            && t.detail
                .as_deref()
                .is_some_and(|d| d.contains("fusermount") || d.contains("libfuse"))
    });
    if xtool_needs_fuse {
        ios.push(FUSE_FIX.to_string());
    } else if !ok("xtool") {
        ios.push("install xtool (docs/linux-setup.md)".to_string());
    }
    if r.xtool_pin.matches == Some(false) {
        ios.push(format!(
            "xtool {} is not the pinned {} that Forge is tested with (still usable): scripts/install-xtool.sh installs it, checksum-verified",
            r.xtool_pin.found.as_deref().unwrap_or("?"),
            r.xtool_pin.expected
        ));
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
    if let Some(v) = &r.ios_device.ios_sdk_version {
        if ios_env::sdk_major(v).is_some_and(|m| m < ios_env::MIN_IOS_SDK_MAJOR) {
            ios.push(format!(
                "xtool's Darwin SDK has iOS {v}, which fails to link with Swift 6.2: run `xtool setup` again with Xcode 26's Xcode.xip"
            ));
        }
    }

    // Installing on a device: signing and the iPhone itself.
    let mut device = Vec::new();
    if !r.ready.ios_xtool {
        device.push("first make \"iOS from Linux\" ready (above)".to_string());
    }
    if r.host.os == "linux" && !ok("usbmuxd") {
        device.push(
            "install usbmuxd, which talks to the iPhone over USB: sudo apt-get install usbmuxd libimobiledevice-utils"
                .to_string(),
        );
    }
    if r.ios_device.apple_sign_in == Some(Auth::LoggedOut.as_str()) {
        device.push(
            "sign xtool in to your Apple account: xtool auth login (docs/ios-signing.md)"
                .to_string(),
        );
    }
    if r.ios_device.usbmuxd_running == Some(false) {
        device.push(
            "usbmuxd is not running (xtool would wait forever): plug in and unlock the iPhone, or sudo systemctl start usbmuxd".to_string(),
        );
    }
    match &r.ios_device.devices {
        _ if r.ios_device.usbmuxd_running == Some(false) => {}
        Some(d) if d.is_empty() => device.push(
            "connect your iPhone by USB, unlock it and tap Trust (then `forge devices`); enable Developer Mode in Settings > Privacy & Security".to_string(),
        ),
        None if ok("xtool") => device.push(
            "could not list devices (`xtool devices --no-wait` failed): sudo systemctl restart usbmuxd, then replug the iPhone".to_string(),
        ),
        _ => {}
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
            name: "Run on an iPhone (Apple sign-in, connected device)",
            ready: r.ready.ios_device,
            fixes: device,
        },
        Workflow {
            name: "Android (Gradle, debug APK; work in progress)",
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
    if r.tools
        .iter()
        .any(|t| t.name == "xtool" && t.status == Status::Ok)
    {
        let d = &r.ios_device;
        out.push_str(&format!(
            "\niOS: Apple sign-in {}, iOS SDK {}, devices: {}\n",
            match d.apple_sign_in {
                Some("logged_in") => "ok",
                Some(_) => "logged out",
                None => "unknown",
            },
            d.ios_sdk_version.as_deref().unwrap_or("unknown"),
            match &d.devices {
                Some(list) if list.is_empty() => "none connected".to_string(),
                Some(list) => list.join(", "),
                None => "could not list".to_string(),
            }
        ));
        if let Some(team) = &d.signing_team {
            out.push_str(&format!("Signing team: {}\n", team.describe()));
            match team.kind {
                ios_env::TeamKind::Free => out.push_str("  Free team: 3 apps per phone, profiles last 7 days, 10 new App IDs a week, no paid-only entitlements\n"),
                ios_env::TeamKind::Paid => out.push_str("  Paid team: no 3-app limit, profiles last up to a year (never past the membership)\n"),
                ios_env::TeamKind::Unknown => out.push_str("  Team type unknown (`xtool ds teams list` failed); free and paid teams have different limits (docs/ios-signing.md)\n"),
            }
        }
    }
    out.push('\n');
    for w in workflows(r) {
        let mark = if w.ready { "ready" } else { "not ready" };
        out.push_str(&format!("{}: {mark}\n", w.name));
        for fix in &w.fixes {
            out.push_str(&format!("  - {fix}\n"));
        }
        if !w.ready && w.name.starts_with("iOS from Linux") && r.host.os == "linux" {
            out.push_str("  `forge setup ios` does most of these for you (it asks before anything needs sudo)\n");
        }
    }
    if r.host.os == "macos" {
        out.push_str("\nOn macOS you can also build iOS apps with Xcode instead of xtool (docs/ios-xcode.md).\n");
    }
    out.push_str("Provisioning profiles and Developer Mode are checked only when you run on the iPhone; forge explains failures there. Tested versions: docs/compatibility.md\n");
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
        let android = &flows[3];
        assert!(android.fixes.iter().any(|f| f == "cargo install cargo-ndk"));
        assert!(android.fixes.iter().any(|f| f.contains("ANDROID_NDK_HOME")));
        let text = render_text(&r);
        assert!(
            text.contains("Android (Gradle, debug APK; work in progress): not ready"),
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
    fn iphone_workflow_needs_sign_in_and_a_device() {
        let mock = MockRunner::default();
        mock.respond("rustc", 0, "rustc 1.97.0\n")
            .respond("cargo --version", 0, "cargo 1.97.0\n")
            .respond("rustup", 0, "aarch64-apple-ios\n")
            .respond("swift", 0, "darwin\n")
            .respond("usbmuxd", 0, "usbmuxd 1.1.1\n")
            .respond("xtool", 0, "xtool 1.24.0\n")
            .respond("xtool auth status", 0, "Logged out\n")
            .respond("xtool devices --no-wait", 0, "\n");
        let r = report(&mock, &workspace_root(), &|_| None);
        assert!(r.ready.ios_xtool);
        assert!(!r.ready.ios_device);
        let v = to_value(&r);
        assert_eq!(v["ios_device"]["apple_sign_in"], "logged_out");
        assert_eq!(v["ios_device"]["devices"], Value::Array(vec![]));
        let device = &workflows(&r)[2];
        assert!(
            device.fixes.iter().any(|f| f.contains("xtool auth login")),
            "{device:?}"
        );
        assert!(
            device.fixes.iter().any(|f| f.contains("tap Trust")),
            "{device:?}"
        );
        assert!(render_text(&r).contains("Apple sign-in logged out"));

        mock.respond("xtool auth status", 0, "Logged in.\n")
            .respond("xtool devices --no-wait", 0, "iPhone [usb]: 0000\n");
        let r = report(&mock, &workspace_root(), &|_| None);
        assert!(r.ready.ios_device);
        assert!(workflows(&r)[2].fixes.is_empty());
    }

    #[test]
    fn xtool_without_fuse_is_an_error_with_the_fuse_fix() {
        let mock = MockRunner::default();
        mock.respond(
            "xtool",
            0,
            "Error: No suitable fusermount binary found on the $PATH\n",
        );
        let r = report(&mock, &workspace_root(), &|_| None);
        let v = to_value(&r);
        assert_eq!(tool(&v, "xtool")["status"], "error");
        assert!(tool(&v, "xtool").get("version").is_none());
        let fixes = &workflows(&r)[1].fixes;
        assert!(fixes.iter().any(|f| f.contains("fuse3")), "{fixes:?}");
        assert!(
            fixes.iter().all(|f| !f.contains("install-xtool.sh")),
            "{fixes:?}"
        );
    }

    #[test]
    fn failing_probe_is_an_error_not_missing() {
        let mock = MockRunner::default();
        mock.respond("xtool", 2, "");
        let v = to_value(&report(&mock, &workspace_root(), &|_| None));
        assert_eq!(tool(&v, "xtool")["status"], "error");
        assert!(tool(&v, "xtool").get("version").is_none());
    }

    #[test]
    fn xtool_pin_matches_the_pinned_build_and_warns_otherwise() {
        let pinned = pinned_xtool_version();
        assert_ne!(
            pinned, "unknown",
            "toolchain/xtool.env has no XTOOL_VERSION"
        );

        let mock = MockRunner::default();
        let v = to_value(&report(&mock, &workspace_root(), &|_| None));
        assert_eq!(v["xtool_pin"]["expected"], pinned);
        assert_eq!(v["xtool_pin"]["matches"], Value::Null);

        for line in [
            format!("xtool {pinned}\n"),
            format!("xtool {pinned}-dirty\n"),
        ] {
            mock.respond("xtool", 0, &line);
            let r = report(&mock, &workspace_root(), &|_| None);
            assert_eq!(r.xtool_pin.matches, Some(true), "{line}");
            assert!(!workflows(&r)[1].fixes.iter().any(|f| f.contains("pinned")));
        }

        mock.respond("xtool", 0, "xtool 1.24.0\n");
        let r = report(&mock, &workspace_root(), &|_| None);
        assert_eq!(r.xtool_pin.found.as_deref(), Some("1.24.0"));
        assert_eq!(r.xtool_pin.matches, Some(false));
        assert!(workflows(&r)[1]
            .fixes
            .iter()
            .any(|f| f.contains("scripts/install-xtool.sh")));
    }
}
