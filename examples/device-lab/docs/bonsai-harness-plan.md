# Bonsai Harness: a local agent that completes projects, proven in Python Lab

Status: **direction set by Reuben** (2026-11-21): AI Lab's defining goal is an on-device agent. Bonsai Harness is the product; Python Lab is the first environment it works in. Shaped by a study of Pi, mini-swe-agent, Aider, Cline and OpenHands (below). The first slice (H1 to H3, thin) is built in the PR that adds this plan (#357); its Rust is tested on Linux against python3, its Swift is not compiled yet, and nothing has run on the phone yet.

## The goal

You give Bonsai a task in plain words. The harness turns it into a plan of small tasks, hands the model one task at a time with only the context that task needs, carries out the model's tool calls, checks the real results, and keeps going (retrying, diagnosing, replanning) until every task is complete or it has to stop and ask. In Python Lab that means Bonsai writes Python files, runs them on the iPhone with the embedded CPython, reads the output, errors and test results, and fixes the code. You can open the project afterwards, read and edit the files and run them yourself. Everything happens on the phone, with no network and no computer.

We don't try to make a 4B or 9B model think like a large one. The harness makes its small steps useful, verifiable and recoverable: **the harness owns the workflow; the model supplies one decision at a time.**

The final benchmark (H8) is Reuben's task, word for word:

> Build a small Python application that tracks personal expenses. Create the files, implement the functionality, write tests, run them, diagnose any failures and fix them. Show me the finished project.

