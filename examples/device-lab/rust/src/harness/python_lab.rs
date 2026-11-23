//! Python Lab as a harness environment: a project folder of plain files,
//! and the app's embedded CPython (through [`PythonHost`]) to run them.
//!
//! Rust does the file tools itself and writes a small wrapper around every
//! run: into the project folder, the project first on `sys.path`, the
//! project's modules forgotten first (the interpreter lives as long as the
//! app, and would otherwise keep running the version it imported before a
//! fix), and a guard of audit hooks that is active only during harness runs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::artifacts;
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
pub(crate) const FAILED_MARK: &str = "__harness_failed__";
/// The line the test wrapper prints with the counts.
const TESTS_MARK: &str = "__harness_tests__";

pub struct PythonLab {
    root: PathBuf,
    host: Arc<dyn PythonHost>,
    run_limit: Duration,
    network: bool,
    running: AtomicBool,
    /// The SHA-256 of each file as Bonsai last read or wrote it in this
    /// task, so a write over a file someone changed since (in Python Lab,
    /// or through the Files app) is refused (python-lab-workspace-plan.md,
    /// PL1e). Its own runs' changes are Bonsai's, so they update it.
    seen: Mutex<BTreeMap<String, String>>,
}

fn lock_seen(
    seen: &Mutex<BTreeMap<String, String>>,
) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
    seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The SHA-256 of the file at `path`, or None when it can't be read.
fn digest(path: &Path) -> Option<String> {
    forge_storage::files::sha256_file(path).ok()
}

