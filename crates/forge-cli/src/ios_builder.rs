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

pub trait IosBuilder: Send + Sync {
    /// Short name for messages and device listings, e.g. `"xtool"`.
    fn name(&self) -> &'static str;

    /// Fails with an actionable message if the backend's tools are missing.
    fn ensure_available(&self, runner: &dyn CommandRunner) -> Result<(), String>;

    /// Compiles the Swift app in `project_dir` (Rust library already packaged).
    fn build(&self, runner: &dyn CommandRunner, project_dir: &Path) -> Result<(), String>;

    /// Builds, installs and launches the app in `project_dir` on a device.
    fn run(&self, runner: &dyn CommandRunner, project_dir: &Path) -> Result<(), String>;

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

// Both commands use xtool's default debug configuration. With Swift 6.2 this needs
// the Darwin SDK from Xcode 26; the iOS 18 SDK from Xcode 16 fails to link with
// `undefined symbol: swift_coroFrameAlloc` (docs/troubleshooting.md).

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

    fn build(&self, runner: &dyn CommandRunner, project_dir: &Path) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_checked(
            runner,
            Cmd::new(XTOOL).args(["dev", "build"]).cwd(project_dir),
        )
    }

    fn run(&self, runner: &dyn CommandRunner, project_dir: &Path) -> Result<(), String> {
        self.ensure_available(runner)?;
        run_checked(
            runner,
            Cmd::new(XTOOL).args(["dev", "run"]).cwd(project_dir),
        )
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
    fn xtool_missing_points_at_package_only() {
        let mock = MockRunner::default();
        let err = XtoolBuilder
            .install(&mock, Path::new("App.ipa"))
            .unwrap_err();
        assert!(err.contains("--package-only"), "{err}");
        assert_eq!(mock.calls().len(), 1, "only the availability probe runs");
    }
}
