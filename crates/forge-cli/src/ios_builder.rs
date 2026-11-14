//! iOS build backends.
//!
//! Everything that is specific to the toolchain that compiles, signs, installs
//! and installs the Swift app lives behind [`IosBuilder`]. The rest of the CLI
//! (packaging the Rust static library, regenerating bindings, reading
//! `forge.toml`) is backend-independent and stays in `commands.rs`.
//!
//! [`XtoolBuilder`] is the only backend today. A second one (for example
//! `xcodebuild` + `devicectl` on macOS, or a future xtool replacement) only has
//! to implement this trait and be selected in `main.rs`.

use std::path::Path;

use crate::commands::{run_explained, which_program};
use crate::diagnose::Stage;
use crate::ios_env::{self, Auth};
use crate::runner::{Cmd, CommandRunner};

/// The Swift build configuration (`forge build ios --release`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IosConfiguration {
    #[default]
    Debug,
    Release,
}

impl IosConfiguration {
    pub fn name(self) -> &'static str {
        match self {
            IosConfiguration::Debug => "debug",
            IosConfiguration::Release => "release",
        }
    }
}

/// What the user is about to do, so the preflight checks only what it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Compile the app (`forge build ios`).
    Build,
    /// What `forge run ios` needs before it builds: the toolchain and the
    /// Apple sign-in, but not the iPhone, which is looked for only once the
    /// app is built ([`Purpose::Install`]).
    Run,
    /// Install an app that is already built: only the iPhone (usbmuxd
    /// running, a phone listed). Checked after the build, so a missing
    /// phone never holds up compiling.
    Install,
}

/// Something that would make the build fail, found before it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The layer it belongs to, for `--json` results and exit codes.
    pub stage: Stage,
    pub problem: String,
    pub fix: String,
}

/// The outcome of [`IosBuilder::preflight`]: problems stop the command,
/// warnings are printed and the command carries on.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub problems: Vec<Problem>,
    pub warnings: Vec<String>,
}

impl Preflight {
    fn problem(&mut self, stage: Stage, problem: impl Into<String>, fix: impl Into<String>) {
        self.problems.push(Problem {
            stage,
            problem: problem.into(),
            fix: fix.into(),
        });
    }

    /// The error for a preflight with problems, or `Ok` after printing any warnings.
    pub fn into_result(self, what: &str) -> Result<(), String> {
        for w in &self.warnings {
            eprintln!("warning: {w}");
        }
        if self.problems.is_empty() {
            return Ok(());
        }
        crate::report::record_failure(
            what,
            self.problems
                .iter()
                .map(|p| crate::report::JsonHint::new(p.stage, &p.problem, &p.fix))
                .collect(),
            "",
        );
        let mut msg = format!("{what} cannot start yet:");
        for p in &self.problems {
            msg.push_str(&format!("\n  - {}\n    Fix: {}", p.problem, p.fix));
        }
        msg.push_str(&format!(
            "\n\nRun `forge doctor` for the full report. To skip these checks, set {SKIP_PREFLIGHT}=1."
        ));
        Err(msg)
    }
}

/// Set to `1` to skip [`IosBuilder::preflight`] (for example when a check is
/// wrong about an unusual setup).
pub const SKIP_PREFLIGHT: &str = "FORGE_SKIP_PREFLIGHT";

pub trait IosBuilder: Send + Sync {
    /// Short name for messages and device listings, e.g. `"xtool"`.
    fn name(&self) -> &'static str;

    /// Fails with an actionable message if the backend's tools are missing.
    fn ensure_available(&self, runner: &dyn CommandRunner) -> Result<(), String>;

    /// Fast checks (seconds) of everything `purpose` needs, run before the
    /// Rust library is built so a missing SDK, sign-in or iPhone is reported
    /// at once instead of after a long build.
    fn preflight(&self, _runner: &dyn CommandRunner, _purpose: Purpose) -> Preflight {
        Preflight::default()
    }

    /// Compiles the Swift app in `project_dir` (Rust library already packaged).
    fn build(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        configuration: IosConfiguration,
    ) -> Result<(), String>;

