//! Quick probes of the Linux iOS toolchain, shared by `forge doctor` and the
//! preflight that `forge build ios` / `forge run ios` run before spending
//! minutes on a Rust build: the iOS Rust target, xtool's Darwin SDK and the iOS
//! version it was made from, the Apple sign-in xtool signs with, and the
//! connected iPhones.

use std::path::{Path, PathBuf};

use crate::runner::{Cmd, CommandRunner};

pub const IOS_TARGET: &str = "aarch64-apple-ios";

/// The oldest iOS SDK that links with Swift 6.2 (Xcode 26). Xcode 16's iOS 18
/// SDK fails with `undefined symbol: swift_coroFrameAlloc`.
pub const MIN_IOS_SDK_MAJOR: u32 = 26;

/// `Some(true/false)` when the answer is known, `None` when the probe itself
/// could not run (the tool that would answer is missing).
pub type Known = Option<bool>;

fn stdout_of(runner: &dyn CommandRunner, cmd: Cmd) -> Option<String> {
    match runner.capture(&cmd) {
        Ok(out) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        _ => None,
    }
}

/// Whether `rustup` has the iOS device target.
pub fn rust_ios_target(runner: &dyn CommandRunner) -> Known {
    stdout_of(
        runner,
        Cmd::new("rustup").args(["target", "list", "--installed"]),
    )
    .map(|out| out.lines().any(|l| l.trim() == IOS_TARGET))
}

/// Whether SwiftPM has xtool's Darwin SDK.
pub fn darwin_swift_sdk(runner: &dyn CommandRunner) -> Known {
    stdout_of(runner, Cmd::new("swift").args(["sdk", "list"]))
        .map(|out| out.to_lowercase().contains("darwin"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    LoggedIn,
    LoggedOut,
}

impl Auth {
    pub fn as_str(self) -> &'static str {
        match self {
            Auth::LoggedIn => "logged_in",
            Auth::LoggedOut => "logged_out",
        }
    }
}

/// xtool's Apple sign-in (`xtool auth status`), or `None` if xtool cannot say.
pub fn xtool_auth(runner: &dyn CommandRunner) -> Option<Auth> {
    let out = stdout_of(runner, Cmd::new("xtool").args(["auth", "status"]))?;
    let first = out
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .to_lowercase();
    if first.starts_with("logged in") {
        Some(Auth::LoggedIn)
    } else if first.starts_with("logged out") {
        Some(Auth::LoggedOut)
    } else {
        None
    }
}

/// Connected iOS devices as xtool prints them (`Name [usb]: UDID`), or `None`
/// if the listing failed or timed out (usbmuxd not answering).
///
/// `--no-wait` makes xtool return at once instead of waiting for a device; the
/// `timeout` wrapper (where available) guards against usbmuxd hanging.
pub fn ios_devices(runner: &dyn CommandRunner) -> Option<Vec<String>> {
    let list = Cmd::new("xtool").args(["devices", "--no-wait"]);
    let cmd = if crate::commands::which_program(runner, "timeout") {
        Cmd::new("timeout")
            .arg("10")
            .arg(&list.program)
            .args(&list.args)
    } else {
        list
    };
    stdout_of(runner, cmd).map(|out| {
        out.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("Waiting for"))
            .map(str::to_string)
            .collect()
    })
}

/// The iOS SDK inside xtool's Darwin SDK: its version (e.g. `"26.0"`), found
/// by looking for `iPhoneOS*.sdk` under the Swift SDK directories. `None` when
/// it cannot be found; callers treat that as unknown, not as missing.
pub fn darwin_ios_sdk_version(
    runner: &dyn CommandRunner,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    // What the Rust build uses (scripts/build-rust-ios.sh): SwiftPM's
    // configuration of the Darwin SDK names the iPhoneOS SDK directly.
    for triple in [IOS_TARGET, "arm64-apple-ios"] {
        let config = stdout_of(
            runner,
            Cmd::new("swift").args(["sdk", "configure", "darwin", triple, "--show-configuration"]),
        );
        let sdk_root = config.as_deref().and_then(|c| {
            c.lines()
                .filter_map(|l| l.trim().strip_prefix("sdkRootPath:"))
                .map(|p| PathBuf::from(p.trim()))
                .next()
        });
        if let Some(v) = sdk_root.as_deref().and_then(sdk_dir_version) {
            return Some(v);
        }
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    // Newer xtool: `xtool sdk status` prints `  Path: <bundle>`.
    if let Some(out) = stdout_of(runner, Cmd::new("xtool").args(["sdk", "status"])) {
        roots.extend(
            out.lines()
                .filter_map(|l| l.trim().strip_prefix("Path:"))
                .map(|p| PathBuf::from(p.trim())),
        );
    }
    if let Some(xdg) = env("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        roots.push(Path::new(&xdg).join("swiftpm/swift-sdks"));
    }
    if let Some(home) = env("HOME").filter(|v| !v.is_empty()) {
        roots.push(Path::new(&home).join(".swiftpm/swift-sdks"));
        roots.push(Path::new(&home).join(".config/swiftpm/swift-sdks"));
        roots.push(Path::new(&home).join(".local/share/swiftpm/swift-sdks"));
    }
    roots.iter().find_map(|root| find_iphoneos_sdk(root, 8))
}

fn find_iphoneos_sdk(dir: &Path, depth: usize) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with("iPhoneOS") && name.ends_with(".sdk") {
            if let Some(v) = sdk_dir_version(&path) {
                return Some(v);
            }
            continue;
        }
        // Do not follow symlinked directories: SDK bundles link back into themselves.
        if depth > 0 && entry.file_type().is_ok_and(|t| t.is_dir()) {
            subdirs.push(path);
        }
    }
    subdirs.sort();
    subdirs.iter().find_map(|d| find_iphoneos_sdk(d, depth - 1))
}

