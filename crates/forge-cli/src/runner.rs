use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>) -> Result<Output, String>;
}

#[derive(Default)]
pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>) -> Result<Output, String> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        cmd.output()
            .map_err(|e| format!("failed to run {program}: {e}"))
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct MockRunner {
    pub responses: Arc<Mutex<HashMap<String, Output>>>,
}

impl MockRunner {}

impl CommandRunner for MockRunner {
    fn run(&self, program: &str, args: &[&str], _cwd: Option<&Path>) -> Result<Output, String> {
        let key = format!("{} {}", program, args.join(" "));
        self.responses
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("no mock for {key}"))
    }
}
