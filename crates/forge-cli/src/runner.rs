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

    /// Like [`stream`](Self::stream), but also returns the last part of what
    /// the child printed (stdout and stderr interleaved), so a failure can be
    /// explained. Output is still forwarded to the terminal as it arrives and
    /// stdin stays attached, so prompts (xtool's Apple ID login) still work.
    fn stream_tail(&self, cmd: &Cmd) -> Result<(ExitStatus, String), String> {
        self.stream(cmd).map(|status| (status, String::new()))
    }

    /// Like [`stream_tail`](Self::stream_tail), for a child that may hang
    /// after its work is done: once `done` holds for the output so far and
    /// the child then prints nothing for `grace`, it is killed. The `bool` is
    /// true when that happened (the exit status is then the kill's).
    fn stream_tail_until(
        &self,
        cmd: &Cmd,
        done: &(dyn Fn(&str) -> bool + Sync),
        grace: std::time::Duration,
    ) -> Result<(ExitStatus, String, bool), String> {
        let _ = (done, grace);
        self.stream_tail(cmd)
            .map(|(status, tail)| (status, tail, false))
    }
}

/// How much of a child's output [`CommandRunner::stream_tail`] keeps.
pub const TAIL_BYTES: usize = 64 * 1024;

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
        // With --json, every step's output also goes into the result's log.
        if crate::report::json_mode() {
            return self.stream_tail(cmd).map(|(status, _)| status);
        }
        cmd.to_command()
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| format!("failed to run {}: {e}", cmd.program.to_string_lossy()))
    }

    fn stream_tail(&self, cmd: &Cmd) -> Result<(ExitStatus, String), String> {
        self.stream_tail_until(cmd, &|_| false, std::time::Duration::ZERO)
            .map(|(status, tail, _)| (status, tail))
    }

    fn stream_tail_until(
        &self,
        cmd: &Cmd,
        done: &(dyn Fn(&str) -> bool + Sync),
        grace: std::time::Duration,
    ) -> Result<(ExitStatus, String, bool), String> {
        use std::io::{Read, Write};
        use std::sync::{Arc, Mutex};

        crate::report::log_bytes(format!("$ {}\n", cmd.display()).as_bytes());
        let mut child = cmd
            .to_command()
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to run {}: {e}", cmd.program.to_string_lossy()))?;
        let tail = Arc::new(Mutex::new(Vec::<u8>::new()));

        // Forwards each chunk as soon as it is read (not line by line), so a
        // prompt without a trailing newline still shows.
        fn pump<R: Read, W: Write>(
            mut from: R,
            mut to: W,
            tail: &Mutex<Vec<u8>>,
            printed: &std::sync::atomic::AtomicUsize,
        ) {
            let mut buf = [0u8; 8192];
            while let Ok(n) = from.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let _ = to.write_all(&buf[..n]);
                let _ = to.flush();
                crate::report::log_bytes(&buf[..n]);
                printed.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                let mut t = tail.lock().unwrap_or_else(|p| p.into_inner());
                t.extend_from_slice(&buf[..n]);
                if t.len() > 2 * TAIL_BYTES {
                    let cut = t.len() - TAIL_BYTES;
                    t.drain(..cut);
                }
            }
        }

        // Bytes printed so far, to tell a quiet child from a busy one.
        let printed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let out = child.stdout.take().map(|o| {
            let (tail, printed) = (Arc::clone(&tail), Arc::clone(&printed));
            std::thread::spawn(move || pump(o, std::io::stdout(), &tail, &printed))
        });
        let err = child.stderr.take().map(|e| {
            let (tail, printed) = (Arc::clone(&tail), Arc::clone(&printed));
            std::thread::spawn(move || pump(e, std::io::stderr(), &tail, &printed))
        });
        let wait_err = |e: std::io::Error| {
            format!("failed to wait for {}: {e}", cmd.program.to_string_lossy())
        };
        // Output keeps the child alive: `quiet_since` restarts whenever the
        // tail grows, so a step that still reports progress is never cut.
        let mut is_done = false;
        let mut seen = 0usize;
        let mut quiet_since = std::time::Instant::now();
        let mut cut_short = false;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(wait_err)? {
                break status;
            }
            {
                let t = tail.lock().unwrap_or_else(|p| p.into_inner());
                let now = printed.load(std::sync::atomic::Ordering::Relaxed);
                if now != seen {
                    seen = now;
                    quiet_since = std::time::Instant::now();
                }
                if !is_done {
                    let start = t.len().saturating_sub(4096);
                    is_done = done(&String::from_utf8_lossy(&t[start..]));
                }
            }
            if is_done && quiet_since.elapsed() >= grace {
                // Its work is done but it doesn't exit: stop it. Its
                // children may hold the pipes, so don't wait for the pumps.
                let _ = child.kill();
                cut_short = true;
                break child.wait().map_err(wait_err)?;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        if !cut_short {
            for handle in [out, err].into_iter().flatten() {
                let _ = handle.join();
            }
        }
        let bytes = tail.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let start = bytes.len().saturating_sub(TAIL_BYTES);
        Ok((
            status,
            String::from_utf8_lossy(&bytes[start..]).into_owned(),
            cut_short,
        ))
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

    fn stream_tail(&self, cmd: &Cmd) -> Result<(ExitStatus, String), String> {
        self.lookup(cmd)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn real_stream_tail_keeps_stdout_and_stderr() {
        let (status, tail) = RealRunner
            .stream_tail(&Cmd::new("sh").args(["-c", "echo out; echo err >&2; exit 3"]))
            .unwrap();
        assert_eq!(status.code(), Some(3));
        assert!(tail.contains("out") && tail.contains("err"), "{tail}");
    }

    #[test]
    fn real_stream_tail_until_stops_a_child_that_hangs_after_its_work() {
        let started = std::time::Instant::now();
        let (_, tail, cut_short) = RealRunner
            .stream_tail_until(
                &Cmd::new("sh").args(["-c", "echo 'Installing 100%'; sleep 30"]),
                &|t| t.contains("100%"),
                std::time::Duration::from_millis(300),
            )
            .unwrap();
        assert!(cut_short);
        assert!(tail.contains("Installing 100%"), "{tail}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));

        // A child that exits by itself is left alone.
        let (status, _, cut_short) = RealRunner
            .stream_tail_until(
                &Cmd::new("sh").args(["-c", "echo 'Installing 100%'; exit 0"]),
                &|t| t.contains("100%"),
                std::time::Duration::from_secs(5),
            )
            .unwrap();
        assert!(status.success() && !cut_short);
    }
}