/// The digest of bytes Bonsai read or wrote, never a later look at the
/// file (which another writer may have changed in between).
fn digest_bytes(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl PythonLab {
    pub fn new(root: PathBuf, host: Arc<dyn PythonHost>, run_limit: Duration) -> Self {
        Self {
            root,
            host,
            run_limit,
            network: false,
            running: AtomicBool::new(false),
            seen: Mutex::new(BTreeMap::new()),
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
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
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
        // A link inside the project could point outside it, and the
        // harness's own writes don't pass through Python's guard.
        let mut current = self.root.clone();
        for part in path.split('/') {
            current.push(part);
            if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(format!("{path:?} must not pass through a symbolic link."));
            }
        }
        Ok(self.root.join(path))
    }

    /// Size and modification time of each Python source file, to notice a
    /// run that changed the project's code.
    fn source_stamps(&self) -> BTreeMap<String, (u64, Option<SystemTime>)> {
        self.files()
            .into_iter()
            .filter(|(path, _)| path.ends_with(".py"))
            .map(|(path, bytes)| {
                let modified = fs::metadata(self.root.join(&path))
                    .and_then(|m| m.modified())
                    .ok();
                (path, (bytes, modified))
            })
            .collect()
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
            Ok(text) => {
                self.remember(path, text.as_bytes());
                ToolOutcome::ok(numbered(&text))
            }
            Err(_) if !full.exists() => ToolOutcome::failed(format!(
                "{path} does not exist. Use list_files to see the files."
            )),
            Err(err) => ToolOutcome::failed(format!("Could not read {path}: {err}")),
        }
    }

    /// `read_symbol`: one definition, found by Python's own `ast` (H6), so
    /// the model reads a function instead of a whole file.
    fn read_symbol(&self, path: &str, name: &str) -> ToolOutcome {
        let full = match self.full(path) {
            Ok(full) => full,
            Err(err) => return ToolOutcome::failed(err),
        };
        let Ok(text) = fs::read_to_string(&full) else {
            return ToolOutcome::failed(format!(
                "{path} does not exist. Use list_files to see the files."
            ));
        };
        if !path.ends_with(".py") {
            return ToolOutcome::failed(format!(
                "read_symbol reads Python files; use read_file for {path}."
            ));
        }
        // The lookup reads these bytes, so they count as read: a later
        // change on disk is refused until Bonsai reads the file again.
        self.remember(path, text.as_bytes());
        // Python parses the text read above, so its line numbers fit the
        // lines shown.
        let script = SYMBOLS_SCRIPT.replace("__SOURCE__", &py_str(&text));
        let (outcome, _) = self.run_script(script);
        let output = self.clean(&outcome.output);
        let symbols: Option<BTreeMap<String, (usize, usize)>> = output
            .lines()
            .find_map(|line| line.strip_prefix(SYMBOLS_MARK))
            .and_then(|json| serde_json::from_str(json).ok());
        let Some(symbols) = symbols.filter(|_| outcome.ok) else {
            return ToolOutcome::failed(format!(
                "Python can't read the definitions in {path}: {}",
                output.trim().lines().last().unwrap_or("it didn't parse")
            ));
        };
        let name = name.trim();
        // A method may be asked for without its class, when only one fits.
        let found = symbols.get(name).copied().or_else(|| {
            let matches: Vec<(usize, usize)> = symbols
                .iter()
                .filter(|(key, _)| key.rsplit('.').next() == Some(name) && key.contains('.'))
                .map(|(_, range)| *range)
                .collect();
            match matches[..] {
                [one] => Some(one),
                _ => None,
            }
        });
        let Some((start, end)) = found else {
            let known: Vec<&str> = symbols.keys().map(String::as_str).collect();
            return ToolOutcome::failed(if known.is_empty() {
                format!("{path} defines no functions or classes. Use read_file.")
            } else {
                format!(
                    "{path} has no function or class {name:?}. It has: {}.",
                    known.join(", ")
                )
            });
        };
        let lines: Vec<String> = text
            .lines()
            .enumerate()
            .skip(start.saturating_sub(1))
            .take(end + 1 - start.max(1))
            .map(|(i, line)| format!("{:>3}| {line}", i + 1))
            .collect();
        ToolOutcome::ok(format!(
            "{path}, lines {start} to {end}:\n{}",
            lines.join("\n")
        ))
    }

    fn write(&self, path: &str, content: &str) -> ToolOutcome {
        self.store(path, content, "Wrote")
    }

    /// `edit_file`: replaces `old` with `new`. `old` must match exactly
    /// once; a match that differs only in spaces at line ends is taken too;
    /// otherwise the error shows the most similar lines (Aider's "did you
    /// mean").
    fn edit(&self, path: &str, old: &str, new: &str) -> ToolOutcome {
        let full = match self.full(path) {
            Ok(full) => full,
            Err(err) => return ToolOutcome::failed(err),
        };
        if let Some(refusal) = self.changed_outside(path, &full) {
            return refusal;
        }
        let Ok(text) = fs::read_to_string(&full) else {
            return ToolOutcome::failed(format!(
                "{path} does not exist. Use write_file to create it."
            ));
        };
        if old.is_empty() {
            return ToolOutcome::failed(
                "old is empty. Copy the exact lines to replace from read_file (without the line numbers)."
                    .to_string(),
            );
        }
        let mut note = None;
        let edited = match text.matches(old).count() {
            1 => text.replacen(old, new, 1),
            0 => match replace_lines_loosely(&text, old, new)
                // Small models copy a line with ' for " (writing " inside
                // the JSON answer needs \") and with the indent off by a
                // space. One place that fits once those are ignored is
                // taken, re-indented, and the answer says so.
                .or_else(|| {
                    replace_lines_forgivingly(&text, old, new).map(|(edited, line)| {
                        note = Some(format!(
                            " old matched line {line} once quotes and indentation were ignored; the new lines were indented to fit."
                        ));
                        edited
                    })
                }) {
                Some(edited) => edited,
                None => {
                    let hint = similar_lines(&text, old)
                        .map(|lines| format!(" Did you mean these lines?\n{lines}"))
                        .unwrap_or_default();
                    return ToolOutcome::failed(format!(
                        "old does not match {path} exactly.{hint}\nCopy the lines exactly from read_file, without the line numbers."
                    ));
                }
            },
            count => {
                return ToolOutcome::failed(format!(
                    "old matches {count} places in {path}. Include more lines around it so it matches once."
                ))
            }
        };
        let mut outcome = self.store(path, &edited, "Edited");
        if let (true, Some(note)) = (outcome.ok, note) {
            outcome.text.push_str(&note);
        }
        outcome
    }

    fn store(&self, path: &str, content: &str, verb: &str) -> ToolOutcome {
        let full = match self.full(path) {
            Ok(full) => full,
            Err(err) => return ToolOutcome::failed(err),
        };
        if let Some(refusal) = self.changed_outside(path, &full) {
            return refusal;
        }
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
        // Checked again just before the write: an outside change since the
        // first check is caught unless it lands in the moment between this
        // check and the rename (files have no compare-and-swap rename).
        if let Some(refusal) = self.changed_outside(path, &full) {
            return refusal;
        }
        // Atomic, so the person (or the Files app) never sees half a file.
        if let Err(err) = forge_storage::write_atomic(&full, content.as_bytes()) {
            return ToolOutcome::failed(format!("Could not write {path}: {err}"));
        }
        self.remember(path, content.as_bytes());
        let lines = content.lines().count();
        if !path.ends_with(".py") {
            return ToolOutcome {
                ok: true,
                text: format!("{verb} {path} ({lines} lines)."),
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
                format!("{verb} {path} ({lines} lines). Syntax OK.")
            } else {
                format!(
                    "{verb} {path} ({lines} lines), but Python can't parse it:\n{}{}",
                    problem.trim(),
                    quote_hint(&problem)
                )
            },
            evidence: vec![Evidence::Wrote {
                path: path.into(),
                syntax_ok,
            }],
        }
    }

    /// Records `path`'s contents as Bonsai read or wrote them.
    fn remember(&self, path: &str, bytes: &[u8]) {
        lock_seen(&self.seen).insert(path.to_string(), digest_bytes(bytes));
    }

    /// A refusal when `path` changed since Bonsai last read or wrote it (a
    /// file it never read can be written; one deleted since can be
    /// written again).
    fn changed_outside(&self, path: &str, full: &Path) -> Option<ToolOutcome> {
        let known = lock_seen(&self.seen).get(path).cloned()?;
        let now = digest(full)?;
        (now != known).then(|| {
            ToolOutcome::failed(format!(
                "{path} changed outside Bonsai since you read it; read it again with read_file before changing it."
            ))
        })
    }

    /// The digest now of each file Bonsai knows, before a run. A file
    /// that already changed outside is left out, so the run can't make
    /// that change known.
    fn snapshot_seen(&self) -> BTreeMap<String, Option<String>> {
        let known: Vec<(String, String)> = lock_seen(&self.seen)
            .iter()
            .map(|(path, sum)| (path.clone(), sum.clone()))
            .collect();
        known
            .into_iter()
            .filter_map(|(path, seen)| {
                let sum = self.full(&path).ok().and_then(|full| digest(&full));
                (sum.as_deref() == Some(seen.as_str())).then_some((path, sum))
            })
            .collect()
    }

    /// After a run: what changed during it is Bonsai's own script's work,
    /// so it becomes known. A file that changed outside before the run
    /// stays as it was known, whatever the run did to it, so a later
    /// write is still refused until Bonsai reads it again.
    fn refresh_seen(&self, before: BTreeMap<String, Option<String>>) {
        for (path, was) in before {
            let now = self.full(&path).ok().and_then(|full| digest(&full));
            if now == was {
                continue;
            }
            let mut seen = lock_seen(&self.seen);
            match now {
                Some(sum) => seen.insert(path, sum),
                None => seen.remove(&path),
            };
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
        let before = artifacts::stamps(&self.root);
        let sources = self.source_stamps();
        let (outcome, timed_out) =
            self.run_script(wrapper(&self.root, self.network, &action, false));
        let failed = !outcome.ok || outcome.output.contains(FAILED_MARK) || timed_out;
        let mut text = self.clean(&outcome.output);
        if timed_out {
            text += &format!(
                "\nStopped after {} s, the time limit. Does it loop forever or wait for input?",
                self.run_limit.as_secs()
            );
        }
        let mut shown = if text.trim().is_empty() {
            "(no output)".to_string()
        } else {
            text.trim_end().to_string()
        };
        // Source the run changed counts as a change, so earlier test
        // results go stale; it comes before the run so the run still counts.
        let after = self.source_stamps();
        let mut evidence: Vec<Evidence> = after
            .iter()
            .filter(|(file, stamp)| sources.get(*file) != Some(stamp))
            .map(|(file, _)| Evidence::Wrote {
                path: file.clone(),
                syntax_ok: true,
            })
            .collect();
        for file in sources.keys().filter(|f| !after.contains_key(*f)) {
            evidence.push(Evidence::Wrote {
                path: file.clone(),
                syntax_ok: true,
            });
        }
        if !evidence.is_empty() {
            let changed: Vec<String> = evidence
                .iter()
                .filter_map(|e| match e {
                    Evidence::Wrote { path, .. } => Some(path.clone()),
                    _ => None,
                })
                .collect();
            shown += &format!("\nThe run changed {}.", changed.join(", "));
        }
        evidence.push(Evidence::Ran {
            path: path.into(),
            ok: !failed,
            output: text,
        });
        for made in artifacts::changed(&self.root, &before) {
            shown += &format!(
                "\nMade {} ({}, {} bytes{}).",
                made.path,
                made.kind,
                made.bytes,
                match (made.width, made.height) {
                    (Some(w), Some(h)) => format!(", {w} by {h} pixels"),
                    _ => String::new(),
                }
            );
            evidence.push(Evidence::Produced {
                path: made.path,
                format: made.kind,
                bytes: made.bytes,
                width: made.width,
                height: made.height,
            });
        }
        ToolOutcome {
            ok: !failed,
            text: format!(
                "{path} {}.\n{shown}",
                if failed { "ended with an error" } else { "ran" }
            ),
            evidence,
        }
    }

    fn run_tests(&self) -> ToolOutcome {
        let action = format!(
            "import unittest\n    _suite = unittest.TestLoader().discover(_root, pattern='test*.py', top_level_dir=_root)\n    _result = unittest.TextTestRunner(stream=sys.stdout, verbosity=2).run(_suite)\n    print('{TESTS_MARK}', _result.testsRun, len(_result.failures), len(_result.errors))\n"
        );
        let (outcome, timed_out) =
            self.run_script(wrapper(&self.root, self.network, &action, false));
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
                "read_symbol",
                &[("path", ArgKind::Path), ("name", ArgKind::Text)],
                "one function or class of a Python file (or Class.method), with line numbers: less to read than the whole file",
            ),
            ToolSpec::new(
                "write_file",
                &[("path", ArgKind::Path), ("content", ArgKind::Text)],
                "create or replace a file; always send the whole file",
            ),
            ToolSpec::new(
                "edit_file",
                &[
                    ("path", ArgKind::Path),
                    ("old", ArgKind::Text),
                    ("new", ArgKind::Text),
                ],
                "replace the exact lines old with new; old must match once",
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

    /// The project map: each file with its size and, for Python files, its
    /// top-level functions (with their arguments) and classes (with their
    /// methods), from Python's own `ast` (Aider's repository map, small).
    /// Falls back to the file list if Python can't run.
    fn overview(&self) -> String {
        let files = self.files();
        if files.is_empty() || !files.iter().any(|(path, _)| path.ends_with(".py")) {
            return self.list().text;
        }
        let entries: Vec<(String, String, u64)> = files
            .iter()
            .map(|(path, bytes)| {
                (
                    path.clone(),
                    self.root.join(path).display().to_string(),
                    *bytes,
                )
            })
            .collect();
        let script = MAP_SCRIPT.replace(
            "__FILES__",
            &serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into()),
        );
        let (outcome, _) = self.run_script(script);
        let map = self.clean(&outcome.output);
        if !outcome.ok || map.trim().is_empty() {
            return self.list().text;
        }
        super::prompt::cut(map.trim(), MAP_MAX_CHARS)
    }

    fn execute(&self, call: &ToolCall) -> ToolOutcome {
        match call.tool.as_str() {
            "list_files" => self.list(),
            "read_file" => self.read(call.arg("path")),
            "read_symbol" => self.read_symbol(call.arg("path"), call.arg("name")),
            "write_file" => self.write(call.arg("path"), call.arg("content")),
            "edit_file" => self.edit(call.arg("path"), call.arg("old"), call.arg("new")),
            "run_python" => {
                let before = self.snapshot_seen();
                let outcome = self.run_python(call.arg("path"));
                self.refresh_seen(before);
                outcome
            }
            "run_tests" => {
                let before = self.snapshot_seen();
                let outcome = self.run_tests();
                self.refresh_seen(before);
                outcome
            }
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
        let folder = self.root.join(".harness/checkpoints");
        let target = folder.join(name);
        // Copied aside first and published whole, so a checkpoint is never
        // half there. Even an empty project gets its folder.
        // Listed so a folder that can't be read fails the checkpoint
        // instead of leaving it out.
        let mut files = Vec::new();
        list_files(&self.root, &self.root, &mut files)?;
        let partial = folder.join(format!(".{name}.partial"));
        if partial.exists() {
            fs::remove_dir_all(&partial)?;
        }
        fs::create_dir_all(&partial)?;
        for path in files {
            let to = partial.join(&path);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(self.root.join(&path), to)?;
        }
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::rename(&partial, &target)
    }

    fn restore(&self, name: &str) -> std::io::Result<()> {
        let source = self.root.join(".harness/checkpoints").join(name);
        if !source.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("there is no checkpoint {name}"),
            ));
        }
        // Copy the whole checkpoint aside first, one file at a time, so one
        // that can't be read leaves the files as they are, and a large
        // project is never held in memory at once.
        let mut listed = Vec::new();
        list_files(&source, &source, &mut listed)?;
        let staging = self.root.join(".harness/restore.partial");
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        let mut sums = BTreeMap::new();
        let staged = (|| {
            for path in &listed {
                let bytes = fs::read(source.join(path))?;
                let to = staging.join(path);
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(to, &bytes)?;
                sums.insert(path.clone(), digest_bytes(&bytes));
            }
            Ok(())
        })();
        if let Err(err) = staged {
            let _ = fs::remove_dir_all(&staging);
            return Err(err);
        }
        // The files a checkpoint keeps go (never hidden ones, such as the
        // harness's records, at any depth), then the checkpoint's come back.
        let mut live = Vec::new();
        list_files(&self.root, &self.root, &mut live)?;
        for path in live {
            fs::remove_file(self.root.join(&path))?;
        }
        prune_empty(&self.root, true)?;
        for path in &listed {
            let to = self.root.join(path);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(staging.join(path), to)?;
        }
        let _ = fs::remove_dir_all(&staging);
        // Bonsai put these bytes back itself, so they are what it knows of
        // each file: the digests come from the copied bytes, never from a
        // later look at the disk. A known file the checkpoint lacks is gone.
        let mut seen = lock_seen(&self.seen);
        let known: Vec<String> = seen.keys().cloned().collect();
        for path in known {
            let key = path.trim_start_matches("./");
            match sums.get(key) {
                Some(sum) => {
                    seen.insert(path, sum.clone());
                }
                None => {
                    seen.remove(&path);
                }
            }
        }
        Ok(())
    }
}

