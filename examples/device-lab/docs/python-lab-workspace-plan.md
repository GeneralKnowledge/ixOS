# Python Lab: from script runner to workspace

Status: **plan agreed in outline; PL1a and PL1b built** (Reuben, 2026-11-22). Their Rust is tested on Linux; their Swift is not compiled yet and nothing has run on the phone. Reuben's review of Python Lab found that it runs Python well but is still a script runner, not a place to work. This plan covers his first stage, **PL1 Workspace and interactive development**, in five small PRs. It checks his reading of the code first, and lists his later stages so that none of them is started early.

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

### PL1b. The project browser and autosave (Swift, one PR)

Built: `PythonLabModel` over `Workspace` (projects, files, autosave a second after typing stops and on leaving or backgrounding, the open file remembered per project and the last project between launches), `PythonProjectsView` and `PythonFilesView` (ForgeUI's `forgeRename` for naming, `forgeRowActions` for rename, delete, Run This File, New File Here), and the Rust `python_lab_file_script(project, path)` with two Linux tests (a file imports its neighbours from its own folder and the project's; a traceback names `main.py, line 3` and `helper.py, line 2`; an edited module is picked up on the next run).

It also fixes a fault the #367 review found: Python Lab's Stop and time limit called a global interrupt, so they could stop a Bonsai script that was running while Python Lab's waited for its turn, and the wait counted against Python Lab's limit. `PythonRuntime` now gives every run an id, interrupts only the run that asked, and starts Python Lab's limit when its script actually starts. Bonsai's harness keeps its own limit in Rust, which still counts its wait; that matters once PL1d lets a script wait for `input()`.

- Python Lab opens on the **last project and file** it had open. A project list (create, rename, delete with a confirmation naming the file count) and a file browser for the project (folders, files with sizes, images opening in the existing preview, swipe to rename or delete, a + menu for a new file or folder).
- The editor edits **a real file**. Autosave a second after typing stops, and when the screen closes or the app goes to the background, so nothing typed is lost; a small "Saved" or "Not saved: reason" line shows which.
- Run runs **the open file** from disk (`runpy.run_path` in the existing wrapper, `__name__ == "__main__"`), so a traceback names `main.py, line 3` and one file can import another. A project can mark a different file as the one Run starts; by default it runs the file you're looking at. The last-expression echo stays for a quick check. Run first saves what is on screen and waits for the save; if it fails, the reason shows and nothing runs, so Run never runs the older copy on disk.
- Examples become "New file from example", which adds the sample as a file to the open project instead of replacing what you wrote.
- **Migration:** whatever is in `scratch` stays. Nothing else exists to migrate, since manual code was never saved.

### PL1c. A basic code editor (Swift view, Rust for the colours, one PR)

A `UITextView` in a `UIViewRepresentable` (a view file, so UIKit is allowed there) instead of `TextEditor`:

- Smart quotes, dashes and insert-delete off; autocorrection, spelling and capitals off; monospaced at the person's text size.
- Line numbers in a gutter.
- Return keeps the line's indentation and adds four spaces after a line ending in `:`; Tab inserts four spaces; Backspace at an indent removes four.
- A key row above the keyboard: Tab, `:`, `(`, `)`, `[`, `]`, `{`, `}`, `"`, `'`, `#`, `=`, `_`, and undo and redo. With an external keyboard, Tab and Shift-Tab indent and outdent the selection, and Command-F finds.
- Find and replace through iOS's own find bar (`UIFindInteraction`, `isFindInteractionEnabled`), not our own.
- **Syntax colours** from a small Python tokenizer in Rust (keywords, strings including triple-quoted and f-strings, comments, numbers, `def` and `class` names, decorators), returning ranges that Swift colours with ForgeUI tones. Tested on Linux against Python's own `tokenize` on the samples. Re-coloured after typing pauses, for the whole file under 1 MB.

Bracket matching and several open files are PL2.

### PL1d. The interactive console (C bridge, Rust wrapper, Swift, one PR)

