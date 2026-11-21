//! Python Lab as a harness environment: a project folder of plain files,
//! and the app's embedded CPython (through [`PythonHost`]) to run them.
//!
//! Rust does the file tools itself and writes a small wrapper around every
//! run: into the project folder, the project first on `sys.path`, the
//! project's modules forgotten first (the interpreter lives as long as the
//! app, and would otherwise keep running the version it imported before a
//! fix), and a guard of audit hooks that is active only during harness runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use super::calls::{check_path, ArgKind, ToolCall, ToolSpec};
use super::evidence::{Evidence, SeedFile};
use super::{Environment, ToolOutcome};

/// What one Python run printed, and whether it finished without an
/// exception.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PythonOutcome {
    pub ok: bool,
    pub output: String,
}

#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum PythonHostError {
    #[error("{reason}")]
    Unavailable { reason: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for PythonHostError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Unavailable { reason: err.reason }
    }
}

/// The app's Python (iOS: `PythonRuntime` over PythonBridge). Rust calls
/// `run` from a thread of its own, one script at a time.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait PythonHost: Send + Sync {
    /// Starts the interpreter if needed and runs `code` as a script.
    async fn run(&self, code: String) -> Result<PythonOutcome, PythonHostError>;
    /// Stops the running script (a KeyboardInterrupt in it).
    fn interrupt(&self);
}

/// Largest file the model may write.
pub const FILE_MAX_BYTES: usize = 64 * 1024;
/// Most files a project may hold.
pub const FILES_MAX: usize = 50;

/// The line the wrapper prints when the program raised.
const FAILED_MARK: &str = "__harness_failed__";
/// The line the test wrapper prints with the counts.
const TESTS_MARK: &str = "__harness_tests__";

pub struct PythonLab {
    root: PathBuf,
    host: Arc<dyn PythonHost>,
    run_limit: Duration,
    network: bool,
    running: AtomicBool,
}

