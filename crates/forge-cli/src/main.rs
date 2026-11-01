mod commands;
mod config;
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
    Doctor,
    /// Create a new Forge Mobile app from template
    Init {
        name: String,
        #[arg(long, default_value = "org.example.myapp")]
        bundle_id: String,
        #[arg(long, default_value = "ios", value_parser = ["ios", "android"])]
        platform: String,
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
enum BindingsCommands {
    Generate {
        #[arg(long, default_value = "examples/counter/Generated")]
        out: PathBuf,
        #[arg(long, default_value = "swift", value_parser = ["swift", "kotlin"])]
        language: String,
    },
}

#[derive(Subcommand)]
enum BuildCommands {
    Ios {
        #[arg(long, default_value = "examples/counter")]
        project: PathBuf,
        #[arg(
            long,
            help = "Only build Rust static library and regenerate bindings (skip xtool dev)"
        )]
        package_only: bool,
    },
    Android {
        #[arg(long, default_value = "examples/counter-android")]
        project: PathBuf,
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
        #[arg(long, default_value = "examples/counter")]
        project: PathBuf,
    },
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
    match cli.command {
        Commands::Doctor => commands::doctor(runner, &root),
        Commands::Init {
            name,
            bundle_id,
            platform,
        } => commands::init(runner, &root, &name, &bundle_id, &platform),
        Commands::Test { project } => commands::test(runner, &root, project.as_deref()),
        Commands::Bindings { cmd } => match cmd {
            BindingsCommands::Generate { out, language } => {
                commands::bindings_generate(runner, &root, &out, &language)
            }
        },
        Commands::Build { cmd } => match cmd {
            BuildCommands::Ios {
                project,
                package_only,
            } => commands::build_ios(runner, &root, &project, package_only),
            BuildCommands::Android { project, install } => {
                commands::build_android(runner, &root, &project, install)
            }
        },
        Commands::Devices => commands::devices(runner),
        Commands::Install { cmd } => match cmd {
            InstallCommands::Ios { ipa } => commands::install_ios(runner, &ipa),
        },
        Commands::Run { cmd } => match cmd {
            RunCommands::Ios { project } => commands::run_ios(runner, &root, &project),
        },
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
        )
        .unwrap_err();
        assert!(err.contains("name"));
    }
}