    /// Signs and installs the app that [`IosBuilder::build`] just built in
    /// `project_dir` (`product` is its `[ios] xtool_product`) on a device,
    /// without building again. Not launched.
    fn install_build(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        product: &str,
    ) -> Result<(), String>;

    /// Installs an already-built `.ipa` on a device.
    fn install(&self, runner: &dyn CommandRunner, ipa: &Path) -> Result<(), String>;

    /// Command that lists connected iOS devices, if the backend has one.
    fn devices_command(&self) -> Option<Cmd>;
}

/// [xtool](https://github.com/xtool-org/xtool): builds and deploys from Linux
/// with the Darwin SDK extracted from Xcode.xip.
#[derive(Debug, Default, Clone, Copy)]
pub struct XtoolBuilder;

const XTOOL: &str = "xtool";

/// How long an xtool install may stay silent after `[Installing] 100%` before
/// Forge stops it. A healthy install goes on to print `[Verifying]` progress
/// (which keeps it alive); on Cati two reinstalls printed nothing more and
/// never exited, though one of them had installed the app.
const INSTALL_GRACE: std::time::Duration = std::time::Duration::from_secs(90);

/// Set when the last `install_build` stopped xtool after `[Installing] 100%` without
/// a verification (reported as `data.verified: false` by `forge run ios`).
pub static INSTALL_UNVERIFIED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether xtool's output (the last few KB) says the install finished:
/// its progress reaches `[Installing] 100%` (it may redraw with `\r`).
fn install_finished(tail: &str) -> bool {
    tail.split(['\n', '\r'])
        .any(|line| line.contains("Installing") && line.contains("100%"))
}

// Debug is xtool's default configuration; `--release` passes `-c release`.
// With Swift 6.2 debug builds need the Darwin SDK from Xcode 26; the iOS 18 SDK
// from Xcode 16 fails to link with `undefined symbol: swift_coroFrameAlloc`
// (docs/troubleshooting.md).

/// Where `xtool dev build` puts the app it builds (unsigned).
pub fn built_app(project_dir: &Path, product: &str) -> std::path::PathBuf {
    project_dir.join("xtool").join(format!("{product}.app"))
}

fn xtool_dev(subcommand: &str, project_dir: &Path, configuration: IosConfiguration) -> Cmd {
    let cmd = Cmd::new(XTOOL).args(["dev", subcommand]);
    let cmd = match configuration {
        IosConfiguration::Debug => cmd,
        IosConfiguration::Release => cmd.args(["-c", "release"]),
    };
    cmd.cwd(project_dir)
}

