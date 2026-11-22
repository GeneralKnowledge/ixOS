# Python Lab: from script runner to workspace

Status: **plan agreed in outline; PL1a, PL1b and PL1c built** (Reuben, 2026-11-22). Their Rust is tested on Linux; their Swift is not compiled yet and nothing has run on the phone. Reuben's review of Python Lab found that it runs Python well but is still a script runner, not a place to work. This plan covers his first stage, **PL1 Workspace and interactive development**, in five small PRs. It checks his reading of the code first, and lists his later stages so that none of them is started early.

It builds on [visual-labs-spec.md](visual-labs-spec.md) (graphics) and [bonsai-harness-plan.md](bonsai-harness-plan.md) (Bonsai). It changes neither of their decisions.

## Names

The visual-labs spec already has a **P1** (projects and image previews: built, on the phone only "images worked" recorded so far) and a **P2** (Matplotlib). To keep those two names from meaning two different things, Reuben's four stages are called **PL1 to PL4** here:

| Stage | What | Includes |
|---|---|---|
| **PL1** | Workspace and interactive development (this plan) | Projects and files, autosave and reopen, a basic code editor, an interactive console with `input()`, Bonsai on the same projects |
| PL2 | Comfortable Python | Better editor and keyboard (bracket matching, several open files), structured tracebacks and debugging, run history, a general artifact browser, pure-Python packages |
| PL3 | Visual and data | The spec's **P2 Matplotlib** (behind its spike S-P2), NumPy and pandas where they can be built, CSV and JSON inspection, notebook-style cells, inline plots |
| PL4 | Advanced | Per-project environments, a test runner, Git, better isolation (see the correction below), profiling |

## Checking Reuben's reading of the code (main at 532c042)

Most of the review is right. Where it isn't, or where the code says a bit more, this table says so.