/// Whether the harness leaves a file or folder alone: hidden ones and
/// Python's caches.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || name == "__pycache__"
}

/// The files `walk` finds, but failing when a folder can't be read.
fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if skipped(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            list_files(root, &path, out)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            out.push(relative.to_string_lossy().to_string());
        }
    }
    Ok(())
}

/// Removes the folders under `dir` left empty, skipping hidden ones and
/// Python's caches; `dir` itself stays when `top`.
fn prune_empty(dir: &Path, top: bool) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && !skipped(&entry.file_name().to_string_lossy()) {
            prune_empty(&entry.path(), false)?;
        }
    }
    if !top && fs::read_dir(dir)?.next().is_none() {
        fs::remove_dir(dir)?;
    }
    Ok(())
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if skipped(&entry.file_name().to_string_lossy()) {
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

/// Replaces the lines of `old` in `text` when they match apart from spaces
/// at line ends, and only once.
fn replace_lines_loosely(text: &str, old: &str, new: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let want: Vec<&str> = old.lines().map(str::trim_end).collect();
    if want.is_empty() || want.len() > lines.len() {
        return None;
    }
    let found: Vec<usize> = (0..=lines.len() - want.len())
        .filter(|&i| {
            lines[i..i + want.len()]
                .iter()
                .zip(&want)
                .all(|(a, b)| a.trim_end() == *b)
        })
        .collect();
    let [at] = found[..] else { return None };
    let mut out: Vec<String> = lines[..at].iter().map(|l| l.to_string()).collect();
    out.extend(new.lines().map(str::to_string));
    out.extend(lines[at + want.len()..].iter().map(|l| l.to_string()));
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    Some(joined)
}

/// A line as a small model might copy it: no indent or trailing space,
/// and each string written with single quotes. Only the delimiters change:
/// a string with a quote or backslash inside keeps its own, so `"Bob's"`
/// never matches `'Bob"s'`.
fn forgiving(line: &str) -> String {
    let chars: Vec<char> = line.trim().chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let quote = chars[i];
        if quote != '"' && quote != '\'' {
            out.push(quote);
            i += 1;
            continue;
        }
        // The string's end: the next unescaped quote of the same kind.
        let mut j = i + 1;
        while j < chars.len() && chars[j] != quote {
            j += if chars[j] == '\\' { 2 } else { 1 };
        }
        let end = j.min(chars.len());
        let inner: String = chars[i + 1..end].iter().collect();
        let plain = !inner.contains(['"', '\'', '\\']);
        let shown = if plain { '\'' } else { quote };
        out.push(shown);
        out.push_str(&inner);
        if j < chars.len() {
            out.push(shown);
        }
        i = j + 1;
    }
    out
}