/// The version of an `iPhoneOS*.sdk` directory: from its SDKSettings.json, or
/// from a versioned name like `iPhoneOS26.0.sdk`.
fn sdk_dir_version(sdk: &Path) -> Option<String> {
    if let Some(v) = sdk_settings_version(sdk) {
        return Some(v);
    }
    let name = sdk.file_name()?.to_str()?;
    let v = name.strip_prefix("iPhoneOS")?.strip_suffix(".sdk")?;
    (!v.is_empty()).then(|| v.to_string())
}

fn sdk_settings_version(sdk: &Path) -> Option<String> {
    let text = std::fs::read_to_string(sdk.join("SDKSettings.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("Version")?.as_str().map(str::to_string)
}

/// The major version of an SDK version string (`"26.0"` → 26).
pub fn sdk_major(version: &str) -> Option<u32> {
    version.split('.').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;
    use std::fs;

    #[test]
    fn auth_status_is_parsed() {
        let mock = MockRunner::default();
        mock.respond("xtool auth status", 0, "Logged in.\nAppleID(...)\n");
        assert_eq!(xtool_auth(&mock), Some(Auth::LoggedIn));
        let mock = MockRunner::default();
        mock.respond("xtool auth status", 0, "Logged out\n");
        assert_eq!(xtool_auth(&mock), Some(Auth::LoggedOut));
        assert_eq!(xtool_auth(&MockRunner::default()), None);
    }

    #[test]
    fn devices_are_listed_without_waiting() {
        let mock = MockRunner::default();
        mock.respond("timeout --version", 0, "timeout (GNU coreutils) 9.4\n")
            .respond(
                "timeout 10 xtool devices --no-wait",
                0,
                "Reuben's iPhone [usb]: 00008110-000A\n\n",
            );
        assert_eq!(
            ios_devices(&mock),
            Some(vec!["Reuben's iPhone [usb]: 00008110-000A".to_string()])
        );
        let mock = MockRunner::default();
        mock.respond("xtool devices --no-wait", 0, "\n");
        assert_eq!(ios_devices(&mock), Some(vec![]));
        // Timed out (exit 124) or failed: unknown, not "none connected".
        let mock = MockRunner::default();
        mock.respond("timeout", 0, "")
            .respond("timeout 10", 124, "");
        assert_eq!(ios_devices(&mock), None);
    }

    #[test]
    fn ios_sdk_version_from_settings_or_name() {
        let home = tempfile::tempdir().unwrap();
        let sdks = home.path().join(".swiftpm/swift-sdks/darwin.artifactbundle/Developer/Platforms/iPhoneOS.platform/Developer/SDKs");
        fs::create_dir_all(sdks.join("iPhoneOS.sdk")).unwrap();
        fs::write(
            sdks.join("iPhoneOS.sdk/SDKSettings.json"),
            r#"{"CanonicalName":"iphoneos26.0","Version":"26.0"}"#,
        )
        .unwrap();
        let h = home.path().to_string_lossy().into_owned();
        let env = move |k: &str| (k == "HOME").then(|| h.clone());
        let mock = MockRunner::default();
        assert_eq!(darwin_ios_sdk_version(&mock, &env).as_deref(), Some("26.0"));

        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            home.path()
                .join(".swiftpm/swift-sdks/darwin/iPhoneOS18.0.sdk"),
        )
        .unwrap();
        let h = home.path().to_string_lossy().into_owned();
        let env = move |k: &str| (k == "HOME").then(|| h.clone());
        assert_eq!(darwin_ios_sdk_version(&mock, &env).as_deref(), Some("18.0"));
        assert_eq!(sdk_major("18.0"), Some(18));

        // SwiftPM's configuration of the Darwin SDK wins.
        let sdk = home.path().join("elsewhere/iPhoneOS26.1.sdk");
        fs::create_dir_all(&sdk).unwrap();
        mock.respond(
            "swift sdk configure darwin aarch64-apple-ios",
            0,
            &format!("swiftResourcesPath: /x\nsdkRootPath: {}\n", sdk.display()),
        );
        assert_eq!(darwin_ios_sdk_version(&mock, &env).as_deref(), Some("26.1"));
        assert_eq!(
            darwin_ios_sdk_version(&MockRunner::default(), &|_| None),
            None
        );
    }
}