| Reuben said | The code | Verdict |
|---|---|---|
| CPython 3.15 runs on the phone, the standard library is bundled, HTTPS works, P1 graphics are in place | `prepare-python.sh`, `PythonBridge.c`, the bundled certifi file and `SSL_CERT_FILE`. The phone results are in the Bonsai plan's table: 22 of 22 modules, the HTTPS recheck and "images worked" on 1.29921350 | Right. One detail: the only P1 result recorded is "images worked". P1's other acceptance checks (an SVG containing `<script>` and an `https://` link shows without running or loading either, a rerun refreshes the thumbnail, a C-level loop that ignores Stop, ladder task T-P1 on the phone) aren't recorded. PL1's phone sitting picks them up |
| Manual runs use one fixed `scratch` project shared with Bonsai, with no picker, no create or rename, and no file browser | `PythonLabModel.live()` hard-codes `Projects/scratch` | Right. **And** the same `Projects` folder holds Bonsai's ladder projects (`t1-hello`, `t2-one-test`, ...), which `PythonLab::reset` **deletes and reseeds on every ladder run**. A picker that simply listed the folder would offer the person projects that vanish. PL1a moves them (below) |
| The editor holds code in the model and runs it as a temporary `<script>`, never saved as a `.py` | `python_lab_script` puts the source in `linecache` under the name `<project>/<script>`; nothing is written to disk | Right, and it is worse than that: `PythonLabModel.init` sets the code to the Hello sample, so anything typed is lost every time the app restarts |
| `input()` gets an empty stream | The wrapper sets `sys.stdin = io.StringIO('')`; the test `input_gets_an_empty_stdin_instead_of_waiting` pins it | Right: `input()` raises `EOFError` at once. That is deliberate for Bonsai (its prompt says "no input()") and must stay so for harness runs |
| Output appears as one block after the run; no separate stdout and stderr | `_devicelab_run` sends both `sys.stdout` and `sys.stderr` to one `StringIO` and returns it when the script ends | Right |
| TextEditor is only a starting point | A SwiftUI `TextEditor` with autocorrection and capitals off | Right, and one gap is missing from the list: **iOS smart punctuation is still on**. Typing `"` or `'` can give `“` or `’` and `--` can give `—`, which Python rejects as a syntax error unless the keyboard setting is off. `TextEditor` can't turn it off; a `UITextView` can (`smartQuotesType = .no` and so on). PL1c fixes it |
| Structured tracebacks are in the spec but not on screen | The wrapper cuts frames to the project's own and prints text; the spec lists "tracebacks as data" for the screen and the harness | Right. They stay in PL2, with debugging |
| There is a `site-packages` folder but no package manager | `PythonRuntime.start` adds `Application Support/Python/site-packages` with `site.addsitedir` | Right, and stronger: `prepare-python.sh` **deletes `ensurepip`**, so there is no pip in the app at all, not just no screen for it. Pure-Python packages in PL2 won't be "pip on the phone": a wheel is a zip file, so Python Lab can download a pinned pure-Python wheel, check its hash and unpack it with `zipfile`. One shared `site-packages` for every project also rules out per-project versions until PL4 |
| Stop is a `KeyboardInterrupt` and can't stop C code that ignores it | `pybridge_interrupt` uses `PyThreadState_SetAsyncExc`; the spec records the gap | Right, and it is permanent for an in-process interpreter |
| Python runs in-process, not in a separate sandboxed process | One interpreter for the app, one script at a time, Python Lab and Bonsai taking turns on one queue | Right about today. **PL4's "separate process" isn't available, though:** an iOS app can't start processes (no `fork`, `exec` or `posix_spawn`; the guard refuses them anyway). The routes that exist are CPython's sub-interpreters (3.14's `concurrent.interpreters`, one GIL each), which would give separate module state and let Python Lab and Bonsai run at once but still can't stop C code, and the WebAssembly route from the spec (Terrarium), the only one that can be killed, at 3 to 7 times slower. PL4 picks between them; neither is in PL1 |
| Bonsai must keep using the same projects | `BonsaiHarness.start_custom` always works in `scratch`; ladder tasks get their own folders | Agreed. Note that the Bonsai plan's **H7** already promised "projects listed in Python Lab; browse, edit and run their files yourself, and the harness sees your edits". PL1 builds that half of H7 early, so H7 shrinks to the per-project network switch and its ladder task T12 |

Two more facts that shape the design:

- Manual runs already go through the harness's guard with the network allowed, so a script can only write inside its own project and the temporary folder. That stays; it is what makes "a project" a real boundary.
- Python Lab and Bonsai share one interpreter. Once `input()` can wait for the person, a Python Lab script waiting for an answer holds the interpreter, and a Bonsai run would queue behind it. PL1d makes that visible on both screens instead of letting Bonsai look stuck.

## What PL1 builds

One rule throughout: **Rust owns the workspace** (projects, files, paths, limits, the wrapper), as it already owns the harness's projects, so it is tested on Linux. Swift draws it and hosts the interpreter.

### PL1a. Projects and files (Rust, one PR) — built

Built as `rust/src/workspace.rs` (the `Workspace` object and `workspace_move_projects`), with 14 Linux tests. Names may use any letters, digits and spaces (no `/`, `\`, `:`, control characters or leading dot); Bonsai's own tools still reach only plain ASCII names, so a file called `my data.csv` is the person's, which Bonsai can see listed but not open by name. Nothing on screen uses it until PL1b.

A `workspace` module in the Device Lab crate (beside `harness/`), exported through UniFFI:

- **Projects:** list (name, file count, last change), create, rename, delete. `scratch` always exists and can't be deleted or renamed (Bonsai's default). Names are checked like file names (no `/`, no leading `.`, at most 60 characters, refused with a reason rather than cut).
- **Files:** a folder tree; create a file or folder, rename or move, delete, read, write. Writes are atomic (a temporary file then a rename). Paths go through the harness's `check_path` and its symbolic-link check, so a path can't leave the project. Limits for the person: 1 MB a text file and 500 files a project, refused with the limit shown; the harness keeps its own smaller limits for Bonsai.
- **Ladder projects move** to `Projects/.ladder/<task>`: still the harness's to wipe and reseed, hidden from the list (as `.runs` already is). This is the one change to the harness's layout; the Bonsai thread is told before it lands.
- **Project state** in `<project>/.workspace/state.json`: the file to run, the files that were open and the cursor in each. Hidden folders are already skipped by the artifact list and the harness's file list.
- Tests (Linux): every operation, refusals (escaping paths, links, names, limits), `scratch` protection, and the ladder move.

### PL1b. The project browser and autosave (Swift, one PR) — built

Built: `PythonLabModel` over `Workspace` (projects, files, autosave a second after typing stops and on leaving or backgrounding, the open file remembered per project and the last project between launches), `PythonProjectsView` and `PythonFilesView` (ForgeUI's `forgeRename` for naming, `forgeRowActions` for rename, delete, Run This File, New File Here), and the Rust `python_lab_file_script(project, path)` with two Linux tests (a file imports its neighbours from its own folder and the project's; a traceback names `main.py, line 3` and `helper.py, line 2`; an edited module is picked up on the next run).

It also fixes a fault the #367 review found: Python Lab's Stop and time limit called a global interrupt, so they could stop a Bonsai script that was running while Python Lab's waited for its turn, and the wait counted against Python Lab's limit. `PythonRuntime` now gives every run an id, interrupts only the run that asked, and starts Python Lab's limit when its script actually starts. Bonsai's harness keeps its own limit in Rust, which still counts its wait; a Stop or limit that comes while a run waits for its turn is kept, and that run ends as soon as its turn comes instead of running without a limit (Python Lab's Stop on a waiting run works the same way). The wait matters more once PL1d lets a script wait for `input()`.

- Python Lab opens on the **last project and file** it had open. A project list (create, rename, delete with a confirmation naming the file count) and a file browser for the project (folders, files with sizes, images opening in the existing preview, swipe to rename or delete, a + menu for a new file or folder).
- The editor edits **a real file**. Autosave a second after typing stops, and when the screen closes or the app goes to the background, so nothing typed is lost; a small "Saved" or "Not saved: reason" line shows which.
- Run runs **the open file** from disk: the existing wrapper parses it with its real file name and runs it in a fresh module registered as `__main__` for the run (so `__name__ == "__main__"`, and `import __main__` from another file finds it), with the file's folder and the project's top folder on the import path, so a traceback names `main.py, line 3` and one file can import another. A project can mark a different file as the one Run starts; by default it runs the file you're looking at. The last-expression echo stays for a quick check. Run first saves what is on screen (again after Python finishes starting, for edits made meanwhile); if the save fails, the reason shows and nothing runs, so Run never runs the older copy on disk. Opening another file or project also saves first, and stays put if the save fails, so the only copy of unsaved edits is never replaced.
- Examples become "New file from example", which adds the sample as a file to the open project instead of replacing what you wrote.
- **Migration:** whatever is in `scratch` stays. Nothing else exists to migrate, since manual code was never saved.

### PL1c. A basic code editor (Swift view, Rust for the colours, one PR) — built

A `UITextView` in a `UIViewRepresentable` (a view file, so UIKit is allowed there) instead of `TextEditor`:

- Smart quotes, dashes and insert-delete off; autocorrection, spelling and capitals off; monospaced at the person's text size.
- Line numbers in a gutter.
- Return keeps the line's indentation and adds four spaces after a line ending in `:`; Tab inserts four spaces; Backspace at an indent removes four.
- A key row above the keyboard: Tab, `:`, `(`, `)`, `[`, `]`, `{`, `}`, `"`, `'`, `#`, `=`, `_`, and undo and redo. With an external keyboard, Tab and Shift-Tab indent and outdent the selection, and Command-F finds.
- Find and replace through iOS's own find bar (`UIFindInteraction`, `isFindInteractionEnabled`), not our own.
- **Syntax colours** from a small Python tokenizer in Rust (keywords, strings including triple-quoted and f-strings, comments, numbers, `def` and `class` names, decorators), returning ranges that Swift colours with ForgeUI tones. Tested on Linux against Python's own `tokenize` on the samples. Re-coloured after typing pauses, for the whole file under 1 MB.

Bracket matching and several open files are PL2.

As built: the editor is `Sources/AppHost/PythonCodeEditor.swift` (TextKit 1, so the gutter can read line fragments) and the tokenizer `rust/src/python_highlight.rs` (`python_highlight(source) -> [HighlightSpan]`, ranges in UTF-16 units so Swift uses them as they are). Colours: keywords accent, strings success, numbers info, decorators warning, comments secondary text, `def` and `class` names semibold. Soft keywords (`match`, `case`, `type`) stay plain, since telling them from names needs a parser. Backspace in the indentation removes back to the last four-space stop. Bonsai Harness can reuse `python_highlight` for its live code view.

### PL1d. The interactive console (C bridge, Rust wrapper, Swift, one PR) — built

- **Streaming:** `PythonBridge` gets a small built-in module whose `write(stream, text)` calls back into Swift, and Python Lab's runs point `sys.stdout` and `sys.stderr` at it, so output appears while the script runs. Swift batches it (at most every 50 ms) so a tight `print` loop can't flood the screen.
- **`input()`:** for Python Lab runs only, `sys.stdin` becomes a reader that blocks in C (with the GIL released) until the console sends a line. The console shows the prompt and a text field. Stop wakes a waiting `input()` with a `KeyboardInterrupt`. Harness runs keep the empty stdin, so `input_gets_an_empty_stdin_instead_of_waiting` still passes.
- **The time limit counts running time, not waiting time:** a script waiting for your answer isn't stopped at 30 s. Running time is still limited.
- **The console:** stdout in the normal colour, stderr in ForgeUI's danger tone, your answers marked as input. Clear, Rerun and Stop always visible. A run's output over 1 MB keeps its last 1 MB and says "Earlier output removed (N KB)" at the top, never cut silently.
- **One interpreter:** while a Python Lab script waits for input, Bonsai's screen says "Python Lab is waiting for an answer" and Python Lab says when a Bonsai run is ahead of it in the queue.
- Tests: the wrapper's interactive mode under the test `python3` with a scripted stdin; Swift model tests with a fake runtime for streaming, input, the limit pausing, and the trimming notice. The C bridge is checked in the Device Lab build and on the phone.

As built: `PythonBridge` registers a built-in module `_devicelab_io` (`write(kind, text)` and a blocking `readline()`), and `pybridge_run_streaming(code, with_input, write, context)` runs a script with `sys.stdout` and `sys.stderr` pointed at it; `pybridge_send_input(line)` answers a waiting `input()`, and `pybridge_interrupt` wakes one with a `KeyboardInterrupt` (also one the script reaches just after Stop, so Stop never leaves it waiting). The callback gets five kinds: stdout, stderr, input waiting, input ended with the answer, input cancelled. Rust's `python_lab_console_script(project, path)` is the file runner's console mode: stdin is left to the bridge, and a traceback, cut to the project's frames with the folder taken out of its paths, goes to stderr once. `PythonRuntime`'s limit is now a 100 ms watchdog on a `RunClock` that pauses while `input()` waits. The model shows `PythonConsoleText` (segments by kind, merged, the last 1 MB kept with "Earlier output removed (N KB)"), drained from the runtime every 50 ms; the screen has the answer field, Clear, Rerun and Stop, and `PythonActivity.shared` tells Bonsai Harness's screen when Python Lab waits for an answer. Checked on Linux: two Rust tests run the console script under `python3` with a piped stdin, and the C bridge was compiled against CPython 3.13 and driven by a small C program (input answered from another thread, Stop while waiting, Stop while running, no input means `EOFError`, a shown traceback isn't printed twice, the old `pybridge_run` still works). The Swift model tests (`PythonConsoleTests`, with a fake runtime) and the Swift side as a whole compile only in the Device Lab build.

### PL1e. Bonsai on the same projects, plus import and export (one PR) — built

- Bonsai's own requests run in a project you choose (by default, the one open in Python Lab), and the Bonsai screen gets "Open in Python Lab". The harness already reads files from disk at every step, so your edits are its input. While Bonsai runs in a project, Python Lab shows that project read-only with a banner, so you and Bonsai can't write one file at once. That banner doesn't stop the Files app or another app, so Bonsai's writes also check: the harness remembers a digest of each file's contents when Bonsai last read or wrote it in the task (not just its size and modification time, which an app can leave unchanged), and a `write_file` or `edit_file` to a file that changed since then is refused with "main.py changed outside Bonsai since you read it; read it again". The trade-off: the model spends a step re-reading, and the check catches every change that is visible when Bonsai writes. It can't close the last gap, an edit through Files in the moment between the check and the atomic rename, because iOS gives an app no lock that Files respects; that window is a few milliseconds per write, and an atomic save at least never leaves a half-written file.
- **Import:** files from the Files app into the open project (the system file picker; text and images; the PL1a limits apply). **Export:** share a file, or the whole project as a `.zip`, with forge-storage's existing zip support (the `backup` feature), so no new dependency enters.
- Bonsai plan: H7 is updated to say what PL1 already delivered.

As built: `BonsaiHarness.start_custom(model, request, project)` runs in the chosen project (named as Python Lab names them; only `scratch` is made when missing, any other missing project is refused). Bonsai Harness gets a Project picker over Python Lab's projects and "Open in Python Lab", which opens that project there. `PythonActivity.bonsaiProject` says which project Bonsai is working in; Python Lab shows it read-only with a banner (the editor, Run, the file actions, examples and import refuse), saves any edits the moment Bonsai starts, and reads the files again when it finishes (edits it couldn't save are kept on screen, never replaced). The harness's `PythonLab` keeps each file's SHA-256 as Bonsai last read or wrote it; a write or edit over a file whose contents changed since is refused, while what Bonsai's own runs change counts as Bonsai's. Its writes are atomic now (`forge_storage::write_atomic`). Import is `Workspace.import_file(project, folder, source)` (its own name, or "name 2.csv" when taken; text up to 1 MB, other files up to 20 MB), from the Files picker in the file browser's + menu; Export Project writes `<project>.zip` with forge-storage's backup archive (hidden folders, so Bonsai's journal and the editor state, left out) and offers it to share. Tests (Linux): the refused write and edit, Bonsai's own run not counting, a request in a chosen project (and refused names), import naming and limits, and the export's contents.

## What PL2 builds (started 2026-11-22)

Reuben asked for bigger strides overnight, so PL2 started straight after PL1e, in the order that answers his packages question first. Each part is tested on Linux; the Swift is checked in the combined Device Lab build.

### PL2a. Pure-Python packages — built

**Packages** in Python Lab's Interpreter section: type a name, Install. Rust's `packages.rs` does the work Swift can't test on Linux: `package_choose_wheel` takes PyPI's JSON (`https://pypi.org/pypi/<name>/json`) and picks the newest release's pure wheel (`py3-none-any`, at most 50 MB); `package_install_wheel` checks the download's SHA-256 against PyPI's, refuses compiled code (`.so`, `.dylib`, a wheel that isn't `Root-Is-Purelib`, `platlib` data) and paths that leave the folder, unpacks beside `site-packages` and then moves it in, replacing an older version of the same package but never another package's files; it returns the requirements this phone needs (PEP 508 markers evaluated for CPython 3.15 on iOS, extras left out, an unreadable marker left out). Swift downloads the JSON and wheel and installs requirements in turn (at most 40). Packages built into the app (the numpy spike's) are listed apart and can't be replaced. Removing uses the package's `RECORD`. A package already imported keeps its old version until Device Lab restarts, which the screen says.