/// Each line as `forgiving` shows it, except lines in or around a
/// triple-quoted string, whose quotes are data: those only lose their
/// indent and trailing space.
fn keys(lines: &[&str]) -> Vec<String> {
    let mut open: Option<&str> = None;
    lines
        .iter()
        .map(|line| {
            if in_triple_string(line, &mut open) {
                line.trim().to_string()
            } else {
                forgiving(line)
            }
        })
        .collect()
}

/// Whether `line` is in or touches a string that spans lines, given the one
/// open before it (`open`, updated for the next line). A small tokenizer:
/// a `#` outside a string ends the line, and short strings that close on
/// their line are skipped, so quotes in a comment or a short string don't
/// count. Strings that span lines are triple-quoted ones and short ones
/// continued with a backslash. It compares bytes, so non-ASCII text can't
/// split a cut.
fn in_triple_string(line: &str, open: &mut Option<&'static str>) -> bool {
    let bytes = line.as_bytes();
    let mut touched = open.is_some();
    let mut i = 0;
    loop {
        if let Some(quote) = *open {
            // Inside a string: a backslash escapes the next character (in
            // raw strings too), so `\"""` doesn't close it.
            while i < bytes.len() && open.is_some() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i..].starts_with(quote.as_bytes()) {
                    i += quote.len();
                    *open = None;
                } else {
                    i += 1;
                }
            }
            if open.is_some() {
                // A short string goes on to the next line only after a
                // backslash at the end of this one (`i` then steps past it).
                if quote.len() == 1 && i != bytes.len() + 1 {
                    *open = None;
                } else {
                    touched = true;
                }
                return touched;
            }
        }
        if i >= bytes.len() || bytes[i] == b'#' {
            return touched;
        }
        let quote = match bytes[i] {
            b'"' if bytes[i..].starts_with(b"\"\"\"") => "\"\"\"",
            b'\'' if bytes[i..].starts_with(b"'''") => "'''",
            b'"' => "\"",
            b'\'' => "'",
            _ => {
                i += 1;
                continue;
            }
        };
        if quote.len() == 3 {
            touched = true;
        }
        *open = Some(quote);
        i += quote.len();
    }
}

