use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output, Stdio};
#[cfg(test)]
use std::sync::Mutex;

/// A command to run: program, arguments, working directory and extra environment.
///
/// Arguments are `OsString`s so non-UTF-8 paths pass through unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, OsString)>,
}

impl Cmd {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, key: &str, value: impl AsRef<OsStr>) -> Self {
        self.env.push((key.to_string(), value.as_ref().to_owned()));
        self
    }

    /// `program arg1 arg2`, for messages and test assertions.
    pub fn display(&self) -> String {
        std::iter::once(&self.program)
            .chain(&self.args)
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn to_command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);
        if let Some(dir) = &self.cwd {
            cmd.current_dir(dir);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }
}

pub trait CommandRunner: Send + Sync {
    /// Runs with captured stdout/stderr (for probes and output the CLI parses).
    fn capture(&self, cmd: &Cmd) -> Result<Output, String>;

    /// Runs attached to the terminal: output streams live and the child can
    /// prompt on stdin (builds, `xtool dev`, Gradle).
    fn stream(&self, cmd: &Cmd) -> Result<ExitStatus, String>;
}

#[derive(Default)]
pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn capture(&self, cmd: &Cmd) -> Result<Output, String> {
        cmd.to_command()
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("failed to run {}: {e}", cmd.program.to_string_lossy()))
    }

    fn stream(&self, cmd: &Cmd) -> Result<ExitStatus, String> {
        cmd.to_command()
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| format!("failed to run {}: {e}", cmd.program.to_string_lossy()))
    }
}

/// Records every command and answers from canned results. Unknown commands fail
/// as if the program were not installed.
#[cfg(test)]
#[derive(Default)]
pub struct MockRunner {
    responses: Mutex<Vec<(String, i32, String)>>,
    pub calls: Mutex<Vec<Cmd>>,
}

#[cfg(test)]
impl MockRunner {
    /// Commands whose display string starts with `prefix` exit with `code` and print `stdout`.
    pub fn respond(&self, prefix: &str, code: i32, stdout: &str) -> &Self {
        self.responses
            .lock()
            .unwrap()
            .push((prefix.to_string(), code, stdout.to_string()));
        self
    }

    pub fn calls(&self) -> Vec<Cmd> {
        self.calls.lock().unwrap().clone()
    }

    pub fn call_lines(&self) -> Vec<String> {
        self.calls().iter().map(Cmd::display).collect()
    }

    fn lookup(&self, cmd: &Cmd) -> Result<(ExitStatus, String), String> {
        self.calls.lock().unwrap().push(cmd.clone());
        let line = cmd.display();
        let responses = self.responses.lock().unwrap();
        responses
            .iter()
            .rev()
            .find(|(prefix, _, _)| line.starts_with(prefix.as_str()))
            .map(|(_, code, out)| (exit_status(*code), out.clone()))
            .ok_or_else(|| format!("failed to run {line}: not found"))
    }
}

#[cfg(test)]
fn exit_status(code: i32) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
}

#[cfg(test)]
impl CommandRunner for MockRunner {
    fn capture(&self, cmd: &Cmd) -> Result<Output, String> {
        let (status, stdout) = self.lookup(cmd)?;
        Ok(Output {
            status,
            stdout: stdout.into_bytes(),
            stderr: Vec::new(),
        })
    }

    fn stream(&self, cmd: &Cmd) -> Result<ExitStatus, String> {
        self.lookup(cmd).map(|(status, _)| status)
    }
}
