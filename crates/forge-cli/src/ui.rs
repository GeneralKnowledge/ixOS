//! `forge ui`: drive an app on a connected iPhone from Linux (Forge UI
//! Testing M0). The phone work is in the `forge-uitest` crate; this file
//! wires it to the command line. See docs/forge-ui-testing.md.

use std::io::Write;
use std::path::Path;

use forge_uitest::phone::{find_ddi, find_runner, resolve_app, Phone, Runner};
use forge_uitest::script::{self, Script};
use forge_uitest::wda::WdaDriver;
use forge_uitest::Driver;

use crate::report;
use crate::runner::{Cmd, CommandRunner};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Free Apple accounts can have this many sideloaded apps on a phone.
const FREE_APP_LIMIT: usize = 3;

const NO_RUNNER: &str = "the WebDriverAgent runner is not installed: run `forge ui setup --runner <WebDriverAgentRunner-Runner.app>` (see docs/forge-ui-testing.md)";

/// `forge ui setup`: mount the Developer Disk Image, install the runner,
/// and check that it starts.
pub fn setup(
    runner: &dyn CommandRunner,
    udid: Option<&str>,
    ddi: Option<&Path>,
    wda_app: Option<&Path>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let phone = Phone::open(udid).map_err(err)?;
    let major = phone.ios_major().map_err(err)?;
    if major < 17 {
        return Err(format!(
            "this phone runs iOS {major}; Forge UI Testing needs iOS 17 or later"
        ));
    }

    if phone.ddi_mounted().map_err(err)? {
        writeln!(out, "Developer Disk Image: mounted").map_err(err)?;
    } else {
        let Some(dir) = ddi else {
            return Err("the Developer Disk Image is not mounted (it is unmounted at every reboot): \
                        pass --ddi <folder> with Xcode's Contents/Resources/CoreDeviceDDIs (docs/forge-ui-testing.md)"
                .into());
        };
        let files = find_ddi(dir)?;
        writeln!(out, "Mounting the Developer Disk Image from {} (Apple signs it for this phone, so this needs internet)…", files.image.display())
            .map_err(err)?;
        phone.mount_ddi(&files).map_err(err)?;
        writeln!(out, "Developer Disk Image: mounted").map_err(err)?;
    }

    let mut apps = phone.user_apps().map_err(err)?;
    if let Some(app) = wda_app {
        if find_runner(&apps).is_none() && apps.len() >= FREE_APP_LIMIT {
            writeln!(
                out,
                "Note: {} apps are installed ({}). A free Apple account allows {FREE_APP_LIMIT}; if the install fails, delete one on the phone first.",
                apps.len(),
                apps.join(", ")
            )
            .map_err(err)?;
        }
        writeln!(
            out,
            "Installing the runner with xtool (it signs it with your Apple ID)…"
        )
        .map_err(err)?;
        let mut cmd = Cmd::new("xtool").arg("install").arg(app);
        if let Some(udid) = udid {
            cmd = cmd.arg("--udid").arg(udid);
        }
        let status = runner.stream(&cmd)?;
        if !status.success() {
            return Err(format!("xtool install failed ({status})"));
        }
        apps = phone.user_apps().map_err(err)?;
    }
    let runner_id = find_runner(&apps).ok_or(NO_RUNNER)?.to_owned();
    writeln!(out, "Runner: {runner_id}").map_err(err)?;
    writeln!(
        out,
        "Apps installed: {} of {FREE_APP_LIMIT} on a free account ({})",
        apps.len(),
        apps.join(", ")
    )
    .map_err(err)?;

    writeln!(out, "Starting WebDriverAgent…").map_err(err)?;
    let (_runner, mut driver) = start(&phone, &runner_id)?;
    let status = driver.status().map_err(err)?;
    let version = status
        .pointer("/build/version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown version");
    writeln!(out, "WebDriverAgent {version} is ready. `forge ui tree`, `forge ui screenshot` and `forge ui run` will work.")
        .map_err(err)
}

