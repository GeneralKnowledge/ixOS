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

/// Which kind of Apple team signs the app: it decides the app limit on a
/// phone, how long a signature lasts and which entitlements Apple provisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamKind {
    /// A free Apple ID's personal team: 3 apps per phone, 7-day profiles.
    Free,
    /// An Apple Developer Program team.
    Paid,
    /// xtool could not say (the team listing failed or the team was not in it).
    Unknown,
}

impl TeamKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamKind::Free => "free",
            TeamKind::Paid => "paid",
            TeamKind::Unknown => "unknown",
        }
    }
}

/// The team xtool signs with, as `xtool auth status` and `xtool ds teams list`
/// report it. The Apple ID itself is never kept.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SigningTeam {
    /// The 10-character team ID (not known for an API key sign-in).
    pub id: Option<String>,
    /// The team's name from the team listing.
    pub name: Option<String>,
    pub kind: TeamKind,
    /// `"apple_id"` (password sign-in) or `"api_key"` (App Store Connect key).
    pub sign_in: &'static str,
}

impl SigningTeam {
    /// `Reuben Dennison (ABCDE12345), paid team`.
    pub fn describe(&self) -> String {
        let who = match (&self.name, &self.id) {
            (Some(name), Some(id)) => format!("{name} ({id})"),
            (None, Some(id)) => id.clone(),
            (Some(name), None) => name.clone(),
            (None, None) if self.sign_in == "api_key" => "an App Store Connect API key".into(),
            (None, None) => "the signed-in Apple ID's team".into(),
        };
        let kind = match self.kind {
            TeamKind::Free => "free personal team",
            TeamKind::Paid => "paid Apple Developer Program team",
            TeamKind::Unknown => "team type unknown",
        };
        format!("{who}, {kind}")
    }
}

/// The team xtool signs with, or `None` when xtool is signed out or cannot
/// say. For a password sign-in this asks Apple for the team list (`xtool ds
/// teams list`, bounded by `timeout` where available), so it needs network.
///
/// xtool counts a team as paid when one of its memberships is the Apple
/// Developer Program for iOS (`DeveloperServicesTeam.isFree`); this uses the
/// same rule on the listing it prints.
pub fn signing_team(runner: &dyn CommandRunner) -> Option<SigningTeam> {
    let status = stdout_of(runner, Cmd::new("xtool").args(["auth", "status"]))?;
    let (sign_in, team_id) = parse_auth_status(&status)?;
    if sign_in == "api_key" {
        // App Store Connect API keys exist only for paid memberships.
        return Some(SigningTeam {
            id: None,
            name: None,
            kind: TeamKind::Paid,
            sign_in,
        });
    }
    let list = Cmd::new("xtool").args(["ds", "teams", "list"]);
    let list = if crate::commands::which_program(runner, "timeout") {
        Cmd::new("timeout")
            .arg("20")
            .arg(&list.program)
            .args(&list.args)
    } else {
        list
    };
    let teams = stdout_of(runner, list)
        .map(|out| parse_teams(&out))
        .unwrap_or_default();
    let found = team_id
        .as_deref()
        .and_then(|id| teams.iter().find(|t| t.id == id));
    Some(SigningTeam {
        id: team_id.clone(),
        name: found.map(|t| t.name.clone()),
        kind: found.map_or(TeamKind::Unknown, |t| t.kind),
        sign_in,
    })
}

/// `("apple_id", Some(team id))` or `("api_key", None)` from `xtool auth
/// status`; `None` when signed out.
fn parse_auth_status(out: &str) -> Option<(&'static str, Option<String>)> {
    let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
    if !lines.next()?.to_lowercase().starts_with("logged in") {
        return None;
    }
    let mut team = None;
    for line in lines {
        let line = line.trim_start_matches("- ");
        if line.starts_with("ASC key ID:") {
            return Some(("api_key", None));
        }
        if let Some(id) = line.strip_prefix("Team ID:") {
            team = Some(id.trim().to_string()).filter(|t| !t.is_empty());
        }
    }
    Some(("apple_id", team))
}

#[derive(Debug, PartialEq, Eq)]
struct ListedTeam {
    id: String,
    name: String,
    kind: TeamKind,
}

