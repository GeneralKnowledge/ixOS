mod agent;
mod app_config;
mod app_diagnose;
mod check;
mod commands;
mod config;
mod crash;
mod device;
mod diagnose;
mod doctor;
mod evidence;
mod focus;
mod history;
mod inspect;
mod ios_builder;
mod ios_env;
mod ledger;
mod live;
mod report;
mod run_record;
mod runner;
mod setup;
mod status;
mod string_catalog;
mod swift_check;
mod ui;
mod ui_session;
mod verify;
mod watch;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "forge", about = "Forge Mobile developer CLI", version)]
struct Cli {
    /// Print one JSON result object on stdout (`forge.result/1`, see
    /// docs/forge-mcp.md); progress goes to stderr and a log file. The exit
    /// code names the failed stage: 10 Rust, 11 Swift, 12 signing, 13 device,
    /// 14 tests. `forge doctor --json` keeps its own report format
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

// Parsed once per process: the size of the largest variant doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Commands {
    /// Report toolchain and environment readiness
    /// (`--json` prints one object: tool versions, Rust targets, env, readiness)
    Doctor,
    /// Get this machine ready to build: checks each piece, installs what it
    /// safely can (asking before anything needs sudo), and ends with the
    /// readiness report. Safe to run again
    Setup {
        #[command(subcommand)]
        cmd: SetupCommands,
    },
    /// Run the full local CI matrix: fmt, clippy, tests, binding drift, template sync, Android JVM and Swift host tests
    Check {
        /// Stop at the first failing step instead of running them all
        #[arg(long)]
        fail_fast: bool,
        #[command(subcommand)]
        what: Option<CheckCommands>,
    },
    /// Create a new Forge Mobile app from template
    Init {
        name: String,
        #[arg(long, default_value = "org.example.myapp")]
        bundle_id: String,
        /// Target platform. Android is work in progress: it builds, but gets no new features
        #[arg(long, default_value = "ios", value_parser = ["ios", "android"])]
        platform: String,
        /// Directory to create the app in (default: ./<NAME>, or
        /// <workspace>/examples/<NAME> when run inside the Forge checkout).
        /// Relative paths are resolved against the current directory.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,
        /// Start from the minimal starter: one screen, a list saved on the
        /// phone, nothing else (iOS). Without it you get the full starter,
        /// which shows every pattern a larger app needs
        #[arg(long)]
        minimal: bool,
        /// Also write the optional AI coding agent files (AGENTS.md,
        /// CLAUDE.md, .mcp.json), as `forge agent setup` does
        #[arg(long)]
        agent: bool,
    },
    /// Optional files for AI coding agents. Nothing in Forge needs them
    Agent {
        #[command(subcommand)]
        cmd: AgentCommands,
    },
    /// iOS app configuration from forge.toml: Info.plist, App.entitlements, xtool.yml
    Config {
        #[command(subcommand)]
        cmd: ConfigCommands,
    },
    /// Run an app's tests: its Rust crate, then its host tests (JVM unit tests,
    /// or XCTest on macOS). Without an app, Forge's own workspace tests.
    Test {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
        /// Also run the Forge workspace's own tests and format check
        #[arg(long)]
        workspace: bool,
    },
    /// The capability ledger: records in docs/ledger/, page generated from them
    Ledger {
        #[command(subcommand)]
        cmd: LedgerCommands,
    },
    /// Generate UniFFI bindings (Swift or Kotlin)
    Bindings {
        #[command(subcommand)]
        cmd: BindingsCommands,
    },
    /// Build a platform application
    Build {
        #[command(subcommand)]
        cmd: BuildCommands,
    },
    /// List connected devices (adb and/or xtool)
    Devices,
    /// Install built IPA to device
    Install {
        #[command(subcommand)]
        cmd: InstallCommands,
    },
    /// Build, sign and install on a connected iPhone (`xtool dev run`); open the app from the home screen
    Run {
        #[command(subcommand)]
        cmd: RunCommands,
    },
    /// Why isn't my app working? Checks the last `forge run ios`, the iPhone,
    /// whether the app is installed and still signed, its newest crash report,
    /// and what it logs while you open it, then says what to do next
    Diagnose {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
        /// The iPhone to check, when more than one is connected
        #[arg(long)]
        udid: Option<String>,
        /// How long to read the app's log while you open it: 15s (the
        /// default), 1m; 0 skips it
        #[arg(long = "for", value_name = "DURATION", value_parser = report::parse_duration)]
        duration: Option<std::time::Duration>,
    },
    /// A connected iPhone over USB, from Linux: its details, system log and
    /// crash reports, and the live connection to a development build of your
    /// app (needs usbmuxd and a phone that trusts this computer)
    Device {
        #[command(subcommand)]
        cmd: DeviceCommands,
    },
    /// Serve Forge to a coding agent over MCP (stdio): check, build, install,
    /// launch, inspect, test and diagnose tools, plus Forge's docs as
    /// resources (docs/forge-mcp.md). Started by the agent's MCP client
    Mcp {
        /// Tools only work inside this folder (default: the current directory)
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// Let the agent do something that spends a phone resource: `install`
        /// lets forge_install_device install apps (an app slot and an App ID
        /// on a free team)
        #[arg(long, value_name = "WHAT", value_parser = ["install"])]
        allow: Vec<String>,
        /// Only these bundle ids may be installed (repeatable), e.g.
        /// org.forgemobile.agentlab, the agent sandbox id
        #[arg(long = "bundle-id", value_name = "ID")]
        bundle_ids: Vec<String>,
        /// The only phone device tools may touch
        #[arg(long)]
        udid: Option<String>,
        /// Leave out every tool that touches a phone (install, launch,
        /// screenshots, logs, UI tests), even if one is plugged in
        #[arg(long, conflicts_with_all = ["allow", "udid"])]
        no_device: bool,
    },
    /// What an app folder holds, from its files: Swift files and their
    /// imports, accessibility identifiers written as literals, UI tests,
    /// generated files, the Rust crate, convention warnings. Changes nothing
    Inspect {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
    },
    /// Does every requirement in the app's requirements.toml have current
    /// evidence? Reads the bundles that `forge check swift` (Linux twin) and
    /// `forge ui run` (iPhone) wrote to .forge/runs/; runs nothing itself
    /// (docs/evidence.md)
    Verify {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
        /// Only require the Linux twin evidence (a session without the phone).
        /// iPhone results are still shown
        #[arg(long)]
        twin_only: bool,
        /// Write the run record for an agent run that started at this run id
        /// (`2026-11-08T10-00-00`, UTC): commits, builds, checks, failures and
        /// repairs, and what was not run, as `<app>/.forge/runs/<id>-run/`
        #[arg(long, value_name = "START", conflicts_with_all = ["accept", "twin_only"])]
        run: Option<String>,
        /// The `forge mcp` journal to read with --run (default: every
        /// journal started since START, in the app's and the repository's runs)
        #[arg(long, value_name = "PATH", requires = "run")]
        journal: Option<PathBuf>,
        /// Record that you, a person, checked this requirement on the phone.
        /// Needs a terminal and asks you to type the id back; refused inside
        /// `forge mcp` and from scripts
        #[arg(
            long,
            value_name = "ID",
            requires = "note",
            conflicts_with = "twin_only"
        )]
        accept: Option<String>,
        /// What you looked at, with --accept
        #[arg(long, value_name = "TEXT", requires = "accept")]
        note: Option<String>,
    },
    /// Read crash reports (.ips) on this computer: the kind of crash, the
    /// message, the likely cause and the crashed thread with Swift and Rust
    /// function, file and line from the app's local build (no phone needed)
    Crash {
        /// .ips files, for example from `forge device crashes --pull`
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        #[arg(
            long,
            help = "App directory whose build symbolicates the reports (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
        /// A binary or .dSYM of the build that crashed (repeatable), when it
        /// isn't in the app folder
        #[arg(long = "binary", value_name = "PATH")]
        binaries: Vec<PathBuf>,
        /// Every thread, not only the one that crashed
        #[arg(long)]
        all_threads: bool,
        /// Frames shown per thread (text output)
        #[arg(long, value_name = "N", default_value_t = crash::TEXT_FRAMES)]
        frames: usize,
    },
    /// Drive an app on a connected iPhone from Linux: find elements by
    /// accessibility identifier, tap, type, read the screen, take screenshots
    /// (XCTest through WebDriverAgent; docs/forge-ui-testing.md)
    Ui {
        #[command(subcommand)]
        cmd: UiCommands,
    },
}