- **Streaming:** `PythonBridge` gets a small built-in module whose `write(stream, text)` calls back into Swift, and Python Lab's runs point `sys.stdout` and `sys.stderr` at it, so output appears while the script runs. Swift batches it (at most every 50 ms) so a tight `print` loop can't flood the screen.
- **`input()`:** for Python Lab runs only, `sys.stdin` becomes a reader that blocks in C (with the GIL released) until the console sends a line. The console shows the prompt and a text field. Stop wakes a waiting `input()` with a `KeyboardInterrupt`. Harness runs keep the empty stdin, so `input_gets_an_empty_stdin_instead_of_waiting` still passes.
- **The time limit counts running time, not waiting time:** a script waiting for your answer isn't stopped at 30 s. Running time is still limited.
- **The console:** stdout in the normal colour, stderr in ForgeUI's danger tone, your answers marked as input. Clear, Rerun and Stop always visible. A run's output over 1 MB keeps its last 1 MB and says "Earlier output removed (N KB)" at the top, never cut silently.
- **One interpreter:** while a Python Lab script waits for input, Bonsai's screen says "Python Lab is waiting for an answer" and Python Lab says when a Bonsai run is ahead of it in the queue.
- Tests: the wrapper's interactive mode under the test `python3` with a scripted stdin; Swift model tests with a fake runtime for streaming, input, the limit pausing, and the trimming notice. The C bridge is checked in the Device Lab build and on the phone.

### PL1e. Bonsai on the same projects, plus import and export (one PR)

- Bonsai's own requests run in a project you choose (by default, the one open in Python Lab), and the Bonsai screen gets "Open in Python Lab". The harness already reads files from disk at every step, so your edits are its input. While Bonsai runs in a project, Python Lab shows that project read-only with a banner, so you and Bonsai can't write one file at once. That banner doesn't stop the Files app or another app, so Bonsai's writes also check: the harness remembers each file's size and modification time when Bonsai last read or wrote it in the task, and a `write_file` or `edit_file` to a file that changed since then is refused with "main.py changed outside Bonsai since you read it; read it again". The trade-off: the model spends a step re-reading, but an edit made through Files is never silently replaced. (An atomic save alone only prevents half-written files.)
- **Import:** files from the Files app into the open project (the system file picker; text and images; the PL1a limits apply). **Export:** share a file, or the whole project as a `.zip`, with forge-storage's existing zip support (the `backup` feature), so no new dependency enters.
- Bonsai plan: H7 is updated to say what PL1 already delivered.

## Decided: projects live in Files

**Reuben, 2026-11-22: projects move to `Documents/Python Lab`.** Device Lab already shows its `Documents` folder in the Files app (Relay's received files are there), so every project is a normal folder under On My iPhone ▸ Device Lab ▸ Python Lab: import, export and opening a file in another app need no screens of ours. Built in PL1a: `PythonLabProjects.folder` points there, and on first use `workspace_move_projects` moves the old `Application Support/Python Lab/Projects` across once (only while the new folder holds nothing but an empty `scratch`, so it never overwrites work). The cost: the person, or another app through Files, can change a project while Bonsai works in it; PL1e's write check (above) refuses a Bonsai write over a file changed outside it, so an outside edit is never silently replaced. PL1e keeps in-app import and export for convenience.

## Not in PL1 (Reuben's avoid list, kept)

No Jupyter, no second Python runtime, no desktop IDE layout (one column, touch first, an external keyboard as a bonus), no arbitrary native packages, and Bonsai stays an agent next to the editor, never inside it. Packages, debugging, run history and Matplotlib are PL2 and PL3.

## Phone sitting (about 15 minutes, in the combined Device Lab build)

Each PR's Swift goes into the one combined Device Lab build through the merge thread, never a build of its own. One sitting at the end of PL1, or two if PL1a and PL1b land well before the rest:

1. Create a project, add `main.py` and `helper.py`, import one from the other, run it; the traceback names the right file and line.
2. Type with quotes and `--`: nothing turns curly. The key row, auto-indent and find and replace work.
3. Close Device Lab from the app switcher, reopen: the same project, file and text are back.
4. `name = input("Name? ")` then `print("Hello", name)`: the prompt appears, the answer is used. A slow loop printing once a second shows lines as they come. Stop works while waiting for input.
5. Rename and delete a project; `scratch` can't be deleted.
6. In Python Lab, create a project `handoff`, then in Bonsai Harness choose `handoff` as the project and ask "write hello.py that prints hi and run it"; tap Open in Python Lab and run hello.py. (Ladder tasks stay in the hidden `.ladder` folder; T1 is run separately as in the Bonsai plan.)
7. Left over from P1: the SVG `<script>` and `https://` check, the thumbnail refreshing after a rerun, `sum(range(10**10))` past the limit, and ladder task T-P1 (`tp1-svg-chart`) with the Scripted model and with Bonsai 4B. Each gets its own row in the results table.

Each result is recorded passed or failed from what Reuben sees, in this document's results table, none assumed.

## Results

| Date | Stage | Check | Build | Result | Evidence |
|---|---|---|---|---|---|
| | | | | | |
