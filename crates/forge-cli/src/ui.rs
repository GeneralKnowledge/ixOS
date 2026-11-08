//! `forge ui`: drive an app on a connected iPhone from Linux (Forge UI
//! Testing M0). The phone work is in the `forge-uitest` crate; this file
//! wires it to the command line. See docs/forge-ui-testing.md.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use forge_uitest::baseline::{self, Mode};
use forge_uitest::explore;
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
        .map_err(|e| format!("{e}\n{}", team.expiry_hint()))?;
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

/// `forge ui tree`: the accessibility tree of the screen, plus an audit.
pub fn tree(
    udid: Option<&str>,
    app: Option<&str>,
    any_app: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (front, tree) = screen_tree(udid, app, any_app, out)?;
    write!(out, "{}", tree.render()).map_err(err)?;
    let findings = tree.audit();
    report::set_data(serde_json::json!({
        "app": front,
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
    let (front, tree) = screen_tree(udid, app, any_app, out)?;
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
) -> Result<(Option<String>, forge_uitest::UiNode), String> {
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
    writeln!(
        out,
        "In front: {}",
        front
            .as_deref()
            .unwrap_or("unknown (WebDriverAgent did not say)")
    )
    .map_err(err)?;
    Ok((front, tree))
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
fn broke_off(
    error: String,
    watcher: Result<crate::watch::Watch, String>,
    out_dir: &Path,
) -> String {
    let value = match watcher {
        Ok(w) => w.finish(out_dir, true).value,
        Err(why) => crate::watch::not_watched(&why),
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
    let mut app_dir = crate::evidence::app_of(file);
    let id = report::timestamp();
    let own = {
        let mut dirs: Vec<&Path> = vec![Path::new(".")];
        if let Some(dir) = app_dir.as_deref() {
            dirs.push(dir);
        }
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

/// `forge ui explore`'s options, as given on the command line.
pub struct ExploreOptions {
    /// M6.2: aim at what changed since this commit.
    pub since: Option<String>,
    /// M6.2: aim at these files instead of git's list.
    pub changed: Vec<String>,
    /// M6.2: also aim at these identifiers (`a.b`, or `a.*`).
    pub focus: Vec<String>,
    /// The earlier walk to learn from; default the latest when aimed.
    pub previous: Option<PathBuf>,
    pub no_previous: bool,
    /// Issues to replay after the walk.
    pub verify: usize,
    /// Walk a model of the app instead of the phone.
    pub fixture: Option<PathBuf>,
    pub actions: usize,
    pub minutes: f64,
    pub seed: Option<u64>,
    /// `IDENTIFIER=TEXT`
    pub texts: Vec<String>,
    pub skip: Vec<String>,
    pub allow_destructive: bool,
    /// Explore the app on the person's own data, looking only.
    pub real_data: bool,
    pub largest_text: bool,
    /// `dark` or `light`; `None` follows the phone.
    pub appearance: Option<String>,
    /// The app's own launch arguments, after Forge's (for example one that
    /// starts past onboarding with fixture data).
    pub launch_args: Vec<String>,
    pub per_screen: usize,
    pub screenshots: bool,
}

/// The explorer's settings for the app in `project` (its forge.toml bundle
/// id), before the phone says what it is installed as.
pub fn explore_config(project: &Path, options: &ExploreOptions) -> Result<explore::Config, String> {
    let forge_toml = project.join("forge.toml");
    let text = std::fs::read_to_string(&forge_toml).map_err(|e| {
        format!(
            "{}: {e} (forge ui explore works on Forge apps: pass --project <app>)",
            forge_toml.display()
        )
    })?;
    let forge = crate::config::ForgeConfig::parse(&text)?;
    let bundle_id = forge.app.bundle_id;
    let test_data_args = forge.ui_test.map(|u| u.test_data_args).unwrap_or_default();
    // Never change the person's data: either the app starts on throwaway
    // data, or the walk only looks (seen on Cati 2026-11-08: a walk on a
    // real library rated a book and saved entries).
    let test_data = !options.real_data && !test_data_args.is_empty();
    if !test_data && !options.real_data {
        return Err(format!(
            "{} has no test-data mode, so exploring would change the data on the phone. \
             Add `[ui_test] test_data_args = [\"-ForgeUITestData\"]` to its forge.toml once the app \
             starts on throwaway data with that argument (see docs/forge-explore.md), or pass \
             --real-data to only look around without changing anything",
            forge_toml.display()
        ));
    }
    if !test_data && options.allow_destructive {
        return Err(
            "--allow-destructive needs the app's test-data mode; it is never used with --real-data"
                .into(),
        );
    }
    let mut fixtures = std::collections::HashMap::new();
    for pair in &options.texts {
        let (id, text) = pair
            .split_once('=')
            .ok_or_else(|| format!("--text {pair:?}: write IDENTIFIER=TEXT"))?;
        fixtures.insert(id.to_owned(), text.to_owned());
    }
    if options.actions < 2 {
        return Err(
            "--actions must be at least 2: the launch is one, and the walk needs another to try anything"
                .into(),
        );
    }
    if !(options.minutes > 0.0 && options.minutes.is_finite()) {
        return Err("--minutes must be more than 0".into());
    }
    let seed = options.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(1)
    });
    Ok(explore::Config {
        app: bundle_id.clone(),
        script_app: bundle_id,
        max_actions: options.actions,
        budget: Duration::from_secs_f64(options.minutes * 60.0),
        seed,
        launch_args: {
            let mut args = if test_data {
                test_data_args
            } else {
                Vec::new()
            };
            if let Some(mode) = &options.appearance {
                args.extend([explore::APPEARANCE_ARG.to_string(), mode.clone()]);
            }
            if options.largest_text {
                args.extend(explore::LARGEST_TEXT_ARGS.iter().map(|a| a.to_string()));
            }
            args.extend(options.launch_args.iter().cloned());
            args
        },
        test_data,
        fixtures,
        allow_destructive: options.allow_destructive,
        skip: options.skip.clone(),
        per_screen: options.per_screen.max(1),
        screenshots: options.screenshots,
        ..explore::Config::default()
    })
}

/// The walk's focus (M6.2): the change's identifiers from `forge ui focus`,
/// identifiers given with `--focus`, and routes and issues from the
/// previous walk. Writes `focus.json` into `out_dir`. Returns the focus and
/// the previous walk's report with its path, when one was used.
fn explore_focus(
    project: &Path,
    options: &ExploreOptions,
    out_dir: &Path,
    kind: &str,
    out: &mut dyn Write,
) -> Result<
    (
        forge_uitest::focus::Focus,
        Option<(serde_json::Value, PathBuf)>,
    ),
    String,
> {
    use forge_uitest::focus::{Link, Strength};
    let aimed = options.since.is_some() || !options.changed.is_empty() || !options.focus.is_empty();
    // A plain walk learns from an earlier one only when asked to.
    if !aimed && options.previous.is_none() {
        return Ok((Default::default(), None));
    }
    let mut focus = forge_uitest::focus::Focus::default();
    let mut impact_json = serde_json::Value::Null;
    if options.since.is_some() || !options.changed.is_empty() {
        let (_, _, impact) =
            crate::focus::read(project, options.since.as_deref(), &options.changed)?;
        writeln!(
            out,
            "Aimed at {} changed file(s): {} identifier(s) declared, {} way(s) in, {} file(s) without a link",
            impact.changed.len(),
            impact.links.iter().filter(|l| l.strength == Strength::Direct).count(),
            impact.links.iter().filter(|l| l.strength == Strength::Entry).count(),
            impact.unmapped.len()
        )
        .map_err(err)?;
        focus = impact.focus();
        impact_json = impact.to_json();
    }
    for id in &options.focus {
        let prefix = id.ends_with(".*");
        focus.links.push(Link {
            id: id.trim_end_matches('*').to_string(),
            prefix,
            strength: Strength::Direct,
            file: String::new(),
            how: "given with --focus".into(),
        });
    }
    if aimed && focus.links.is_empty() {
        writeln!(
            out,
            "No identifier is tied to the change (see focus.json): the walk is the plain seeded one"
        )
        .map_err(err)?;
    }
    // Learning from the previous walk: its routes to the focus, and its
    // issues to check again.
    // Only a walk of the same kind: a model's issues say nothing about the
    // phone, and the phone's say nothing about a model.
    let previous = if options.no_previous {
        None
    } else if let Some(p) = &options.previous {
        let theirs = walk_kind(p);
        if theirs != kind {
            return Err(format!(
                "--previous {} is a {theirs} walk and this is a {kind} walk: their issues can't be compared (a fixture walk shows how Forge explores, not how the app behaves on the phone)",
                p.display()
            ));
        }
        Some(p.clone())
    } else {
        latest_walk(project, out_dir, kind)
    };
    let previous = match previous {
        Some(path) => {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("--previous {}: {e}", path.display()))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("--previous {}: {e}", path.display()))?;
            focus.routes = forge_uitest::focus::routes_from(&value, &focus, 6);
            writeln!(
                out,
                "Learning from {}: {} route(s) to follow first",
                path.display(),
                focus.routes.len()
            )
            .map_err(err)?;
            Some((value, path))
        }
        None => None,
    };
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let doc = serde_json::json!({
        "change": impact_json,
        "given": options.focus,
        "previous": previous.as_ref().map(|(_, p)| p.display().to_string()),
        "routes": focus.routes.iter().map(|r| serde_json::json!({
            "kind": r.kind, "target": r.target,
            "steps": r.actions.iter().map(|a| a.describe()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    std::fs::write(
        out_dir.join("focus.json"),
        serde_json::to_vec_pretty(&doc).unwrap_or_default(),
    )
    .map_err(|e| format!("{}: {e}", out_dir.display()))?;
    Ok((focus, previous))
}

/// The app's latest walk with an `explore.json`, other than `out_dir`.
/// `device` or `fixture`, from the `evidence.json` next to a walk's
/// `explore.json`; `unknown` without one.
fn walk_kind(explore_json: &Path) -> String {
    explore_json
        .parent()
        .map(|d| d.join("evidence.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["kind"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// The newest earlier walk of the same kind (`device` or `fixture`).
fn latest_walk(project: &Path, out_dir: &Path, kind: &str) -> Option<PathBuf> {
    let runs = project.join(".forge/runs");
    let mut walks: Vec<PathBuf> = std::fs::read_dir(&runs)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("-ui-explore"))
        })
        .filter(|p| p != out_dir && p.join("explore.json").is_file())
        .filter(|p| walk_kind(&p.join("explore.json")) == kind)
        .collect();
    walks.sort();
    walks.pop().map(|p| p.join("explore.json"))
}

/// The newest earlier device walk launched with the same arguments and with
/// no error: what this walk's screenshots are compared with (M6.3).
fn last_clean_walk(
    project: &Path,
    out_dir: &Path,
    launch_args: &[String],
) -> Option<(PathBuf, serde_json::Value)> {
    let runs = project.join(".forge/runs");
    let mut walks: Vec<PathBuf> = std::fs::read_dir(&runs)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("-ui-explore"))
        })
        .filter(|p| p != out_dir && walk_kind(&p.join("explore.json")) == "device")
        .collect();
    walks.sort();
    walks.into_iter().rev().find_map(|dir| {
        let text = std::fs::read_to_string(dir.join("explore.json")).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let same_launch = value["launch_args"].as_array().is_some_and(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .eq(launch_args.iter().map(String::as_str))
        });
        let clean = !value["issues"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|i| i["severity"] == "error");
        (same_launch && clean).then_some((dir, value))
    })
}

/// `forge ui explore`: walk a Forge app's screens and report what is wrong.
/// Fails when the walk found an error (a crash, an unlabelled control).
pub fn explore(
    runner: &dyn CommandRunner,
    udid: Option<&str>,
    project: &Path,
    options: ExploreOptions,
    out_dir: Option<&Path>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let mut config = explore_config(project, &options)?;
    let out_dir = out_dir.map(Path::to_path_buf).unwrap_or_else(|| {
        project
            .join(".forge/runs")
            .join(format!("{}-ui-explore", report::timestamp()))
    });
    let kind = if options.fixture.is_some() {
        "fixture"
    } else {
        "device"
    };
    let (focus, previous) = explore_focus(project, &options, &out_dir, kind, out)?;
    config.focus = focus;
    config.verify = options.verify;
    let started = std::time::Instant::now();
    // Read before the walk, so the bundle names the code that was explored.
    let mut build = crate::evidence::provenance(runner, project);
    let (report, watched, device) = match &options.fixture {
        // A model of the app: no phone, no watch, and the bundle says so.
        Some(model) => {
            let mut app = forge_uitest::model::Model::load(model)?;
            if app.app != config.script_app {
                return Err(format!(
                    "{} models {}, but the app's bundle id is {}",
                    model.display(),
                    app.app,
                    config.script_app
                ));
            }
            config.app = app.app.clone();
            writeln!(
                out,
                "Walking a model of the app ({}), not the phone{}",
                model.display(),
                app.note
                    .as_deref()
                    .map(|n| format!(": {n}"))
                    .unwrap_or_default()
            )
            .map_err(err)?;
            let report = explore::run(&config, &mut app, Some(&out_dir), out).map_err(err)?;
            let device = serde_json::json!({
                "fixture": model.display().to_string(),
                "synthesized_screens": app.synthesized,
            });
            let watched = crate::watch::Finished {
                value: crate::watch::not_watched("a model of the app: there is no phone to watch"),
                tail: Vec::new(),
                artifacts: Vec::new(),
            };
            (report, watched, device)
        }
        None => {
            // Reuses a running `forge ui start` session's runner when there
            // is one.
            let (phone, mut apps, held, mut driver) = connect(udid, out)?;
            config.app = resolve(&phone, &mut apps, &held, &config.script_app)?;
            let watcher = crate::watch::product(project)
                .map(|product| {
                    crate::watch::Watch::start(crate::watch::usb, &phone.udid, &product, project)
                })
                .map_err(|e| format!("the app's executable name isn't known ({e})"));
            let report = match explore::run(&config, &mut driver, Some(&out_dir), out) {
                Ok(report) => report,
                Err(e) => return Err(broke_off(err(e), watcher, &out_dir)),
            };
            let closed = report.issues.iter().any(|i| i.issue == "app_closed");
            let watched = match watcher {
                Ok(w) => w.finish(&out_dir, closed),
                Err(why) => crate::watch::Finished {
                    value: crate::watch::not_watched(&why),
                    tail: Vec::new(),
                    artifacts: Vec::new(),
                },
            };
            let device = serde_json::json!({
                "udid": phone.udid,
                "ios_major": phone.ios_major().ok(),
            });
            (report, watched, device)
        }
    };
    writeln!(out, "Report: {}", out_dir.join("explore.json").display()).map_err(err)?;
    // Screens against the last clean walk launched the same way (M6.3):
    // information for the reader, never an issue.
    let visual = match (kind, options.screenshots) {
        ("device", true) => match last_clean_walk(project, &out_dir, &report.launch_args) {
            Some((dir, earlier)) => {
                let screens: Vec<(String, Option<String>)> = report
                    .screens
                    .iter()
                    .map(|s| (s.id.clone(), s.screenshot.clone()))
                    .collect();
                let v = forge_uitest::baseline::compare_walks(&out_dir, &screens, &dir, &earlier);
                writeln!(
                    out,
                    "Against the last clean walk ({}): {} of {} screen(s) look different (not issues; see visual/)",
                    dir.display(),
                    v["changed"],
                    v["compared"]
                )
                .map_err(err)?;
                v
            }
            None => serde_json::json!({
                "compared": 0,
                "note": "No earlier clean walk launched the same way (same test data, appearance and text size) to compare screenshots with."
            }),
        },
        ("device", false) => serde_json::json!({"compared": 0, "note": "--no-screenshots"}),
        _ => serde_json::json!({"compared": 0, "note": "A model of the app has no screenshots."}),
    };
    // What changed since the previous walk: new, still there, gone (only
    // when this walk did what showed it), not rechecked.
    let compared = previous.as_ref().map(|(value, path)| {
        let mut c = forge_uitest::focus::compare(value, &path.display().to_string(), &report);
        // Both walks are of this kind (see explore_focus).
        c["kind"] = serde_json::json!(kind);
        c
    });
    if let Some(c) = &compared {
        let n = |k: &str| c[k].as_array().map_or(0, Vec::len);
        writeln!(
            out,
            "Against the previous walk: {} new, {} still there, {} gone, {} not rechecked",
            n("new"),
            n("still_present"),
            n("gone"),
            n("not_rechecked")
        )
        .map_err(err)?;
        let _ = std::fs::write(
            out_dir.join("compare.json"),
            serde_json::to_vec_pretty(c).unwrap_or_default(),
        );
    }
    if let Some(f) = &report.focus {
        let reached = f["targets"].as_array().map_or(0, Vec::len)
            - f["not_reached"].as_array().map_or(0, Vec::len);
        writeln!(
            out,
            "Focus: {reached} of {} declared identifier(s) reached, {} tried; first at step {}",
            f["targets"].as_array().map_or(0, Vec::len),
            f["tried"].as_object().map_or(0, |t| t.len()),
            f["first_direct_action"]
        )
        .map_err(err)?;
        writeln!(
            out,
            "  Ties to the change are read from the source as text: each issue's focus_basis says declared or inferred, never proven"
        )
        .map_err(err)?;
    }
    let summary = serde_json::json!({
        // `fixture`: a model of the app, no phone. It shows how Forge
        // explores and reports, never how the app behaves.
        "kind": kind,
        "app": report.app,
        "seed": report.seed,
        "stopped": report.stopped,
        "actions": report.actions,
        "screens": report.screens.len(),
        "issues": report.issues,
        "focus": report.focus,
        "compare": compared,
        "visual": visual,
    });
    build["installed_as"] = serde_json::json!(config.app);
    let tried = report
        .journal
        .iter()
        .filter(|s| s.action != "launch")
        .count();
    let evidence = crate::evidence::write_explore_bundle(
        &crate::evidence::ExploreRun {
            build,
            device,
            explore: summary.clone(),
            watch: Some(watched.value.clone()),
            artifacts: watched.artifacts,
            // A walk that tried nothing checked nothing: never a pass.
            ok: report.errors() == 0 && tried > 0,
            duration_ms: started.elapsed().as_millis() as u64,
            kind,
        },
        &out_dir,
        &report::timestamp(),
    )?;
    writeln!(out, "Evidence: {}", evidence.display()).map_err(err)?;
    if kind == "fixture" {
        writeln!(
            out,
            "Fixture walk: this shows how Forge explores and reports on a model, not how the app behaves on a phone. Its bundle is kind \"fixture\" and never counts as a device result."
        )
        .map_err(err)?;
    }
    let mut data = summary;
    data["report"] = serde_json::json!(out_dir.join("explore.json"));
    data["out"] = serde_json::json!(out_dir.display().to_string());
    data["evidence"] = serde_json::json!(evidence.display().to_string());
    report::set_data(data);
    if tried == 0 {
        return Err(format!(
            "exploring tried no control: {} screen(s) seen, every control skipped or unreachable (see skipped in {}). Nothing was checked",
            report.screens.len(),
            out_dir.join("explore.json").display()
        ));
    }
    match report.errors() {
        0 => Ok(()),
        n => Err(format!(
            "exploring found {n} error(s); each has a replay file under {}",
            out_dir.join("repro").display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> ExploreOptions {
        ExploreOptions {
            since: None,
            changed: vec![],
            focus: vec![],
            previous: None,
            no_previous: false,
            verify: 3,
            fixture: None,
            actions: 60,
            minutes: 10.0,
            seed: Some(3),
            texts: vec!["bookForm.title=Dune".into()],
            skip: vec![],
            allow_destructive: false,
            real_data: false,
            largest_text: true,
            appearance: None,
            launch_args: vec![],
            per_screen: 15,
            screenshots: true,
        }
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

    fn pocket_library() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/pocket-library")
    }

    #[test]
    fn explore_reads_the_bundle_id_and_options() {
        let config = explore_config(&pocket_library(), &options()).unwrap();
        assert_eq!(config.script_app, "org.forgemobile.pocketlibrary");
        assert_eq!(config.seed, 3);
        assert_eq!(config.fixtures["bookForm.title"], "Dune");
        assert!(config.test_data);
        assert_eq!(
            config.launch_args,
            [
                "-ForgeUITestData",
                explore::LARGEST_TEXT_ARGS[0],
                explore::LARGEST_TEXT_ARGS[1]
            ]
        );
        assert_eq!(config.budget, Duration::from_secs(600));
        // --real-data: no test-data argument, and the walk only looks.
        let real = ExploreOptions {
            real_data: true,
            largest_text: false,
            ..options()
        };
        let config = explore_config(&pocket_library(), &real).unwrap();
        assert!(!config.test_data);
        assert!(config.launch_args.is_empty());
        // Dark mode is passed on every launch, like the text size.
        let dark = ExploreOptions {
            appearance: Some("dark".into()),
            ..options()
        };
        let config = explore_config(&pocket_library(), &dark).unwrap();
        assert_eq!(
            config.launch_args[..3],
            ["-ForgeUITestData", "-ForgeAppearance", "dark"]
        );
        // The app's own arguments come after Forge's.
        let own = ExploreOptions {
            launch_args: vec!["-GoodDogSampleDog".into()],
            largest_text: false,
            ..options()
        };
        let config = explore_config(&pocket_library(), &own).unwrap();
        assert_eq!(config.launch_args.last().unwrap(), "-GoodDogSampleDog");
        let reckless = ExploreOptions {
            allow_destructive: true,
            ..real
        };
        assert!(explore_config(&pocket_library(), &reckless).is_err());
    }

    #[test]
    fn explore_refuses_options_that_would_check_nothing() {
        for bad in [
            ExploreOptions {
                actions: 1,
                ..options()
            },
            ExploreOptions {
                minutes: 0.0,
                ..options()
            },
            ExploreOptions {
                texts: vec!["no-equals".into()],
                ..options()
            },
        ] {
            assert!(explore_config(&pocket_library(), &bad).is_err());
        }
    }

    #[test]
    fn explore_refuses_an_app_without_test_data() {
        let dir = tempfile::tempdir().unwrap();
        let forge = std::fs::read_to_string(pocket_library().join("forge.toml")).unwrap();
        let without = forge.split("\n# UI automation.").next().unwrap();
        assert!(!without.contains("[ui_test]"));
        std::fs::write(dir.path().join("forge.toml"), without).unwrap();
        let err = explore_config(dir.path(), &options()).unwrap_err();
        assert!(err.contains("test-data mode"), "{err}");
        assert!(err.contains("--real-data"), "{err}");
        let real = ExploreOptions {
            real_data: true,
            ..options()
        };
        assert!(!explore_config(dir.path(), &real).unwrap().test_data);
    }

    #[test]
    fn explore_needs_a_forge_app_and_well_formed_options() {
        let dir = tempfile::tempdir().unwrap();
        let err = explore_config(dir.path(), &options()).unwrap_err();
        assert!(err.contains("--project"), "{err}");
        let bad = ExploreOptions {
            texts: vec!["no-equals".into()],
            ..options()
        };
        let err = explore_config(&pocket_library(), &bad).unwrap_err();
        assert!(err.contains("IDENTIFIER=TEXT"), "{err}");
        let bad = ExploreOptions {
            minutes: 0.0,
            ..options()
        };
        assert!(explore_config(&pocket_library(), &bad).is_err());
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
    fn a_walk_learns_only_from_walks_of_its_own_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let runs = tmp.path().join(".forge/runs");
        let walk = |name: &str, kind: Option<&str>| {
            let dir = runs.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("explore.json"), "{}").unwrap();
            if let Some(kind) = kind {
                std::fs::write(
                    dir.join("evidence.json"),
                    serde_json::json!({"kind": kind}).to_string(),
                )
                .unwrap();
            }
            dir
        };
        let device = walk("2026-11-08T10-00-00-ui-explore", Some("device"));
        // Newer, but a model of the app: never the previous of a phone walk.
        let fixture = walk("2026-11-08T11-00-00-ui-explore", Some("fixture"));
        walk("2026-11-08T12-00-00-ui-explore", None);
        let now = runs.join("2026-11-08T13-00-00-ui-explore");
        assert_eq!(
            latest_walk(tmp.path(), &now, "device"),
            Some(device.join("explore.json"))
        );
        assert_eq!(
            latest_walk(tmp.path(), &now, "fixture"),
            Some(fixture.join("explore.json"))
        );
        assert_eq!(walk_kind(&fixture.join("explore.json")), "fixture");
        // Named by hand, a walk of the other kind is refused, not compared.
        let options = ExploreOptions {
            previous: Some(fixture.join("explore.json")),
            ..options()
        };
        let mut out = Vec::new();
        let refused = explore_focus(tmp.path(), &options, &now, "device", &mut out).unwrap_err();
        assert!(
            refused.contains("is a fixture walk and this is a device walk"),
            "{refused}"
        );
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

    #[test]
    fn screenshots_are_compared_with_the_last_clean_walk_launched_the_same_way() {
        let tmp = tempfile::tempdir().unwrap();
        let runs = tmp.path().join(".forge/runs");
        let walk = |name: &str, kind: &str, args: &[&str], error: bool| {
            let dir = runs.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let issues = if error {
                serde_json::json!([{"severity": "error"}])
            } else {
                serde_json::json!([{"severity": "warning"}])
            };
            std::fs::write(
                dir.join("explore.json"),
                serde_json::json!({"launch_args": args, "issues": issues, "screens": []})
                    .to_string(),
            )
            .unwrap();
            std::fs::write(
                dir.join("evidence.json"),
                serde_json::json!({"kind": kind}).to_string(),
            )
            .unwrap();
            dir
        };
        let light = walk(
            "2026-11-08T09-00-00-ui-explore",
            "device",
            &["-ForgeUITestData"],
            false,
        );
        let dark = walk(
            "2026-11-08T10-00-00-ui-explore",
            "device",
            &["-ForgeUITestData", "-ForgeAppearance", "dark"],
            false,
        );
        // Newer, but with a crash: not clean.
        walk(
            "2026-11-08T11-00-00-ui-explore",
            "device",
            &["-ForgeUITestData"],
            true,
        );
        // Newer, but a model.
        walk(
            "2026-11-08T12-00-00-ui-explore",
            "fixture",
            &["-ForgeUITestData"],
            false,
        );
        let now = runs.join("2026-11-08T13-00-00-ui-explore");
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            last_clean_walk(tmp.path(), &now, &args(&["-ForgeUITestData"])).map(|(d, _)| d),
            Some(light)
        );
        assert_eq!(
            last_clean_walk(
                tmp.path(),
                &now,
                &args(&["-ForgeUITestData", "-ForgeAppearance", "dark"])
            )
            .map(|(d, _)| d),
            Some(dark)
        );
        assert!(last_clean_walk(tmp.path(), &now, &args(&["-ForgeAppearance", "light"])).is_none());
    }
}
