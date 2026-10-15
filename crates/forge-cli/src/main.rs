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
    },
    /// Run Rust tests and checks
    Test {
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Generate UniFFI Swift bindings
    Bindings {
        #[command(subcommand)]
        cmd: BindingsCommands,
    },
    /// Build the iOS application
    Build {
        #[command(subcommand)]
        cmd: BuildCommands,
    },
    /// List connected iOS devices (xtool)
    Devices,
    /// Install built IPA to device
    Install {
        #[command(subcommand)]
        cmd: InstallCommands,
    },
    /// Build, install, and launch on device
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
    },
}

#[derive(Subcommand)]
enum BuildCommands {
    Ios {
        #[arg(long, default_value = "examples/counter")]
        project: PathBuf,
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
    match cli.command {
        Commands::Doctor => commands::doctor(runner),
        Commands::Init { name, bundle_id } => commands::init(&name, &bundle_id),
        Commands::Test { project } => commands::test(runner, project.as_deref()),
        Commands::Bindings { cmd } => match cmd {
            BindingsCommands::Generate { out } => commands::bindings_generate(runner, &out),
        },
        Commands::Build { cmd } => match cmd {
            BuildCommands::Ios { project } => commands::build_ios(runner, &project),
        },
        Commands::Devices => commands::devices(runner),
        Commands::Install { cmd } => match cmd {
            InstallCommands::Ios { ipa } => commands::install_ios(runner, &ipa),
        },
        Commands::Run { cmd } => match cmd {
            RunCommands::Ios { project } => commands::run_ios(runner, &project),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;

    #[test]
    fn doctor_runs_without_panic() {
        let mock = MockRunner::default();
        let _ = commands::doctor(&mock);
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
        let err = commands::init("bad name!", "org.example.app").unwrap_err();
        assert!(err.contains("name"));
    }
}