#[derive(Subcommand)]
enum SetupCommands {
    /// Linux: the iOS Rust target, usbmuxd and FUSE, the pinned xtool, Swift,
    /// the Darwin SDK from an Xcode.xip you downloaded, and xtool's Apple
    /// sign-in. Never downloads Xcode or sees your Apple ID
    Ios {
        /// The Xcode 26 .xip to install the Darwin SDK from (default: the
        /// newest Xcode*.xip in ~/Downloads, if the SDK isn't installed yet)
        #[arg(long, value_name = "PATH")]
        xip: Option<PathBuf>,
        /// Install the SDK from the .xip even when a compatible one is
        /// already installed (normally a rerun keeps it)
        #[arg(long)]
        reinstall_sdk: bool,
        /// Answer yes to every question, for scripts
        #[arg(long, short)]
        yes: bool,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum UiCommands {
    /// Mount the Developer Disk Image, install the WebDriverAgent runner and
    /// check that it starts. Run after every phone restart
    Setup {
        /// Which phone, when several are connected
        #[arg(long)]
        udid: Option<String>,
        /// Folder with a personalised Developer Disk Image (only needed when
        /// none is mounted; see docs/forge-ui-testing.md)
        #[arg(long, value_name = "DIR")]
        ddi: Option<PathBuf>,
        /// Install this WebDriverAgentRunner-Runner.app with `xtool install`
        /// (skipped when a runner is already on the phone)
        #[arg(long, value_name = "APP")]
        runner: Option<PathBuf>,
        /// Install the runner even if one is on the phone, e.g. to re-sign
        /// it before a free account's 7 days run out
        #[arg(long, requires = "runner")]
        reinstall: bool,
        /// The Apple team xtool signs with, `free` or `paid` (default: the
        /// current app's `[ios.signing] team_type`). Paid teams have no
        /// 3-app limit and no 7-day expiry
        #[arg(long, value_name = "TYPE", value_parser = ["free", "paid"])]
        team: Option<String>,
    },
    /// Print the accessibility tree of the screen and flag unlabelled controls
    Tree {
        #[arg(long)]
        udid: Option<String>,
        /// Bring this app (its forge.toml bundle id) to the front first
        #[arg(long, value_name = "BUNDLE_ID")]
        app: Option<String>,
        /// Also read apps Forge didn't build, as far as
        /// ~/.config/forge/device-control.toml allows
        #[arg(long)]
        any_app: bool,
        /// Only elements whose identifier, label or value contains this
        /// (any case), one line each
        #[arg(long, value_name = "TEXT")]
        find: Option<String>,
        /// Only elements on screen, one line each
        #[arg(long)]
        visible: bool,
        /// At most this many elements, one line each
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Audit the screen for accessibility: unlabelled controls, missing or
    /// duplicate identifiers, tap targets under 44 x 44 pt, text cut off.
    /// Fails on an error
    Audit {
        #[arg(long)]
        udid: Option<String>,
        /// Bring this app (its forge.toml bundle id) to the front first
        #[arg(long, value_name = "BUNDLE_ID")]
        app: Option<String>,
        /// Also read apps Forge didn't build, as far as
        /// ~/.config/forge/device-control.toml allows
        #[arg(long)]
        any_app: bool,
    },
    /// Open the app on the phone (or stop it with --terminate), optionally
    /// in dark or light mode or at the largest text size. Needs `forge ui
    /// setup`; `forge run ios` installs but does not open the app
    Launch {
        #[arg(long)]
        udid: Option<String>,
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
        /// Stop the app instead of opening it
        #[arg(long, conflicts_with_all = ["appearance", "largest_text", "launch_arg"])]
        terminate: bool,
        /// Dark or light mode for this launch (the app is restarted)
        #[arg(long, value_parser = ["dark", "light"])]
        appearance: Option<String>,
        /// The largest accessibility text size for this launch (the app is
        /// restarted)
        #[arg(long)]
        largest_text: bool,
        /// Another launch argument (repeatable; the app is restarted)
        #[arg(long = "launch-arg", value_name = "ARG", allow_hyphen_values = true)]
        launch_arg: Vec<String>,
    },
    /// Tap one control on the app's screen, found by identifier, label,
    /// kind or the text beside it (or, for a person, at a point with --at)
    Tap {
        #[command(flatten)]
        target: UiTarget,
        /// Tap this point instead, as X,Y in points from the top-left: for
        /// a control with no identifier or label. Terminal only; test
        /// files and agents never use coordinates
        #[arg(long, value_name = "X,Y")]
        at: Option<String>,
        #[command(flatten)]
        on: UiOn,
    },
    /// Type text into a field
    Type {
        #[command(flatten)]
        target: UiTarget,
        /// The text to type (a newline presses Return)
        text: String,
        /// Empty the field first
        #[arg(long)]
        clear: bool,
        #[command(flatten)]
        on: UiOn,
    },
    /// Empty a text field
    Clear {
        #[command(flatten)]
        target: UiTarget,
        #[command(flatten)]
        on: UiOn,
    },
    /// Scroll until the element is on screen (paging through a list for a
    /// row it hasn't drawn yet), or scroll a list itself up, down, left or
    /// right
    Scroll {
        #[command(flatten)]
        target: UiTarget,
        /// `visible` (default): until the element is on screen; up, down,
        /// left or right: scroll the element (a list or scroll view) itself
        #[arg(default_value = "visible", value_parser = ["visible", "up", "down", "left", "right"])]
        direction: String,
        /// With `visible`: the list to page through (its identifier;
        /// default: the first list on screen)
        #[arg(long, value_name = "ID")]
        in_id: Option<String>,
        /// With `visible`: which way to page (default down)
        #[arg(long, value_parser = ["up", "down", "left", "right"])]
        toward: Option<String>,
        /// With `visible`: at most this many pages (default 10)
        #[arg(long)]
        pages: Option<usize>,
        #[command(flatten)]
        on: UiOn,
    },
    /// Swipe across an element: a list row's swipe actions, a page, a sheet
    Swipe {
        #[command(flatten)]
        target: UiTarget,
        #[arg(value_parser = ["up", "down", "left", "right"], required_unless_present = "from")]
        direction: Option<String>,
        /// Swipe between two points instead, as X,Y in points (with --to).
        /// Terminal only; test files and agents never use coordinates
        #[arg(
            long,
            value_name = "X,Y",
            requires = "to",
            conflicts_with = "direction"
        )]
        from: Option<String>,
        /// Where a --from swipe ends
        #[arg(long, value_name = "X,Y", requires = "from")]
        to: Option<String>,
        /// With --from: seconds to hold before moving (a drag rather than a swipe)
        #[arg(long, requires = "from", default_value_t = 0.0)]
        hold: f64,
        #[command(flatten)]
        on: UiOn,
    },
    /// Touch and hold an element: context menus, text selection
    LongPress {
        #[command(flatten)]
        target: UiTarget,
        /// How long to hold
        #[arg(long, default_value_t = 1.0)]
        seconds: f64,
        /// Press this point instead, as X,Y in points. Terminal only
        #[arg(long, value_name = "X,Y")]
        at: Option<String>,
        #[command(flatten)]
        on: UiOn,
    },
    /// Tap an element twice, quickly
    DoubleTap {
        #[command(flatten)]
        target: UiTarget,
        /// Double-tap this point instead, as X,Y in points. Terminal only
        #[arg(long, value_name = "X,Y")]
        at: Option<String>,
        #[command(flatten)]
        on: UiOn,
    },
    /// Pinch an element: --scale above 1 zooms in, below 1 zooms out
    Pinch {
        #[command(flatten)]
        target: UiTarget,
        #[arg(long)]
        scale: f64,
        #[command(flatten)]
        on: UiOn,
    },
    /// Turn an element with two fingers (clockwise when positive)
    Rotate {
        #[command(flatten)]
        target: UiTarget,
        #[arg(long, allow_hyphen_values = true)]
        degrees: f64,
        #[command(flatten)]
        on: UiOn,
    },
    /// Tap an element with two fingers
    TwoFingerTap {
        #[command(flatten)]
        target: UiTarget,
        #[command(flatten)]
        on: UiOn,
    },
    /// Turn a picker wheel to a value
    Pick {
        #[command(flatten)]
        target: UiTarget,
        value: String,
        #[command(flatten)]
        on: UiOn,
    },
    /// Press Home or a hardware button
    Press {
        #[arg(value_parser = ["home", "volume-up", "volume-down", "action", "camera"])]
        button: String,
        #[command(flatten)]
        on: UiOn,
    },
    /// Open a URL as the system would: a web page, a deep link, a universal link
    OpenUrl {
        url: String,
        #[command(flatten)]
        on: UiOn,
    },
    /// Answer a system alert or permission prompt: accept, dismiss, or tap
    /// --button; --text checks what it says first (alone, only checks)
    Alert {
        #[arg(value_parser = ["accept", "dismiss", "check"], default_value = "check")]
        answer: String,
        /// Tap the button with this label
        #[arg(long)]
        button: Option<String>,
        /// The alert's title or message contains this
        #[arg(long)]
        text: Option<String>,
        #[command(flatten)]
        on: UiOn,
    },
    /// Tap the navigation bar's back button
    Back {
        #[command(flatten)]
        on: UiOn,
    },
    /// Wait until the app is foreground, background or not_running
    AppState {
        #[arg(value_parser = ["foreground", "background", "not_running"])]
        state: String,
        #[command(flatten)]
        on: UiOn,
    },
    /// Forget the app's answer to a permission prompt, so it asks again
    /// (iOS stops the app)
    ResetPermission {
        #[arg(value_parser = ["contacts", "calendar", "reminders", "photos", "microphone", "camera", "media_library", "home", "bluetooth", "location"])]
        permission: String,
        #[command(flatten)]
        on: UiOn,
    },
    /// Turn the interface portrait or landscape
    Orientation {
        #[arg(value_parser = ["portrait", "landscape"])]
        orientation: String,
        #[command(flatten)]
        on: UiOn,
    },
    /// Lock the phone's screen
    Lock {
        #[command(flatten)]
        on: UiOn,
    },
    /// Unlock it (only a phone without a passcode)
    Unlock {
        #[command(flatten)]
        on: UiOn,
    },
    /// Wait until the screen stops changing
    Settle {
        #[command(flatten)]
        on: UiOn,
    },
    /// Drag an element onto another (--to-id, --to-label), or drag it by
    /// part of its own size (--by down: pull a list to refresh)
    Drag {
        #[command(flatten)]
        target: UiTarget,
        /// Drop it on the element with this identifier
        #[arg(long, value_name = "ID", conflicts_with_all = ["to_label", "by"])]
        to_id: Option<String>,
        /// Drop it on the element with this label
        #[arg(long, value_name = "LABEL", conflicts_with = "by")]
        to_label: Option<String>,
        /// Drag this way from the element's centre, with no hold first
        #[arg(long, value_parser = ["up", "down", "left", "right"])]
        by: Option<String>,
        /// With --by: how far, as a fraction of the element's size (default 0.5)
        #[arg(long, requires = "by")]
        distance: Option<f64>,
        /// With --to-*: seconds to hold before dragging (default 1)
        #[arg(long, conflicts_with = "by")]
        hold: Option<f64>,
        #[command(flatten)]
        on: UiOn,
    },
    /// Press a keyboard key (return, delete) or hide the keyboard (dismiss)
    Key {
        #[arg(value_parser = ["return", "delete", "dismiss"])]
        key: String,
        #[command(flatten)]
        on: UiOn,
    },
    /// Check an element: it is on screen, has a value or label, is enabled
    /// or disabled, or is gone. Waits up to --timeout seconds for it
    Expect {
        #[command(flatten)]
        target: UiTarget,
        /// Its value (a field's text) is this
        #[arg(long)]
        value: Option<String>,
        /// Its label is this
        #[arg(long)]
        has_label: Option<String>,
        /// It is enabled (true) or disabled (false)
        #[arg(long)]
        enabled: Option<bool>,
        /// It is on screen (true) or off it (false)
        #[arg(long)]
        visible: Option<bool>,
        /// A tap would reach it (true) or not (false)
        #[arg(long)]
        hittable: Option<bool>,
        /// It is selected (true) or not (false)
        #[arg(long)]
        selected: Option<bool>,
        /// Exactly this many elements match
        #[arg(long)]
        count: Option<usize>,
        /// Nothing matches it any more
        #[arg(long)]
        gone: bool,
        #[command(flatten)]
        on: UiOn,
    },
    /// Save a screenshot of the phone's screen here
    Screenshot {
        #[arg(long)]
        udid: Option<String>,
        /// Where to save the PNG
        #[arg(default_value = "screenshot.png")]
        path: PathBuf,
        /// Also when an app Forge didn't build is in front, as far as
        /// ~/.config/forge/device-control.toml allows
        #[arg(long)]
        any_app: bool,
    },
    /// Run a UI test file (TOML steps) against its app
    Run {
        #[arg(long)]
        udid: Option<String>,
        /// The test file
        file: PathBuf,
        /// Where screenshots, failure details and evidence.json go (default:
        /// <app>/.forge/runs/<UTC>-ui-run/ when the file is inside an app,
        /// else forge-ui-out)
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Allow a test file that names an app Forge didn't build. What it
        /// may do there comes from ~/.config/forge/device-control.toml;
        /// taps that send, post, pay or delete ask each time
        #[arg(long)]
        any_app: bool,
        /// Screenshots against their baselines (uitests/baselines/<test>/):
        /// `check` fails the run when one changed, `record` makes every
        /// screenshot the baseline, `off` skips it (docs/visual-regression.md)
        #[arg(long, value_name = "MODE", default_value = "check", value_parser = ["check", "record", "off"])]
        visual: String,
        /// Don't read the app's log or look for its crash reports during
        /// the run (one-step runs, where each second counts)
        #[arg(long)]
        no_watch: bool,
    },
    /// Make a run's screenshots the baselines its test is compared with
    Accept {
        /// The run's folder (forge ui run prints it; <app>/.forge/runs/<id>-ui-run)
        run: PathBuf,
        /// The test file (default: the one the run's evidence.json names)
        #[arg(long, value_name = "FILE")]
        scenario: Option<PathBuf>,
        /// Only this screenshot (its name without .png); repeatable
        #[arg(long, value_name = "NAME")]
        only: Vec<String>,
    },
    /// Explore an app without a script: visit its screens, try each safe
    /// control once, and report crashes, error alerts, dead ends and
    /// accessibility problems, each with a `forge ui run` file that replays
    /// it (docs/forge-explore.md)
    Explore {
        #[arg(long)]
        udid: Option<String>,
        /// The app to explore: its folder with forge.toml
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Explore an installed app Forge didn't build, by bundle id (a
        /// reference app for a replication benchmark). Needs --any-app and a
        /// line for the app in ~/.config/forge/device-control.toml:
        /// `operate` looks only (with --real-data), `full` may change its data
        #[arg(long, value_name = "BUNDLE_ID", conflicts_with = "project")]
        app: Option<String>,
        /// Allow --app to name an app Forge didn't build, as far as the
        /// person's device-control policy allows
        #[arg(long)]
        any_app: bool,
        /// Stop after this many actions
        #[arg(long, default_value_t = 60)]
        actions: usize,
        /// Stop after this many minutes, whatever is left
        #[arg(long, default_value_t = 10.0)]
        minutes: f64,
        /// Fixes the order controls are tried in (default: from the clock;
        /// printed, so a walk can be repeated)
        #[arg(long)]
        seed: Option<u64>,
        /// Text for a field, as IDENTIFIER=TEXT (repeatable); other fields
        /// get "Forge"
        #[arg(long = "text", value_name = "ID=TEXT")]
        texts: Vec<String>,
        /// Never touch this identifier or label (repeatable)
        #[arg(long, value_name = "ID_OR_LABEL")]
        skip: Vec<String>,
        /// Also tap delete, remove, reset, sign out and the like (test data
        /// only). Send, pay, share and the like are never tapped
        #[arg(long)]
        allow_destructive: bool,
        /// Explore the app on the data already on the phone, only looking:
        /// nothing that saves, edits, rates, types or changes a setting is
        /// touched. Without it the app needs `[ui_test] test_data_args`
        #[arg(long)]
        real_data: bool,
        /// Launch with the largest accessibility text size
        #[arg(long)]
        largest_text: bool,
        /// Launch in dark or light mode (`-ForgeAppearance`, which Forge
        /// apps honour; without it the app follows the phone)
        #[arg(long, value_parser = ["dark", "light"])]
        appearance: Option<String>,
        /// One of the app's own launch arguments, added after Forge's;
        /// repeat for more (for example `--launch-arg=-GoodDogSampleDog`, so
        /// the walk starts past onboarding)
        #[arg(long = "launch-arg", value_name = "ARG", allow_hyphen_values = true)]
        launch_arg: Vec<String>,
        /// At most this many controls tried per screen
        #[arg(long, default_value_t = 15)]
        per_screen: usize,
        /// Don't save a screenshot of every new screen
        #[arg(long)]
        no_screenshots: bool,
        /// Where explore.json, screens and replay files go (default:
        /// <app>/.forge/runs/<UTC>-ui-explore/)
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Aim the walk at what changed since this commit (`HEAD`: the
        /// uncommitted work): the controls the changed files declare, and
        /// the ways into them, go first (see `forge ui focus`)
        #[arg(long, value_name = "REV")]
        since: Option<String>,
        /// Aim the walk at these changed files instead of git's list
        /// (repeatable)
        #[arg(long, value_name = "FILE")]
        changed: Vec<String>,
        /// Also aim at this identifier, or a prefix ending in `.*`
        /// (repeatable)
        #[arg(long, value_name = "ID")]
        focus: Vec<String>,
        /// The earlier walk to learn from (its explore.json): routes to the
        /// focus, and its issues to check again. Default: the app's latest
        /// walk under .forge/runs, when the walk has a focus
        #[arg(long, value_name = "EXPLORE_JSON")]
        previous: Option<PathBuf>,
        /// Don't use an earlier walk
        #[arg(long)]
        no_previous: bool,
        /// After the walk, replay up to this many issues once more to see
        /// whether they come back (not counted in --actions)
        #[arg(long, default_value_t = 3)]
        verify: usize,
        /// Walk a model of the app (a screen graph of recorded trees,
        /// docs/forge-m6.md) instead of the phone. Proves how Forge
        /// explores, never how the app behaves
        #[arg(long, value_name = "MODEL_JSON")]
        fixture: Option<PathBuf>,
    },
    /// Which screens and controls a change touches, from the source: the
    /// identifiers the changed files declare, the ways into them, the UI
    /// tests that use them, and the files with no link (and why). Needs no
    /// phone
    Focus {
        /// The app: its folder with forge.toml
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Changes since this commit (default `HEAD`: the uncommitted work)
        #[arg(long, value_name = "REV")]
        since: Option<String>,
        /// These files instead of git's list (repeatable)
        #[arg(long, value_name = "FILE")]
        changed: Vec<String>,
    },
    /// Keep one WebDriverAgent runner running in the background, so later
    /// `forge ui` commands skip starting it (about 2 s each). Stops on
    /// `forge ui stop` or after --idle-minutes without a command
    Start {
        #[arg(long)]
        udid: Option<String>,
        /// Stop after this many minutes without a `forge ui` command; 0 never
        #[arg(long, value_name = "MIN", default_value_t = 30)]
        idle_minutes: u64,
        /// Also stop when this process exits (forge mcp passes its own)
        #[arg(long, value_name = "PID")]
        exit_with: Option<u32>,
    },
    /// Stop the background UI session and its runner
    Stop {
        #[arg(long)]
        udid: Option<String>,
    },
    /// Show the background UI session, if any (doesn't touch the phone)
    Status {
        #[arg(long)]
        udid: Option<String>,
    },
    /// Hold a UI session in the foreground until Ctrl-C (what `forge ui
    /// start` runs in the background)
    Serve {
        #[arg(long)]
        udid: Option<String>,
        /// Stop after this many minutes without a `forge ui` command; 0 never
        #[arg(long, value_name = "MIN", default_value_t = 30)]
        idle_minutes: u64,
        /// Also stop when this process exits
        #[arg(long, value_name = "PID")]
        exit_with: Option<u32>,
    },
}

#[derive(Subcommand)]
enum DeviceCommands {
    /// Connected phones: UDID, name, model, iOS version
    List,
    /// One phone's name, model, iOS version and whether Developer Mode is on
    Info {
        /// Which phone, when several are connected (see `forge device list`)
        #[arg(long)]
        udid: Option<String>,
    },
    /// Apps you installed, with their provisioning profile's team and
    /// expiry, and how many of a free team's 3 app slots are in use
    Apps {
        /// Which phone, when several are connected
        #[arg(long)]
        udid: Option<String>,
        /// Also list apps with no provisioning profile (the App Store's)
        #[arg(long)]
        all: bool,
    },
    /// Stream the phone's system log; Ctrl-C stops
    Logs {
        /// Which phone, when several are connected
        #[arg(long)]
        udid: Option<String>,
        /// Only this process's lines: the app's executable as the log shows
        /// it (`<App>Xtool` for apps made with `forge init`, `[ios] xtool_product`)
        #[arg(long, value_name = "NAME")]
        process: Option<String>,
        /// Only lines containing this text (case-insensitive)
        #[arg(long, value_name = "TEXT")]
        grep: Option<String>,
        /// Stop after this many matching lines
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Stop after this long, even if the phone logs nothing: 30s, 2m, 500ms.
        /// With --json, --for or --limit is required
        #[arg(long = "for", value_name = "DURATION", value_parser = report::parse_duration)]
        duration: Option<std::time::Duration>,
    },
    /// List crash reports on the phone, or copy them here with --pull and
    /// symbolicate them against the app's build
    Crashes {
        /// Which phone, when several are connected
        #[arg(long)]
        udid: Option<String>,
        /// Only this app's reports (its executable name), newest first
        #[arg(long, value_name = "NAME")]
        app: Option<String>,
        /// Copy the reports into this folder, symbolicate each one and write
        /// the result next to it (<report>.txt)
        #[arg(long, value_name = "DIR")]
        pull: Option<PathBuf>,
        /// Only the first N reports (the newest N with --app)
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        #[arg(
            long,
            help = "App directory whose build symbolicates the reports (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
    },
    /// Pair this computer with the development build on screen (once per app
    /// and phone; the app must be built with `forge run ios --devtools`)
    Pair {
        #[command(flatten)]
        live: LiveArgs,
    },
    /// Forget the pairing on the phone and on this computer
    Unpair {
        #[command(flatten)]
        live: LiveArgs,
    },
    /// The paired app's phase, uptime, probes and commands
    Status {
        #[command(flatten)]
        live: LiveArgs,
    },
    /// The values of the state probes the app registered
    Snapshot {
        #[command(flatten)]
        live: LiveArgs,
    },
    /// Run one of the app's own commands; with no name, list them
    Cmd {
        #[command(flatten)]
        live: LiveArgs,
        /// The command's name
        name: Option<String>,
        /// Its arguments
        args: Vec<String>,
    },
    /// The app's own log and lifecycle, live; carries on when the app comes
    /// back from the background. Ctrl-C stops
    Connect {
        #[command(flatten)]
        live: LiveArgs,
        /// Show this many buffered records from before connecting
        #[arg(long, value_name = "N", default_value_t = 50)]
        tail: u32,
        /// Stop after this many records
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Stop when the app goes to the background instead of waiting
        #[arg(long)]
        no_follow: bool,
    },
}

/// Which element a one-step `forge ui` command acts on: exactly one of
/// these, as in a test file's step.
#[derive(clap::Args)]
struct UiTarget {
    /// Accessibility identifier, e.g. library.add (preferred)
    #[arg(long)]
    id: Option<String>,
    /// Exact VoiceOver label (with --kind: of that kind)
    #[arg(long)]
    label: Option<String>,
    /// Part of the VoiceOver label
    #[arg(long, value_name = "TEXT")]
    label_contains: Option<String>,
    /// Element type (Button, TextField, CollectionView…), with --label or
    /// --index, for a control with no identifier
    #[arg(long)]
    kind: Option<String>,
    /// With --kind: the nth of that kind on screen, from 0
    #[arg(long, requires = "kind")]
    index: Option<usize>,
    /// The control nearest this text (a field's placeholder or caption)
    #[arg(long, value_name = "TEXT")]
    near: Option<String>,
}

impl UiTarget {
    fn table(self) -> toml::Table {
        let mut t = toml::Table::new();
        let text = [
            ("id", self.id),
            ("label", self.label),
            ("label_contains", self.label_contains),
            ("kind", self.kind),
            ("near", self.near),
        ];
        for (key, value) in text {
            if let Some(v) = value {
                t.insert(key.into(), v.into());
            }
        }
        if let Some(i) = self.index {
            t.insert("index".into(), toml::Value::Integer(i as i64));
        }
        t
    }
}

/// Where a one-step `forge ui` command runs.
#[derive(clap::Args)]
struct UiOn {
    /// Which phone, when several are connected
    #[arg(long)]
    udid: Option<String>,
    #[arg(
        long,
        help = "App directory (default: the current directory if it has forge.toml)"
    )]
    project: Option<PathBuf>,
    /// The app by bundle id instead of --project
    #[arg(long, value_name = "BUNDLE_ID", conflicts_with = "project")]
    app: Option<String>,
    /// Allow --app to name an app Forge didn't build, as far as
    /// ~/.config/forge/device-control.toml allows
    #[arg(long)]
    any_app: bool,
    /// Seconds to wait for the element
    #[arg(long, default_value_t = 10.0)]
    timeout: f64,
}

