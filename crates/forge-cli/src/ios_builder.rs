//! iOS build backends.
//!
//! Everything that is specific to the toolchain that compiles, signs, installs
//! and launches the Swift app lives behind [`IosBuilder`]. The rest of the CLI
//! (packaging the Rust static library, regenerating bindings, reading
//! `forge.toml`) is backend-independent and stays in `commands.rs`.
//!
//! [`XtoolBuilder`] is the only backend today. A second one (for example
//! `xcodebuild` + `devicectl` on macOS, or a future xtool replacement) only has
//! to implement this trait and be selected in `main.rs`.

use std::path::Path;

use crate::commands::{run_checked, which_program};
use crate::runner::{Cmd, CommandRunner};

/// The Swift build configuration. The Rust library is built with Cargo's
/// release profile either way (`FORGE_IOS_PROFILE` overrides it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IosConfiguration {
    /// Fast to build, debuggable: the everyday loop.
    #[default]
    Debug,
    /// Optimised Swift, as an app would ship.
    Release,
}

impl IosConfiguration {
    pub fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

pub trait IosBuilder: Send + Sync {
    /// Short name for messages and device listings, e.g. `"xtool"`.
    fn name(&self) -> &'static str;

    /// Fails with an actionable message if the backend's tools are missing.
    fn ensure_available(&self, runner: &dyn CommandRunner) -> Result<(), String>;

    /// Compiles the Swift app in `project_dir` (Rust library already packaged).
    fn build(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        configuration: IosConfiguration,
    ) -> Result<(), String>;

    /// Builds, installs and launches the app in `project_dir` on a device.
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

// Debug builds with Swift 6.2 need the Darwin SDK from Xcode 26; the iOS 18 SDK
// from Xcode 16 fails to link them with `undefined symbol: swift_coroFrameAlloc`
// (docs/troubleshooting.md).

/// `xtool dev <verb>`, plus `-c release` for release builds.
fn xtool_dev(verb: &str, project_dir: &Path, configuration: IosConfiguration) -> Cmd {
    let mut cmd = Cmd::new(XTOOL).args(["dev", verb]);
    if configuration == IosConfiguration::Release {
        cmd = cmd.args(["-c", "release"]);
    }
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

    fn build(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        configuration: IosConfiguration,
    ) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_checked(runner, xtool_dev("build", project_dir, configuration))
    }

    fn run(
        &self,
        runner: &dyn CommandRunner,
        project_dir: &Path,
        configuration: IosConfiguration,
    ) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_checked(runner, xtool_dev("run", project_dir, configuration))
    }

    fn install(&self, runner: &dyn CommandRunner, ipa: &Path) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_checked(runner, Cmd::new(XTOOL).arg("install").arg(ipa))
    }

    fn devices_command(&self) -> Option<Cmd> {
        Some(Cmd::new(XTOOL).arg("devices"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;

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
        let mock = MockRunner::default();
        mock.respond("xtool", 0, "");
        let dir = Path::new("/apps/demo");
        XtoolBuilder
            .build(&mock, dir, IosConfiguration::Release)
            .unwrap();
        XtoolBuilder
            .run(&mock, dir, IosConfiguration::Release)
            .unwrap();
        XtoolBuilder
            .run(&mock, dir, IosConfiguration::Debug)
            .unwrap();
        let lines = mock.call_lines();
        let xtool: Vec<_> = lines
            .iter()
            .filter(|l| l.starts_with("xtool dev"))
            .collect();
        assert_eq!(
            xtool,
            [
                "xtool dev build -c release",
                "xtool dev run -c release",
                "xtool dev run"
            ]
        );
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