/// `old` matched against `text` ignoring indentation and quote style. When
/// exactly one run of lines fits, it is replaced by `new`, shifted so its
/// first line takes the indent of the line it replaces. Returns the edited
/// text and the (1-based) first line replaced.
fn replace_lines_forgivingly(text: &str, old: &str, new: &str) -> Option<(String, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let olds: Vec<&str> = old.lines().collect();
    let want = keys(&olds);
    if want.is_empty() || want.len() > lines.len() || want.iter().all(String::is_empty) {
        return None;
    }
    let have = keys(&lines);
    let found: Vec<usize> = (0..=lines.len() - want.len())
        .filter(|&i| have[i..i + want.len()] == want[..])
        .collect();
    let [at] = found[..] else { return None };
    // Indents are runs of ASCII spaces and tabs, so every cut below falls on
    // a character boundary. Depths compare in columns (a tab to the next
    // multiple of 8, as Python counts), and the file's own indent (tabs
    // included) is what the new lines get.
    let indent = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
    // Tab-indented code is only re-indented when the edit uses tabs too:
    // mixing tabs and spaces makes Python refuse the file.
    let tabbed = |line: &str| line[..indent(line)].contains('\t');
    let file_tabs = lines[at..at + want.len()].iter().any(|l| tabbed(l));
    if file_tabs != new.lines().any(tabbed) && new.lines().count() > 1 {
        return None;
    }
    let column = |ws: &str| {
        ws.chars().fold(
            0usize,
            |col, c| {
                if c == '\t' {
                    col / 8 * 8 + 8
                } else {
                    col + 1
                }
            },
        )
    };
    let first_old = old.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let first_file = lines[at..at + want.len()]
        .iter()
        .find(|l| !l.trim().is_empty())
        .copied()
        .unwrap_or("");
    let prefix = &first_file[..indent(first_file)];
    let old_width = column(&first_old[..indent(first_old)]);
    let shifted: Option<Vec<String>> = new
        .lines()
        .map(|line| {
            let own = &line[..indent(line)];
            let rest = &line[own.len()..];
            if rest.is_empty() {
                return Some(String::new());
            }
            if column(own) >= old_width {
                // As deep as the first old line or deeper: the file's indent,
                // then whatever this line has beyond the old line's depth.
                let skip = (0..=own.len())
                    .find(|&n| column(&own[..n]) >= old_width)
                    .unwrap_or(own.len());
                let deeper = &own[skip..];
                // A tab after a file indent that stops between tab stops lands
                // on the next stop, not the columns the edit asked for: no guess.
                if deeper.contains('\t') && column(prefix) % 8 != 0 {
                    return None;
                }
                Some(format!("{prefix}{deeper}{rest}"))
            } else {
                // Shallower: that many columns less of the file's indent.
                let want = column(prefix).saturating_sub(old_width - column(own));
                let keep = (0..=prefix.len())
                    .rev()
                    .find(|&n| column(&prefix[..n]) <= want)
                    .unwrap_or(0);
                Some(format!("{}{rest}", &prefix[..keep]))
            }
        })
        .collect();
    let mut out: Vec<String> = lines[..at].iter().map(|l| l.to_string()).collect();
    out.extend(shifted?);
    out.extend(lines[at + want.len()..].iter().map(|l| l.to_string()));
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    Some((joined, at + 1))
}

/// The run of lines in `text` most like `old`, numbered, when it is close
/// enough to be worth showing.
fn similar_lines(text: &str, old: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let want: Vec<&str> = old.lines().collect();
    if lines.is_empty() || want.is_empty() {
        return None;
    }
    let size = want.len().min(lines.len());
    let (best, score) = (0..=lines.len() - size)
        .map(|i| {
            let score: f64 = lines[i..i + size]
                .iter()
                .zip(&want)
                .map(|(a, b)| similarity(a.trim(), b.trim()))
                .sum::<f64>()
                / size as f64;
            (i, score)
        })
        .fold(
            (0, -1.0),
            |best, next| if next.1 > best.1 { next } else { best },
        );
    if score < 0.5 {
        return None;
    }
    Some(
        lines[best..best + size]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>3}| {line}", best + i + 1))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 0 to 1: twice the longest common subsequence over the total length.
fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut row = vec![0usize; b.len() + 1];
    for x in &a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                row[j + 1].max(row[j])
            };
            diagonal = above;
        }
    }
    2.0 * row[b.len()] as f64 / (a.len() + b.len()) as f64
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

/// The project map's longest length, in characters.
const MAP_MAX_CHARS: usize = 1_500;

/// What starts the line `SYMBOLS_SCRIPT` prints its JSON on.
const SYMBOLS_MARK: &str = "__harness_symbols__";