#[derive(clap::Args)]
struct LiveArgs {
    /// Which phone, when several are connected
    #[arg(long)]
    udid: Option<String>,
    /// The port the development build listens on
    #[arg(long, default_value_t = forge_devproto::DEFAULT_PORT, hide = true)]
    port: u16,
}

#[derive(Subcommand)]
enum AgentCommands {
    /// Write AGENTS.md (the app's rules and commands), CLAUDE.md and
    /// .mcp.json (starts `forge mcp`) into the app. Existing files are kept
    Setup {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
        /// Also add the hosted sosumi.ai MCP server (Apple's developer
        /// documentation as text) to .mcp.json
        #[arg(long)]
        apple_docs: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Regenerate Info.plist, App.entitlements and xtool.yml (and Package.swift's
    /// iOS platform) from forge.toml. `forge build ios` and `forge run ios` do this too.
    Sync {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
        /// Replace hand-written files even if they hold settings forge.toml lacks
        #[arg(long)]
        force: bool,
    },
    /// Fail if the generated files are out of date with forge.toml (for CI); also
    /// reports Xcode project settings that differ
    Check {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
    },
    /// Print the effective iOS settings: bundle, version, permissions, entitlements, signing
    Show {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml)"
        )]
        project: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum LedgerCommands {
    /// Write docs/capability-ledger.md and docs/status.md from the records in docs/ledger/
    Generate,
    /// Fail if docs/capability-ledger.md or docs/status.md differs from the records (for CI)
    Check,
    /// List the records, optionally only those mentioning TEXT
    List {
        #[arg(value_name = "TEXT")]
        filter: Option<String>,
    },
}

#[derive(Subcommand)]
enum BindingsCommands {
    /// Regenerates an app's Generated/ from its Rust crate (Swift or Kotlin,
    /// from the app's platform): the app given by --project, or the current
    /// directory when it has forge.toml.
    Generate {
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum BuildCommands {
    Ios {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml, else examples/counter)"
        )]
        project: Option<PathBuf>,
        #[arg(
            long,
            help = "Only build Rust static library and regenerate bindings (skip xtool dev)"
        )]
        package_only: bool,
        /// Build the Swift app in the release configuration (xtool dev build -c release)
        #[arg(long, conflicts_with = "package_only")]
        release: bool,
        /// Include the live connection to `forge device` (pair, connect,
        /// status, snapshot, cmd). For development only, so never in a
        /// release build
        #[arg(long, conflicts_with = "release")]
        devtools: bool,
    },
    /// Build an Android app (work in progress: no new features while Forge focuses on iOS)
    Android {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml, else examples/counter-android)"
        )]
        project: Option<PathBuf>,
        #[arg(
            long,
            help = "Install debug APK on a connected device/emulator after build"
        )]
        install: bool,
    },
}