Every stage runs a fixed ladder of tasks on the phone and records each as passed or failed. **Evidence decides a pass, never the model's own report:** a task is complete when Python's own output shows it (the test runner's counts, the expected printed line). Bonsai cannot mark its own work done.

## What exists (#356)

- **Bonsai on iPhone:** Ternary-Bonsai-2 4B and 9B (Q2_0 GGUF) through llama.cpp's prebuilt XCFramework, Metal. About 29 tok/s (4B) and 19 tok/s (9B) on the iPhone 15 Pro Max, 4,096-token context. Rust's `AiLab` drives it through the `AiModel` foreign trait (`AiLabLlama.swift`).
- **Python Lab:** CPython 3.15 from python.org, embedded; `PythonBridge` (start, run, interrupt). One interpreter for the app's life, scripts one at a time, a time limit and Stop (a `KeyboardInterrupt`).
- Not yet confirmed on the phone: Python Lab's standard library check (H0).

## What we borrow from other agents

Reuben asked for a study of existing agent harnesses before the design settled (2026-11-21). We read the code of Pi (`badlogic/pi-mono`: `packages/agent`, `packages/durable`, the coding agent's tools), mini-swe-agent (`SWE-agent/mini-swe-agent`), Aider (`Aider-AI/aider`), Cline (`cline/cline`) and OpenHands (its agent core, now `OpenHands/software-agent-sdk`). **Ideas only: no code, library or dependency from any of them enters ixOS.** The question was the smallest set of mechanisms that lets a small local model finish increasingly hard tasks.

| From | What they do | What Bonsai Harness takes |
|---|---|---|
| **mini-swe-agent** | About 100 lines of agent loop. One action per answer, a completely linear history (the transcript *is* the prompt), every action run independently, the trajectory saved after every step, limits on steps and time, format errors sent back as a short message and the run ended after 3 in a row, long output cut to its head and tail with a note saying how to get less | The executor loop exactly this small. Linear history within a task. Independent runs (the `sys.modules` purge below). Save after every step. The 3-strikes rule for invalid answers. Head and tail truncation with guidance. A short reasoning line before each action (its `THOUGHT`) |
| **Pi** | A minimal core loop with events for everything (turn start, model text, tool start and end), tools as plain definitions, the model behind a provider interface. Its `durable` package commits every transcript entry and task checkpoint to storage *before* showing it; a task is a state machine whose phase reruns from its top after a restart; a model request the process died during counts as a failed attempt, so a request that keeps crashing ends | The task journal (below): append-only, written before shown, replayed on open. Tasks as state machines with checkpoints. The crash-counts-as-attempt rule, which matters on a phone where iOS can end the app. Events for the screen |
| **Aider** | Edits as exact search and replace blocks; when a block doesn't match, it tolerates whitespace differences and otherwise answers with the most similar lines from the file ("did you mean"). Weaker models are given the whole-file format instead. A repository map: every file's definitions and signatures, ranked, cut to a token budget. Lint and test errors fed straight back after an edit, at most 3 times. A commit per change, so any change can be undone | `write_file` (whole file) first, `edit_file` with exact match plus "did you mean" errors in H2. A project map from Python's own `ast` module (definitions and signatures per file, within a budget) instead of resending files. A syntax check after every write, its error in the write's result. Feedback loops capped at 3 |
| **Cline** | Plan mode (explore, ask, propose) separate from Act mode (change things), an approval gate on every edit and command unless auto-approve is on, and a checkpoint of the workspace at each step that can be restored | Planner and Executor as separate modes, the plan shown before acting, with an approval switch. A checkpoint of the project files at every verified task, so a failed task can be rolled back to the last good state and earlier work stays intact |
| **OpenHands** | The agent separate from the workspace it runs in (local, container, remote) behind one interface. A stuck detector for repeated action and error pairs, repeated identical actions, monologues and alternating loops, which first *nudges* the model ("you've made the same failing call 3 times; change the arguments or the approach") and only then stops | The `Environment` trait. The stuck rules: a nudge at the threshold, then Failed (and later Recovery) if it persists |

Later, Reuben pointed at **Terrarium** (`mitsuhiko/terrarium`), a Python IDE for iPhone with an agent built on Pi's durable package. Two of its ideas enter the harness: a scripted model that drives every tool on the device, so a phone run can prove the plumbing apart from the model, and tracebacks as data, so Bonsai gets the failing line of its own code rather than the whole traceback. The rest is in `visual-labs-spec.md`.

smolagents (code as the action format: the model writes a Python snippet that calls tools) is interesting for Python Lab in particular, but a grammar can't keep arbitrary Python valid, and it gives up the narrow, checkable calls a small model needs. It stays a later experiment.

What we deliberately don't take: sub-agents and parallel tool calls (one model, one action at a time on a phone), LLM-written summaries for compaction in the early stages (a small model summarising its own history loses facts; the harness keeps facts itself), MCP, and anything that runs a shell (iOS has none).

## Architecture

```
 Request ("Build me an expense tracker")
        │
 Bonsai Harness (Rust)          owns task state, modes, context, verification, recovery, limits
  ├─ modes of ONE model:        Planner · Executor · Reviewer · Recovery
  │                             each = its own instructions + its own grammar-constrained output
  ├─ task engine:               task records, dependencies, state machine, persisted
  ├─ context builder:           project summary, the task, recent results, file excerpts
  └─ verifier:                  acceptance criteria checked against evidence
        │ tool calls                         ▲ results + evidence
 Environment adapter            the tools it offers, how to run them, what evidence they produce
  └─ Python Lab (first)         files in a project folder · run_python · run_tests
        │                                    ▲
 Model backend (AiModel)        Bonsai 4B · Bonsai 9B · later Apple Foundation Models, others
```

Decisions:

- **One model, several modes.** No separate planner, executor, reviewer and recovery models: they would cost memory and time the phone doesn't have. The same Bonsai runs with different instructions and a different constrained output for each mode, and the harness decides which mode runs and what it sees.
- **The harness is independent of Python Lab.** Environments plug in through one Rust trait (`Environment`: its tools, executing a call, the evidence it reports, a short description of the workspace, interrupting a run). Model backends plug in through the existing `AiModel`. Python Lab is the first environment; documentation, data analysis or other Forge environments can follow without changing the harness.
- **Apps first.** It lives in Device Lab (`rust/src/harness/`, Python Lab's adapter beside it) until a second environment or a second app needs it; then it moves out as its own crate, following the extraction rule ([architecture.md](../../../docs/architecture.md#framework-principles)).
- **Rust owns state and rules; Swift hosts the engines.** Rust builds prompts, parses and checks every call, keeps task records, does file operations (plain files in a folder it is given), writes the Python wrapper, enforces limits and decides pass or fail. Swift supplies the model (`AiModel`, existing) and a `PythonHost` foreign trait over `PythonBridge`. Model files stay free of SwiftUI.

### The task engine

Every task has a persistent record:

```json
{
  "id": "task_004",
  "description": "Implement expense persistence",
  "status": "running",
  "dependencies": ["task_002"],
  "acceptance": ["tests_pass"],
  "attempts": 1,
  "result": null
}
```

The harness owns `status`, as a state machine:

```
Planned → Ready → Running → Verifying → Completed ──→ (next ready task, or done)
                     ▲          │
                     │          └→ Failed → Recovery → Ready (retry or revised task)
                     │                              └→ Blocked (needs the person)
                     └── "not done yet": evidence missing, back to work
```

When the executor says a task is finished, the task goes to **Verifying** and the harness checks its acceptance criteria against the evidence the environment recorded. Missing evidence sends it back to Running with the reason ("the tests have not passed yet"), counted as an attempt.

### The task journal

Reuben's idea: Bonsai can stop, unload its model, resume later and continue from the last verified task without losing work. With slow local inference and tight memory, that is what makes long tasks practical.

- **One append-only journal per project** (`.harness/journal.jsonl`). Every event is a line: the request, the plan, each task's status change, each model answer (the call, with its timings), each tool result and its evidence, each verification. A line is written and flushed **before** the screen shows it, so the screen never shows what the journal would forget.
- **State is the journal replayed.** Opening a project reads the journal and rebuilds the plan, every task's status and attempts, and the current task's linear history. Nothing else holds state, so there is nothing to get out of step.
- **A checkpoint at every verified task:** a copy of the project's files (`.harness/checkpoints/<task>/`; projects are small). Resuming after a failure, or Recovery deciding to retry, starts from the last verified state, so earlier work stays intact.
- **Interruptions:** a step that was running when the app ended is replayed as interrupted. A model answer that was cut off is discarded and asked again; a Python run that was cut off counts as a failed run. A task interrupted this way twice in a row is marked Failed rather than crashing the app forever (Pi's rule).
- **Unloading:** because the journal holds everything, the harness can free the model between tasks (or when iOS warns about memory) and load it again for the next one; the model's own cache holds nothing that matters.

H1 writes the journal and replays it for the transcript; resuming a half-finished plan arrives with planning in H4.

Acceptance criteria are machine-checkable where possible: `tests_pass` (the last test run reported at least one test and no failures or errors), `run_prints(file, text)`, `file_unchanged(file)` (a fix must not edit the test that caught the bug), `ran_ok(file)`. The Reviewer mode (H5) judges what a fixed rule can't, still only from evidence the harness shows it.

### Constrained tool calls

Every executor turn is exactly one JSON object, a short reasoning line first, then the action:

```json
{"thought": "Create the module first, then its test.", "tool": "write_file", "path": "mathutil.py", "content": "def add(a, b):\n    return a + b\n"}
```

The `thought` (at most 200 characters, enforced by the grammar) is mini-swe-agent's and Aider's "explain briefly, then act": with thinking turned off, one sentence of reasoning before the action is cheap and helps a small model choose. The screen shows it as the step's headline.

llama.cpp's grammar sampler (`llama_sampler_init_grammar`, GBNF) lets the model produce only objects of the allowed shapes: one of the environment's tool names, its arguments in a fixed order, paths from a safe character set, JSON strings with valid escapes. The harness **generates the grammar from the environment's tool list**, so the grammar and the parser can't drift apart. Every turn parses; a small model never loses a step to a malformed call. Rust still checks every argument (no absolute paths, no `..`, sizes capped): a grammar shapes text, not meaning. Planner, Reviewer and Recovery modes get their own schemas (a plan, a verdict, a diagnosis) the same way.

Plain JSON rather than Qwen's `<tool_call>` tags, because those are special tokens and grammar matching on them is fragile. The cost is code travelling JSON-escaped (`\n`, `\"`); if the ladder shows failures dominated by escaping, H2 tries a raw block format under its own grammar and keeps whichever passes more. Thinking stays off and sampling is greedy by default, so a run can be repeated.

### Python Lab as an environment

| Tool | Arguments | Evidence it records |
|---|---|---|
| `list_files` | | |
| `read_file` | `path` | |
| `write_file` | `path`, `content` | file written |
| `edit_file` (H2) | `path`, `old`, `new` | file written |
| `run_python` | `path` | ran ok or failed, its output |
| `run_tests` | | tests run, failures, errors, the runner's output |

Each run's result comes back as the tool's answer, cut to its start and end when long, with a note saying what was left out and how to get less (print less, run one test); a paged `get_output` comes only if the ladder shows the model needs the middle. Every `write_file` of a `.py` file is checked with Python's own parser at once, and a syntax error (line, column, message) is the write's result, so the model fixes it before running anything.

Rust writes a wrapper around each run. It changes into the project folder, puts the project first on `sys.path`, and **forgets the project's modules from `sys.modules` before every run**: the interpreter lives for the app's whole life and would otherwise run the version of a file it imported before the model fixed it. Afterwards it restores the folder and path.

A guard (Python's audit hooks, `sys.addaudithook`, installed once and active only during harness runs) refuses writing or deleting files outside the project (and the temporary folder), network connections unless the project allows them, and starting processes. Audit hooks are a guard rail against a model's mistakes, not a boundary against hostile code: the code runs with Device Lab's rights inside its sandbox. That is acceptable for a lab on Reuben's phone, not for an App Store product.

Limits (first defaults, shown on screen): 12 executor steps per task, 30 s per Python run, 10 minutes per task, 1,200 tokens per model turn, 1,500 characters of any result sent back. An invalid answer (possible on a backend without a grammar, or when an answer is cut off by the token limit) goes back to the model as one short message; three in a row end the task. The same failing call made three times in a row gets one nudge ("the same call failed 3 times with the same error; change the arguments or the approach"); a fourth ends the task as stuck. **Stop** is always visible while the harness works: it stops the model at its next token or interrupts the Python run.

### Context

The window is 4,096 tokens, so the harness budgets every prompt. Within a task the history is linear (mini-swe-agent); across tasks it is not carried at all: each task starts fresh with the request, its own description and acceptance criteria, what earlier tasks produced (one line each, from the journal), and the project map. The mode's instructions and the current task always stay; files are not kept in the prompt (the model reads them again when it needs them); results are cut; when a task's steps no longer fit, older results shrink to one line ("ran tests: 1 failed"), older file contents in calls shrink to their length, and then the oldest steps drop. Facts are kept by the harness, not by asking the model to summarise itself.

The **project map** (from H2) is Aider's repository map made cheap: a small Python script, run by the harness in the embedded CPython, lists each file's classes and functions with their signatures using `ast`, cut to a token budget. `list_files` shows it, and each task's prompt includes it.

## Stages

Reuben's H1 to H8, reordered only in one way: **the first PR is a thin slice through H1, H2 and H3**, so the first phone sitting already proves a real write, run and fix cycle instead of scaffolding. Each stage is one PR and one short phone sitting in the next combined Device Lab build (never a build of its own). A stage passes when its ladder passes on the 4B; the 9B is recorded beside it.

### H0: the engines work on the phone (#356)

Python Lab's "Standard library check" prints 22 of 22; AI Lab with Bonsai 4B answers a short prompt (seen 2026-11-21).

### H1 to H3 (thin): one task, end to end (this PR)

- **Harness foundation:** the task record and state machine (Ready, Running, Verifying, Completed, Failed, Stopped); the task journal, written before the screen shows a step and replayed for the transcript; a checkpoint of the files when the task is verified; the `Environment` trait.
- **Structured model interface:** the grammar generated from the tool list (with the `thought` line), the parser, argument checks, the 3-strikes rule for invalid answers; a grammar option on the llama.cpp model.
- **Single-task executor:** the mini-swe-agent-sized loop with a linear history, limits, Stop, the stuck nudge and stop, the context budget, head and tail truncation with guidance, the transcript (copyable).
- **Verifier:** acceptance criteria against evidence; `finish` without evidence goes back to work.
- **Python Lab adapter:** `list_files`, `read_file`, `write_file` (with the syntax check), `run_python`, `run_tests`, the wrapper and the guard.
- **Screen:** Tools ▸ **Bonsai Harness**: pick the model, pick a ladder task or type one, watch each step and the task's status, Stop, copy the transcript, read the project's files.
- **Scripted model** (from Terrarium's self-test): a model choice that replays known-good answers for T1 to T3, so the sitting first proves the tools, guard, evidence checks and journal on the phone with no model involved. Scripted T1 starts with a probe that tries a write outside the project and a network connection; both must print `blocked`.

| Task | Acceptance (from evidence) |
|---|---|
| T1 Hello: "Write hello.py that prints Hello from Bonsai, then run it." | `run_python hello.py` printed `Hello from Bonsai` |
| T2 One file, one test: "Write mathutil.py with add(a, b) and test_mathutil.py with a unittest test for it, then run the tests." | tests pass (at least 1) |
| T3 One fix: the project has `average.py` with a bug and a failing test. "Run the tests and fix the bug." | tests pass, `test_average.py` unchanged |

The sitting starts with Scripted T1, T2 and T3 (all three must complete, T1's probe shows both blocked), then the same tasks on the 4B. Also in the sitting: Stop during a model turn and during an endless Python loop; writing `../escape.py` is refused; T1 in airplane mode.

### H2: a reliable model interface

`edit_file` with exact matching and Aider-style "did you mean" errors; the project map from `ast`; reusing llama.cpp's cache for the shared start of the prompt (each turn now re-reads the whole prompt, seconds on the phone); the raw block format if T1 to T3 showed escaping failures; limits tuned from the H1 transcripts. Ladder: T4 two modules, one importing the other; T5 fix a bug from a traceback with no tests.

### H4: planning and multi-task orchestration

Planner mode: the request becomes a constrained plan (tasks, dependencies, acceptance criteria chosen from the harness's list). As in Cline's Plan and Act, the plan is shown before anything changes, with an approval switch (approve each plan, or run on). The harness runs ready tasks in dependency order, one executor at a time, each with a fresh context, and shows the plan with each task's status (as in "3 / 6 tasks passed"). The journal now resumes a half-finished plan from its last verified task, and the model can be unloaded between tasks. Ladder: T6 "a module with two functions, each with its own test" as a two-task plan; T7 a three-task plan with a dependency.

### H5: recovery and replanning

Failed tasks go to Recovery mode: the files roll back to the last checkpoint, and the model gets the failure's evidence and chooses retry with a diagnosis, a revised task, or blocked. Reviewer mode judges criteria a fixed rule can't. Asking the person a question or for permission (Blocked) is shown on screen and answered there. Ladder: T8 a seeded project whose plan must change halfway (a dependency is missing); T9 a task that can't succeed ends Blocked with a clear reason, not a loop.

### H6: context management

A persistent project summary the harness keeps up to date from the journal; file excerpts retrieved by name and symbol (through the project map) instead of whole files; measuring 8,192 tokens of context if memory allows (the KV cache sits next to Python and the 9B). Interruption tested hard: iOS ending the app mid-step, low-memory warnings unloading the model. Ladder: T10 kill the app mid-plan, reopen, resume; T11 a second request on an earlier project ("now add…").

### H7: a small multi-file project in Python Lab

Projects listed in Python Lab; browse, edit and run their files yourself, and the harness sees your edits; a per-project network switch, off by default, asked for explicitly. Ladder: T12 a three-file program with tests, built from one request.

### H8: the benchmark

Reuben's expenses task, word for word, offline, three attempts on each model. It passes when the finished project has a working program and its tests pass in Python's own output, and Reuben has looked at the project on the phone.

## Risks

- **The model:** two-bit 4B weights may write plausible but wrong code, or loop on the same fix. The stuck rule, the attempt count and the step limit keep a failure short; the ladder shows where it breaks.
- **Context:** 4,096 tokens is tight for files plus results; H6 is the fix, and 8,192 tokens if memory allows.
- **Speed:** re-reading a 3,000-token prompt each turn costs seconds; H2's prompt cache reuse is the fix.
- **Escaping code in JSON:** measured in H1, fixed in H2 if it matters.
- **Memory and heat:** a 9B model, its KV cache and CPython together. Runs record the app's memory and thermal state.

## Later, not planned yet

Apple's on-device model as another backend (its guided generation, `DynamicGenerationSchema`, can constrain output the same way); the GTX 1060 laptop over the tailnet; other environments (project documentation, data analysis); pure-Python packages in a project. Running the same ladder off the phone (the Rust harness against llama.cpp on a computer) would make iterating on prompts much faster, with the phone kept as the proof; it needs Reuben's go-ahead for a machine to run it on. None of these starts before H8 unless Reuben asks.

## Results

| Date | Stage | Task | Model | Result | Evidence |
|---|---|---|---|---|---|
| | H0 | Standard library check | | not run | |