/// Opens the phone, starts the runner and a WebDriver session.
fn connect(
    udid: Option<&str>,
) -> Result<
    (
        Phone,
        Vec<String>,
        Runner,
        WdaDriver<forge_uitest::phone::UsbHttp>,
    ),
    String,
> {
    let phone = Phone::open(udid).map_err(err)?;
    if !phone.ddi_mounted().map_err(err)? {
        return Err("the Developer Disk Image is not mounted (the phone restarted?): run `forge ui setup --ddi <folder>`".into());
    }
    let apps = phone.user_apps().map_err(err)?;
    let runner_id = find_runner(&apps).ok_or(NO_RUNNER)?.to_owned();
    report::set_device(&phone.udid, None);
    let (runner, driver) = start(&phone, &runner_id)?;
    Ok((phone, apps, runner, driver))
}

fn start(
    phone: &Phone,
    runner_id: &str,
) -> Result<(Runner, WdaDriver<forge_uitest::phone::UsbHttp>), String> {
    let runner = phone.start_runner(runner_id).map_err(|e| {
        format!("{e}\nIf the runner was installed more than 7 days ago its free-account signature has expired: run `forge ui setup --runner …` again.")
    })?;
    let driver = WdaDriver::connect(phone.transport()).map_err(err)?;
    Ok((runner, driver))
}

/// `forge ui tree`: the accessibility tree of the screen, plus an audit.
pub fn tree(udid: Option<&str>, app: Option<&str>, out: &mut dyn Write) -> Result<(), String> {
    let (_phone, apps, _runner, mut driver) = connect(udid)?;
    if let Some(app) = app {
        driver.activate(resolve_app(&apps, app)?).map_err(err)?;
    }
    let tree = driver.tree().map_err(err)?;
    write!(out, "{}", tree.render()).map_err(err)?;
    let findings = tree.audit();
    report::set_data(serde_json::json!({ "tree": tree.render(), "findings": findings }));
    if !findings.is_empty() {
        writeln!(out, "\nAccessibility ({}):", findings.len()).map_err(err)?;
        for finding in findings {
            writeln!(out, "  {finding}").map_err(err)?;
        }
    }
    Ok(())
}

/// `forge ui screenshot`
pub fn screenshot(udid: Option<&str>, path: &Path, out: &mut dyn Write) -> Result<(), String> {
    let (_phone, _apps, _runner, mut driver) = connect(udid)?;
    let png = driver.screenshot().map_err(err)?;
    std::fs::write(path, &png).map_err(|e| format!("{}: {e}", path.display()))?;
    report::set_data(serde_json::json!({ "path": path, "bytes": png.len() }));
    writeln!(out, "Saved {} ({} KB)", path.display(), png.len() / 1024).map_err(err)
}

/// `forge ui run`: a test file's steps; fails if any step fails.
pub fn run(
    udid: Option<&str>,
    file: &Path,
    out_dir: &Path,
    out: &mut dyn Write,
) -> Result<(), String> {
    let script = Script::load(file)?;
    let (_phone, apps, _runner, mut driver) = connect(udid)?;
    let app = resolve_app(&apps, &script.app)?.to_owned();
    let report = script::run(&script, &app, &mut driver, out_dir, out).map_err(err)?;
    report::set_data(serde_json::json!({
        "test": script.name,
        "passed_steps": report.passed,
        "failed_step": report.failed.as_ref().map(|(step, why)| serde_json::json!({ "step": step, "error": why })),
        "screenshots": report.screenshots,
    }));
    if !report.screenshots.is_empty() {
        writeln!(out, "Screenshots: {}", out_dir.display()).map_err(err)?;
    }
    match report.failed {
        None => Ok(()),
        Some((step, why)) => Err(format!("step {step} failed: {why}")),
    }
}