/// `xtool ds teams list`: `Name [status]: TEAMID`, then one
/// `- Membership name (platform)` line per membership.
fn parse_teams(out: &str) -> Vec<ListedTeam> {
    let mut teams: Vec<ListedTeam> = Vec::new();
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(membership) = line.strip_prefix("- ") {
            if let Some(team) = teams.last_mut() {
                if membership.contains("Apple Developer Program") && membership.ends_with("(ios)") {
                    team.kind = TeamKind::Paid;
                }
            }
        } else if let Some((head, id)) = line.rsplit_once("]: ") {
            let name = head.rsplit_once(" [").map_or(head, |(n, _)| n);
            teams.push(ListedTeam {
                id: id.trim().to_string(),
                name: name.trim().to_string(),
                kind: TeamKind::Free,
            });
        }
    }
    teams
}

/// What to say when `forge.toml`'s `[ios.signing] team_type` and the team
/// xtool actually signs with disagree, or when it is unset. `None` when they
/// agree or the team's kind is unknown.
pub fn team_mismatch(configured: Option<&str>, team: &SigningTeam) -> Option<String> {
    let actual = team.kind;
    match (configured, actual) {
        (_, TeamKind::Unknown) => None,
        (Some("paid"), TeamKind::Free) => Some(format!(
            "forge.toml says [ios.signing] team_type = \"paid\", but xtool signs with {}. Apps will get the free limits (3 per phone, 7 days) and xtool drops paid-only entitlements. Sign in with the paid team: `xtool auth logout`, then `xtool auth login` and pick it (`xtool ds teams list` shows the teams)",
            team.describe()
        )),
        (Some("free"), TeamKind::Paid) => Some(format!(
            "xtool signs with {}, but forge.toml says [ios.signing] team_type = \"free\". Set team_type = \"paid\" so Forge allows paid-only entitlements",
            team.describe()
        )),
        (None, kind) => Some(format!(
            "xtool signs with {}. Set [ios.signing] team_type = \"{}\" in forge.toml so Forge checks entitlements for it",
            team.describe(),
            kind.as_str()
        )),
        _ => None,
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

/// Whether usbmuxd is running on Linux: its socket exists. `None` off Linux,
/// or when `USBMUXD_SOCKET_ADDRESS` points libusbmuxd somewhere else.
///
/// On many distributions udev starts usbmuxd when an iPhone is plugged in
/// and stops it when the last one is unplugged, so a missing socket also
/// means no iPhone is connected. Without it xtool waits forever, silently.
pub fn usbmuxd_running(
    runner: &dyn CommandRunner,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<bool> {
    if !cfg!(target_os = "linux") || env("USBMUXD_SOCKET_ADDRESS").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    // Through the runner (`test -S`), so tests can answer it.
    runner
        .capture(&Cmd::new("test").args(["-S", USBMUXD_SOCKET]))
        .ok()
        .map(|out| out.status.success())
}

/// Where usbmuxd listens on Linux.
pub const USBMUXD_SOCKET: &str = "/var/run/usbmuxd";

/// The environment variable that caps how many times one command restarts
/// usbmuxd while it expects an iPhone and sees none (`0` turns it off).
pub const USBMUXD_RESTARTS_ENV: &str = "FORGE_USBMUXD_RESTARTS";

/// Restarts per command when [`USBMUXD_RESTARTS_ENV`] is unset.
pub const DEFAULT_USBMUXD_RESTARTS: u32 = 3;

/// How long usbmuxd gets after a restart to find the phone again.
const USBMUXD_SETTLE_SECS: &str = "4";

/// What happened when Forge tried to restart usbmuxd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbmuxdRestart {
    /// `systemctl restart usbmuxd` ran and succeeded.
    Restarted,
    /// This user may not run it without a password, so Forge never tries
    /// (it never prompts). The fix is a one-line sudoers rule.
    NotAllowed,
    /// It was allowed but failed or timed out.
    Failed(String),
}

/// The one-time setup that lets Forge restart usbmuxd on its own.
pub const USBMUXD_SUDOERS_HINT: &str = "To let Forge restart usbmuxd by itself when the iPhone drops off USB, allow that one command without a password (docs/linux-setup.md#let-forge-restart-usbmuxd-optional): echo \"$USER ALL=(root) NOPASSWD: /usr/bin/systemctl restart usbmuxd\" | sudo tee /etc/sudoers.d/forge-usbmuxd && sudo chmod 440 /etc/sudoers.d/forge-usbmuxd";

/// How many restarts one command may make: `None` when it must not try at
/// all (not Linux, a custom usbmuxd socket, or `FORGE_USBMUXD_RESTARTS=0`).
pub fn usbmuxd_restarts_allowed(env: &dyn Fn(&str) -> Option<String>) -> Option<u32> {
    if !cfg!(target_os = "linux") || env("USBMUXD_SOCKET_ADDRESS").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let n = match env(USBMUXD_RESTARTS_ENV) {
        Some(v) if !v.trim().is_empty() => v.trim().parse().unwrap_or(DEFAULT_USBMUXD_RESTARTS),
        _ => DEFAULT_USBMUXD_RESTARTS,
    };
    (n > 0).then_some(n)
}

/// Restarts usbmuxd once (`sudo -n systemctl restart usbmuxd`, under a
/// 20-second `timeout`), then gives it a few seconds to find the phone.
///
/// `sudo -n` never asks for a password: unless the sudoers rule in
/// [`USBMUXD_SUDOERS_HINT`] allows the command, `sudo -n -l` says no first
/// and nothing runs.
pub fn restart_usbmuxd(runner: &dyn CommandRunner) -> UsbmuxdRestart {
    let restart = ["systemctl", "restart", "usbmuxd"];
    let allowed = runner
        .capture(&Cmd::new("sudo").args(["-n", "-l"]).args(restart))
        .is_ok_and(|o| o.status.success());
    if !allowed {
        return UsbmuxdRestart::NotAllowed;
    }
    let sudo = Cmd::new("sudo").arg("-n").args(restart);
    let cmd = if crate::commands::which_program(runner, "timeout") {
        Cmd::new("timeout")
            .arg("20")
            .arg(&sudo.program)
            .args(&sudo.args)
    } else {
        sudo
    };
    match runner.capture(&cmd) {
        Ok(out) if out.status.success() => {
            // Through the runner, so tests don't wait.
            let _ = runner.capture(&Cmd::new("sleep").arg(USBMUXD_SETTLE_SECS));
            UsbmuxdRestart::Restarted
        }
        Ok(out) if out.status.code() == Some(124) => {
            UsbmuxdRestart::Failed("`systemctl restart usbmuxd` timed out after 20 s".into())
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            UsbmuxdRestart::Failed(if err.is_empty() {
                format!("`systemctl restart usbmuxd` failed ({})", out.status)
            } else {
                format!("`systemctl restart usbmuxd` failed: {err}")
            })
        }
        Err(e) => UsbmuxdRestart::Failed(e),
    }
}

/// Bounded usbmuxd restarts for a command that waits for an iPhone it
/// cannot see (`forge run ios --wait-for-device`).
///
/// On the device host the phone sometimes drops off usbmuxd while still
/// plugged in, and only a restart (or a replug) brings it back. The first
/// restart comes after `first_after` without a phone, so a person still
/// plugging it in is left alone; each later one waits twice as long as the
/// gap before, and there are at most [`usbmuxd_restarts_allowed`] of them.
/// Without the sudoers rule it says how to add it, once, and stops.
#[derive(Debug)]
pub struct UsbmuxdRecovery {
    left: u32,
    max: u32,
    next: std::time::Duration,
    gap: std::time::Duration,
}

impl UsbmuxdRecovery {
    pub fn new(env: &dyn Fn(&str) -> Option<String>, first_after: std::time::Duration) -> Self {
        let max = usbmuxd_restarts_allowed(env).unwrap_or(0);
        Self {
            left: max,
            max,
            next: first_after,
            gap: first_after.max(std::time::Duration::from_secs(15)) * 2,
        }
    }

    /// Call while no phone is seen, with how long the command has been
    /// waiting. Restarts usbmuxd when one is due and returns a line saying
    /// what happened; `None` when nothing was done.
    pub fn tick(
        &mut self,
        runner: &dyn CommandRunner,
        waited: std::time::Duration,
    ) -> Option<String> {
        if self.left == 0 || waited < self.next {
            return None;
        }
        let attempt = self.max - self.left + 1;
        self.left -= 1;
        self.next = waited + self.gap;
        self.gap *= 2;
        let secs = waited.as_secs();
        Some(match restart_usbmuxd(runner) {
            UsbmuxdRestart::Restarted => format!(
                "No iPhone seen for {secs} s: restarted usbmuxd (attempt {attempt} of {})",
                self.max
            ),
            UsbmuxdRestart::NotAllowed => {
                self.left = 0;
                format!("No iPhone seen for {secs} s, and Forge may not restart usbmuxd. {USBMUXD_SUDOERS_HINT}")
            }
            UsbmuxdRestart::Failed(why) => format!(
                "No iPhone seen for {secs} s; restarting usbmuxd failed (attempt {attempt} of {}): {why}",
                self.max
            ),
        })
    }
}

/// The Swift compiler's version, e.g. `(6, 2)` from `Swift version 6.2.3 (...)`.
pub fn swift_version(runner: &dyn CommandRunner) -> Option<(u32, u32)> {
    let out = stdout_of(runner, Cmd::new("swift").arg("--version"))?;
    let rest = &out[out.find("Swift version ")? + "Swift version ".len()..];
    let mut parts = rest
        .split(|c: char| !c.is_ascii_digit())
        .take(2)
        .map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??))
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
        let swift = MockRunner::default();
        swift.respond(
            "swift --version",
            0,
            "Swift version 6.2.3 (swift-6.2.3-RELEASE)\nTarget: x86_64-unknown-linux-gnu\n",
        );
        assert_eq!(swift_version(&swift), Some((6, 2)));

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

    #[test]
    fn signing_team_comes_from_auth_status_and_the_team_list() {
        let mock = MockRunner::default();
        mock.respond(
            "xtool auth status",
            0,
            "Logged in.\n- Apple ID: someone@example.com\n- Team ID: ABCDE12345\n- Token expiry: 1 Dec 2026\n",
        )
        .respond(
            "xtool ds teams list",
            0,
            "Someone [active]: ZZZZZ99999\n- Xcode Free Provisioning Program (ios)\nSomeone Ltd [active]: ABCDE12345\n- Apple Developer Program (ios)\n- Apple Developer Program (mac)\n",
        );
        let team = signing_team(&mock).unwrap();
        assert_eq!(team.id.as_deref(), Some("ABCDE12345"));
        assert_eq!(team.name.as_deref(), Some("Someone Ltd"));
        assert_eq!(team.kind, TeamKind::Paid);
        assert_eq!(team.sign_in, "apple_id");
        assert!(!format!("{team:?}").contains("someone@example.com"));

        // The personal team of the same Apple ID is free.
        let mock = MockRunner::default();
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
        assert_eq!(signing_team(&mock).unwrap().kind, TeamKind::Free);

        // The listing failed (no network): the team is known, its kind is not.
        let mock = MockRunner::default();
        mock.respond(
            "xtool auth status",
            0,
            "Logged in.\n- Team ID: ZZZZZ99999\n",
        )
        .respond("xtool ds teams list", 1, "");
        let team = signing_team(&mock).unwrap();
        assert_eq!(
            (team.id.as_deref(), team.kind),
            (Some("ZZZZZ99999"), TeamKind::Unknown)
        );

        // API keys are paid-only; signed out means no team.
        let mock = MockRunner::default();
        mock.respond(
            "xtool auth status",
            0,
            "Logged in.\n- ASC key ID: K\n- Issuer ID: I\n",
        );
        let team = signing_team(&mock).unwrap();
        assert_eq!((team.kind, team.sign_in), (TeamKind::Paid, "api_key"));
        let mock = MockRunner::default();
        mock.respond("xtool auth status", 0, "Logged out\n");
        assert_eq!(signing_team(&mock), None);
    }

    /// A host whose sudoers rule allows the restart.
    fn restart_allowed() -> MockRunner {
        let mock = MockRunner::default();
        mock.respond("sudo -n -l systemctl restart usbmuxd", 0, "")
            .respond("timeout --version", 0, "timeout (GNU coreutils) 9.4\n")
            .respond("timeout 20 sudo -n systemctl restart usbmuxd", 0, "")
            .respond("sleep", 0, "");
        mock
    }

    fn restarts(mock: &MockRunner) -> usize {
        mock.call_lines()
            .iter()
            .filter(|l| l.ends_with("sudo -n systemctl restart usbmuxd"))
            .count()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn usbmuxd_restarts_are_spaced_out_and_capped() {
        use std::time::Duration;
        let mock = restart_allowed();
        let mut r = UsbmuxdRecovery::new(&|_| None, Duration::from_secs(20));
        // Not yet: someone may still be plugging the phone in.
        assert_eq!(r.tick(&mock, Duration::from_secs(10)), None);
        let note = r.tick(&mock, Duration::from_secs(20)).unwrap();
        assert!(
            note.contains("restarted usbmuxd (attempt 1 of 3)"),
            "{note}"
        );
        // Then 40 s later, then 80 s after that, then never again.
        assert_eq!(r.tick(&mock, Duration::from_secs(59)), None);
        assert!(r.tick(&mock, Duration::from_secs(60)).is_some());
        assert_eq!(r.tick(&mock, Duration::from_secs(139)), None);
        assert!(r
            .tick(&mock, Duration::from_secs(140))
            .unwrap()
            .contains("3 of 3"));
        assert_eq!(r.tick(&mock, Duration::from_secs(10_000)), None);
        assert_eq!(restarts(&mock), 3);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn usbmuxd_is_not_restarted_without_the_sudoers_rule_or_when_turned_off() {
        use std::time::Duration;
        // `sudo -n -l` says no: explain the rule once, never run sudo itself.
        let mock = MockRunner::default();
        mock.respond("sudo -n -l", 1, "");
        let mut r = UsbmuxdRecovery::new(&|_| None, Duration::ZERO);
        let note = r.tick(&mock, Duration::ZERO).unwrap();
        assert!(
            note.contains("NOPASSWD: /usr/bin/systemctl restart usbmuxd"),
            "{note}"
        );
        assert_eq!(r.tick(&mock, Duration::from_secs(600)), None);
        assert_eq!(restarts(&mock), 0);

        let mock = restart_allowed();
        let off = |k: &str| (k == USBMUXD_RESTARTS_ENV).then(|| "0".to_string());
        let mut r = UsbmuxdRecovery::new(&off, Duration::ZERO);
        assert_eq!(r.tick(&mock, Duration::ZERO), None);
        assert!(mock.call_lines().is_empty());
        let one = |k: &str| (k == USBMUXD_RESTARTS_ENV).then(|| "1".to_string());
        assert_eq!(usbmuxd_restarts_allowed(&one), Some(1));
        let custom =
            |k: &str| (k == "USBMUXD_SOCKET_ADDRESS").then(|| "127.0.0.1:27015".to_string());
        assert_eq!(usbmuxd_restarts_allowed(&custom), None);
    }

    #[test]
    fn a_failed_or_hung_restart_is_reported() {
        let mock = restart_allowed();
        mock.respond("timeout 20 sudo", 124, "");
        assert!(
            matches!(restart_usbmuxd(&mock), UsbmuxdRestart::Failed(why) if why.contains("timed out"))
        );
        let mock = restart_allowed();
        mock.respond("timeout 20 sudo", 5, "");
        assert!(matches!(restart_usbmuxd(&mock), UsbmuxdRestart::Failed(_)));
        assert_eq!(
            restart_usbmuxd(&restart_allowed()),
            UsbmuxdRestart::Restarted
        );
    }

    #[test]
    fn team_type_mismatches_are_explained() {
        let team = |kind| SigningTeam {
            id: Some("ABCDE12345".into()),
            name: Some("Someone".into()),
            kind,
            sign_in: "apple_id",
        };
        assert!(team_mismatch(Some("paid"), &team(TeamKind::Paid)).is_none());
        assert!(team_mismatch(Some("free"), &team(TeamKind::Free)).is_none());
        assert!(team_mismatch(Some("paid"), &team(TeamKind::Unknown)).is_none());
        let wrong = team_mismatch(Some("paid"), &team(TeamKind::Free)).unwrap();
        assert!(wrong.contains("free personal team") && wrong.contains("xtool auth login"));
        assert!(team_mismatch(Some("free"), &team(TeamKind::Paid))
            .unwrap()
            .contains("team_type = \"paid\""));
        assert!(team_mismatch(None, &team(TeamKind::Paid))
            .unwrap()
            .contains("Set [ios.signing] team_type = \"paid\""));
    }
}
