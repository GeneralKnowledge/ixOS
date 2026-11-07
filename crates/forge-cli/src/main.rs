mod app_config;
mod check;
mod commands;
mod config;
mod device;
mod diagnose;
mod doctor;
mod ios_builder;
mod ios_env;
mod ledger;
mod live;
mod report;
mod runner;
mod swift_check;
mod ui;

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

#[derive(Subcommand)]
enum Commands {
    /// Report toolchain and environment readiness
    /// (`--json` prints one object: tool versions, Rust targets, env, readiness)
    Doctor,
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
    /// A connected iPhone over USB, from Linux: its details, system log and
    /// crash reports, and the live connection to a development build of your
    /// app (needs usbmuxd and a phone that trusts this computer)
    Device {
        #[command(subcommand)]
        cmd: DeviceCommands,
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
enum UiCommands {
    /// Mount the Developer Disk Image, install the WebDriverAgent runner and
    /// check that it starts. Run after every phone restart
    Setup {
        /// Which phone, when several are connected
        #[arg(long)]
        udid: Option<String>,
        /// Folder with the Developer Disk Image: Xcode's
        /// Contents/Resources/CoreDeviceDDIs (only needed when not mounted)
        #[arg(long, value_name = "DIR")]
        ddi: Option<PathBuf>,
        /// Install this WebDriverAgentRunner-Runner.app with `xtool install`
        #[arg(long, value_name = "APP")]
        runner: Option<PathBuf>,
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
    },
    /// Save a screenshot of the phone's screen here
    Screenshot {
        #[arg(long)]
        udid: Option<String>,
        /// Where to save the PNG
        #[arg(default_value = "screenshot.png")]
        path: PathBuf,
    },
    /// Run a UI test file (TOML steps) against its app
    Run {
        #[arg(long)]
        udid: Option<String>,
        /// The test file
        file: PathBuf,
        /// Where screenshots and failure details go
        #[arg(long, value_name = "DIR", default_value = "forge-ui-out")]
        out: PathBuf,
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
    /// List crash reports on the phone, or copy them here with --pull
    Crashes {
        /// Which phone, when several are connected
        #[arg(long)]
        udid: Option<String>,
        /// Only this app's reports (its executable name), newest first
        #[arg(long, value_name = "NAME")]
        app: Option<String>,
        /// Copy the reports into this folder
        #[arg(long, value_name = "DIR")]
        pull: Option<PathBuf>,
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
    /// Write docs/capability-ledger.md from the records in docs/ledger/
    Generate,
    /// Fail if docs/capability-ledger.md differs from the records (for CI)
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
        /// status, snapshot, cmd). For development only
        #[arg(long)]
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
        /// status, snapshot, cmd). For development only
        #[arg(long)]
        devtools: bool,
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
    let runner = runner::RealRunner;
    // doctor --json has its own report shape (it predates the result contract).
    if !cli.json || matches!(cli.command, Commands::Doctor) {
        if let Err(err) = dispatch(cli, &runner) {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
        return;
    }
    report::enter_json_mode();
    let started = std::time::Instant::now();
    let command = command_name(&cli.command);
    let project = result_project(&cli.command);
    let fallback_stage = match cli.command {
        Commands::Test { .. } | Commands::Check { .. } => "test",
        // usbmuxd missing, no phone, not paired, the app not listening.
        Commands::Device { .. } | Commands::Devices => "device",
        Commands::Ui {
            cmd: UiCommands::Run { .. },
        } => "test",
        // No DDI, no runner, WebDriverAgent not answering.
        Commands::Ui { .. } => "device",
        _ => "forge",
    };
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
    std::process::exit(report::finish(&outcome, res));
}

/// `run ios`, `device logs`, ...: the command as the result names it.
fn command_name(command: &Commands) -> String {
    let sub = match command {
        Commands::Doctor => return "doctor".into(),
        Commands::Check {
            what: Some(CheckCommands::Swift { .. }),
            ..
        } => return "check swift".into(),
        Commands::Check { .. } => return "check".into(),
        Commands::Init { .. } => return "init".into(),
        Commands::Test { .. } => return "test".into(),
        Commands::Devices => return "devices".into(),
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
            UiCommands::Screenshot { .. } => "ui screenshot",
            UiCommands::Run { .. } => "ui run",
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
        Commands::Doctor if cli.json => doctor::print_json(runner, &root),
        Commands::Doctor => doctor::print_text(runner, &root),
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
        } => {
            let path = path
                .map(|p| absolute_from_cwd(&p))
                .or_else(|| default_init_dest(&root, &name));
            commands::init(runner, &root, &name, &bundle_id, &platform, path.as_deref())
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
        Commands::Ui { cmd } => {
            let mut out = std::io::stdout().lock();
            match cmd {
                UiCommands::Setup {
                    udid,
                    ddi,
                    runner: app,
                    team,
                } => {
                    let team = ui::Team::resolve(team.as_deref(), std::path::Path::new("."))?;
                    ui::setup(
                        runner,
                        udid.as_deref(),
                        ddi.as_deref(),
                        app.as_deref(),
                        team,
                        &mut out,
                    )
                }
                UiCommands::Tree { udid, app } => ui::tree(udid.as_deref(), app.as_deref(), &mut out),
                UiCommands::Screenshot { udid, path } => ui::screenshot(udid.as_deref(), &path, &mut out),
                UiCommands::Run { udid, file, out: dir } => ui::run(udid.as_deref(), &file, &dir, &mut out),
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
            } => {
                let project = project_or_default(project, "examples/counter");
                commands::run_ios(
                    runner,
                    &ios,
                    &root,
                    &project,
                    configuration(release),
                    devtools,
                )
            }
        },
    }
}

/// `--project` when given; otherwise the current directory when it is an app
/// (has forge.toml), so `forge build android` works from inside an app;
/// otherwise the bundled example.
fn project_or_default(project: Option<PathBuf>, example: &str) -> PathBuf {
    project
        .or_else(app_in_cwd)
        .unwrap_or_else(|| PathBuf::from(example))
}

/// The current directory, if it is an app (has forge.toml).
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

fn device_command(cmd: DeviceCommands, json: bool) -> Result<(), String> {
    let backend = forge_device::UsbBackend::new().map_err(|e| e.to_string())?;
    if json {
        let data = match &cmd {
            DeviceCommands::List => Some(device::list_data(&backend)?),
            DeviceCommands::Info { udid } => Some(device::info_data(&backend, udid.as_deref())?),
            DeviceCommands::Crashes { udid, app, pull } => Some(device::crashes_data(
                &backend,
                udid.as_deref(),
                app.as_deref(),
                pull.as_deref(),
            )?),
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
        DeviceCommands::Crashes { udid, app, pull } => device::crashes(
            backend,
            udid.as_deref(),
            app.as_deref(),
            pull.as_deref(),
            out,
        ),
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