Checked on Linux: five tests with wheels built in the test (install, upgrade, uninstall, a bad checksum, compiled code, a non-pure wheel, another package's files, markers), plus an `#[ignore]` test that installed the real PyPI wheels of rich (with markdown-it-py, pygments and mdurl), attrs and six from a folder downloaded beforehand.

Not in PL2a: version pins (always the newest release), per-project environments (PL4), and source-only packages (no build tools on the phone).

### PL2b. Tracebacks you can tap — built

After a failed run, each of the project's lines in the traceback is a button under the console ("helper.py, line 2 in half"); tapping it opens the file and puts the cursor on that line. Rust's `python_traceback_frames(output)` reads the console's stderr (the last traceback; the standard library's and packages' frames are left out, since only the project's files open). Debugging with breakpoints stays later in PL2.

### PL2c. Run history — built

Each run is recorded in the project's hidden `.workspace/runs.json` (the newest 30, each with the file, when, how long, how it ended and the last 16 KB of its output). History, beside Clear under the console, lists them; a run opens its output; Clear History forgets them. `Workspace.runs`, `record_run` and `clear_runs`, tested on Linux.

### PL2d. Several open files and bracket matching — built

Opening a file adds it as a tab above the editor (up to 8; a ninth closes the oldest). Tapping a tab saves the file in the editor and opens that one; its cross closes it, and the last tab can't be closed. The tabs are the project's `openFiles` in `.workspace/state.json`, which PL1b already kept, so they come back with the project; renaming and deleting carry them along, and Bonsai removing a file closes its tab. Only the file in the editor is held in memory.

