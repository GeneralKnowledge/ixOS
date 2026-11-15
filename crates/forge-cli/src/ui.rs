//! `forge ui`: drive an app on a connected iPhone from Linux (Forge UI
//! Testing M0). The phone work is in the `forge-uitest` crate; this file
//! wires it to the command line. See docs/forge-ui-testing.md.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use forge_uitest::baseline::{self, Mode};
use forge_uitest::phone::{find_ddi, find_runner, resolve_app, sideloaded, Phone, Runner};
use forge_uitest::policy::{self, Access, Gate, Journal, Policy, Scope, TerminalApprover};
use forge_uitest::script::{self, Script};
use forge_uitest::session::{self, Attach, SessionInfo};
use forge_uitest::wda::WdaDriver;
use forge_uitest::{ensure_front, Driver};

use crate::report;
use crate::runner::{Cmd, CommandRunner};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Free Apple accounts can have this many sideloaded apps on a phone.
const FREE_APP_LIMIT: usize = 3;

/// Which Apple team signs the runner: it decides the app limit and how long
/// the signature lasts. From `--team`, else what xtool's sign-in says
/// ([`crate::ios_env::signing_team`]), else `[ios.signing] team_type` in the
/// current directory's forge.toml, else unknown (treated like free, but said
/// so).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Team {
    Free,
    Paid,
    Unknown,
}

