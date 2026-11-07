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
    /// Compile, sign and install on an iPhone (`forge run ios`).
    Run,
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

    /// Builds and installs the app in `project_dir` on a device (not launched).
    fn run(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        configuration: IosConfiguration,
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

// Debug is xtool's default configuration; `--release` passes `-c release`.
// With Swift 6.2 debug builds need the Darwin SDK from Xcode 26; the iOS 18 SDK
// from Xcode 16 fails to link with `undefined symbol: swift_coroFrameAlloc`
// (docs/troubleshooting.md).

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

    fn run(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        configuration: IosConfiguration,
    ) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_explained(runner, xtool_dev("run", project_dir, configuration))
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

    if purpose == Purpose::Run {
        if ios_env::xtool_auth(runner) == Some(Auth::LoggedOut) {
            pf.problem(
                Stage::Signing,
                "xtool is not signed in to an Apple account, so it cannot sign the app for your iPhone.",
                "xtool auth login   (any Apple ID works; a paid account can use an API key)",
            );
        }
        if ios_env::usbmuxd_running(runner, env) == Some(false) {
            pf.problem(
                Stage::Device,
                "usbmuxd is not running, so xtool would wait for the iPhone forever without saying so.",
                "Plug the iPhone in by USB and unlock it (udev usually starts usbmuxd then), or `sudo systemctl start usbmuxd`. `forge devices` should then list it.",
            );
            return pf;
        }
        match ios_env::ios_devices(runner) {
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
    pf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;

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
        assert_eq!(fixes.len(), 4, "{fixes:?}");
        assert!(fixes[0].contains("rustup target add aarch64-apple-ios"));
        assert!(fixes[1].contains("xtool setup"));
        assert!(fixes[2].contains("xtool auth login"));
        assert!(fixes[3].contains("forge devices"));

        // A build needs neither the sign-in nor the iPhone.
        let pf = xtool_preflight(&mock, Purpose::Build, &|_| None);
        assert_eq!(pf.problems.len(), 2, "{pf:?}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn run_without_usbmuxd_stops_before_xtool_can_hang() {
        let mock = ready_host();
        mock.respond("test -S /var/run/usbmuxd", 1, "");
        let pf = xtool_preflight(&mock, Purpose::Run, &|_| None);
        assert_eq!(pf.problems.len(), 1, "{pf:?}");
        assert!(pf.problems[0].problem.contains("usbmuxd is not running"));
        assert!(
            mock.call_lines()
                .iter()
                .all(|l| !l.contains("xtool devices")),
            "xtool devices must not run without usbmuxd"
        );
        // A build does not need the iPhone.
        assert!(xtool_preflight(&mock, Purpose::Build, &|_| None)
            .problems
            .is_empty());
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
    fn failed_xtool_run_is_explained() {
        let mock = ready_host();
        mock.respond(
            "xtool dev run",
            1,
            "Error: The maximum number of apps for free development profiles has been reached.",
        );
        let err = XtoolBuilder
            .run(&mock, Path::new("/tmp/app"), IosConfiguration::Debug)
            .unwrap_err();
        assert!(err.contains("`xtool dev run` failed"), "{err}");
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
        for (configuration, build, run) in [
            (IosConfiguration::Debug, "xtool dev build", "xtool dev run"),
            (
                IosConfiguration::Release,
                "xtool dev build -c release",
                "xtool dev run -c release",
            ),
        ] {
            let mock = MockRunner::default();
            mock.respond("xtool", 0, "");
            XtoolBuilder.build(&mock, dir, configuration).unwrap();
            XtoolBuilder.run(&mock, dir, configuration).unwrap();
            let lines = mock.call_lines();
            assert!(lines.contains(&build.to_string()), "{lines:?}");
            assert!(lines.contains(&run.to_string()), "{lines:?}");
        }
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