/// Each top-level function and class of a file, and each method as
/// Class.method, with its first line (decorators included) and last line.
const SYMBOLS_SCRIPT: &str = r#"
import ast, json
def _harness_symbols(source):
    tree = ast.parse(source)
    defs = (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)
    def span(node):
        return [min([d.lineno for d in node.decorator_list] + [node.lineno]), node.end_lineno]
    out = {}
    for node in tree.body:
        if isinstance(node, defs):
            out[node.name] = span(node)
            if isinstance(node, ast.ClassDef):
                for inner in node.body:
                    if isinstance(inner, defs):
                        out[node.name + '.' + inner.name] = span(inner)
    print('__harness_symbols__' + json.dumps(out))
_harness_symbols(__SOURCE__)
del _harness_symbols
"#;

/// Prints one line per file: `path (bytes)` and its definitions.
const MAP_SCRIPT: &str = r#"
import ast
def _harness_map(files):
    for shown, full, size in files:
        line = f'{shown} ({size} bytes)'
        if shown.endswith('.py'):
            try:
                with open(full, encoding='utf-8') as f:
                    tree = ast.parse(f.read())
            except SyntaxError as e:
                print(f'{line}: Python cannot parse it (line {e.lineno})')
                continue
            except Exception:
                print(line)
                continue
            found = []
            for node in tree.body:
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    found.append(f'def {node.name}({ast.unparse(node.args)})')
                elif isinstance(node, ast.ClassDef):
                    methods = [n.name for n in node.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]
                    found.append(f'class {node.name}' + (f' ({", ".join(methods)})' if methods else ''))
            if found:
                line += ': ' + '; '.join(found)
        print(line)
_harness_map(__FILES__)
del _harness_map
"#;

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
        elif event in ('os.symlink', 'os.link'):
            raise PermissionError('Bonsai Harness: making links is not allowed')
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
///
/// `console` is Python Lab's live console (PL1d): stdin is left as the
/// host set it (PythonBridge's reader, so `input()` waits for the person),
/// and a failure's traceback, cut to the project's frames and with the
/// project folder taken out of its paths, goes to stderr as it happens.
/// The exception is then raised again, marked `_devicelab_shown`, so the
/// host knows the run failed without printing it twice. Otherwise stdin
/// is empty and the traceback is printed to stdout with the failure mark,
/// for `python_lab_output`.
fn wrapper(root: &Path, network: bool, action: &str, console: bool) -> String {
    let failed = if console {
        "    _text = ''.join(traceback.format_list(_frames)) + ''.join(traceback.format_exception_only(type(_error), _error))\n    for _prefix in (_root, _given.rstrip(os.sep)):\n        _text = _text.replace(_prefix + os.sep, '')\n    sys.stderr.write('Traceback (most recent call last):\\n' + _text)\n    try:\n        _error._devicelab_shown = True\n    except Exception:\n        pass\n    raise".to_string()
    } else {
        format!("    print('Traceback (most recent call last):')\n    print(''.join(traceback.format_list(_frames)), end='')\n    print(''.join(traceback.format_exception_only(type(_error), _error)), end='')\n    print('{FAILED_MARK}')")
    };
    let stdin = if console {
        "pass"
    } else {
        "sys.stdin = io.StringIO('')"
    };
    format!(
        r#"{GUARD}
import io, traceback
_given = {root}
_root = os.path.realpath(_given)
_saved = (os.getcwd(), list(sys.path), sys.stdin)
# Forget every module a run imported from a project, this one's or
# another's, so `import helper` never finds another project's helper.
_ran = _guard.__dict__.setdefault('project_modules', set())
for _name, _module in list(sys.modules.items()):
    _file = getattr(_module, '__file__', None) or ''
    if _name in _ran or (_file and os.path.realpath(_file).startswith(_root + os.sep)):
        sys.modules.pop(_name, None)
_ran.clear()
os.chdir(_root)
sys.path.insert(0, _root)
{stdin}
_guard.state.update(active=True, roots=[_root, os.path.realpath(tempfile.gettempdir())], network={network})
try:
    {action}
except BaseException as _error:
    _frames = [f for f in traceback.extract_tb(_error.__traceback__) if f.filename.startswith((_root, _given))]
{failed}
finally:
    _guard.state['active'] = False
    for _name, _module in list(sys.modules.items()):
        _file = getattr(_module, '__file__', None) or ''
        if _file and os.path.realpath(_file).startswith(_root + os.sep):
            _ran.add(_name)
    os.chdir(_saved[0])
    sys.path[:] = _saved[1]
    sys.stdin = _saved[2]
"#,
        root = py_str(&root.display().to_string()),
        network = if network { "True" } else { "False" },
    )
}

/// A script Python Lab runs for the person, in the project folder
/// `project`: the harness's wrapper (the folder, `sys.path`, the project's
/// modules forgotten, tracebacks cut to the project's frames, writes kept
/// inside the project) with the network allowed, since the person wrote
/// the code. If the last line is an expression its value is printed, as
/// before. Run it with PythonBridge, then pass the output through
/// `python_lab_output`.
#[uniffi::export]
pub fn python_lab_script(project: String, code: String) -> String {
    let action = format!(
        "import ast, linecache\n    _source = {source}\n    _name = os.path.join(_root, '<script>')\n    linecache.cache[_name] = (len(_source), None, _source.splitlines(True), _name)\n    _tree = ast.parse(_source, _name)\n    _last = ast.Expression(_tree.body.pop().value) if _tree.body and isinstance(_tree.body[-1], ast.Expr) else None\n    _scope = {{'__name__': '__main__', '__file__': _name}}\n    exec(compile(_tree, _name, 'exec'), _scope)\n    if _last is not None:\n        _value = eval(compile(_last, _name, 'eval'), _scope)\n        if _value is not None:\n            print(repr(_value))\n",
        source = py_str(&code)
    );
    wrapper(Path::new(&project), true, &action, false)
}