impl PythonLab {
    pub fn new(root: PathBuf, host: Arc<dyn PythonHost>, run_limit: Duration) -> Self {
        Self {
            root,
            host,
            run_limit,
            network: false,
            running: AtomicBool::new(false),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Empties the project folder and writes `seed` into it. Only for a
    /// ladder task, whose folder the harness owns.
    pub fn reset(&self, seed: &[SeedFile]) -> std::io::Result<()> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)?;
        }
        fs::create_dir_all(&self.root)?;
        for file in seed {
            let path = self.root.join(file.path);
            fs::write(path, file.content)?;
        }
        Ok(())
    }

    /// The project's files (not the harness's own), sorted, with sizes.
    pub fn files(&self) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out);
        out.sort();
        out
    }

    fn full(&self, path: &str) -> Result<PathBuf, String> {
        check_path(path)?;
        Ok(self.root.join(path))
    }

    /// Runs a wrapper script with the time limit; `timed_out` when the
    /// limit interrupted it.
    fn run_script(&self, code: String) -> (PythonOutcome, bool) {
        let (done, wait) = mpsc::channel::<()>();
        let host = Arc::clone(&self.host);
        let limit = self.run_limit;
        let timed_out = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&timed_out);
        let watchdog = std::thread::spawn(move || {
            if wait.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
                flag.store(true, Ordering::SeqCst);
                host.interrupt();
            }
        });
        self.running.store(true, Ordering::SeqCst);
        let outcome =
            forge_runtime::block_on(self.host.run(code)).unwrap_or_else(|err| PythonOutcome {
                ok: false,
                output: format!("Python is not available: {err}"),
            });
        self.running.store(false, Ordering::SeqCst);
        let _ = done.send(());
        let _ = watchdog.join();
        (outcome, timed_out.load(Ordering::SeqCst))
    }

    /// Paths in tracebacks relative to the project, and the marks removed.
    fn clean(&self, output: &str) -> String {
        let mut output = output.replace(&format!("{}/", self.root.display()), "");
        if let Ok(real) = fs::canonicalize(&self.root) {
            output = output.replace(&format!("{}/", real.display()), "");
        }
        output
            .lines()
            .filter(|l| !l.starts_with(FAILED_MARK) && !l.starts_with(TESTS_MARK))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn list(&self) -> ToolOutcome {
        let files = self.files();
        let text = if files.is_empty() {
            "The project has no files yet.".to_string()
        } else {
            files
                .iter()
                .map(|(path, bytes)| format!("{path} ({bytes} bytes)"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        ToolOutcome::ok(text)
    }

    fn read(&self, path: &str) -> ToolOutcome {
        let full = match self.full(path) {
            Ok(full) => full,
            Err(err) => return ToolOutcome::failed(err),
        };
        match fs::read_to_string(&full) {
            Ok(text) => ToolOutcome::ok(numbered(&text)),
            Err(_) if !full.exists() => ToolOutcome::failed(format!(
                "{path} does not exist. Use list_files to see the files."
            )),
            Err(err) => ToolOutcome::failed(format!("Could not read {path}: {err}")),
        }
    }

    fn write(&self, path: &str, content: &str) -> ToolOutcome {
        let full = match self.full(path) {
            Ok(full) => full,
            Err(err) => return ToolOutcome::failed(err),
        };
        if content.len() > FILE_MAX_BYTES {
            return ToolOutcome::failed(format!(
                "That file is {} bytes; the limit is {FILE_MAX_BYTES}. Split it into smaller modules.",
                content.len()
            ));
        }
        if !full.exists() && self.files().len() >= FILES_MAX {
            return ToolOutcome::failed(format!("The project already has {FILES_MAX} files."));
        }
        if let Some(parent) = full.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                return ToolOutcome::failed(format!("Could not create the folder: {err}"));
            }
        }
        if let Err(err) = fs::write(&full, content) {
            return ToolOutcome::failed(format!("Could not write {path}: {err}"));
        }
        let lines = content.lines().count();
        if !path.ends_with(".py") {
            return ToolOutcome {
                ok: true,
                text: format!("Wrote {path} ({lines} lines)."),
                evidence: vec![Evidence::Wrote {
                    path: path.into(),
                    syntax_ok: true,
                }],
            };
        }
        // Python's own parser, at once, so a syntax error is fixed before
        // anything runs.
        let (outcome, _) = self.run_script(syntax_script(&full));
        let problem = self.clean(&outcome.output);
        let syntax_ok = outcome.ok && problem.trim().is_empty();
        ToolOutcome {
            ok: syntax_ok,
            text: if syntax_ok {
                format!("Wrote {path} ({lines} lines). Syntax OK.")
            } else {
                format!(
                    "Wrote {path} ({lines} lines), but Python can't parse it:\n{}",
                    problem.trim()
                )
            },
            evidence: vec![Evidence::Wrote {
                path: path.into(),
                syntax_ok,
            }],
        }
    }

    fn run_python(&self, path: &str) -> ToolOutcome {
        let full = match self.full(path) {
            Ok(full) => full,
            Err(err) => return ToolOutcome::failed(err),
        };
        if !path.ends_with(".py") {
            return ToolOutcome::failed(format!("{path} is not a Python file."));
        }
        if !full.is_file() {
            return ToolOutcome::failed(format!("{path} does not exist. Write it first."));
        }
        let action = format!(
            "import runpy\n    try:\n        runpy.run_path({}, run_name='__main__')\n    except SystemExit as _exit:\n        if _exit.code not in (None, 0):\n            raise\n",
            py_str(&full.display().to_string())
        );
        let (outcome, timed_out) = self.run_script(wrapper(&self.root, self.network, &action));
        let failed = !outcome.ok || outcome.output.contains(FAILED_MARK) || timed_out;
        let mut text = self.clean(&outcome.output);
        if timed_out {
            text += &format!(
                "\nStopped after {} s, the time limit. Does it loop forever or wait for input?",
                self.run_limit.as_secs()
            );
        }
        let shown = if text.trim().is_empty() {
            "(no output)".to_string()
        } else {
            text.trim_end().to_string()
        };
        ToolOutcome {
            ok: !failed,
            text: format!(
                "{path} {}.\n{shown}",
                if failed { "ended with an error" } else { "ran" }
            ),
            evidence: vec![Evidence::Ran {
                path: path.into(),
                ok: !failed,
                output: text,
            }],
        }
    }

    fn run_tests(&self) -> ToolOutcome {
        let action = format!(
            "import unittest\n    _suite = unittest.TestLoader().discover(_root, pattern='test*.py', top_level_dir=_root)\n    _result = unittest.TextTestRunner(stream=sys.stdout, verbosity=2).run(_suite)\n    print('{TESTS_MARK}', _result.testsRun, len(_result.failures), len(_result.errors))\n"
        );
        let (outcome, timed_out) = self.run_script(wrapper(&self.root, self.network, &action));
        let counts = outcome
            .output
            .lines()
            .find_map(|l| l.strip_prefix(TESTS_MARK))
            .and_then(|rest| {
                let n: Vec<u32> = rest
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                (n.len() == 3).then(|| (n[0], n[1], n[2]))
            });
        let mut text = self.clean(&outcome.output).trim().to_string();
        let Some((ran, failures, errors)) = counts.filter(|_| !timed_out) else {
            if timed_out {
                text += &format!(
                    "\nThe tests were stopped after {} s, the time limit.",
                    self.run_limit.as_secs()
                );
            }
            return ToolOutcome {
                ok: false,
                text: format!("The tests could not run.\n{text}"),
                evidence: vec![Evidence::Tests {
                    ran: 0,
                    failures: 0,
                    errors: 1,
                }],
            };
        };
        let ok = ran > 0 && failures == 0 && errors == 0;
        let headline = if ran == 0 {
            "No tests were found. Test files are named test_*.py and define unittest.TestCase \
             classes with test_ methods."
                .to_string()
        } else {
            format!("{ran} tests ran: {failures} failures, {errors} errors.")
        };
        ToolOutcome {
            ok,
            text: format!("{headline}\n{text}"),
            evidence: vec![Evidence::Tests {
                ran,
                failures,
                errors,
            }],
        }
    }
}

impl Environment for PythonLab {
    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec::new("list_files", &[], "the project's files"),
            ToolSpec::new(
                "read_file",
                &[("path", ArgKind::Path)],
                "a file, with line numbers",
            ),
            ToolSpec::new(
                "write_file",
                &[("path", ArgKind::Path), ("content", ArgKind::Text)],
                "create or replace a file; always send the whole file",
            ),
            ToolSpec::new(
                "run_python",
                &[("path", ArgKind::Path)],
                "run a Python file and see what it prints",
            ),
            ToolSpec::new(
                "run_tests",
                &[],
                "run every unittest test in files named test_*.py",
            ),
        ]
    }

    fn overview(&self) -> String {
        self.list().text
    }

    fn execute(&self, call: &ToolCall) -> ToolOutcome {
        match call.tool.as_str() {
            "list_files" => self.list(),
            "read_file" => self.read(call.arg("path")),
            "write_file" => self.write(call.arg("path"), call.arg("content")),
            "run_python" => self.run_python(call.arg("path")),
            "run_tests" => self.run_tests(),
            other => ToolOutcome::failed(format!("There is no tool called {other:?}.")),
        }
    }

    fn read_text(&self, path: &str) -> Option<String> {
        fs::read_to_string(self.full(path).ok()?).ok()
    }

    fn interrupt(&self) {
        if self.running.load(Ordering::SeqCst) {
            self.host.interrupt();
        }
    }

    fn checkpoint(&self, name: &str) -> std::io::Result<()> {
        let target = self.root.join(".harness/checkpoints").join(name);
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        for (path, _) in self.files() {
            let to = target.join(&path);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(self.root.join(&path), to)?;
        }
        Ok(())
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "__pycache__" {
            continue;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(root, &path, out);
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((relative, bytes));
        }
    }
}

