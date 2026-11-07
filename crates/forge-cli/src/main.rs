mod app_config;
mod check;
mod commands;
mod config;
mod doctor;
mod ios_builder;
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
        #[arg(long, default_value = "ios", value_parser = ["ios", "android"])]
        platform: String,
        /// Directory to create the app in (default: <workspace>/examples/<NAME>).
        /// Relative paths are resolved against the current directory.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,
    },
    /// iOS app configuration from forge.toml: Info.plist, App.entitlements, xtool.yml
    Config {
        #[command(subcommand)]
        cmd: ConfigCommands,
    },
    /// Run Rust tests and checks; with --project, also that app's host tests
    Test {
        #[arg(
            long,
            help = "Also run this app's host tests (JVM unit tests or XCTest on macOS)"
        )]
        project: Option<PathBuf>,
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
    /// Build, install, and launch on device (iOS via `xtool dev`)
    Run {
        #[command(subcommand)]
        cmd: RunCommands,
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
enum BindingsCommands {
    /// Regenerates an app's Generated/ from its Rust crate (Swift or Kotlin,
    /// from the app's platform): the app given by --project, or the current
    /// directory when it has forge.toml. With --out, writes the shared
    /// forge-ffi bindings there instead (default: examples/counter/Generated).
    Generate {
        #[arg(long, conflicts_with_all = ["out", "language"])]
        project: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "swift", value_parser = ["swift", "kotlin"])]
        language: String,
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
        #[arg(long)]
        release: bool,
    },
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
    },
}

fn configuration(release: bool) -> ios_builder::BuildConfiguration {
    if release {
        ios_builder::BuildConfiguration::Release
    } else {
        ios_builder::BuildConfiguration::Debug
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
            let path = path.map(|p| absolute_from_cwd(&p));
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
        Commands::Test { project } => commands::test(runner, &root, project.as_deref()),
        Commands::Bindings { cmd } => match cmd {
            BindingsCommands::Generate {
                project: Some(project),
                ..
            } => commands::bindings_generate_for_project(runner, &root, &project),
            BindingsCommands::Generate {
                project: None,
                out: Some(out),
                language,
            } => commands::bindings_generate(runner, &root, &out, &language),
            BindingsCommands::Generate {
                project: None,
                out: None,
                language,
            } => match project_or_default(None, "") {
                app if !app.as_os_str().is_empty() => {
                    commands::bindings_generate_for_project(runner, &root, &app)
                }
                _ => commands::bindings_generate(
                    runner,
                    &root,
                    std::path::Path::new("examples/counter/Generated"),
                    &language,
                ),
            },
        },
        Commands::Build { cmd } => match cmd {
            BuildCommands::Ios {
                project,
                package_only,
                release,
            } => {
                let project = project_or_default(project, "examples/counter");
                commands::build_ios(
                    runner,
                    &ios,
                    &root,
                    &project,
                    package_only,
                    configuration(release),
                )
            }
            BuildCommands::Android { project, install } => {
                let project = project_or_default(project, "examples/counter-android");
                commands::build_android(runner, &root, &project, install)
            }
        },
        Commands::Devices => commands::devices(runner, &ios),
        Commands::Install { cmd } => match cmd {
            InstallCommands::Ios { ipa } => commands::install_ios(runner, &ios, &ipa),
        },
        Commands::Run { cmd } => match cmd {
            RunCommands::Ios { project, release } => {
                let project = project_or_default(project, "examples/counter");
                commands::run_ios(runner, &ios, &root, &project, configuration(release))
            }
        },
    }
}

/// `--project` when given; otherwise the current directory when it is an app
/// (has forge.toml), so `forge build android` works from inside an app;
/// otherwise the bundled example.
fn project_or_default(project: Option<PathBuf>, example: &str) -> PathBuf {
    project.unwrap_or_else(|| match std::env::current_dir() {
        Ok(cwd) if cwd.join("forge.toml").is_file() => cwd,
        _ => PathBuf::from(example),
    })
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
core_crate = "forge-core"
ffi_crate = "forge-ffi"
[build]
ios_device = true
ios_simulator = false
"#,
        )
        .unwrap_err();
        assert!(err.contains("bundle_id"));
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