Putting the cursor beside a bracket marks it and its partner; a bracket with no partner, or the wrong one (`(]`), is marked alone in the danger tone. Rust's `python_matching_bracket(source, cursor)` finds the pair in UTF-16 units and skips brackets in strings and comments (the highlighter's tokens), tested on Linux. The marks are TextKit temporary attributes, so they never enter the text or Undo.

### PL2e. Artifact browser — built

Artifacts now lists every file a run made or changed, not only images: anything in the project except Python files, hidden folders and `__pycache__` (Rust's `project_outputs`; Bonsai's image check still uses `project_artifacts`). Each opens in a preview that fits it: images as before; CSV and TSV as a grid with the header row in bold (Rust's `artifact_table` reads quoted cells as Python's `csv` writes them, and keeps the first 500 rows, 50 columns and 200 characters a cell, saying so when it cuts); text and JSON as selectable monospaced text, JSON indented (`artifact_text`, first 256 KB); HTML in the same script-free web view as SVG; PDFs and anything else in Quick Look. Every preview shares the file. In the Files list, text, JSON and CSV still open in the editor.

### Still in PL2

Debugging (breakpoints, stepping, variables).

## Decided: projects live in Files

**Reuben, 2026-11-22: projects move to `Documents/Python Lab`.** Device Lab already shows its `Documents` folder in the Files app (Relay's received files are there), so every project is a normal folder under On My iPhone ▸ Device Lab ▸ Python Lab: import, export and opening a file in another app need no screens of ours. Built in PL1a: `PythonLabProjects.folder` points there, and on first use `workspace_move_projects` moves the old `Application Support/Python Lab/Projects` across once (only while the new folder holds nothing but an empty `scratch`, so it never overwrites work). The cost: the person, or another app through Files, can change a project while Bonsai works in it; PL1e's write check (above) refuses a Bonsai write over a file changed outside it, so an outside edit is caught unless it lands in the moment between the check and the save. PL1e keeps in-app import and export for convenience.

## Not in PL1 (Reuben's avoid list, kept)

No Jupyter, no second Python runtime, no desktop IDE layout (one column, touch first, an external keyboard as a bonus), no arbitrary native packages, and Bonsai stays an agent next to the editor, never inside it. Packages, debugging, run history and Matplotlib are PL2 and PL3.

## Phone sitting (about 15 minutes, in the combined Device Lab build)

Each PR's Swift goes into the one combined Device Lab build through the merge thread, never a build of its own. One sitting at the end of PL1, or an early one for what is built so far. Each step names the part it needs; in an early sitting, a step whose part isn't in the build is left out, not recorded as failed.

1. (PL1a, PL1b) Create a project, add `main.py` and `helper.py` with a function that divides by zero, call it from `main.py` and run it; the traceback names both files and their lines.
2. (PL1c) Type with quotes and `--`: nothing turns curly. The key row, auto-indent and find and replace work.
3. (PL1b) Close Device Lab from the app switcher, reopen: the same project, file and text are back.
4. (PL1d) `name = input("Name? ")` then `print("Hello", name)`: the prompt appears, the answer is used. A slow loop printing once a second shows lines as they come. Stop works while waiting for input.
5. (PL1a, PL1b) Rename and delete a project; `scratch` can't be deleted.
6. (PL1e) In Python Lab, create a project `handoff`, then in Bonsai Harness choose `handoff` as the project and ask "write hello.py that prints hi and run it"; tap Open in Python Lab and run hello.py. (Ladder tasks stay in the hidden `.ladder` folder; T1 is run separately as in the Bonsai plan.)
7. (any build) Left over from P1: the SVG `<script>` and `https://` check, the thumbnail refreshing after a rerun, `sum(range(10**10))` past the limit, and ladder task T-P1 (`tp1-svg-chart`) with the Scripted model and with Bonsai 4B. Each gets its own row in the results table.

Each result is recorded passed or failed from what Reuben sees, in this document's results table, none assumed.

## Results

| Date | Stage | Check | Build | Result | Evidence |
|---|---|---|---|---|---|
| | | | | | |