fn numbered(text: &str) -> String {
    if text.is_empty() {
        return "(empty file)".into();
    }
    text.lines()
        .enumerate()
        .map(|(i, line)| format!("{:>3}| {line}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A Python string literal (JSON's escapes are valid Python).
fn py_str(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "''".into())
}

/// Checks that `path` parses, printing the error the way Python reports it.
fn syntax_script(path: &Path) -> String {
    format!(
        "import ast\n_path = {}\ntry:\n    with open(_path, encoding='utf-8') as _f:\n        ast.parse(_f.read(), _path)\nexcept SyntaxError as _e:\n    print(f'line {{_e.lineno}}, column {{_e.offset}}: {{_e.msg}}')\n    if _e.text:\n        print('    ' + _e.text.rstrip())\n",
        py_str(&path.display().to_string())
    )
}

/// The guard: audit hooks installed once per interpreter, active only while
/// a harness run sets `state['active']`. A guard rail for a model's
/// mistakes, not a security boundary.
const GUARD: &str = r#"
import os, sys, tempfile, types
_guard = sys.modules.get('_bonsai_harness_guard')
if _guard is None:
    _guard = types.ModuleType('_bonsai_harness_guard')
    _guard.state = {'active': False, 'roots': [], 'network': False}
    _WRITE_FLAGS = os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_APPEND | os.O_TRUNC
    def _inside(path, _s=_guard.state):
        try:
            full = os.path.realpath(os.fsdecode(path))
        except Exception:
            return False
        return any(full == r or full.startswith(r + os.sep) for r in _s['roots'])
    def _hook(event, args, _s=_guard.state):
        if not _s['active']:
            return
        if event == 'open':
            path, mode, flags = args
            writes = (mode is not None and any(c in mode for c in 'wax+')) or (mode is None and flags and flags & _WRITE_FLAGS)
            if writes and isinstance(path, (str, bytes, os.PathLike)) and not _inside(path):
                raise PermissionError(f'Bonsai Harness: writing outside the project is not allowed ({path})')
        elif event in ('os.remove', 'os.rmdir', 'shutil.rmtree', 'os.rename', 'os.replace'):
            for path in args[:2]:
                if isinstance(path, (str, bytes, os.PathLike)) and not _inside(path):
                    raise PermissionError(f'Bonsai Harness: changing files outside the project is not allowed ({path})')
        elif event in ('socket.connect', 'socket.getaddrinfo') and not _s['network']:
            raise PermissionError('Bonsai Harness: the network is off for this project')
        elif event in ('subprocess.Popen', 'os.system', 'os.exec', 'os.posix_spawn', 'os.fork', 'os.spawn'):
            raise PermissionError('Bonsai Harness: starting other programs is not allowed')
    sys.addaudithook(_hook)
    sys.modules['_bonsai_harness_guard'] = _guard
"#;

/// A harness run: `action` (indented by four spaces, in a `try`) runs in
/// the project folder with the guard on, and everything is put back after.
fn wrapper(root: &Path, network: bool, action: &str) -> String {
    format!(
        r#"{GUARD}
import io, traceback
_given = {root}
_root = os.path.realpath(_given)
_saved = (os.getcwd(), list(sys.path), sys.stdin)
for _name, _module in list(sys.modules.items()):
    _file = getattr(_module, '__file__', None) or ''
    if _file and os.path.realpath(_file).startswith(_root + os.sep):
        del sys.modules[_name]
os.chdir(_root)
sys.path.insert(0, _root)
sys.stdin = io.StringIO('')
_guard.state.update(active=True, roots=[_root, os.path.realpath(tempfile.gettempdir())], network={network})
try:
    {action}
except BaseException as _error:
    _frames = [f for f in traceback.extract_tb(_error.__traceback__) if f.filename.startswith((_root, _given))]
    print('Traceback (most recent call last):')
    print(''.join(traceback.format_list(_frames)), end='')
    print(''.join(traceback.format_exception_only(type(_error), _error)), end='')
    print('{FAILED_MARK}')
finally:
    _guard.state['active'] = False
    os.chdir(_saved[0])
    sys.path[:] = _saved[1]
    sys.stdin = _saved[2]
"#,
        root = py_str(&root.display().to_string()),
        network = if network { "True" } else { "False" },
    )
}

#[cfg(test)]
pub(crate) mod testing {
    //! Runs the wrapper with the machine's own python3 when there is one,
    //! so the tests check real Python behaviour on Linux.

    use super::*;
    use std::process::Command;

    pub struct LocalPython;

    #[async_trait::async_trait]
    impl PythonHost for LocalPython {
        async fn run(&self, code: String) -> Result<PythonOutcome, PythonHostError> {
            // The same contract as PythonBridge's runner: stdout and stderr
            // together, ok when no exception escaped.
            let output = Command::new("python3")
                .arg("-I")
                .arg("-c")
                .arg(&code)
                .output()
                .map_err(|e| PythonHostError::Unavailable {
                    reason: e.to_string(),
                })?;
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            text += &String::from_utf8_lossy(&output.stderr);
            Ok(PythonOutcome {
                ok: output.status.success(),
                output: text,
            })
        }

        fn interrupt(&self) {}
    }

    pub fn has_python() -> bool {
        Command::new("python3").arg("--version").output().is_ok()
    }
}