#[derive(Subcommand)]
enum InstallCommands {
    Ios {
        #[arg(long)]
        ipa: PathBuf,
    },
}

#[derive(Subcommand)]
enum CheckCommands {
    /// Compile the app's Swift that needs no Apple UI framework (the view
    /// model, the Rust bindings) and run its tests that need none, with plain
    /// Swift on Linux: no iOS SDK, Mac or phone. SwiftUI views are left out
    Swift {
        #[arg(long, help = "App directory (default: the current directory)")]
        project: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum RunCommands {
    Ios {
        #[arg(
            long,
            help = "App directory (default: the current directory if it has forge.toml, else examples/counter)"
        )]
        project: Option<PathBuf>,
        /// Build the Swift app in the release configuration (xtool dev run -c release)
        #[arg(long)]
        release: bool,
        /// Include the live connection to `forge device` (pair, connect,
        /// status, snapshot, cmd). For development only, so never in a
        /// release build
        #[arg(long, conflicts_with = "release")]
        devtools: bool,
        /// Build first (Rust and Swift), then wait up to this many minutes
        /// for the iPhone: ask for the phone only when the status says
        /// waiting_for_device (<app>/.forge/status/run-ios.json)
        #[arg(long, value_name = "MINUTES")]
        wait_for_device: Option<u64>,
    },
}

fn configuration(release: bool) -> ios_builder::IosConfiguration {
    if release {
        ios_builder::IosConfiguration::Release
    } else {
        ios_builder::IosConfiguration::Debug
    }
}