impl IosBuilder for XtoolBuilder {
    fn name(&self) -> &'static str {
        XTOOL
    }

    fn ensure_available(&self, runner: &dyn CommandRunner) -> Result<(), String> {
        if which_program(runner, XTOOL) {
            Ok(())
        } else {
            Err("xtool is not installed: install it and its Darwin SDK (docs/linux-setup.md; `forge doctor` lists what is missing). On macOS you can instead build with --package-only and Xcode (docs/ios-xcode.md)".into())
        }
    }

    fn preflight(&self, runner: &dyn CommandRunner, purpose: Purpose) -> Preflight {
        xtool_preflight(runner, purpose, &|k| {
            std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
        })
    }

    fn build(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        configuration: IosConfiguration,
    ) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_explained(runner, xtool_dev("build", project_dir, configuration))
    }

    fn install_build(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        product: &str,
    ) -> Result<(), String> {
        self.ensure_available(runner)?;
        // `xtool dev build` leaves the unsigned app in xtool/<product>.app;
        // `xtool install` signs it for the connected phone and installs it,
        // exactly as `xtool dev run` does after its own build.
        let app = built_app(project_dir, product);
        if !app.is_dir() {
            return Err(format!(
                "{} is missing: the Swift build did not produce the app (run `forge build ios` and read its output)",
                app.display()
            ));
        }
        let cmd = Cmd::new(XTOOL).arg("install").arg(&app).cwd(project_dir);
        let (status, tail, cut_short) =
            runner.stream_tail_until(&cmd, &install_finished, INSTALL_GRACE)?;
        if cut_short {
            // Seen on Cati (iOS 26.3.1, 2026-11-07): two of four installs
            // stopped after [Installing] 100% with no [Verifying] line.
            println!(
                "[forge] xtool reached [Installing] 100% and then printed nothing for {} s, so Forge stopped it. The app is most likely installed, but xtool did not report verifying it: open it to check, and if the phone shows the old version, run this again.",
                INSTALL_GRACE.as_secs()
            );
            INSTALL_UNVERIFIED.store(true, std::sync::atomic::Ordering::Relaxed);
            // Not a silent success: the result says it wasn't verified.
            crate::report::add_hint(crate::report::JsonHint::new(
                crate::diagnose::Stage::Device,
                "xtool was stopped after [Installing] 100% without verifying the install (data.verified: false).",
                "Check the app opens and is the new build (`forge device apps` shows its version); if not, run the install again.",
            ));
            return Ok(());
        }
        if status.success() {
            Ok(())
        } else {
            Err(crate::diagnose::explain(&cmd.display(), status, &tail))
        }
    }

    fn install(&self, runner: &dyn CommandRunner, ipa: &Path) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_explained(runner, Cmd::new(XTOOL).arg("install").arg(ipa))
    }

    fn devices_command(&self) -> Option<Cmd> {
        // Without --no-wait, xtool waits until an iPhone is plugged in.
        Some(Cmd::new(XTOOL).args(["devices", "--no-wait"]))
    }
}

fn xtool_preflight(
    runner: &dyn CommandRunner,
    purpose: Purpose,
    env: &dyn Fn(&str) -> Option<String>,
) -> Preflight {
    let mut pf = Preflight::default();

    if purpose == Purpose::Install {
        device_checks(runner, env, &mut pf);
        return pf;
    }
    if !which_program(runner, XTOOL) {
        pf.problem(
            Stage::Swift,
            "xtool is not installed (it builds, signs and installs iOS apps from Linux).",
            "Install xtool and run `xtool setup` (docs/linux-setup.md). On macOS you can instead build with --package-only and Xcode (docs/ios-xcode.md).",
        );
        // Everything below asks xtool or its SDK.
        return pf;
    }
    match ios_env::rust_ios_target(runner) {
        Some(false) => pf.problem(
            Stage::Rust,
            format!("The Rust target {} is not installed.", ios_env::IOS_TARGET),
            format!("rustup target add {}", ios_env::IOS_TARGET),
        ),
        Some(true) => {}
        None => pf.warnings.push(format!(
            "rustup not found, so the {} target was not checked",
            ios_env::IOS_TARGET
        )),
    }
    match ios_env::darwin_swift_sdk(runner) {
        Some(false) => pf.problem(
            Stage::Swift,
            "xtool's Darwin SDK is not installed.",
            "Run `xtool setup` and give it Xcode.xip from Xcode 26 (docs/linux-setup.md).",
        ),
        Some(true) => {}
        None => pf.problem(
            Stage::Swift,
            "Swift is not installed (or `swift sdk list` fails).",
            "Install Swift 6.2 (docs/linux-setup.md).",
        ),
    }
    if let Some(version) = ios_env::darwin_ios_sdk_version(runner, env) {
        if ios_env::sdk_major(&version).is_some_and(|m| m < ios_env::MIN_IOS_SDK_MAJOR) {
            match ios_env::swift_version(runner) {
                // Swift 6.2 emits swift_coroFrameAlloc, which only the iOS 26
                // SDK's runtime exports: the link is certain to fail.
                Some((major, minor)) if (major, minor) >= (6, 2) => pf.problem(
                    Stage::Swift,
                    format!(
                        "Swift {major}.{minor} does not match xtool's Darwin SDK (iOS {version}): the app will fail to link with `undefined symbol: swift_coroFrameAlloc`."
                    ),
                    "Run `xtool setup` again with Xcode.xip from Xcode 26 (docs/troubleshooting.md).",
                ),
                Some(_) => {}
                None => pf.warnings.push(format!(
                    "xtool's Darwin SDK has iOS {version}; with Swift 6.2 builds fail to link (swift_coroFrameAlloc). Run `xtool setup` again with Xcode 26's Xcode.xip"
                )),
            }
        }
    }

    if purpose == Purpose::Run && ios_env::xtool_auth(runner) == Some(Auth::LoggedOut) {
        pf.problem(
            Stage::Signing,
            "xtool is not signed in to an Apple account, so it cannot sign the app for your iPhone.",
            "xtool auth login   (any Apple ID works; a paid account can use an API key)",
        );
    }
    pf
}

