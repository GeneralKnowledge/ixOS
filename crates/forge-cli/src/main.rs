mod app_config;
mod check;
mod commands;
mod config;
mod device;
mod diagnose;
mod doctor;
mod ios_builder;
mod ios_env;
mod live;
mod runner;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "forge", about = "Forge Mobile developer CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Report toolchain and environment readiness
    Doctor {
        /// Print one JSON object (tool versions, Rust targets, env, readiness) instead of the text report
        #[arg(long)]
        json: bool,
    },
    /// Run the full local CI matrix: fmt, clippy, tests, binding drift, template sync, Android JVM and Swift host tests
    Check {
        /// Stop at the first failing step instead of running them all
        #[arg(long)]
        fail_fast: bool,
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
    if let Err(err) = dispatch(cli, &runner) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn dispatch(cli: Cli, runner: &dyn runner::CommandRunner) -> Result<(), String> {
    let root = commands::workspace_root();
    let ios = ios_builder::XtoolBuilder;
    match cli.command {
        Commands::Doctor { json: false } => doctor::print_text(runner, &root),
        Commands::Doctor { json: true } => doctor::print_json(runner, &root),
        Commands::Check { fail_fast } => {
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
        Commands::Device { cmd } => {
            let backend = forge_device::UsbBackend::new().map_err(|e| e.to_string())?;
            let mut out = std::io::stdout().lock();
            match cmd {
                DeviceCommands::List => device::list(&backend, &mut out),
                DeviceCommands::Info { udid } => device::info(&backend, udid.as_deref(), &mut out),
                DeviceCommands::Logs {
                    udid,
                    process,
                    grep,
                    limit,
                } => {
                    let filter = forge_device::LogFilter {
                        process,
                        contains: grep,
                    };
                    device::logs(&backend, udid.as_deref(), &filter, limit, &mut out)
                }
                DeviceCommands::Crashes { udid, app, pull } => device::crashes(
                    &backend,
                    udid.as_deref(),
                    app.as_deref(),
                    pull.as_deref(),
                    &mut out,
                ),
                DeviceCommands::Pair { live } => {
                    with_target(&backend, &live, |t| live::pair(t, &mut out))
                }
                DeviceCommands::Unpair { live } => {
                    with_target(&backend, &live, |t| live::unpair(t, &mut out))
                }
                DeviceCommands::Status { live } => {
                    with_target(&backend, &live, |t| live::status(t, &mut out))
                }
                DeviceCommands::Snapshot { live } => {
                    with_target(&backend, &live, |t| live::snapshot(t, &mut out))
                }
                DeviceCommands::Cmd { live, name, args } => with_target(&backend, &live, |t| {
                    live::cmd(t, name.as_deref(), &args, &mut out)
                }),
                DeviceCommands::Connect {
                    live,
                    tail,
                    limit,
                    no_follow,
                } => with_target(&backend, &live, |t| {
                    live::connect(t, tail, limit, !no_follow, &mut out)
                }),
            }
        }
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