fn main() {
    let cli = Cli::parse();
    // stdout is the MCP transport: nothing else may print there.
    if let Commands::Mcp {
        root,
        allow,
        bundle_ids,
        udid,
        no_device,
    } = cli.command
    {
        if let Err(err) = serve_mcp(root, allow, bundle_ids, udid, no_device, cli.json) {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
        return;
    }
    let runner = runner::RealRunner;
    // doctor --json has its own report shape (it predates the result contract).
    if !cli.json || matches!(cli.command, Commands::Doctor) {
        let started = std::time::Instant::now();
        let command = command_name(&cli.command);
        let project = result_project(&cli.command);
        let fallback = fallback_stage(&cli.command);
        let res = dispatch(cli, &runner);
        if let Some(app) = &project {
            history::append(
                app,
                &history::Entry {
                    command: &command,
                    ok: res.is_ok(),
                    stage: if res.is_ok() { "ok" } else { fallback },
                    error: res.as_ref().err().map(String::as_str),
                    duration_ms: started.elapsed().as_millis() as u64,
                    log: None,
                },
            );
        }
        if let Err(err) = res {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
        return;
    }
    report::enter_json_mode();
    let started = std::time::Instant::now();
    let command = command_name(&cli.command);
    let project = result_project(&cli.command);
    let fallback_stage = fallback_stage(&cli.command);
    if let (Some(dir), true) = (&project, logs_to_file(&cli.command)) {
        report::open_log(dir, &command);
    }
    let res = dispatch(cli, &runner);
    if let Err(err) = &res {
        eprintln!("error: {err}");
    }
    let outcome = report::Outcome {
        command: &command,
        started,
        project: project
            .as_deref()
            .filter(|p| p.join("forge.toml").is_file()),
        fallback_stage,
    };
    let log = report::log_path();
    let (code, value) = report::finish(&outcome, res);
    if let Some(app) = outcome.project {
        history::append(
            app,
            &history::Entry {
                command: &command,
                ok: value["ok"] == serde_json::json!(true),
                stage: value["stage"].as_str().unwrap_or(""),
                error: value["error"].as_str(),
                duration_ms: value["duration_ms"].as_u64().unwrap_or(0),
                log: log.as_deref(),
            },
        );
    }
    std::process::exit(code);
}

/// The stage an unrecognised failure of `command` counts as.
fn fallback_stage(command: &Commands) -> &'static str {
    match command {
        Commands::Test { .. } | Commands::Check { .. } | Commands::Verify { .. } => "test",
        // usbmuxd missing, no phone, not paired, the app not listening.
        Commands::Device { .. } | Commands::Devices | Commands::Diagnose { .. } => "device",
        Commands::Ui {
            cmd: UiCommands::Run { .. } | UiCommands::Explore { .. },
        } => "test",
        Commands::Ui {
            cmd: UiCommands::Accept { .. } | UiCommands::Focus { .. },
        } => "forge",
        // No DDI, no runner, WebDriverAgent not answering.
        Commands::Ui { .. } => "device",
        _ => "forge",
    }
}

/// `run ios`, `device logs`, ...: the command as the result names it.
fn command_name(command: &Commands) -> String {
    let sub = match command {
        Commands::Doctor => return "doctor".into(),
        Commands::Setup { .. } => return "setup ios".into(),
        Commands::Check {
            what: Some(CheckCommands::Swift { .. }),
            ..
        } => return "check swift".into(),
        Commands::Check { .. } => return "check".into(),
        Commands::Init { .. } => return "init".into(),
        Commands::Test { .. } => return "test".into(),
        Commands::Mcp { .. } => return "mcp".into(),
        Commands::Verify { .. } => return "verify".into(),
        Commands::Inspect { .. } => return "inspect".into(),
        Commands::Agent { .. } => return "agent setup".into(),
        Commands::Crash { .. } => return "crash".into(),
        Commands::Devices => return "devices".into(),
        Commands::Diagnose { .. } => return "diagnose".into(),
        Commands::Config { cmd } => match cmd {
            ConfigCommands::Sync { .. } => "config sync",
            ConfigCommands::Check { .. } => "config check",
            ConfigCommands::Show { .. } => "config show",
        },
        Commands::Bindings { .. } => "bindings generate",
        Commands::Ledger { cmd } => match cmd {
            LedgerCommands::Generate => "ledger generate",
            LedgerCommands::Check => "ledger check",
            LedgerCommands::List { .. } => "ledger list",
        },
        Commands::Build { cmd } => match cmd {
            BuildCommands::Ios { .. } => "build ios",
            BuildCommands::Android { .. } => "build android",
        },
        Commands::Install { .. } => "install ios",
        Commands::Run { .. } => "run ios",
        Commands::Device { cmd } => match cmd {
            DeviceCommands::List => "device list",
            DeviceCommands::Info { .. } => "device info",
            DeviceCommands::Apps { .. } => "device apps",
            DeviceCommands::Logs { .. } => "device logs",
            DeviceCommands::Crashes { .. } => "device crashes",
            DeviceCommands::Pair { .. } => "device pair",
            DeviceCommands::Unpair { .. } => "device unpair",
            DeviceCommands::Status { .. } => "device status",
            DeviceCommands::Snapshot { .. } => "device snapshot",
            DeviceCommands::Cmd { .. } => "device cmd",
            DeviceCommands::Connect { .. } => "device connect",
        },
        Commands::Ui { cmd } => match cmd {
            UiCommands::Setup { .. } => "ui setup",
            UiCommands::Tree { .. } => "ui tree",
            UiCommands::Audit { .. } => "ui audit",
            UiCommands::Screenshot { .. } => "ui screenshot",
            UiCommands::Launch { .. } => "ui launch",
            UiCommands::Run { .. } => "ui run",
            UiCommands::Explore { .. } => "ui explore",
            UiCommands::Focus { .. } => "ui focus",
            UiCommands::Start { .. } => "ui start",
            UiCommands::Stop { .. } => "ui stop",
            UiCommands::Status { .. } => "ui status",
            UiCommands::Serve { .. } => "ui serve",
            UiCommands::Accept { .. } => "ui accept",
            UiCommands::Tap { .. } => "ui tap",
            UiCommands::Type { .. } => "ui type",
            UiCommands::Clear { .. } => "ui clear",
            UiCommands::Scroll { .. } => "ui scroll",
            UiCommands::Swipe { .. } => "ui swipe",
            UiCommands::LongPress { .. } => "ui long-press",
            UiCommands::DoubleTap { .. } => "ui double-tap",
            UiCommands::Drag { .. } => "ui drag",
            UiCommands::Key { .. } => "ui key",
            UiCommands::Expect { .. } => "ui expect",
            UiCommands::Pinch { .. } => "ui pinch",
            UiCommands::Rotate { .. } => "ui rotate",
            UiCommands::TwoFingerTap { .. } => "ui two-finger-tap",
            UiCommands::Pick { .. } => "ui pick",
            UiCommands::Press { .. } => "ui press",
            UiCommands::OpenUrl { .. } => "ui open-url",
            UiCommands::Alert { .. } => "ui alert",
            UiCommands::Back { .. } => "ui back",
            UiCommands::AppState { .. } => "ui app-state",
            UiCommands::ResetPermission { .. } => "ui reset-permission",
            UiCommands::Orientation { .. } => "ui orientation",
            UiCommands::Lock { .. } => "ui lock",
            UiCommands::Unlock { .. } => "ui unlock",
            UiCommands::Settle { .. } => "ui settle",
        },
    };
    sub.into()
}

/// The app directory a command works on, for the result's `app` and log.
fn result_project(command: &Commands) -> Option<PathBuf> {
    let dir = match command {
        Commands::Init { name, path, .. } => {
            let root = commands::workspace_root();
            return Some(
                path.as_ref()
                    .map(|p| absolute_from_cwd(p))
                    .or_else(|| default_init_dest(&root, name))
                    .unwrap_or_else(|| root.join("examples").join(name)),
            );
        }
        Commands::Build {
            cmd: BuildCommands::Ios { project, .. },
        }
        | Commands::Run {
            cmd: RunCommands::Ios { project, .. },
        }
        | Commands::Test { project, .. }
        | Commands::Diagnose { project, .. }
        | Commands::Verify { project, .. }
        | Commands::Inspect { project }
        | Commands::Agent {
            cmd: AgentCommands::Setup { project, .. },
        }
        | Commands::Crash { project, .. }
        | Commands::Device {
            cmd: DeviceCommands::Crashes { project, .. },
        }
        | Commands::Check {
            what: Some(CheckCommands::Swift { project }),
            ..
        } => project.clone().or_else(app_in_cwd),
        Commands::Config { cmd } => match cmd {
            ConfigCommands::Sync { project, .. }
            | ConfigCommands::Check { project }
            | ConfigCommands::Show { project } => project.clone().or_else(app_in_cwd),
        },
        _ => app_in_cwd(),
    }?;
    Some(absolute_from_cwd(&dir))
}

/// Commands that run long build steps keep their output in a log file.
fn logs_to_file(command: &Commands) -> bool {
    matches!(
        command,
        Commands::Build { .. }
            | Commands::Run { .. }
            | Commands::Test { .. }
            | Commands::Check { what: Some(_), .. }
    )
}

fn dispatch(cli: Cli, runner: &dyn runner::CommandRunner) -> Result<(), String> {
    let root = commands::workspace_root();
    let ios = ios_builder::XtoolBuilder;
    match cli.command {
        Commands::Setup {
            cmd:
                SetupCommands::Ios {
                    xip,
                    reinstall_sdk,
                    yes,
                },
        } => {
            use std::io::IsTerminal;
            let env = |k: &str| std::env::var_os(k).map(|v| v.to_string_lossy().into_owned());
            setup::setup_ios(
                &setup::Host {
                    runner,
                    root: &root,
                    env: &env,
                    ask: &setup::ask_terminal,
                    os: std::env::consts::OS,
                    arch: std::env::consts::ARCH,
                },
                &setup::Options {
                    xip: xip.map(|p| absolute_from_cwd(&p)),
                    reinstall_sdk,
                    yes,
                    interactive: std::io::stdin().is_terminal() && !cli.json,
                },
            )
        }
        Commands::Diagnose {
            project,
            udid,
            duration,
        } => diagnose_app(&root, project, udid, duration, cli.json),
        Commands::Doctor if cli.json => doctor::print_json(runner, &root),
        Commands::Doctor => doctor::print_text(runner, &root),
        Commands::Verify {
            project,
            twin_only,
            run,
            journal,
            accept,
            note,
        } => match project.map(|p| absolute_from_cwd(&p)).or_else(app_in_cwd) {
            Some(app) => {
                let mut out = std::io::stdout();
                if let Some(start) = run {
                    let journal = journal.map(|p| absolute_from_cwd(&p));
                    run_record::record(runner, &app, &start, journal.as_deref(), &mut out)
                } else if let Some(id) = accept {
                    run_record::accept(
                        runner,
                        &app,
                        &id,
                        note.as_deref().unwrap_or(""),
                        &mut std::io::stdin().lock(),
                        &mut out,
                    )
                } else {
                    verify::verify(runner, &app, twin_only, &mut out)
                }
            }
            None => Err(
                "no app here: run inside an app directory (one with forge.toml) or pass --project <app>"
                    .into(),
            ),
        },
        Commands::Check {
            what: Some(CheckCommands::Swift { project }),
            ..
        } => match project.or_else(app_in_cwd) {
            Some(app) => swift_check::check_swift(runner, &root, &app),
            None => Err(
                "no app here: run inside an app directory (one with forge.toml) or pass --project <app>"
                    .into(),
            ),
        },
        Commands::Check { fail_fast, .. } => {
            let host = check::Host::detect(runner, &root);
            check::check(runner, &root, host, fail_fast)
        }
        Commands::Init {
            name,
            bundle_id,
            platform,
            path,
            minimal,
            agent,
        } => {
            if agent && platform != "ios" {
                return Err(
                    "--agent is for iOS apps (Android is work in progress, docs/roadmap.md)".into(),
                );
            }
            let path = path
                .map(|p| absolute_from_cwd(&p))
                .or_else(|| default_init_dest(&root, &name));
            let dest = commands::init_with(
                runner,
                &root,
                &name,
                &bundle_id,
                &platform,
                path.as_deref(),
                minimal,
            )?;
            if agent {
                agent::run(&root, &dest, false)?;
            }
            Ok(())
        }
        Commands::Agent {
            cmd: AgentCommands::Setup {
                project,
                apple_docs,
            },
        } => {
            let project = project_or_default(project, "");
            if project.as_os_str().is_empty() {
                return Err(
                    "no app here: run inside an app directory (with forge.toml) or pass --project"
                        .into(),
                );
            }
            agent::run(&root, &project, apple_docs)
        }
        Commands::Config { cmd } => {
            let (project, action) = match cmd {
                ConfigCommands::Sync { project, force } => {
                    (project, commands::ConfigAction::Sync { force })
                }
                ConfigCommands::Check { project } => (project, commands::ConfigAction::Check),
                ConfigCommands::Show { project } => (project, commands::ConfigAction::Show),
            };
            let project = project_or_default(project, "");
            if project.as_os_str().is_empty() {
                return Err(
                    "no app here: run inside an app directory (with forge.toml) or pass --project"
                        .into(),
                );
            }
            commands::config(&root, &project, action)
        }
        Commands::Ledger { cmd } => match cmd {
            LedgerCommands::Generate => ledger::generate(&root),
            LedgerCommands::Check => ledger::check(&root),
            LedgerCommands::List { filter } => ledger::list(&root, filter.as_deref()),
        },
        Commands::Test { project, workspace } => {
            let project = project.or_else(app_in_cwd);
            commands::test(runner, &root, project.as_deref(), workspace)
        }
        Commands::Bindings {
            cmd: BindingsCommands::Generate { project },
        } => match project.or_else(app_in_cwd) {
            Some(app) => commands::bindings_generate_for_project(runner, &root, &app),
            None => Err(
                "no app here: run inside an app directory (one with forge.toml) or pass --project <app>"
                    .into(),
            ),
        },
        Commands::Build { cmd } => match cmd {
            BuildCommands::Ios {
                project,
                package_only,
                release,
                devtools,
            } => {
                let project = project_or_default(project, "examples/counter");
                commands::build_ios(
                    runner,
                    &ios,
                    &root,
                    &project,
                    package_only,
                    configuration(release),
                    devtools,
                )
            }
            BuildCommands::Android { project, install } => {
                let project = project_or_default(project, "examples/counter-android");
                commands::build_android(runner, &root, &project, install)
            }
        },
        Commands::Devices => commands::devices(runner, &ios),
        Commands::Mcp { .. } => Err("forge mcp is served from main".into()),
        Commands::Inspect { project } => {
            let project = project_or_default(project, "");
            if project.as_os_str().is_empty() {
                return Err(
                    "no app here: run inside an app directory (with forge.toml) or pass --project"
                        .into(),
                );
            }
            inspect::run(&project)
        }
        Commands::Crash {
            files,
            project,
            binaries,
            all_threads,
            frames,
        } => {
            let project = project.map(|p| absolute_from_cwd(&p)).or_else(app_in_cwd);
            let options = crash::options(project.as_deref(), &binaries, all_threads);
            if cli.json {
                report::set_data(crash::show_data(&files, &options)?);
                Ok(())
            } else {
                crash::show(&files, &options, frames, &mut std::io::stdout().lock())
            }
        }
        Commands::Ui { cmd } => {
            // Not `stdout().lock()`: `ui setup` runs xtool, and with --json
            // its output is forwarded to stdout from another thread, which
            // would wait forever for a lock held here (seen on Cati).
            let mut out = std::io::stdout();
            match cmd {
                UiCommands::Setup {
                    udid,
                    ddi,
                    runner: app,
                    reinstall,
                    team,
                } => {
                    let signing = ios_env::signing_team(runner);
                    if let Some(signing) = &signing {
                        let _ = std::io::Write::write_fmt(&mut out, format_args!("xtool signs with {}\n", signing.describe()));
                    }
                    let team = ui::Team::resolve(
                        team.as_deref(),
                        signing.map(|s| s.kind),
                        std::path::Path::new("."),
                    )?;
                    ui::setup(
                        runner,
                        udid.as_deref(),
                        ddi.as_deref(),
                        app.as_deref(),
                        reinstall,
                        team,
                        &mut out,
                    )
                }
                UiCommands::Tree { udid, app, any_app, find, visible, limit } => {
                    ui::tree(udid.as_deref(), app.as_deref(), any_app, &ui::TreeFilter { find, visible, limit }, &mut out)
                }
                UiCommands::Audit { udid, app, any_app } => ui::audit(udid.as_deref(), app.as_deref(), any_app, &mut out),
                UiCommands::Screenshot { udid, path, any_app } => ui::screenshot(udid.as_deref(), &path, any_app, &mut out),
                UiCommands::Launch {
                    udid,
                    project,
                    terminate,
                    appearance,
                    largest_text,
                    launch_arg,
                } => {
                    let project = project_or_default(project, "");
                    if project.as_os_str().is_empty() {
                        return Err("no app here: run inside an app directory (with forge.toml) or pass --project".into());
                    }
                    let mut args = Vec::new();
                    if let Some(mode) = &appearance {
                        args.extend([forge_uitest::explore::APPEARANCE_ARG.to_string(), mode.clone()]);
                    }
                    if largest_text {
                        args.extend(forge_uitest::explore::LARGEST_TEXT_ARGS.iter().map(|a| a.to_string()));
                    }
                    args.extend(launch_arg);
                    ui::launch(runner, udid.as_deref(), &project, terminate, &args, &mut out)
                }
                UiCommands::Run {
                    udid,
                    file,
                    out: dir,
                    any_app,
                    visual,
                    no_watch,
                } => {
                    let visual = match visual.as_str() {
                        "record" => Some(forge_uitest::baseline::Mode::Record),
                        "off" => None,
                        _ => Some(forge_uitest::baseline::Mode::Check),
                    };
                    ui::run(runner, udid.as_deref(), &file, dir.as_deref(), any_app, visual, !no_watch, &mut out)
                }
                UiCommands::Tap { target, at: Some(at), on } => {
                    no_target(&target, "--at")?;
                    let (x, y) = ui::parse_point(&at)?;
                    touch_at(on, forge_uitest::PointTouch::Tap { x, y }, &mut out)
                }
                UiCommands::Tap { target, at: None, on } => one_step(runner, on, "tap", "tap", target.table().into(), &mut out),
                UiCommands::Type { target, text, clear, on } => {
                    let mut t = target.table();
                    t.insert("text".into(), text.into());
                    if clear {
                        t.insert("clear".into(), true.into());
                    }
                    one_step(runner, on, "type", "type", t.into(), &mut out)
                }
                UiCommands::Clear { target, on } => one_step(runner, on, "clear", "clear", target.table().into(), &mut out),
                UiCommands::Scroll { target, direction, in_id, toward, pages, on } => {
                    let mut t = target.table();
                    t.insert("direction".into(), direction.into());
                    if let Some(id) = in_id {
                        let mut within = toml::Table::new();
                        within.insert("id".into(), id.into());
                        t.insert("in".into(), within.into());
                    }
                    if let Some(toward) = toward {
                        t.insert("toward".into(), toward.into());
                    }
                    if let Some(pages) = pages {
                        t.insert("pages".into(), toml::Value::Integer(pages as i64));
                    }
                    one_step(runner, on, "scroll", "scroll", t.into(), &mut out)
                }
                UiCommands::Swipe { target, from: Some(from), to, hold, on, .. } => {
                    no_target(&target, "--from")?;
                    let from = ui::parse_point(&from)?;
                    let to = ui::parse_point(to.as_deref().unwrap_or_default())?;
                    touch_at(on, forge_uitest::PointTouch::Drag { from, to, hold }, &mut out)
                }
                UiCommands::Swipe { target, direction, on, .. } => {
                    let mut t = target.table();
                    t.insert("direction".into(), direction.unwrap_or_default().into());
                    one_step(runner, on, "swipe", "swipe", t.into(), &mut out)
                }
                UiCommands::LongPress { target, seconds, at: Some(at), on } => {
                    no_target(&target, "--at")?;
                    let (x, y) = ui::parse_point(&at)?;
                    touch_at(on, forge_uitest::PointTouch::LongPress { x, y, seconds }, &mut out)
                }
                UiCommands::LongPress { target, seconds, at: None, on } => {
                    let mut t = target.table();
                    t.insert("seconds".into(), seconds.into());
                    one_step(runner, on, "long-press", "long_press", t.into(), &mut out)
                }
                UiCommands::DoubleTap { target, at: Some(at), on } => {
                    no_target(&target, "--at")?;
                    let (x, y) = ui::parse_point(&at)?;
                    touch_at(on, forge_uitest::PointTouch::DoubleTap { x, y }, &mut out)
                }
                UiCommands::DoubleTap { target, at: None, on } => one_step(runner, on, "double-tap", "double_tap", target.table().into(), &mut out),
                UiCommands::Pinch { target, scale, on } => {
                    let mut t = target.table();
                    t.insert("scale".into(), scale.into());
                    one_step(runner, on, "pinch", "pinch", t.into(), &mut out)
                }
                UiCommands::Rotate { target, degrees, on } => {
                    let mut t = target.table();
                    t.insert("degrees".into(), degrees.into());
                    one_step(runner, on, "rotate", "rotate", t.into(), &mut out)
                }
                UiCommands::TwoFingerTap { target, on } => one_step(runner, on, "two-finger-tap", "two_finger_tap", target.table().into(), &mut out),
                UiCommands::Pick { target, value, on } => {
                    let mut t = target.table();
                    t.insert("value".into(), value.into());
                    one_step(runner, on, "pick", "pick", t.into(), &mut out)
                }
                UiCommands::Press { button, on } if button == "home" => one_step(runner, on, "press", "home", toml::Table::new().into(), &mut out),
                UiCommands::Press { button, on } => one_step(runner, on, "press", "button", button.replace('-', "_").into(), &mut out),
                UiCommands::OpenUrl { url, on } => one_step(runner, on, "open-url", "open_url", url.into(), &mut out),
                UiCommands::Alert { answer, button, text, on } => {
                    let mut t = toml::Table::new();
                    match (answer.as_str(), button) {
                        (_, Some(b)) => {
                            t.insert("button".into(), b.into());
                        }
                        ("accept", None) => {
                            t.insert("accept".into(), true.into());
                        }
                        ("dismiss", None) => {
                            t.insert("dismiss".into(), true.into());
                        }
                        _ => {}
                    }
                    if let Some(text) = text {
                        t.insert("text".into(), text.into());
                    }
                    one_step(runner, on, "alert", "alert", t.into(), &mut out)
                }
                UiCommands::Back { on } => one_step(runner, on, "back", "back", toml::Table::new().into(), &mut out),
                UiCommands::AppState { state, on } => one_step(runner, on, "app-state", "app_state", state.into(), &mut out),
                UiCommands::ResetPermission { permission, on } => one_step(runner, on, "reset-permission", "reset_permission", permission.into(), &mut out),
                UiCommands::Orientation { orientation, on } => one_step(runner, on, "orientation", "orientation", orientation.into(), &mut out),
                UiCommands::Lock { on } => one_step(runner, on, "lock", "lock", toml::Table::new().into(), &mut out),
                UiCommands::Unlock { on } => one_step(runner, on, "unlock", "unlock", toml::Table::new().into(), &mut out),
                UiCommands::Settle { on } => one_step(runner, on, "settle", "settle", toml::Table::new().into(), &mut out),
                UiCommands::Drag { target, to_id, to_label, by, distance, hold, on } => {
                    let mut t = target.table();
                    let mut to = toml::Table::new();
                    if let Some(id) = to_id {
                        to.insert("id".into(), id.into());
                    }
                    if let Some(label) = to_label {
                        to.insert("label".into(), label.into());
                    }
                    if !to.is_empty() {
                        t.insert("to".into(), to.into());
                    }
                    if let Some(by) = by {
                        t.insert("by".into(), by.into());
                    }
                    if let Some(d) = distance {
                        t.insert("distance".into(), d.into());
                    }
                    if let Some(h) = hold {
                        t.insert("hold".into(), h.into());
                    }
                    one_step(runner, on, "drag", "drag", t.into(), &mut out)
                }
                UiCommands::Key { key, on } => one_step(runner, on, "key", "key", key.into(), &mut out),
                UiCommands::Expect { target, value, has_label, enabled, visible, hittable, selected, count, gone, on } => {
                    let mut t = target.table();
                    for (key, v) in [("visible", visible), ("hittable", hittable), ("selected", selected)] {
                        if let Some(v) = v {
                            t.insert(key.into(), v.into());
                        }
                    }
                    if let Some(n) = count {
                        t.insert("count".into(), toml::Value::Integer(n as i64));
                    }
                    if let Some(v) = value {
                        t.insert("value".into(), v.into());
                    }
                    if let Some(v) = has_label {
                        t.insert("has_label".into(), v.into());
                    }
                    if let Some(v) = enabled {
                        t.insert("enabled".into(), v.into());
                    }
                    if gone {
                        t.insert("gone".into(), true.into());
                    }
                    one_step(runner, on, "expect", "expect", t.into(), &mut out)
                }
                UiCommands::Accept { run, scenario, only } => {
                    ui::accept(&run, scenario.as_deref(), &only, &mut out)
                }
                UiCommands::Explore {
                    udid,
                    project,
                    app,
                    any_app,
                    actions,
                    minutes,
                    seed,
                    texts,
                    skip,
                    allow_destructive,
                    real_data,
                    largest_text,
                    appearance,
                    launch_arg,
                    per_screen,
                    no_screenshots,
                    out: dir,
                    since,
                    changed,
                    focus,
                    previous,
                    no_previous,
                    verify,
                    fixture,
                } => {
                    let options = ui::ExploreOptions {
                        since,
                        changed,
                        focus,
                        previous,
                        no_previous,
                        verify,
                        fixture,
                        actions,
                        minutes,
                        seed,
                        texts,
                        skip,
                        allow_destructive,
                        real_data,
                        largest_text,
                        appearance,
                        launch_args: launch_arg,
                        per_screen,
                        screenshots: !no_screenshots,
                    };
                    match app {
                        Some(app) => ui::explore_app(udid.as_deref(), &app, any_app, options, dir.as_deref(), &mut out),
                        None => ui::explore(runner, udid.as_deref(), &project, options, dir.as_deref(), &mut out),
                    }
                }
                UiCommands::Focus { project, since, changed } => focus::show(&project, since.as_deref(), &changed, &mut out),
                UiCommands::Start { udid, idle_minutes, exit_with } => ui_session::start(udid.as_deref(), idle_minutes, exit_with, &mut out),
                UiCommands::Stop { udid } => ui_session::stop(udid.as_deref(), &mut out),
                UiCommands::Status { udid } => ui_session::status(udid.as_deref(), &mut out),
                UiCommands::Serve { udid, idle_minutes, exit_with } => ui_session::serve(udid.as_deref(), idle_minutes, exit_with, &mut out),
            }
        }
        Commands::Device { cmd } => device_command(cmd, cli.json),
        Commands::Install { cmd } => match cmd {
            InstallCommands::Ios { ipa } => commands::install_ios(runner, &ios, &ipa),
        },
        Commands::Run { cmd } => match cmd {
            RunCommands::Ios {
                project,
                release,
                devtools,
                wait_for_device,
            } => {
                let project = project_or_default(project, "examples/counter");
                commands::run_ios_with(
                    runner,
                    &ios,
                    &root,
                    &project,
                    configuration(release),
                    devtools,
                    wait_for_device.map(|m| std::time::Duration::from_secs(m * 60)),
                )
            }
        },
    }
}

fn serve_mcp(
    root: Option<PathBuf>,
    allow: Vec<String>,
    bundle_ids: Vec<String>,
    udid: Option<String>,
    no_device: bool,
    json: bool,
) -> Result<(), String> {
    if json {
        return Err("forge mcp speaks MCP on stdout; it has no --json result".into());
    }
    let root = root
        .map(|r| absolute_from_cwd(&r))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let workspace = commands::workspace_root();
    commands::require_workspace(&workspace)?;
    let forge = std::env::current_exe().map_err(|e| e.to_string())?;
    eprintln!(
        "forge mcp: serving {} on stdio (install {})",
        root.display(),
        if no_device {
            "refused, no phone tools"
        } else if allow.iter().any(|a| a == "install") {
            "allowed"
        } else {
            "refused"
        }
    );
    forge_mcp::serve_stdio(forge_mcp::Options {
        forge,
        root,
        workspace,
        allow_install: allow.iter().any(|a| a == "install"),
        bundle_ids,
        udid,
        no_device,
    })
}

/// `--project` when given; otherwise the current directory when it is an app
/// (has forge.toml), so `forge build android` works from inside an app;
/// otherwise the bundled example.
/// A one-step `forge ui` command (`forge ui tap` and the like): the step
/// runs as a one-step test file, exactly as `forge_ui_act` runs it, so the
/// same waiting, policy checks and failure details apply.
fn one_step(
    runner: &dyn crate::runner::CommandRunner,
    on: UiOn,
    verb: &str,
    key: &str,
    value: toml::Value,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    let project = match &on.app {
        Some(_) => None,
        None => {
            let p = project_or_default(on.project, "");
            if p.as_os_str().is_empty() {
                return Err("no app here: run inside an app directory (with forge.toml), or pass --project or --app".into());
            }
            Some(p)
        }
    };
    ui::one_step(
        runner,
        on.udid.as_deref(),
        project.as_deref(),
        on.app.as_deref(),
        on.any_app,
        on.timeout,
        verb,
        key,
        value,
        out,
    )
}

/// A coordinate gesture names no element.
fn no_target(target: &UiTarget, flag: &str) -> Result<(), String> {
    if target.id.is_some()
        || target.label.is_some()
        || target.label_contains.is_some()
        || target.kind.is_some()
        || target.near.is_some()
    {
        return Err(format!(
            "{flag} is a point on the screen: leave out --id, --label, --kind and --near"
        ));
    }
    Ok(())
}

/// A touch at a point, for a person at the terminal (`--at`, `--from`).
fn touch_at(
    on: UiOn,
    touch: forge_uitest::PointTouch,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    let project = match &on.app {
        Some(_) => None,
        None => {
            let p = project_or_default(on.project, "");
            if p.as_os_str().is_empty() {
                return Err("no app here: run inside an app directory (with forge.toml), or pass --project or --app".into());
            }
            Some(p)
        }
    };
    ui::touch_at(
        on.udid.as_deref(),
        project.as_deref(),
        on.app.as_deref(),
        on.any_app,
        &touch,
        out,
    )
}

fn project_or_default(project: Option<PathBuf>, example: &str) -> PathBuf {
    project
        .or_else(app_in_cwd)
        .unwrap_or_else(|| PathBuf::from(example))
}

/// The current directory, if it is an app (has forge.toml).
/// The app folder `device crashes` symbolicates against.
fn crashes_project(project: Option<&std::path::Path>) -> Option<PathBuf> {
    project.map(absolute_from_cwd).or_else(app_in_cwd)
}

fn app_in_cwd() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .filter(|cwd| cwd.join("forge.toml").is_file())
}

