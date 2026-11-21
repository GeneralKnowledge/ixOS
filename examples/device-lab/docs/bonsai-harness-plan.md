# Bonsai Harness: a local agent that completes projects, proven in Python Lab

Status: **direction set by Reuben** (2026-11-21): AI Lab's defining goal is an on-device agent. Bonsai Harness is the product; Python Lab is the first environment it works in. The first slice (H1 to H3, thin) is built in the PR that adds this plan; nothing has run on the phone yet.

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

When the executor says a task is finished, the task goes to **Verifying** and the harness checks its acceptance criteria against the evidence the environment recorded. Missing evidence sends it back to Running with the reason ("the tests have not passed yet"), counted as an attempt. Records are saved in the project (`.harness/`), so a plan survives the app being closed.

Acceptance criteria are machine-checkable where possible: `tests_pass` (the last test run reported at least one test and no failures or errors), `run_prints(file, text)`, `file_unchanged(file)` (a fix must not edit the test that caught the bug), `ran_ok(file)`. The Reviewer mode (H5) judges what a fixed rule can't, still only from evidence the harness shows it.

### Constrained tool calls

Every executor turn is exactly one JSON object:

```json
{"tool": "write_file", "path": "mathutil.py", "content": "def add(a, b):\n    return a + b\n"}
```

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

Each run's result comes back as the tool's answer, cut to its start and end when long; a paged `get_output` comes only if the ladder shows the model needs the middle.

Rust writes a wrapper around each run. It changes into the project folder, puts the project first on `sys.path`, and **forgets the project's modules from `sys.modules` before every run**: the interpreter lives for the app's whole life and would otherwise run the version of a file it imported before the model fixed it. Afterwards it restores the folder and path.

A guard (Python's audit hooks, `sys.addaudithook`, installed once and active only during harness runs) refuses writing or deleting files outside the project (and the temporary folder), network connections unless the project allows them, and starting processes. Audit hooks are a guard rail against a model's mistakes, not a boundary against hostile code: the code runs with Device Lab's rights inside its sandbox. That is acceptable for a lab on Reuben's phone, not for an App Store product.

Limits (first defaults, shown on screen): 12 executor steps per task, 30 s per Python run, 10 minutes per task, 1,200 tokens per model turn, 1,500 characters of any result sent back; the same failing call three times in a row ends the task as stuck. **Stop** is always visible while the harness works: it stops the model at its next token or interrupts the Python run.

### Context

The window is 4,096 tokens, so the harness budgets every prompt: the mode's instructions and the current task always stay; files are not kept in the prompt (the model reads them again when it needs them); results are cut; when the steps no longer fit, older results shrink to one line ("ran tests: 1 failed") and then the oldest steps drop. From H6 the harness keeps a persistent project summary and gives each task only its relevant excerpts, retrieved when needed, instead of the whole project.

## Stages

Reuben's H1 to H8, reordered only in one way: **the first PR is a thin slice through H1, H2 and H3**, so the first phone sitting already proves a real write, run and fix cycle instead of scaffolding. Each stage is one PR and one short phone sitting in the next combined Device Lab build (never a build of its own). A stage passes when its ladder passes on the 4B; the 9B is recorded beside it.

### H0: the engines work on the phone (#356)

Python Lab's "Standard library check" prints 22 of 22; AI Lab with Bonsai 4B answers a short prompt (seen 2026-11-21).

### H1 to H3 (thin): one task, end to end (this PR)

- **Harness foundation:** the task record and state machine (Ready, Running, Verifying, Completed, Failed, Stopped), saved in `.harness/` after every step; the `Environment` trait.
- **Structured model interface:** the grammar generated from the tool list, the parser, argument checks; a grammar option on the llama.cpp model.
- **Single-task executor:** the loop, limits, Stop, the stuck rule, the context budget, the transcript (copyable, and saved in the project).
- **Verifier:** acceptance criteria against evidence; `finish` without evidence goes back to work.
- **Python Lab adapter:** `list_files`, `read_file`, `write_file`, `run_python`, `run_tests`, the wrapper and the guard.
- **Screen:** Tools ▸ **Bonsai Harness**: pick the model, pick a ladder task or type one, watch each step and the task's status, Stop, copy the transcript, read the project's files.

| Task | Acceptance (from evidence) |
|---|---|
| T1 Hello: "Write hello.py that prints Hello from Bonsai, then run it." | `run_python hello.py` printed `Hello from Bonsai` |
| T2 One file, one test: "Write mathutil.py with add(a, b) and test_mathutil.py with a unittest test for it, then run the tests." | tests pass (at least 1) |
| T3 One fix: the project has `average.py` with a bug and a failing test. "Run the tests and fix the bug." | tests pass, `test_average.py` unchanged |

Also in the sitting: Stop during a model turn and during an endless Python loop; writing `../escape.py` is refused; T1 in airplane mode.

### H2: a reliable model interface

`edit_file`; reusing llama.cpp's cache for the shared start of the prompt (each turn now re-reads the whole prompt, seconds on the phone); the raw block format if T1 to T3 showed escaping failures; limits tuned from the H1 transcripts. Ladder: T4 two modules, one importing the other; T5 fix a bug from a traceback with no tests.

### H4: planning and multi-task orchestration

Planner mode: the request becomes a constrained plan (tasks, dependencies, acceptance criteria chosen from the harness's list). The harness runs ready tasks in dependency order, one executor at a time, and shows the plan with each task's status (as in "3 / 6 tasks passed"). Ladder: T6 "a module with two functions, each with its own test" as a two-task plan; T7 a three-task plan with a dependency.

### H5: recovery and replanning

Failed tasks go to Recovery mode: the model gets the failure's evidence and chooses retry with a diagnosis, a revised task, or blocked. Reviewer mode judges criteria a fixed rule can't. Asking the person a question or for permission (Blocked) is shown on screen and answered there. Ladder: T8 a seeded project whose plan must change halfway (a dependency is missing); T9 a task that can't succeed ends Blocked with a clear reason, not a loop.

### H6: context management

A persistent project summary the harness keeps up to date; task-specific context; file excerpts retrieved by name and symbol instead of whole files; measuring 8,192 tokens of context if memory allows (the KV cache sits next to Python and the 9B). Persistence across interruptions: a plan interrupted by leaving the app or by iOS resumes from its last finished task. Ladder: T10 kill the app mid-plan, reopen, resume; T11 a second request on an earlier project ("now add…").

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

Apple's on-device model as another backend (its guided generation, `DynamicGenerationSchema`, can constrain output the same way); the GTX 1060 laptop over the tailnet; other environments (project documentation, data analysis); pure-Python packages in a project. None starts before H8 unless Reuben asks.

## Results

| Date | Stage | Task | Model | Result | Evidence |
|---|---|---|---|---|---|
| | H0 | Standard library check | | not run | |
