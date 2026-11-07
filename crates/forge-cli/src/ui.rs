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

/// Which Apple team signs the runner: it decides the app limit and how long
/// the signature lasts. From `--team`, else `[ios.signing] team_type` in the
/// current directory's forge.toml, else unknown (treated like free, but said
/// so).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Team {
    Free,
    Paid,
    Unknown,
}

impl Team {
    pub fn resolve(flag: Option<&str>, dir: &Path) -> Result<Team, String> {
        let from = |value: &str| match value {
            "free" => Ok(Team::Free),
            "paid" => Ok(Team::Paid),
            other => Err(format!("team type `{other}`: use \"free\" or \"paid\"")),
        };
        if let Some(flag) = flag {
            return from(flag);
        }
        let Ok(text) = std::fs::read_to_string(dir.join("forge.toml")) else {
            return Ok(Team::Unknown);
        };
        let configured = crate::config::ForgeConfig::parse(&text)
            .ok()
            .and_then(|cfg| cfg.ios)
            .and_then(|ios| ios.signing.team_type);
        match configured {
            Some(value) => from(&value),
            None => Ok(Team::Unknown),
        }
    }

    /// Said before installing the runner, when the phone may be full.
    fn full_note(self, apps: &[String]) -> Option<String> {
        if self == Team::Paid || apps.len() < FREE_APP_LIMIT {
            return None;
        }
        let who = if self == Team::Free {
            "A free Apple account"
        } else {
            "If you sign with a free Apple account, it"
        };
        Some(format!(
            "Note: {} apps are installed ({}). {who} allows {FREE_APP_LIMIT}; if the install fails, delete one on the phone first.",
            apps.len(),
            apps.join(", ")
        ))
    }

    /// The installed-apps line after setup.
    fn apps_line(self, apps: &[String]) -> String {
        let list = apps.join(", ");
        match self {
            Team::Free => format!("Apps installed: {} of {FREE_APP_LIMIT} on a free account ({list}). The runner's signature lasts 7 days", apps.len()),
            Team::Paid => format!("Apps installed: {} ({list}). Paid team: no 3-app limit; the runner's signature lasts as long as its provisioning profile (up to a year)", apps.len()),
            Team::Unknown => format!("Apps installed: {} ({list}). With a free Apple account the limit is {FREE_APP_LIMIT} and signatures last 7 days; pass --team paid if xtool signs with a paid team", apps.len()),
        }
    }

    /// Added to a runner start failure.
    fn expiry_hint(self) -> &'static str {
        match self {
            Team::Paid => "If the runner's provisioning profile has expired, run `forge ui setup --runner …` again.",
            _ => "If the runner was installed more than 7 days ago with a free Apple account, its signature has expired: run `forge ui setup --runner …` again.",
        }
    }
}

const NO_RUNNER: &str = "the WebDriverAgent runner is not installed: run `forge ui setup --runner <WebDriverAgentRunner-Runner.app>` (see docs/forge-ui-testing.md)";