/// Whether an iPhone is there to install on ([`Purpose::Install`]).
fn device_checks(
    runner: &dyn CommandRunner,
    env: &dyn Fn(&str) -> Option<String>,
    pf: &mut Preflight,
) {
    // The phone sometimes drops off usbmuxd while still plugged in: when
    // none is seen, restart usbmuxd once and look again.
    let mut running = ios_env::usbmuxd_running(runner, env);
    let mut devices = (running != Some(false)).then(|| ios_env::ios_devices(runner));
    if !matches!(devices, Some(Some(ref d)) if !d.is_empty()) {
        let mut recovery = ios_env::UsbmuxdRecovery::new(env, std::time::Duration::ZERO);
        if let Some(note) = recovery.tick(runner, std::time::Duration::ZERO) {
            println!("[forge] {note}");
            running = ios_env::usbmuxd_running(runner, env);
            devices = (running != Some(false)).then(|| ios_env::ios_devices(runner));
        }
    }
    if running == Some(false) {
        pf.problem(
            Stage::Device,
            "usbmuxd is not running, so xtool would wait for the iPhone forever without saying so.",
            "Plug the iPhone in by USB and unlock it (udev usually starts usbmuxd then), or `sudo systemctl start usbmuxd`. `forge devices` should then list it.",
        );
        return;
    }
    match devices.flatten() {
        Some(devices) if devices.is_empty() => pf.problem(
            Stage::Device,
            "No iPhone is connected.",
            "Connect it by USB, unlock it and tap Trust; `forge devices` should list it. If it does not: `sudo systemctl restart usbmuxd` and replug.",
        ),
        Some(_) => {}
        None => pf.warnings.push(
            "could not list iOS devices (`xtool devices --no-wait` failed or timed out): usbmuxd may be stuck, and then xtool waits forever without a message. If the install seems to hang: sudo systemctl restart usbmuxd, replug and unlock the iPhone".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;

    #[test]
    fn install_finished_reads_xtool_progress() {
        assert!(install_finished("Installing 37%\rInstalling 100%"));
        assert!(install_finished("[forge] x\nInstalling app: 100%\n"));
        assert!(!install_finished("Installing 99%\n"));
        assert!(!install_finished("Building 100%\n"));
    }

    /// A Linux machine with everything set up and an iPhone connected.
    fn ready_host() -> MockRunner {
        let mock = MockRunner::default();
        mock.respond("xtool --version", 0, "xtool 1.24.0\n")
            .respond(
                "rustup target list",
                0,
                "aarch64-apple-ios\nx86_64-unknown-linux-gnu\n",
            )
            .respond("swift sdk list", 0, "darwin\n")
            .respond("xtool auth status", 0, "Logged in.\n")
            .respond("xtool devices --no-wait", 0, "iPhone [usb]: 0000\n");
        mock
    }

    #[test]
    fn preflight_passes_on_a_ready_host() {
        let pf = xtool_preflight(&ready_host(), Purpose::Run, &|_| None);
        assert!(pf.problems.is_empty(), "{pf:?}");
        assert!(pf.into_result("forge run ios").is_ok());
    }

    #[test]
    fn preflight_without_xtool_stops_at_xtool() {
        let pf = xtool_preflight(&MockRunner::default(), Purpose::Build, &|_| None);
        assert_eq!(pf.problems.len(), 1, "{pf:?}");
        let err = pf.into_result("forge build ios").unwrap_err();
        assert!(err.contains("xtool setup"), "{err}");
        assert!(err.contains("FORGE_SKIP_PREFLIGHT=1"), "{err}");
    }

    #[test]
    fn preflight_reports_every_missing_piece_for_run() {
        let mock = ready_host();
        mock.respond("rustup target list", 0, "x86_64-unknown-linux-gnu\n")
            .respond("swift sdk list", 0, "\n")
            .respond("xtool auth status", 0, "Logged out\n")
            .respond("xtool devices --no-wait", 0, "\n");
        let pf = xtool_preflight(&mock, Purpose::Run, &|_| None);
        let fixes: Vec<&str> = pf.problems.iter().map(|p| p.fix.as_str()).collect();
        // The iPhone is not one of them: it is looked for after the build.
        assert_eq!(fixes.len(), 3, "{fixes:?}");
        assert!(fixes[0].contains("rustup target add aarch64-apple-ios"));
        assert!(fixes[1].contains("xtool setup"));
        assert!(fixes[2].contains("xtool auth login"));

        // Installing needs only the iPhone.
        let pf = xtool_preflight(&mock, Purpose::Install, &|_| None);
        assert_eq!(pf.problems.len(), 1, "{pf:?}");
        assert!(pf.problems[0].fix.contains("forge devices"));

        // A build needs neither the sign-in nor the iPhone.
        let pf = xtool_preflight(&mock, Purpose::Build, &|_| None);
        assert_eq!(pf.problems.len(), 2, "{pf:?}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn install_without_usbmuxd_stops_before_xtool_can_hang() {
        let mock = ready_host();
        mock.respond("test -S /var/run/usbmuxd", 1, "");
        let pf = xtool_preflight(&mock, Purpose::Install, &|_| None);
        assert_eq!(pf.problems.len(), 1, "{pf:?}");
        assert!(pf.problems[0].problem.contains("usbmuxd is not running"));
        assert!(
            mock.call_lines()
                .iter()
                .all(|l| !l.contains("xtool devices")),
            "xtool devices must not run without usbmuxd"
        );
        // Neither a build nor the checks before `forge run ios` builds need the iPhone.
        for purpose in [Purpose::Build, Purpose::Run] {
            assert!(xtool_preflight(&mock, purpose, &|_| None)
                .problems
                .is_empty());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn install_without_a_phone_restarts_usbmuxd_once_and_looks_again() {
        let mock = ready_host();
        mock.respond("xtool devices --no-wait", 0, "\n")
            .respond("sudo -n -l systemctl restart usbmuxd", 0, "")
            .respond("sudo -n systemctl restart usbmuxd", 0, "")
            .respond("sleep", 0, "");
        let pf = xtool_preflight(&mock, Purpose::Install, &|_| None);
        assert_eq!(pf.problems.len(), 1, "{pf:?}");
        assert!(pf.problems[0].problem.contains("No iPhone"), "{pf:?}");
        let calls = mock.call_lines();
        let restart = calls
            .iter()
            .position(|l| l == "sudo -n systemctl restart usbmuxd")
            .expect("restarted");
        assert_eq!(
            calls
                .iter()
                .filter(|l| l.contains("restart usbmuxd") && !l.contains("-l"))
                .count(),
            1,
            "{calls:?}"
        );
        assert!(
            calls[restart..].iter().any(|l| l.contains("xtool devices")),
            "looked again: {calls:?}"
        );

        // A phone in the first listing: nothing is restarted.
        let mock = ready_host();
        mock.respond("sudo", 0, "");
        assert!(xtool_preflight(&mock, Purpose::Install, &|_| None)
            .problems
            .is_empty());
        assert!(mock.call_lines().iter().all(|l| !l.contains("sudo")));
    }

    #[test]
    fn swift_6_2_with_an_old_sdk_is_a_problem() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(
            home.path()
                .join(".swiftpm/swift-sdks/darwin/iPhoneOS18.0.sdk"),
        )
        .unwrap();
        let h = home.path().to_string_lossy().into_owned();
        let mock = ready_host();
        mock.respond(
            "swift --version",
            0,
            "Swift version 6.2.3 (swift-6.2.3-RELEASE)\n",
        );
        let pf = xtool_preflight(&mock, Purpose::Build, &move |k| {
            (k == "HOME").then(|| h.clone())
        });
        assert_eq!(pf.problems.len(), 1, "{pf:?}");
        assert!(pf.problems[0].problem.contains("swift_coroFrameAlloc"));
        assert!(pf.problems[0].fix.contains("Xcode 26"));
    }

    #[test]
    fn old_ios_sdk_is_a_warning() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(
            home.path()
                .join(".swiftpm/swift-sdks/darwin/iPhoneOS18.0.sdk"),
        )
        .unwrap();
        let h = home.path().to_string_lossy().into_owned();
        let pf = xtool_preflight(&ready_host(), Purpose::Build, &move |k| {
            (k == "HOME").then(|| h.clone())
        });
        assert!(pf.problems.is_empty());
        assert!(pf.warnings[0].contains("iOS 18.0"), "{pf:?}");
    }

    #[test]
    fn failed_xtool_install_is_explained() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(built_app(tmp.path(), "DemoXtool")).unwrap();
        let mock = ready_host();
        mock.respond(
            "xtool install",
            1,
            "Error: The maximum number of apps for free development profiles has been reached.",
        );
        let err = XtoolBuilder
            .install_build(&mock, tmp.path(), "DemoXtool")
            .unwrap_err();
        assert!(err.contains("`xtool install"), "{err}");
        assert!(err.contains("[Signing and provisioning]"), "{err}");
        assert!(err.contains("Remove App"), "{err}");
    }

    #[test]
    fn xtool_install_runs_xtool_install() {
        let mock = MockRunner::default();
        mock.respond("xtool", 0, "");
        XtoolBuilder
            .install(&mock, Path::new("/tmp/App.ipa"))
            .unwrap();
        assert_eq!(
            mock.call_lines().last().unwrap(),
            "xtool install /tmp/App.ipa"
        );
    }

    #[test]
    fn release_builds_pass_the_configuration_to_xtool() {
        let dir = Path::new("/apps/Demo");
        for (configuration, build) in [
            (IosConfiguration::Debug, "xtool dev build"),
            (IosConfiguration::Release, "xtool dev build -c release"),
        ] {
            let mock = MockRunner::default();
            mock.respond("xtool", 0, "");
            XtoolBuilder.build(&mock, dir, configuration).unwrap();
            let lines = mock.call_lines();
            assert!(lines.contains(&build.to_string()), "{lines:?}");
        }
    }

    #[test]
    fn install_build_installs_the_app_the_build_left_without_building_again() {
        let tmp = tempfile::tempdir().unwrap();
        let app = built_app(tmp.path(), "DemoXtool");
        let mock = MockRunner::default();
        mock.respond("xtool", 0, "");
        // Nothing built yet: say so rather than let xtool fail on a path.
        let err = XtoolBuilder
            .install_build(&mock, tmp.path(), "DemoXtool")
            .unwrap_err();
        assert!(err.contains("DemoXtool.app is missing"), "{err}");

        std::fs::create_dir_all(&app).unwrap();
        XtoolBuilder
            .install_build(&mock, tmp.path(), "DemoXtool")
            .unwrap();
        let lines = mock.call_lines();
        assert!(
            lines.contains(&format!("xtool install {}", app.display())),
            "{lines:?}"
        );
        assert!(lines.iter().all(|l| !l.contains("dev")), "{lines:?}");
    }

    #[test]
    fn xtool_missing_points_at_package_only() {
        let mock = MockRunner::default();
        let err = XtoolBuilder
            .install(&mock, Path::new("App.ipa"))
            .unwrap_err();
        assert!(err.contains("--package-only"), "{err}");
        assert_eq!(mock.calls().len(), 1, "only the availability probe runs");
    }
}