/// A script that runs the project's file `path` for the person, as
/// `__main__`, with the same wrapper as [`python_lab_script`]. Tracebacks
/// name the file and line (`main.py, line 3`) after `python_lab_output`,
/// other files of the project import normally, and if the file ends with
/// an expression its value is printed. `path` is checked like a harness
/// path is, but may contain spaces and any letters (the person named it).
#[uniffi::export]
pub fn python_lab_file_script(project: String, path: String) -> String {
    file_script(&project, &path, false, None)
}

/// [`python_lab_file_script`] for Python Lab's live console (PL1d): run it
/// with PythonBridge's streaming runner, which gives it a stdin that waits
/// for the person. Its output is final as it streams: tracebacks already
/// name `main.py, line 3`, on stderr, so it needs no `python_lab_output`.
#[uniffi::export]
pub fn python_lab_console_script(project: String, path: String) -> String {
    file_script(&project, &path, true, None)
}

/// Python Lab's Debug (PL2f): `python_lab_console_script` run under a
/// debugger (`bdb`) that pauses on `breakpoints`. Each pause writes the
/// stack and variables as JSON to the console stream with kind 6
/// (`python_debug_pause` reads it), then waits on the console's input for
/// a command: `continue`, `step` (into a call), `next` (over it) or `out`
/// (to the caller). Only the project's own files pause; stepping goes
/// over the standard library and packages.
#[uniffi::export]
pub fn python_lab_debug_script(
    project: String,
    path: String,
    breakpoints: Vec<crate::workspace::DebugBreakpoint>,
) -> String {
    file_script(&project, &path, true, Some(&breakpoints))
}

/// The debugger `python_lab_debug_script` runs a file under, indented to
/// sit in the wrapper's `try`. `_lab_debugger(root, [(file, line)])`.
const DEBUGGER: &str = r#"import bdb, json, reprlib, _devicelab_io
class _LabDebugger(bdb.Bdb):
    def __init__(self, root, breakpoints):
        # The interpreter is shared: forget breakpoints an earlier
        # session left behind, which Bdb.__init__ would otherwise load.
        bdb.Breakpoint.next = 1
        bdb.Breakpoint.bplist = {}
        bdb.Breakpoint.bpbynumber = [None]
        bdb.Bdb.__init__(self)
        self._lab_root = root
        self._lab_first = True
        self._lab_repr = reprlib.Repr()
        self._lab_repr.maxstring = 160
        self._lab_repr.maxother = 160
        for _kind in ('maxlist', 'maxtuple', 'maxdict', 'maxset', 'maxfrozenset', 'maxdeque'):
            setattr(self._lab_repr, _kind, 20)
        for _file, _line in breakpoints:
            self.set_break(os.path.join(root, _file), _line)
    def reset(self):
        # Each run (the file, then its last expression) starts by running
        # on to the first breakpoint.
        bdb.Bdb.reset(self)
        self._lab_first = True
    def _lab_mine(self, frame):
        return self.canonic(frame.f_code.co_filename).startswith(self._lab_root + os.sep)
    def stop_here(self, frame):
        return self._lab_mine(frame) and bdb.Bdb.stop_here(self, frame)
    def user_call(self, frame, args):
        pass
    def user_return(self, frame, value):
        pass
    def user_exception(self, frame, info):
        pass
    def user_line(self, frame):
        if self._lab_first:
            # bdb stops on the first line; run on unless it has a breakpoint.
            self._lab_first = False
            if not self.get_breaks(self.canonic(frame.f_code.co_filename), frame.f_lineno):
                self.set_continue()
                return
        self._lab_pause(frame)
    def _lab_pause(self, frame):
        stack = []
        f = frame
        while f is not None:
            if self._lab_mine(f):
                stack.append({'file': os.path.relpath(self.canonic(f.f_code.co_filename), self._lab_root), 'line': f.f_lineno, 'function': f.f_code.co_name})
            f = f.f_back
        stack.reverse()
        names = [n for n in frame.f_locals if not (n.startswith('__') and n.endswith('__'))]
        variables = []
        for n in names:
            v = frame.f_locals[n]
            if isinstance(v, (types.ModuleType, types.FunctionType, type)):
                continue
            if len(variables) == 100:
                break
            try:
                text = self._lab_repr.repr(v)
            except Exception:
                text = "(can't be shown)"
            variables.append({'name': n, 'kind': type(v).__name__, 'value': text})
        _devicelab_io.write(6, json.dumps({'stack': stack, 'variables': variables, 'more': len(variables) == 100}))
        command = _devicelab_io.readline().strip()
        if command == 'step':
            self.set_step()
        elif command == 'next':
            self.set_next(frame)
        elif command == 'out':
            self.set_return(frame)
        else:
            self.set_continue()
"#;