/// `forge ui setup`: mount the Developer Disk Image, install the runner,
/// and check that it starts.
pub fn setup(
    runner: &dyn CommandRunner,
    udid: Option<&str>,
    ddi: Option<&Path>,
    wda_app: Option<&Path>,
    team: Team,
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
        phone.mount_ddi(&files).map_err(|e| {
            format!("{e}\nCheck that the phone is unlocked, Developer Mode is on, this computer can reach Apple (gs.apple.com signs the image), and the folder's image matches an Xcode that supports this iOS version")
        })?;
        // Trust the phone, not the absence of an error.
        if !phone.ddi_mounted().map_err(err)? {
            return Err(
                "the mount reported success but the phone lists no mounted Developer Disk Image"
                    .into(),
            );
        }
        writeln!(out, "Developer Disk Image: mounted").map_err(err)?;
    }

    let mut apps = phone.user_apps().map_err(err)?;
    if let Some(app) = wda_app {
        if find_runner(&apps).is_none() {
            if let Some(note) = team.full_note(&apps) {
                writeln!(out, "{note}").map_err(err)?;
            }
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
        if find_runner(&apps).is_none() {
            return Err(format!(
                "xtool install finished, but no WebDriverAgent runner is installed (installed: {}). Check xtool's output above",
                apps.join(", ")
            ));
        }
    }
    let runner_id = find_runner(&apps).ok_or(NO_RUNNER)?.to_owned();
    writeln!(out, "Runner: {runner_id}").map_err(err)?;
    writeln!(out, "{}", team.apps_line(&apps)).map_err(err)?;

    writeln!(out, "Starting WebDriverAgent…").map_err(err)?;
    let (_runner, mut driver) = start(&phone, &runner_id, team)?;
    let status = driver.status().map_err(err)?;
    // WDA answers /status before it can serve a session if XCTest is still
    // starting; don't report a runner that isn't ready as working.
    if status.get("ready").and_then(|v| v.as_bool()) == Some(false) {
        return Err(format!(
            "WebDriverAgent answered but says it is not ready: {status}"
        ));
    }
    let version = status
        .pointer("/build/version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown version");
    report::set_device(&phone.udid, None);
    report::set_data(serde_json::json!({
        "runner": runner_id,
        "wda_version": version,
        "installed_apps": apps,
        "team": format!("{team:?}").to_lowercase(),
    }));
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
    let team = Team::resolve(None, Path::new(".")).unwrap_or(Team::Unknown);
    let (runner, driver) = start(&phone, &runner_id, team)?;
    Ok((phone, apps, runner, driver))
}

fn start(
    phone: &Phone,
    runner_id: &str,
    team: Team,
) -> Result<(Runner, WdaDriver<forge_uitest::phone::UsbHttp>), String> {
    let runner = phone
        .start_runner(runner_id)
        .map_err(|e| format!("{e}\n{}", team.expiry_hint()))?;
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
    report::set_data(serde_json::json!({
        "tree": tree,
        "text": tree.render(),
        "findings": findings,
    }));
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
        "failed_step": report.failed.as_ref().map(|(step, why)| serde_json::json!({
            "step": step,
            "action": report.failed_step,
            "error": why,
            "on_screen": report.on_screen,
            "tree": report.tree_file,
        })),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn apps(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("XTL-1.app{i}")).collect()
    }

    #[test]
    fn team_comes_from_the_flag_then_forge_toml() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Team::resolve(None, dir.path()), Ok(Team::Unknown));
        let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/pocket-library/forge.toml");
        let mut toml = std::fs::read_to_string(example).unwrap();
        toml.push_str("\n[ios.signing]\nteam_type = \"paid\"\n");
        std::fs::write(dir.path().join("forge.toml"), toml).unwrap();
        assert_eq!(Team::resolve(None, dir.path()), Ok(Team::Paid));
        assert_eq!(Team::resolve(Some("free"), dir.path()), Ok(Team::Free));
        assert!(Team::resolve(Some("pro"), dir.path()).is_err());
    }

    #[test]
    fn only_free_or_unknown_teams_hear_about_the_app_limit() {
        assert!(Team::Paid.full_note(&apps(5)).is_none());
        assert!(Team::Free.full_note(&apps(2)).is_none());
        assert!(Team::Free
            .full_note(&apps(3))
            .unwrap()
            .starts_with("Note: 3 apps are installed"));
        assert!(Team::Unknown
            .full_note(&apps(3))
            .unwrap()
            .contains("If you sign with a free Apple account"));
        assert!(Team::Paid.apps_line(&apps(4)).contains("no 3-app limit"));
        assert!(Team::Free
            .apps_line(&apps(2))
            .starts_with("Apps installed: 2 of 3 on a free account"));
        assert!(Team::Unknown.apps_line(&apps(1)).contains("--team paid"));
        assert!(!Team::Paid.expiry_hint().contains("7 days"));
    }
}