impl Team {
    pub fn resolve(
        flag: Option<&str>,
        detected: Option<crate::ios_env::TeamKind>,
        dir: &Path,
    ) -> Result<Team, String> {
        let from = |value: &str| match value {
            "free" => Ok(Team::Free),
            "paid" => Ok(Team::Paid),
            other => Err(format!("team type `{other}`: use \"free\" or \"paid\"")),
        };
        if let Some(flag) = flag {
            return from(flag);
        }
        match detected {
            Some(crate::ios_env::TeamKind::Free) => return Ok(Team::Free),
            Some(crate::ios_env::TeamKind::Paid) => return Ok(Team::Paid),
            _ => {}
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
            Team::Free => format!("Sideloaded apps: {} of {FREE_APP_LIMIT} on a free account ({list}). The runner's signature lasts 7 days", apps.len()),
            Team::Paid => format!("Sideloaded apps: {} ({list}). Paid team: no 3-app limit; the runner's signature lasts as long as its provisioning profile (up to a year)", apps.len()),
            Team::Unknown => format!("Sideloaded apps: {} ({list}). With a free Apple account the limit is {FREE_APP_LIMIT} and signatures last 7 days; pass --team paid if xtool signs with a paid team", apps.len()),
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

/// First on a runner start failure: a phone that locked itself while idle is
/// the usual cause ("unexpected response from device"), and an expired
/// signature the rarer one.
const LOCKED_HINT: &str = "If the iPhone is locked (it locks itself after a few idle minutes), unlock it, keep it awake (Settings > Display & Brightness > Auto-Lock > Never while testing) and run again.";

pub(crate) const NO_RUNNER: &str = "the WebDriverAgent runner is not installed: run `forge ui setup --runner <WebDriverAgentRunner-Runner.app>` (see docs/forge-ui-testing.md)";

/// `forge ui setup`: mount the Developer Disk Image, install the runner,
/// and check that it starts.
pub fn setup(
    runner: &dyn CommandRunner,
    udid: Option<&str>,
    ddi: Option<&Path>,
    wda_app: Option<&Path>,
    reinstall: bool,
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
                        pass --ddi <folder> with a personalised iOS Developer Disk Image, e.g. PersonalizedImages/Xcode_iOS_DDI_Personalized from github.com/doronz88/DeveloperDiskImage (docs/forge-ui-testing.md)"
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
    let wda_app = match (wda_app, find_runner(&apps)) {
        (Some(_), Some(installed)) if !reinstall => {
            writeln!(
                out,
                "The runner is already installed ({installed}); not installing it again (pass --reinstall to re-sign it)"
            )
            .map_err(err)?;
            None
        }
        (app, _) => app,
    };
    let live = session::attach(&phone).map_err(err)?;
    if let (Some(_), Attach::Ready(info)) = (wda_app, &live) {
        return Err(format!(
            "a UI session (process {}) is using the runner: `forge ui stop` first, then install it again",
            info.pid
        ));
    }
    if let Some(app) = wda_app {
        if find_runner(&apps).is_none() {
            if let Some(note) = team.full_note(&sideloaded(&apps)) {
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
    writeln!(out, "{}", team.apps_line(&sideloaded(&apps))).map_err(err)?;

    let (_runner, mut driver) = match live {
        Attach::Ready(info) => {
            writeln!(out, "{}", session_line(&info)).map_err(err)?;
            let driver = WdaDriver::connect(phone.transport()).map_err(err)?;
            (Held::Session(info), driver)
        }
        Attach::None => {
            writeln!(out, "Starting WebDriverAgent…").map_err(err)?;
            let (runner, driver) = start(&phone, &runner_id, team)?;
            (Held::Own(runner), driver)
        }
    };
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
        "sideloaded_apps": sideloaded(&apps),
        "team": format!("{team:?}").to_lowercase(),
    }));
    writeln!(out, "WebDriverAgent {version} is ready. `forge ui tree`, `forge ui screenshot` and `forge ui run` will work.")
        .map_err(err)
}

/// The runner a command talks to: one it started (stopped when the command
/// ends), or a running UI session's (`forge ui start`).
#[allow(dead_code)] // held for its Drop, or for what it says
enum Held {
    Own(Runner),
    Session(SessionInfo),
}

/// Said when a command uses a session's runner instead of starting one.
fn session_line(info: &SessionInfo) -> String {
    format!(
        "Using the UI session (process {}): WebDriverAgent {} is already running",
        info.pid,
        info.wda_version.as_deref().unwrap_or("(unknown version)")
    )
}

/// Opens the phone and a WebDriver session: on a running UI session's
/// runner if there is one, else on a runner started for this command.
fn connect(
    udid: Option<&str>,
    out: &mut dyn Write,
) -> Result<
    (
        Phone,
        Vec<String>,
        Held,
        WdaDriver<forge_uitest::phone::UsbHttp>,
    ),
    String,
> {
    let phone = Phone::open(udid).map_err(err)?;
    report::set_device(&phone.udid, None);
    if let Attach::Ready(info) = session::attach(&phone).map_err(err)? {
        // The session checked the disk image and listed the apps when it
        // started; its runner answering means both still hold.
        writeln!(out, "{}", session_line(&info)).map_err(err)?;
        let driver = WdaDriver::connect(phone.transport()).map_err(err)?;
        let apps = info.apps.clone();
        return Ok((phone, apps, Held::Session(info), driver));
    }
    if !phone.ddi_mounted().map_err(err)? {
        return Err("the Developer Disk Image is not mounted (the phone restarted?): run `forge ui setup --ddi <folder>`".into());
    }
    let apps = phone.user_apps().map_err(err)?;
    let runner_id = find_runner(&apps).ok_or(NO_RUNNER)?.to_owned();
    let team = Team::resolve(None, None, Path::new(".")).unwrap_or(Team::Unknown);
    let (runner, driver) = start(&phone, &runner_id, team)?;
    Ok((phone, apps, Held::Own(runner), driver))
}

/// The installed bundle id for `app`. A session's app list is from when it
/// started, so an app installed since is looked up on the phone again.
fn resolve(
    phone: &Phone,
    apps: &mut Vec<String>,
    held: &Held,
    app: &str,
) -> Result<String, String> {
    match resolve_app(apps, app) {
        Ok(id) => Ok(id.to_owned()),
        Err(_) if matches!(held, Held::Session(_)) => {
            *apps = phone.user_apps().map_err(err)?;
            resolve_app(apps, app).map(str::to_owned)
        }
        Err(e) => Err(e),
    }
}

fn start(
    phone: &Phone,
    runner_id: &str,
    team: Team,
) -> Result<(Runner, WdaDriver<forge_uitest::phone::UsbHttp>), String> {
    let runner = phone
        .start_runner(runner_id)
        .map_err(|e| format!("{e}\n{LOCKED_HINT}\n{}", team.expiry_hint()))?;
    let driver = WdaDriver::connect(phone.transport()).map_err(err)?;
    Ok((runner, driver))
}

/// Bundle ids of the developer's own apps: the forge.toml apps in `dirs`.
fn own_apps(dirs: &[&Path]) -> Vec<String> {
    dirs.iter()
        .filter_map(|d| std::fs::read_to_string(d.join("forge.toml")).ok())
        .filter_map(|text| crate::config::ForgeConfig::parse(&text).ok())
        .map(|cfg| cfg.app.bundle_id)
        .collect()
}

/// The app in this checkout's `examples/` whose forge.toml has `bundle_id`.
fn checkout_app_dir(bundle_id: &str) -> Option<PathBuf> {
    let examples = crate::commands::workspace_root().join("examples");
    std::fs::read_dir(examples)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|d| own_apps(&[d.as_path()]).iter().any(|b| b == bundle_id))
}

/// `forge ui tree`: the accessibility tree of the screen, plus an audit.
/// What `forge ui tree --find/--visible/--limit` keeps.
#[derive(Default)]
pub struct TreeFilter {
    pub find: Option<String>,
    pub visible: bool,
    pub limit: Option<usize>,
}

impl TreeFilter {
    fn is_set(&self) -> bool {
        self.find.is_some() || self.visible || self.limit.is_some()
    }
}

pub fn tree(
    udid: Option<&str>,
    app: Option<&str>,
    any_app: bool,
    filter: &TreeFilter,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (front, tree, screen) = screen_tree(udid, app, any_app, out)?;
    if let Some(s) = &screen {
        writeln!(
            out,
            "Screen: {} x {} pt, scale {}, {}",
            s.width, s.height, s.scale, s.orientation
        )
        .map_err(err)?;
    }
    if filter.is_set() {
        let found = tree.matching(
            filter.find.as_deref(),
            filter.visible,
            filter.limit.unwrap_or(usize::MAX),
        );
        for node in &found {
            writeln!(out, "{}", node.summary()).map_err(err)?;
        }
        if found.is_empty() {
            writeln!(out, "nothing matches").map_err(err)?;
        }
        report::set_data(serde_json::json!({
            "app": front,
            "screen": screen,
            "matches": found,
        }));
        return Ok(());
    }
    write!(out, "{}", tree.render()).map_err(err)?;
    let findings = tree.audit();
    report::set_data(serde_json::json!({
        "app": front,
        "screen": screen,
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

/// `forge ui audit` (M6.3): the accessibility audit of the screen, without
/// the tree. Fails when it finds an error (a control VoiceOver can't name).
pub fn audit(
    udid: Option<&str>,
    app: Option<&str>,
    any_app: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (front, tree, _) = screen_tree(udid, app, any_app, out)?;
    let result = audit_result(front.as_deref(), &tree);
    report::set_data(result.clone());
    write!(out, "{}", audit_text(&result)).map_err(err)?;
    match result["errors"].as_u64().unwrap_or(0) {
        0 => Ok(()),
        n => Err(format!("the audit found {n} error(s)")),
    }
}

/// What the audit checks, said with every result: a clean audit is clean
/// for these checks only.
const AUDIT_CHECKS: [&str; 5] = [
    "no_label: a control VoiceOver has no name for (error)",
    "no_identifier, duplicate_identifier: a control tests can't find reliably (warning)",
    "small_tap_target: under 44 x 44 pt, counting a Form row as the target; bar buttons and links inside a sentence are left out (warning)",
    "clipped: text or a labelled control running off the side of the screen, or text drawn over text in the same list (warning)",
    "Measured on the accessibility tree, not the pixels: contrast, text truncated with an ellipsis inside its own frame and colour-only meaning are not checked",
];

fn audit_result(front: Option<&str>, tree: &forge_uitest::UiNode) -> serde_json::Value {
    let findings = tree.audit();
    let count = |severity: &str| findings.iter().filter(|f| f.severity == severity).count();
    let mut by_issue = std::collections::BTreeMap::<&str, usize>::new();
    for f in &findings {
        *by_issue.entry(f.issue).or_default() += 1;
    }
    serde_json::json!({
        "app": front,
        "screen": tree.frame.map(|f| serde_json::json!({"width": f.width, "height": f.height})),
        "errors": count("error"),
        "warnings": count("warning"),
        "by_issue": by_issue,
        "findings": findings,
        "checks": AUDIT_CHECKS,
    })
}

fn audit_text(result: &serde_json::Value) -> String {
    let findings = result["findings"].as_array().cloned().unwrap_or_default();
    let mut text = format!(
        "Accessibility audit: {} error(s), {} warning(s)\n",
        result["errors"], result["warnings"]
    );
    for f in &findings {
        let what = f["issue"].as_str().unwrap_or("?");
        let detail = f["detail"]
            .as_str()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        text.push_str(&format!(
            "  {} {what}{detail}: {}\n",
            f["severity"].as_str().unwrap_or("?"),
            f["element"]["summary"].as_str().unwrap_or("")
        ));
    }
    if findings.is_empty() {
        text.push_str("  Nothing found by these checks (contrast and truncation inside a frame aren't measured).\n");
    }
    text
}

/// The app in front and its tree, after the any-app policy allowed reading
/// it (and bringing `app` to the front first).
fn screen_tree(
    udid: Option<&str>,
    app: Option<&str>,
    any_app: bool,
    out: &mut dyn Write,
) -> Result<
    (
        Option<String>,
        forge_uitest::UiNode,
        Option<forge_uitest::Screen>,
    ),
    String,
> {
    let policy = Policy::load()?;
    let own = own_apps(&[Path::new(".")]);
    let scope = Scope {
        any_app,
        own: &own,
        policy: &policy,
    };
    let (phone, mut apps, held, mut driver) = connect(udid, out)?;
    if let Some(app) = app {
        let app = resolve(&phone, &mut apps, &held, app)?;
        // Decided before the app is even brought to the front.
        scope.access(&app, &mut TerminalApprover)?;
        driver.activate(&app).map_err(err)?;
        // Never audit whatever else happens to be open.
        ensure_front(&mut driver, &app, Duration::from_secs(10)).map_err(err)?;
    }
    let front = driver.foreground().map_err(err)?;
    if app.is_none() {
        scope.may_read(front.as_deref(), &mut TerminalApprover)?;
    }
    let tree = driver.tree().map_err(err)?;
    // Best effort: an older runner may not answer every route.
    let screen = driver.screen().ok();
    writeln!(
        out,
        "In front: {}",
        front
            .as_deref()
            .unwrap_or("unknown (WebDriverAgent did not say)")
    )
    .map_err(err)?;
    Ok((front, tree, screen))
}

/// `forge ui screenshot`
pub fn screenshot(
    udid: Option<&str>,
    path: &Path,
    any_app: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let policy = Policy::load()?;
    let own = own_apps(&[Path::new(".")]);
    let scope = Scope {
        any_app,
        own: &own,
        policy: &policy,
    };
    let (_phone, _apps, _held, mut driver) = connect(udid, out)?;
    let front = driver.foreground().map_err(err)?;
    scope.may_read(front.as_deref(), &mut TerminalApprover)?;
    let png = driver.screenshot().map_err(err)?;
    std::fs::write(path, &png).map_err(|e| format!("{}: {e}", path.display()))?;
    report::set_data(serde_json::json!({ "path": path, "bytes": png.len(), "app": front }));
    writeln!(
        out,
        "Saved {} ({} KB), {} in front",
        path.display(),
        png.len() / 1024,
        front.as_deref().unwrap_or("unknown app")
    )
    .map_err(err)
}

/// A run that broke off with `error` before it finished: stops the watch,
/// keeps what it collected in `out_dir` (`device.log`, `crashes/`,
/// `watch.json`) and puts it in the result, so the evidence gathered before
/// the failure isn't lost with it.
/// Whether a run's error means the phone went away (unplugged on Cati,
/// 2026-11-08: "connecting to WebDriverAgent: bad device").
fn phone_gone(error: &str) -> bool {
    let e = error.to_lowercase();
    [
        "bad device",
        "device not found",
        "no device",
        "connecting to webdriveragent",
        "broken pipe",
        "connection reset",
        "socket io failed",
        "close_notify",
    ]
    .iter()
    .any(|w| e.contains(w))
}

fn broke_off(
    error: String,
    watcher: Result<crate::watch::Watch, String>,
    out_dir: &Path,
) -> String {
    let value = match watcher {
        Ok(w) => w.finish(out_dir, true).value,
        Err(why) => crate::watch::not_watched(&why),
    };
    // The phone or WebDriverAgent went away: a device failure, said plainly.
    let error = match phone_gone(&error) {
        true => {
            report::set_failed_stage("device");
            format!("the iPhone was disconnected or stopped answering during the run ({error})")
        }
        false => error,
    };
    let _ = std::fs::create_dir_all(out_dir);
    let saved = std::fs::write(
        out_dir.join("watch.json"),
        serde_json::to_vec_pretty(&value).unwrap_or_default(),
    )
    .is_ok();
    report::set_data(serde_json::json!({
        "broke_off": error,
        "out": out_dir.display().to_string(),
        "watch": value,
    }));
    if saved {
        format!(
            "{error}. The app's log and any new crash report from before it are in {}",
            out_dir.display()
        )
    } else {
        error
    }
}

/// The one-step test file `forge ui launch` runs: stop the app, or open it
/// (restarting it first when there are launch arguments: a running app
/// keeps the ones it started with, seen on Cati 2026-11-07).
pub fn launch_script(bundle_id: &str, terminate: bool, args: &[String]) -> String {
    let mut doc = toml::Table::new();
    doc.insert("name".into(), "forge ui launch".into());
    doc.insert("app".into(), bundle_id.into());
    let step = |key: &str, value: toml::Table| {
        let mut t = toml::Table::new();
        t.insert(key.into(), toml::Value::Table(value));
        toml::Value::Table(t)
    };
    let mut steps = Vec::new();
    if terminate || !args.is_empty() {
        steps.push(step("terminate", toml::Table::new()));
    }
    if !terminate {
        let mut launch = toml::Table::new();
        if !args.is_empty() {
            launch.insert(
                "args".into(),
                toml::Value::Array(args.iter().map(|a| a.as_str().into()).collect()),
            );
        }
        steps.push(step("launch", launch));
    }
    doc.insert("step".into(), toml::Value::Array(steps));
    toml::to_string(&doc).unwrap_or_default()
}

/// `forge ui launch`: opens (or stops) the app at `project` on the phone,
/// as a one-step UI test in `<app>/.forge/ui/<UTC>-launch/`. Not evidence:
/// no requirement names it.
pub fn launch(
    runner: &dyn CommandRunner,
    udid: Option<&str>,
    project: &Path,
    terminate: bool,
    args: &[String],
    out: &mut dyn Write,
) -> Result<(), String> {
    let forge_toml = project.join("forge.toml");
    let text = std::fs::read_to_string(&forge_toml)
        .map_err(|e| format!("{}: {e} (pass --project <app>)", forge_toml.display()))?;
    let cfg = crate::config::ForgeConfig::parse(&text)?;
    let dir = project
        .join(".forge")
        .join("ui")
        .join(format!("{}-launch", report::timestamp()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join("steps.toml");
    std::fs::write(&file, launch_script(&cfg.app.bundle_id, terminate, args))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    run(runner, udid, &file, Some(&dir), false, None, false, out)
}

/// The test file a one-step command runs: `key = value` as its only step.
pub fn one_step_script(
    bundle_id: &str,
    verb: &str,
    timeout: f64,
    key: &str,
    value: toml::Value,
) -> String {
    let mut doc = toml::Table::new();
    doc.insert("name".into(), format!("forge ui {verb}").into());
    doc.insert("app".into(), bundle_id.into());
    doc.insert("timeout".into(), timeout.into());
    let mut step = toml::Table::new();
    step.insert(key.into(), value);
    doc.insert("step".into(), toml::Value::Array(vec![step.into()]));
    toml::to_string(&doc).unwrap_or_default()
}

/// `forge ui tap|type|clear|scroll|swipe|long-press|double-tap|drag|key|expect`:
/// one step on the app at `project` (or `app`, by bundle id), run as a
/// one-step test in `<app>/.forge/ui/<UTC>-<verb>/` (a temporary folder
/// for `--app`). Not evidence: no requirement names it.
#[allow(clippy::too_many_arguments)]
pub fn one_step(
    runner: &dyn CommandRunner,
    udid: Option<&str>,
    project: Option<&Path>,
    app: Option<&str>,
    any_app: bool,
    timeout: f64,
    verb: &str,
    key: &str,
    value: toml::Value,
    out: &mut dyn Write,
) -> Result<(), String> {
    let name = format!("{}-{verb}", report::timestamp());
    let (bundle_id, dir) = match (app, project) {
        (Some(app), _) => (
            app.to_owned(),
            std::env::temp_dir().join("forge-ui").join(name),
        ),
        (None, Some(project)) => {
            let forge_toml = project.join("forge.toml");
            let text = std::fs::read_to_string(&forge_toml)
                .map_err(|e| format!("{}: {e} (pass --project <app>)", forge_toml.display()))?;
            let cfg = crate::config::ForgeConfig::parse(&text)?;
            (
                cfg.app.bundle_id,
                project.join(".forge").join("ui").join(name),
            )
        }
        (None, None) => return Err("pass --project or --app".into()),
    };
    let text = one_step_script(&bundle_id, verb, timeout, key, value);
    // Checked before the phone is touched: a wrong target fails here.
    Script::parse(&text).map_err(|e| e.replace("step 1: ", ""))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join("steps.toml");
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    run(runner, udid, &file, Some(&dir), any_app, None, false, out)
}

/// `forge ui tap --at X,Y` and the other coordinate gestures: a touch at a
/// point on the screen, for a person driving an app whose controls have
/// no identifier or label. Never in a test file or `forge mcp` (AGENTS.md
/// rule 4): only from this command, with the app under test in front. In
/// an app Forge didn't build every such touch is a commit, asked each time,
/// since nothing says what is at that point.
#[allow(clippy::too_many_arguments)]
pub fn touch_at(
    udid: Option<&str>,
    project: Option<&Path>,
    app: Option<&str>,
    any_app: bool,
    touch: &forge_uitest::PointTouch,
    out: &mut dyn Write,
) -> Result<(), String> {
    let bundle_id = match (app, project) {
        (Some(app), _) => app.to_owned(),
        (None, Some(project)) => {
            let forge_toml = project.join("forge.toml");
            let text = std::fs::read_to_string(&forge_toml)
                .map_err(|e| format!("{}: {e} (pass --project <app>)", forge_toml.display()))?;
            crate::config::ForgeConfig::parse(&text)?.app.bundle_id
        }
        (None, None) => return Err("pass --project or --app".into()),
    };
    if let Some(why) = policy::never(&bundle_id) {
        return Err(why);
    }
    let policy = Policy::load()?;
    let mut dirs = vec![Path::new(".")];
    if let Some(p) = project {
        dirs.push(p);
    }
    let own = own_apps(&dirs);
    let scope = Scope {
        any_app,
        own: &own,
        policy: &policy,
    };
    let (phone, mut apps, held, mut driver) = connect(udid, out)?;
    let app = resolve(&phone, &mut apps, &held, &bundle_id)?;
    let mut gate = match scope.access(&app, &mut TerminalApprover)? {
        Access::Own => Gate::own(&app),
        Access::Foreign(level) => Gate::foreign(&app, level, Box::new(TerminalApprover), None),
    };
    // Only ever the app under test: a point on another app's screen could
    // be anything.
    ensure_front(&mut driver, &app, Duration::from_secs(10)).map_err(err)?;
    gate.check(&policy::Action::Gesture {
        gesture: touch.name(),
        target: policy::Target::default(),
    })?;
    driver.touch_at(touch).map_err(err)?;
    writeln!(
        out,
        "{} at {}",
        touch.name().replace('_', " "),
        describe_point(touch)
    )
    .map_err(err)?;
    report::set_data(serde_json::json!({ "app": app, "touch": format!("{touch:?}") }));
    Ok(())
}

fn describe_point(touch: &forge_uitest::PointTouch) -> String {
    use forge_uitest::PointTouch::*;
    match touch {
        Tap { x, y } | DoubleTap { x, y } | LongPress { x, y, .. } => format!("{x},{y}"),
        Drag { from, to, .. } => format!("{},{} to {},{}", from.0, from.1, to.0, to.1),
    }
}

/// `X,Y` in points.
pub fn parse_point(text: &str) -> Result<(f64, f64), String> {
    let bad = || format!("`{text}`: a point as X,Y in points from the top-left, such as 200,640");
    let (x, y) = text.split_once(',').ok_or_else(bad)?;
    let x: f64 = x.trim().parse().map_err(|_| bad())?;
    let y: f64 = y.trim().parse().map_err(|_| bad())?;
    if x < 0.0 || y < 0.0 || !x.is_finite() || !y.is_finite() {
        return Err(bad());
    }
    Ok((x, y))
}

/// `forge ui run`: a test file's steps; fails if any step fails, or (with
/// `visual` `Some(Check)`) if a screenshot differs from its baseline.
#[allow(clippy::too_many_arguments)]
pub fn run(
    runner: &dyn CommandRunner,
    udid: Option<&str>,
    file: &Path,
    out_dir: Option<&Path>,
    any_app: bool,
    visual: Option<Mode>,
    watch: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let script = Script::load(file)?;
    let policy = Policy::load()?;
    // Inside an app, the run is an evidence bundle (docs/evidence.md):
    // <app>/.forge/runs/<id>-ui-run/ unless --out says otherwise.
    let mut app_dir = crate::evidence::app_of(file).or_else(|| {
        // A test file kept outside the app (a scratch folder on the laptop)
        // still runs a Forge app: find it by its bundle id, from here or
        // in this checkout's examples, so its log is watched and the run
        // is its evidence. Seen on Cati: a soak ran unwatched for this.
        let here = std::env::current_dir().ok()?;
        here.ancestors()
            .find(|d| own_apps(&[d]).contains(&script.app))
            .map(Path::to_path_buf)
            .or_else(|| checkout_app_dir(&script.app))
    });
    // A test kept with one app may drive the app that hosts it on the phone
    // (Pocket Library's tests run in Device Lab): that host is in this
    // checkout too, so it is the developer's own app.
    let host_dir = checkout_app_dir(&script.app);
    let id = report::timestamp();
    let own = {
        let mut dirs: Vec<&Path> = vec![Path::new(".")];
        dirs.extend(app_dir.as_deref());
        dirs.extend(host_dir.as_deref());
        own_apps(&dirs)
    };
    let scope = Scope {
        any_app,
        own: &own,
        policy: &policy,
    };
    // Settled before the phone is touched where the file alone decides it.
    if let Some(why) = policy::never(&script.app) {
        return Err(why);
    }
    let started = std::time::Instant::now();
    let (phone, mut apps, held, mut driver) = connect(udid, out)?;
    let app = resolve(&phone, &mut apps, &held, &script.app)?;
    let mut session = None;
    let mut gate = match scope.access(&app, &mut TerminalApprover)? {
        Access::Own => Gate::own(&app),
        Access::Foreign(level) => {
            // Another developer's app: its screens are the person's data.
            // They go to a folder outside every project, never into an
            // app's evidence (docs/research/general-device-control.md#safety).
            app_dir = None;
            let dir = match out_dir {
                Some(d) => d.to_path_buf(),
                None => policy::session_root()
                    .ok_or("no home folder for the any-app session; pass --out")?
                    .join(format!("{id}-ui-run")),
            };
            let journal = Journal::create(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            writeln!(
                out,
                "{app} is not a Forge app: acting at the `{}` level from {}. Screens and the journal go to {}",
                level.name(),
                policy.where_(),
                dir.display()
            )
            .map_err(err)?;
            session = Some(dir);
            Gate::foreign(&app, level, Box::new(TerminalApprover), Some(journal))
        }
    };
    let out_dir: PathBuf = match (session, out_dir, &app_dir) {
        (Some(d), _, _) => d,
        (None, Some(d), _) => d.to_path_buf(),
        (None, None, Some(app)) => app.join(".forge/runs").join(format!("{id}-ui-run")),
        (None, None, None) => PathBuf::from("forge-ui-out"),
    };
    // Read before the run, so the bundle names the code that was tested.
    let build = app_dir
        .as_deref()
        .map(|a| crate::evidence::provenance(runner, a));
    // The app's own log and crash reports, for a Forge app only: another
    // developer's app logs the person's data.
    let watcher = match (&app_dir, watch) {
        (Some(dir), true) => crate::watch::product(dir)
            .map(|product| {
                crate::watch::Watch::start(crate::watch::usb, &phone.udid, &product, dir)
            })
            .map_err(|e| format!("the app's executable name isn't known ({e})")),
        (Some(_), false) => Err("--no-watch".to_string()),
        (None, _) => Err("not a Forge app: its log would hold the person's data".to_string()),
    };
    let report = match script::run_gated(&script, &app, &mut driver, &out_dir, out, &mut gate) {
        Ok(report) => report,
        // The run broke off (the phone or WebDriverAgent stopped answering):
        // keep what the watch saw, and say where it is.
        Err(e) => return Err(broke_off(err(e), watcher, &out_dir)),
    };
    let watched = match watcher {
        Ok(w) => w.finish(&out_dir, report.failed.is_some()),
        Err(why) => crate::watch::Finished {
            value: crate::watch::not_watched(&why),
            tail: Vec::new(),
            artifacts: Vec::new(),
        },
    };
    let watched = Some(watched);
    // Visual regression runs on Linux, after the phone part: the screenshots
    // taken before a failed step are still compared.
    let visual = match visual {
        Some(mode) => Some(baseline::compare_run(
            file,
            &script.visual,
            &report.screenshots,
            &out_dir,
            mode,
        )?),
        None => None,
    };
    if let Some(visual) = &visual {
        if !visual.screens.is_empty() {
            writeln!(out, "Screenshots against {}:", visual.baselines).map_err(err)?;
            write!(out, "{}", visual.render()).map_err(err)?;
        }
    }
    let visual_failures: Vec<String> = visual
        .iter()
        .flat_map(|v| v.screens.iter().filter(|s| s.failed()))
        .map(|s| s.name.clone())
        .collect();
    let failed_step = report.failed.as_ref().map(|(step, why)| {
        let mut failed = serde_json::json!({
            "step": step,
            "action": report.failed_step,
            "error": why,
            "on_screen": report.on_screen,
            "tree": report.tree_file,
        });
        // What the app logged just before, and where it crashed if it did.
        if let Some(w) = &watched {
            failed["log_tail"] = serde_json::json!(w.tail);
            // `found`, `none_new`, `unavailable` or `not_checked`: no
            // `crash` below is only "didn't crash" when this says none_new.
            failed["crash_reports"] = w.value["crash_reports"].clone();
            failed["log_status"] = w.value["log_status"].clone();
            if let Some(crash) = w.value["crashes"].as_array().and_then(|c| c.first()) {
                failed["crash"] = crash.clone();
            }
        }
        failed
    });
    if let Some(crash) = failed_step.as_ref().and_then(|f| f.get("crash")) {
        writeln!(
            out,
            "The app crashed: {}",
            crash["title"]
                .as_str()
                .or(crash["kind"].as_str())
                .unwrap_or("see crashes/")
        )
        .map_err(err)?;
    }
    let evidence = match (&app_dir, build) {
        (Some(app_dir), Some(mut build)) => {
            build["installed_as"] = serde_json::json!(app);
            let reqs = crate::evidence::load_requirements(app_dir)?;
            let rel_to = |p: &Path| {
                p.strip_prefix(&out_dir)
                    .map(|r| r.display().to_string())
                    .ok()
            };
            let mut artifacts: Vec<(&'static str, String)> = report
                .screenshots
                .iter()
                .filter_map(|p| rel_to(p))
                .map(|p| ("screenshot", p))
                .collect();
            if let Some(tree) = report.tree_file.as_deref().and_then(rel_to) {
                artifacts.push(("ui_tree", tree));
            }
            for shot in &report.screenshots {
                let tree = baseline::tree_path(shot);
                if let Some(tree) = tree.is_file().then(|| rel_to(&tree)).flatten() {
                    artifacts.push(("ui_tree", tree));
                }
            }
            for screen in visual.iter().flat_map(|v| &v.screens) {
                if let Some(diff) = &screen.diff {
                    artifacts.push(("visual_diff", diff.clone()));
                }
            }
            if let Some(w) = &watched {
                artifacts.extend(w.artifacts.iter().cloned());
            }
            let scenario = std::fs::canonicalize(file)
                .ok()
                .and_then(|f| {
                    let app_dir = std::fs::canonicalize(app_dir).ok()?;
                    f.strip_prefix(app_dir)
                        .ok()
                        .map(|r| r.display().to_string())
                })
                .unwrap_or_else(|| file.display().to_string());
            let run = crate::evidence::DeviceRun {
                scenario: &scenario,
                build,
                device: serde_json::json!({
                    "udid": phone.udid,
                    "ios_major": phone.ios_major().ok(),
                }),
                passed_steps: report.passed,
                failed_step: failed_step.clone(),
                artifacts,
                duration_ms: started.elapsed().as_millis() as u64,
                visual: visual.as_ref().and_then(|v| serde_json::to_value(v).ok()),
                visual_failures: visual_failures.clone(),
                watch: watched.as_ref().map(|w| w.value.clone()),
            };
            let path = crate::evidence::write_device_bundle(&run, &reqs, &out_dir, &id)?;
            writeln!(out, "Evidence: {}", path.display()).map_err(err)?;
            Some(path)
        }
        _ => None,
    };
    report::set_data(serde_json::json!({
        "test": script.name,
        "passed_steps": report.passed,
        "failed_step": failed_step,
        "screenshots": report.screenshots,
        "visual": visual,
        "watch": watched.as_ref().map(|w| &w.value),
        "out": out_dir.display().to_string(),
        "evidence": evidence.map(|p| p.display().to_string()),
        // Another developer's app: the level it ran at and its journal.
        "any_app": gate.is_foreign().then(|| serde_json::json!({
            "level": gate.level().name(),
            "journal": out_dir.join("journal.jsonl"),
        })),
    }));
    if !report.screenshots.is_empty() {
        writeln!(out, "Screenshots: {}", out_dir.display()).map_err(err)?;
    }
    if let Some(v) = &visual {
        let new: Vec<&str> = v
            .screens
            .iter()
            .filter(|s| s.verdict == "new")
            .map(|s| s.name.as_str())
            .collect();
        if !new.is_empty() && visual_failures.is_empty() && report.failed.is_none() {
            writeln!(
                out,
                "No baseline yet for {}. Look at the screenshots; if they are right: forge ui accept {}",
                new.join(", "),
                out_dir.display()
            )
            .map_err(err)?;
        }
    }
    match report.failed {
        Some((step, why)) => Err(format!("step {step} failed: {why}")),
        None if !visual_failures.is_empty() => Err(format!(
            "{} differ{} from {} baseline{}: {}. Look at the diffs in {}; if the change is intended, `forge ui accept {}` makes these the baselines (then run again)",
            if visual_failures.len() == 1 { "1 screenshot".to_string() } else { format!("{} screenshots", visual_failures.len()) },
            if visual_failures.len() == 1 { "s" } else { "" },
            if visual_failures.len() == 1 { "its" } else { "their" },
            if visual_failures.len() == 1 { "" } else { "s" },
            visual_failures.join(", "),
            out_dir.join("visual").display(),
            out_dir.display()
        )),
        None => Ok(()),
    }
}

/// `forge ui accept`: make a run's screenshots the scenario's baselines.
/// The scenario comes from `--scenario`, else from the run's evidence.json.
pub fn accept(
    run_dir: &Path,
    scenario: Option<&Path>,
    only: &[String],
    out: &mut dyn Write,
) -> Result<(), String> {
    let scenario = match scenario {
        Some(s) => s.to_path_buf(),
        None => scenario_of(run_dir)?,
    };
    if !scenario.is_file() {
        return Err(format!("{}: no such test file", scenario.display()));
    }
    let written = baseline::accept(&scenario, run_dir, only)?;
    for path in &written {
        writeln!(out, "Baseline: {}", path.display()).map_err(err)?;
    }
    report::set_data(serde_json::json!({
        "scenario": scenario.display().to_string(),
        "baselines": written,
    }));
    writeln!(
        out,
        "{} baseline{} updated. Commit {} with the change that caused {}.",
        written.len(),
        if written.len() == 1 { "" } else { "s" },
        baseline::dir_for(&scenario).display(),
        if written.len() == 1 { "it" } else { "them" }
    )
    .map_err(err)
}

/// The scenario a run bundle names (`scenario`, relative to its app).
fn scenario_of(run_dir: &Path) -> Result<PathBuf, String> {
    let evidence = run_dir.join("evidence.json");
    let text = std::fs::read_to_string(&evidence).map_err(|_| {
        format!(
            "{} has no evidence.json (the run was outside an app): pass --scenario <test file>",
            run_dir.display()
        )
    })?;
    let doc: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", evidence.display()))?;
    let rel = doc["scenario"]
        .as_str()
        .ok_or_else(|| format!("{} names no scenario: pass --scenario", evidence.display()))?;
    let app = crate::evidence::app_of(&evidence).ok_or_else(|| {
        format!(
            "{} is not inside an app: pass --scenario",
            run_dir.display()
        )
    })?;
    Ok(app.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_are_x_comma_y() {
        assert_eq!(parse_point("200,640"), Ok((200.0, 640.0)));
        assert_eq!(parse_point(" 20.5 , 7 "), Ok((20.5, 7.0)));
        for bad in ["200", "a,b", "-1,5", "1,inf", ""] {
            assert!(parse_point(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_one_step_command_is_a_test_file_forge_reads() {
        let text = one_step_script(
            "org.example.tipster",
            "swipe",
            10.0,
            "swipe",
            toml::toml! { id = "row"
            direction = "left" }
            .into(),
        );
        let script = forge_uitest::script::Script::parse(&text).unwrap();
        assert_eq!(script.app, "org.example.tipster");
        assert_eq!(script.steps.len(), 1);
    }

    fn unplugged() -> Result<Box<dyn forge_device::DeviceBackend>, String> {
        Err("usbmuxd: connection refused".into())
    }

    #[test]
    fn a_run_that_breaks_off_keeps_its_watch_and_says_where() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("run");
        let watch = crate::watch::Watch::start(unplugged, "0000", "GoodDogXtool", tmp.path());
        let msg = broke_off("WebDriverAgent stopped answering".into(), Ok(watch), &out);
        assert!(
            msg.starts_with("WebDriverAgent stopped answering."),
            "{msg}"
        );
        assert!(msg.contains(&out.display().to_string()), "{msg}");
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("watch.json")).unwrap())
                .unwrap();
        // Nothing could be collected, and the file says so.
        assert_eq!(saved["log_status"], "unavailable");
        assert_eq!(saved["crash_reports"], "unavailable");
        assert!(out.join("device.log").is_file());
        // Not watched at all: the reason is kept.
        let msg = broke_off("stopped".into(), Err("--no-watch".into()), &out);
        assert!(msg.contains("stopped"));
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("watch.json")).unwrap())
                .unwrap();
        assert_eq!(saved["status"], "not_watched");
        assert_eq!(saved["reason"], "--no-watch");
    }

    #[test]
    fn an_unplugged_phone_is_a_device_failure() {
        // Seen on Cati when the cable was pulled at step 3.
        assert!(phone_gone("connecting to WebDriverAgent: bad device"));
        assert!(phone_gone("device socket io failed"));
        assert!(!phone_gone("no element matches id `library.nope`"));
    }

    #[test]
    fn launch_files_restart_only_for_new_arguments() {
        let plain: toml::Table = toml::from_str(&launch_script("org.x.app", false, &[])).unwrap();
        assert_eq!(plain["app"].as_str(), Some("org.x.app"));
        let steps = plain["step"].as_array().unwrap();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].get("launch").is_some());
        let dark: toml::Table = toml::from_str(&launch_script(
            "org.x.app",
            false,
            &["-ForgeAppearance".into(), "dark".into()],
        ))
        .unwrap();
        let steps = dark["step"].as_array().unwrap();
        assert!(steps[0].get("terminate").is_some());
        assert_eq!(steps[1]["launch"]["args"][1].as_str(), Some("dark"));
        let stop: toml::Table = toml::from_str(&launch_script("org.x.app", true, &[])).unwrap();
        let steps = stop["step"].as_array().unwrap();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].get("terminate").is_some());
        // Every file is one forge ui run accepts.
        for text in [
            launch_script("org.x.app", false, &[]),
            launch_script("org.x.app", true, &[]),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let f = dir.path().join("s.toml");
            std::fs::write(&f, text).unwrap();
            Script::load(&f).unwrap();
        }
    }

    fn apps(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("XTL-1.app{i}")).collect()
    }

    #[test]
    fn team_comes_from_the_flag_then_xtool_then_forge_toml() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Team::resolve(None, None, dir.path()), Ok(Team::Unknown));
        let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/pocket-library/forge.toml");
        let mut toml = std::fs::read_to_string(example).unwrap();
        toml.push_str("\n[ios.signing]\nteam_type = \"paid\"\n");
        std::fs::write(dir.path().join("forge.toml"), toml).unwrap();
        assert_eq!(Team::resolve(None, None, dir.path()), Ok(Team::Paid));
        assert_eq!(
            Team::resolve(Some("free"), None, dir.path()),
            Ok(Team::Free)
        );
        assert!(Team::resolve(Some("pro"), None, dir.path()).is_err());
        // What xtool says beats forge.toml; the flag beats both.
        use crate::ios_env::TeamKind;
        assert_eq!(
            Team::resolve(None, Some(TeamKind::Free), dir.path()),
            Ok(Team::Free)
        );
        assert_eq!(
            Team::resolve(None, Some(TeamKind::Unknown), dir.path()),
            Ok(Team::Paid)
        );
        assert_eq!(
            Team::resolve(Some("paid"), Some(TeamKind::Free), dir.path()),
            Ok(Team::Paid)
        );
    }

    #[test]
    fn accept_finds_the_scenario_from_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Tipster");
        let run = app.join(".forge/runs/2026-11-08T10-00-00-ui-run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(app.join("uitests")).unwrap();
        std::fs::write(app.join("forge.toml"), "").unwrap();
        std::fs::write(
            app.join("uitests/tip.toml"),
            "name = \"tip\"\napp = \"org.forgemobile.tipster\"\n[[step]]\nlaunch = {}\n[[step]]\nscreenshot = \"tip\"\n",
        )
        .unwrap();
        let png = forge_uitest::visual::encode_png(&forge_uitest::visual::Image::new(
            2,
            3,
            [0, 0, 0, 255],
        ))
        .unwrap();
        std::fs::write(run.join("form.png"), &png).unwrap();
        std::fs::write(run.join("failure-step4.png"), &png).unwrap();

        let mut out = Vec::new();
        let missing = accept(&run, None, &[], &mut out).unwrap_err();
        assert!(missing.contains("--scenario"), "{missing}");

        std::fs::write(
            run.join("evidence.json"),
            r#"{"scenario": "uitests/tip.toml"}"#,
        )
        .unwrap();
        accept(&run, None, &[], &mut out).unwrap();
        assert!(app.join("uitests/baselines/tip/form@2x3.png").is_file());
        assert!(!app
            .join("uitests/baselines/tip/failure-step4@2x3.png")
            .exists());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("1 baseline updated"), "{text}");
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
            .starts_with("Sideloaded apps: 2 of 3 on a free account"));
        assert!(Team::Unknown.apps_line(&apps(1)).contains("--team paid"));
        assert!(!Team::Paid.expiry_hint().contains("7 days"));
    }

    #[test]
    fn the_audit_result_counts_findings_and_says_what_it_checks() {
        // Good Dog's My Dog screen from the iPhone, with one control
        // shrunk to Murmur's 44 x 18 pt like button.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/good-dog/uitests/baselines/journey/5-my-dog@1290x2796.tree.json");
        let mut tree: forge_uitest::UiNode =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(audit_result(Some("app"), &tree)["warnings"], 0);
        let index = tree
            .walk()
            .position(|n| {
                n.kind == "Button" && n.identifier.is_some() && n.frame.is_some_and(|f| f.y > 160.0)
            })
            .unwrap();
        let button = tree.nth_mut(index).unwrap();
        let id = button.identifier.clone().unwrap();
        let frame = button.frame.as_mut().unwrap();
        (frame.x, frame.width, frame.height) = (200.0, 44.0, 18.0);
        let result = audit_result(Some("app"), &tree);
        assert_eq!(result["errors"], 0);
        assert_eq!(result["by_issue"]["small_tap_target"], 1, "{result}");
        assert_eq!(result["screen"]["width"], 430.0);
        let text = audit_text(&result);
        assert!(
            text.contains("warning small_tap_target (44 x 18 pt, under 44 x 44)"),
            "{text}"
        );
        assert!(text.contains(&id), "{text}");
        // What isn't measured is said with every result.
        assert!(result["checks"].to_string().contains("contrast"));
    }
}