fn file_script(
    project: &str,
    path: &str,
    console: bool,
    debug: Option<&[crate::workspace::DebugBreakpoint]>,
) -> String {
    let (debugger, exec) = match debug {
        Some(breakpoints) => {
            let list = breakpoints
                .iter()
                .map(|b| format!("({}, {})", py_str(&b.file), b.line))
                .collect::<Vec<_>>()
                .join(", ");
            (
                DEBUGGER.replace('\n', "\n    "),
                format!("_lab = _LabDebugger(_root, [{list}])\n        try:\n            _lab.run(compile(_tree, _name, 'exec'), _main.__dict__)\n            _value = _lab.runeval(compile(_last, _name, 'eval'), _main.__dict__) if _last is not None else None\n        finally:\n            _lab.clear_all_breaks()"),
            )
        }
        None => (
            String::new(),
            "exec(compile(_tree, _name, 'exec'), _main.__dict__)\n        _value = eval(compile(_last, _name, 'eval'), _main.__dict__) if _last is not None else None".to_string(),
        ),
    };
    let action = format!(
        "{debugger}import ast, types\n    _name = os.path.join(_root, {path})\n    if not os.path.realpath(_name).startswith(_root + os.sep):\n        raise PermissionError('That file is outside the project.')\n    with open(_name, encoding='utf-8') as _f:\n        _source = _f.read()\n    _tree = ast.parse(_source, _name)\n    _last = ast.Expression(_tree.body.pop().value) if _tree.body and isinstance(_tree.body[-1], ast.Expr) else None\n    _main = types.ModuleType('__main__')\n    _main.__file__ = _name\n    _main.__builtins__ = __builtins__\n    sys.path[0] = os.path.dirname(_name)\n    if sys.path[0] != _root:\n        sys.path.insert(1, _root)\n    _host_main = sys.modules.get('__main__')\n    sys.modules['__main__'] = _main\n    try:\n        {exec}\n        if _value is not None:\n            print(repr(_value))\n    finally:\n        if _host_main is None:\n            sys.modules.pop('__main__', None)\n        else:\n            sys.modules['__main__'] = _host_main\n",
        path = py_str(path)
    );
    wrapper(Path::new(project), true, &action, console)
}

/// What a `python_lab_script` run printed, made readable: whether it
/// succeeded, and the output with the project folder's paths removed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PythonLabOutput {
    pub ok: bool,
    pub output: String,
}

#[uniffi::export]
pub fn python_lab_output(project: String, ok: bool, output: String) -> PythonLabOutput {
    let root = PathBuf::from(&project);
    let mut text = output.clone();
    for prefix in [Some(root.clone()), fs::canonicalize(&root).ok()]
        .into_iter()
        .flatten()
    {
        text = text.replace(&format!("{}/", prefix.display()), "");
    }
    let text = text
        .lines()
        .filter(|l| !l.starts_with(FAILED_MARK))
        .collect::<Vec<_>>()
        .join("\n");
    PythonLabOutput {
        ok: ok && !output.contains(FAILED_MARK),
        output: text,
    }
}

/// Small models keep putting double quotes inside a double-quoted string
/// (SVG and HTML attributes), and read the parser's message as a puzzle
/// for several steps. Name the usual cause.
fn quote_hint(problem: &str) -> &'static str {
    if problem.contains("unterminated string literal")
        || problem.contains("unterminated f-string literal")
    {
        "\nIf that line puts double quotes inside a double-quoted string, wrap the \
         string in single quotes instead: f'<rect width=\"{w}\"/>'."
    } else {
        ""
    }
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

/// Where Debug paused and what it sees (PL2f), from the JSON line
/// `python_lab_debug_script` writes.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DebugPause {
    /// The project's frames, outermost first; the last is where it paused.
    pub stack: Vec<TracebackFrame>,
    /// The paused frame's variables (not modules, functions or classes),
    /// each shown as `repr` cut short.
    pub variables: Vec<DebugVariable>,
    /// There were more than the 100 shown.
    pub more_variables: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DebugVariable {
    pub name: String,
    /// Its type's name, like `int` or `list`.
    pub kind: String,
    pub value: String,
}

/// Reads a pause the debugger wrote; none if it isn't one.
#[uniffi::export]
pub fn python_debug_pause(json: String) -> Option<DebugPause> {
    #[derive(serde::Deserialize)]
    struct Frame {
        file: String,
        line: u32,
        function: String,
    }
    #[derive(serde::Deserialize)]
    struct Variable {
        name: String,
        kind: String,
        value: String,
    }
    #[derive(serde::Deserialize)]
    struct Pause {
        stack: Vec<Frame>,
        variables: Vec<Variable>,
        #[serde(default)]
        more: bool,
    }
    let pause: Pause = serde_json::from_str(&json).ok()?;
    if pause.stack.is_empty() {
        return None;
    }
    Some(DebugPause {
        stack: pause
            .stack
            .into_iter()
            .map(|f| TracebackFrame {
                file: f.file,
                line: f.line,
                function: f.function,
            })
            .collect(),
        variables: pause
            .variables
            .into_iter()
            .map(|v| DebugVariable {
                name: v.name,
                kind: v.kind,
                value: v.value,
            })
            .collect(),
        more_variables: pause.more,
    })
}

/// A line of a traceback that names a file of the project, for Python Lab
/// to open there (PL2b).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TracebackFrame {
    /// Relative to the project, like `lib/helper.py`.
    pub file: String,
    pub line: u32,
    /// The function, or `<module>`.
    pub function: String,
}

/// The project's frames in the last traceback of a console run's output,
/// outermost first, so the last is where the error happened. Frames of
/// other files (the standard library, a package) are left out: only the
/// project's files can be opened. A console traceback already names them
/// relative to the project (`python_lab_console_script`).
#[uniffi::export]
pub fn python_traceback_frames(output: String) -> Vec<TracebackFrame> {
    let Some(start) = output.rfind("Traceback (most recent call last):") else {
        return Vec::new();
    };
    output[start..]
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("File \"")?;
            let (file, rest) = rest.split_once('"')?;
            let rest = rest.strip_prefix(", line ")?;
            let (number, function) = match rest.split_once(", in ") {
                Some((number, function)) => (number, function.trim()),
                None => (rest, ""),
            };
            let line: u32 = number.trim().parse().ok()?;
            // Only a project file: relative, not `<string>` or a full path.
            if file.starts_with('/') || file.starts_with('<') || file.split('/').any(|p| p == "..")
            {
                return None;
            }
            Some(TracebackFrame {
                file: file.to_string(),
                line,
                function: function.to_string(),
            })
        })
        .collect()
}