/// `forge init MyApp` outside the Forge checkout creates `./MyApp`, so
/// `forge init MyApp && cd MyApp && forge run ios` works. Inside the checkout
/// it keeps creating `examples/MyApp` (`None`).
fn default_init_dest(root: &std::path::Path, name: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    init_dest_for(root, &cwd, name)
}

fn init_dest_for(root: &std::path::Path, cwd: &std::path::Path, name: &str) -> Option<PathBuf> {
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon(cwd).starts_with(canon(root)) {
        None
    } else {
        Some(cwd.join(name))
    }
}

fn absolute_from_cwd(path: &std::path::Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// `forge diagnose`: the checklist, its next step, and a non-zero exit while
/// something is wrong.
fn diagnose_app(
    root: &std::path::Path,
    project: Option<PathBuf>,
    udid: Option<String>,
    duration: Option<std::time::Duration>,
    json: bool,
) -> Result<(), String> {
    let project_dir = match project {
        Some(p) => commands::resolve_project_dir(root, &p),
        None => app_in_cwd().ok_or_else(|| {
            report::usage_error(
                "run `forge diagnose` in your app's folder (the one with forge.toml), or pass --project <dir>",
            )
        })?,
    };
    let cfg = commands::load_config(&project_dir)?;
    let usb = forge_device::UsbBackend::new().map_err(|e| e.to_string());
    let backend = usb
        .as_ref()
        .map(|b| b as &dyn forge_device::DeviceBackend)
        .map_err(Clone::clone);
    let diagnosis = app_diagnose::diagnose(
        &app_diagnose::Inputs {
            app_dir: &project_dir,
            cfg: &cfg,
            backend,
            udid: udid.as_deref(),
            listen: duration.unwrap_or(std::time::Duration::from_secs(15)),
            now: now_secs(),
        },
        &mut |line| {
            if !json {
                println!("{line}");
            }
        },
    );
    if json {
        report::set_data(app_diagnose::to_json(&diagnosis));
    } else {
        app_diagnose::print(&diagnosis, &mut std::io::stdout().lock())
            .map_err(|e| e.to_string())?;
    }
    let problems = diagnosis
        .checks
        .iter()
        .filter(|c| c.mark == app_diagnose::Mark::Fail)
        .count();
    if problems > 0 {
        return Err(format!("{problems} problem(s) found: see Next above"));
    }
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn device_command(cmd: DeviceCommands, json: bool) -> Result<(), String> {
    let backend = forge_device::UsbBackend::new().map_err(|e| e.to_string())?;
    if json {
        let data = match &cmd {
            DeviceCommands::List => Some(device::list_data(&backend)?),
            DeviceCommands::Info { udid } => Some(device::info_data(&backend, udid.as_deref())?),
            DeviceCommands::Apps { udid, all } => Some(device::apps_data(&backend, udid.as_deref(), now_secs(), *all)?),
            DeviceCommands::Crashes {
                udid,
                app,
                pull,
                limit,
                project,
            } => {
                let project = crashes_project(project.as_deref());
                Some(device::crashes_data(
                    &backend,
                    &device::CrashQuery {
                        udid: udid.as_deref(),
                        app: app.as_deref(),
                        pull: pull.as_deref(),
                        limit: *limit,
                        project: project.as_deref(),
                    },
                )?)
            }
            DeviceCommands::Logs {
                limit: None,
                duration: None,
                ..
            } => {
                return Err(report::usage_error(
                    "with --json, `forge device logs` needs --for <DURATION> or --limit <N>, so it ends",
                ))
            }
            DeviceCommands::Connect { limit: None, .. } => {
                return Err(report::usage_error(
                    "with --json, `forge device connect` needs --limit <N>, so it ends",
                ))
            }
            _ => None,
        };
        if let Some(data) = data {
            report::set_data(data);
            return Ok(());
        }
        // The rest print lines; with --json they become `data.lines`.
        let mut buf = Vec::new();
        let res = run_device_command(&backend, cmd, &mut buf);
        let text = String::from_utf8_lossy(&buf);
        report::set_data(serde_json::json!({
            "lines": text.lines().collect::<Vec<_>>(),
        }));
        return res;
    }
    run_device_command(&backend, cmd, &mut std::io::stdout().lock())
}

fn run_device_command(
    backend: &dyn forge_device::DeviceBackend,
    cmd: DeviceCommands,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    match cmd {
        DeviceCommands::List => device::list(backend, out),
        DeviceCommands::Info { udid } => device::info(backend, udid.as_deref(), out),
        DeviceCommands::Apps { udid, all } => {
            device::apps(backend, udid.as_deref(), now_secs(), all, out)
        }
        DeviceCommands::Logs {
            udid,
            process,
            grep,
            limit,
            duration,
        } => {
            let filter = forge_device::LogFilter {
                process,
                contains: grep,
            };
            device::logs(backend, udid.as_deref(), &filter, limit, duration, out)
        }
        DeviceCommands::Crashes {
            udid,
            app,
            pull,
            limit,
            project,
        } => {
            let project = crashes_project(project.as_deref());
            device::crashes(
                backend,
                &device::CrashQuery {
                    udid: udid.as_deref(),
                    app: app.as_deref(),
                    pull: pull.as_deref(),
                    limit,
                    project: project.as_deref(),
                },
                out,
            )
        }
        DeviceCommands::Pair { live } => with_target(backend, &live, |t| live::pair(t, out)),
        DeviceCommands::Unpair { live } => with_target(backend, &live, |t| live::unpair(t, out)),
        DeviceCommands::Status { live } => with_target(backend, &live, |t| live::status(t, out)),
        DeviceCommands::Snapshot { live } => {
            with_target(backend, &live, |t| live::snapshot(t, out))
        }
        DeviceCommands::Cmd { live, name, args } => with_target(backend, &live, |t| {
            live::cmd(t, name.as_deref(), &args, out)
        }),
        DeviceCommands::Connect {
            live,
            tail,
            limit,
            no_follow,
        } => with_target(backend, &live, |t| {
            live::connect(t, tail, limit, !no_follow, out)
        }),
    }
}

/// Runs a live-connection command against the default pairing store.
fn with_target(
    backend: &dyn forge_device::DeviceBackend,
    args: &LiveArgs,
    run: impl FnOnce(&live::Target) -> Result<(), String>,
) -> Result<(), String> {
    let store = forge_device::PairingStore::default_location().map_err(|e| e.to_string())?;
    run(&live::Target {
        backend,
        udid: args.udid.as_deref(),
        port: args.port,
        store: &store,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;

    #[test]
    fn devtools_is_refused_in_release_builds() {
        use clap::Parser;
        for args in [
            &["forge", "build", "ios", "--release", "--devtools"][..],
            &["forge", "run", "ios", "--release", "--devtools"][..],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
        assert!(Cli::try_parse_from(["forge", "build", "ios", "--devtools"]).is_ok());
        assert!(Cli::try_parse_from(["forge", "build", "ios", "--release"]).is_ok());
        assert!(Cli::try_parse_from(["forge", "run", "ios", "--devtools"]).is_ok());
    }

    #[test]
    fn config_rejects_invalid_bundle_id() {
        let err = config::ForgeConfig::parse(
            r#"
[app]
name = "Bad"
bundle_id = "not a bundle id"
minimum_ios = "17.0"
[rust]
path = "rust"
[build]
ios_device = true
ios_simulator = false
"#,
        )
        .unwrap_err();
        assert!(err.contains("bundle_id"));
    }

    #[test]
    fn init_defaults_to_the_current_directory_outside_the_checkout() {
        let root = commands::workspace_root();
        let elsewhere = tempfile::tempdir().unwrap();
        assert_eq!(
            init_dest_for(&root, elsewhere.path(), "MyApp"),
            Some(elsewhere.path().join("MyApp"))
        );
        assert_eq!(init_dest_for(&root, &root, "MyApp"), None);
        assert_eq!(init_dest_for(&root, &root.join("examples"), "MyApp"), None);
    }

    #[test]
    fn init_rejects_invalid_name() {
        let root = commands::workspace_root();
        let err = commands::init(
            &MockRunner::default(),
            &root,
            "bad name!",
            "org.example.app",
            "ios",
            None,
        )
        .unwrap_err();
        assert!(err.contains("name"));
    }
}
