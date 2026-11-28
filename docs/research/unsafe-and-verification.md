# Unsafe Rust and checking the security-critical logic

An audit of every `unsafe` in ixOS's Rust, and a proportionate plan for catching mistakes in the code that decides who is trusted and what they may do (Reuben, 2026-11-28: "check we aren't using unnecessary unsafe Rust", and the earlier idea of verifying that logic with Lean). Audited at main `10039e7e`.

**In short.** ixOS has 15 `unsafe` blocks, every one a direct operating-system call in a framework or tools crate, and none in any app, Relay included (Relay's Stage 0 spike exports one C symbol). All are needed; one was a duplicate and is gone. A lint now makes each one an explicit exception, and app crates forbid `unsafe` outright. For the security logic, the best value is model-based property tests on the real Rust code, then Lean for the one place where a proof says something tests can't (revocation spreading across several devices), and a CI job for the Lean model that already exists. Reading the trust rules for this plan found one real bug, fixed in the same PR.

## 1. Unsafe Rust

### Where it is

`grep -rn unsafe --include=*.rs` finds 19 lines; 4 are prose ("unsafe names" in backups, a crash hint). The rest:

| Where | What it calls | Needed? | Done here |
|---|---|---|---|
| `forge-cli/src/build_lock.rs` `pid_alive` | `kill(pid, 0)`: is a ticket's process alive | Yes, but duplicated `forge-uitest`'s copy, minus its `pid <= 0` guard | Now calls `forge_uitest::session::pid_alive`. A ticket named `…-0` no longer counts as alive forever (`kill(0, 0)` asks about the caller's own process group, so it always succeeded) |
| `forge-cli/src/build_lock.rs` `runtime_dir`, `scope_support` | `getuid()` twice | Yes (std has no safe `getuid`) | One `uid()` helper, so one block instead of two |
| `forge-cli/src/build_lock.rs` `lower_priority` | `getpriority`/`setpriority`, `ioprio_set` | Yes: the build queue's nice 10 and idle I/O | Kept |
| `forge-cli/src/ui_session.rs` `catch_stop_signals` | `signal()` with a handler that stores to an atomic | Yes | Kept |
| `forge-cli/src/ui_session.rs` `start` | `pre_exec(setsid)` | Yes (`pre_exec` is unsafe by design) | Moved into a small `own_session` helper, so `start` itself has no `unsafe` |
| `forge-cli/src/ui_session.rs` `end` | `kill(SIGTERM)`, `kill(SIGKILL)` | Yes | The second block lacked a `SAFETY:` comment; added |
| `forge-cli/src/report.rs` `enter_json_mode` | `dup(1)`, `dup2(2, 1)`, `File::from_raw_fd` | Yes: keeps stdout for the JSON result | Kept |
| `forge-uitest/src/session.rs` `pid_alive` | `kill(pid, 0)` | Yes | Now the only copy |
| `forge-runtime/src/logging.rs` (Android, iOS) | `__android_log_write`, `syslog("%s", …)` | Yes: the platform log | Kept; the format string is a constant `%s` |
| `forge-runtime/src/jni_init.rs` | `#[unsafe(no_mangle)]` on the JNI entry point | Yes: the JVM finds it by symbol name | Kept; the body is safe Rust through the `jni` crate |
| `examples/relay/spike/src/lib.rs` | `#[no_mangle]` on the spike's start function | Yes, for the Stage 0 iOS size check; the spike is research code, not the app | Its crate denies `unsafe_code` instead of forbidding it, with this one exception |
| `forge-app/src/runtime.rs` `try_lock` (Android only) | `flock` | Yes: std's `File::try_lock` reports "not supported" on Android | Kept |

Every block is a single syscall with no pointer arithmetic, no `transmute`, no lifetime tricks and no `unsafe impl Send/Sync`. None touches user data or anything in Relay.

**Could the rest go too?** The `rustix` crate wraps all of these calls safely (`getuid`, `kill`, `dup2`, `flock`, `setsid`, priorities), and it is already in the dependency tree through `tempfile`. Moving `forge-cli` onto it would make the CLI free of `unsafe`. It is optional: the blocks are small, commented and now fenced by the lint, so it is worth doing only when someone next works in those files.

**Miri** is not worth running: it interprets Rust, and it can't execute `kill`, `ioprio_set` or the platform log, which are the only things the `unsafe` does.

### The lint

- The workspace now sets `unsafe_code = "deny"` and `clippy::undocumented_unsafe_blocks = "deny"` (`[workspace.lints]`), and every workspace crate inherits them. Each function above carries `#[allow(unsafe_code)]`, so `grep allow(unsafe_code)` lists every exception, and a new `unsafe` anywhere fails `cargo clippy -D warnings` in CI until someone adds the exception on purpose.
- Every app crate (`examples/*/rust` and Relay's messaging crates) and both templates set `unsafe_code = "forbid"`, which nothing can override. UniFFI's generated scaffolding passes it; all app crates were built and linted with it. New apps get it from `forge init`.
- No new CI step is needed: the existing `cargo clippy --workspace` and `scripts/check-app-crates.sh` runs enforce it.

### Where the real unsafe lives

Relay's security rests on its dependencies: iroh (QUIC and its TLS), `hpke`, `ed25519-dalek`, `chacha20poly1305`, `sha2`, `spake2`, `ring`, `rusqlite`. Their `unsafe` is far larger than ours and is not ours to audit. What covers it is what CI already runs: `cargo deny check` against the RustSec advisory database, pinned versions, and Dependabot. Keep those; don't fork or re-implement any of them.

## 2. The security-critical logic

### What is there, and how it is checked today

| Logic | Where | Checked by now |
|---|---|---|
| Word-code session (one guess per code, one receiver) | `code_session.rs` | **Lean proof** `verification/relay/CodeSession.lean` (no `sorry`), plus 18 tests, one replaying 2,000 random 60-step sequences against the model's invariant |
| Trust, revoke, forget, pairing policy | `trust.rs` (pure functions) | 12 unit tests |
| Rights per device (`receives_from`, `may_ask`, `sees_devices`, capability grants) | `trust.rs`, re-checked in `device.rs` before saving | Unit tests; `still_*` re-checks after each await |
| Introducer and device lists (My Devices) | `trust.rs` `introduced`, `list_for` | Unit tests |
| Mailbox sealing (HPKE, header as AAD, inner signature, expiry) | `mailbox.rs` | 16 tests, golden fixture, negative cases (wrong key, redirected, extended, forged sender) |
| Deposit passes, quotas, receipts | `mailbox_server.rs` `register`, `admit`, `rules` (SQL) | 7 tests |
| Device key in the Keychain | `node.rs` `key_from`, `device_key_in` | Unit tests with a fake store; the iPhone run of #348 |

This is better than most projects at this stage: the rules are pure functions, separated from I/O, with tests written as the rules. What's missing is checking the rules against *every sequence* of operations, not the sequences someone thought to write down.

### What it found already

Writing down the trust properties for this plan turned up a real gap. An introducer's revoke only reached devices **trusted** here, not ones the person had **forgotten**. Under Pair Automatically a forgotten device is trusted again after a word-code transfer, so a device revoked on the laptop, but forgotten on the phone, came back as trusted on the phone the next time it used a code. Fixed in this PR (`trust.rs` `introduced`), with the test `an_introducers_revoke_reaches_a_forgotten_device`. The device now shows in the list as revoked by the introducer, so the person can see and undo it.

Two smaller notes, not changed:

- In Private mode the mailbox client never runs, so a revoked device's pass stays registered on the server until Relay is back in Hybrid. The revoked device can still leave items, which the recipient refuses to open (`open` checks trust), so the cost is storage up to the per-pass quota for 7 days, not a breach.
- `DepositPass::for_peer` is derived, so trusting a revoked device again gives it the same pass it had. Correct as designed (it is trusted again), but worth knowing before the mailbox ever goes public.

### The plan, cheapest and most useful first

| Step | What | Tool | Why this tool | Cost |
|---|---|---|---|---|
| **V1** | Check the existing Lean model in CI: a `lean-toolchain` file in `verification/relay/` and a job that runs `lean CodeSession.lean` when that folder changes | Lean 4, no Mathlib | foundation-review.md said "a CI job once a proof exists"; one exists, but nothing checks it still holds | An hour; about a minute of CI, only when the folder changes |
| **V2** | Trust as a state machine: random sequences of trust, revoke, forget, code transfers under each policy, introducer lists, grants and person marks, checking after every step: a revoked device becomes trusted only through the person's `trust`; anything granted is trusted; a person's device never introduces or sees the list; an introducer never trusts, nor revokes itself or this device; revoke and forget clear grants and the mailbox | `proptest` (already used in forge-runtime) | Runs on the real Rust, so it can't drift; the state per device is small, so random sequences cover it well. This is the test that would have caught the bug above | Half a day |
| **V3** | Mailbox server rules against a plain in-memory model: random register, deregister, deposit, ack, sweep and clock moves; checks that a deposit is admitted only with a pass its recipient registered, that no pass or recipient ever goes over its quota, that a duplicate is never stored twice and that a pass registered by one recipient is never taken by another | `proptest`, model-based | The rules are SQL, which Lean can't see; a model test checks the SQL itself | Half a day |
| **V4** | Feed random bytes to everything that parses network input: the mailbox server's requests, sealed items (`open`), announcements, `relay/device/1` messages, device lists | `proptest` with arbitrary bytes in CI; `cargo-fuzz` (nightly) for longer runs on the laptop when a parser changes | Decoders of untrusted input are where panics and odd accepts hide; fuzzing finds them, proofs don't | Half a day for the targets; laptop runs are free |
| **V5** | Revocation across several devices: several of the person's devices exchanging device lists in any order, with messages delayed, duplicated or lost; proves a device revoked anywhere is never trusted again anywhere without the person, and that once lists stop changing every device agrees | **Lean** | This is the one place where interleavings across devices matter and tests only sample them; the property ("never, in any order") is what a proof is for. Same pattern as CodeSession: the model names the Rust functions, and V2's tests check the code follows its transitions | Two to three days. Do it when My Devices grows past step 1 (N1), not before |
| **V6** | Message delivery: stored once and its final state reported once, through retries, relaunches and the mailbox | Lean, as the platform plan already lists | Same reason as V5 | Two to three days; after the delivery code stops changing |
| Later | The cryptographic composition (word-code proof bound to both device keys; mailbox announcement and sealing) | Tamarin or ProVerif, or an outside review | A symbolic protocol prover fits this; Lean doesn't. Before Relay or its mailbox is offered to the public, an outside review of this part is worth more than our own model | Days to weeks; not now |

**Not planned, on purpose.**

- **Proving the cryptography.** HPKE, SPAKE2, QUIC and Ed25519 come from reviewed libraries. We check that we *use* them correctly (V4, the existing negative tests), not the algorithms.
- **Kani.** It shines on bounded arithmetic and `unsafe`, and ours has neither: the quota sums are bounded by `MAX_ITEM` and come from SQLite counts. V3 covers the same ground more cheaply.
- **Lean for single-step rules** (a revoked device may not connect, a grant needs trust). They are one function each; V2 checks them on the real code, and a proof of a model of them adds a second copy that can drift.
- **The Keychain.** The logic (never make a new key when the store fails, never fall back to a file on the iPhone, read back before deleting the file) is already tested with a fake store. What remains is iOS behaviour, which only the phone shows.

### How the Lean models stay honest

Today `CodeSession.lean` is tied to the Rust only loosely: the model's header names the Rust it describes, the Rust has an `invariant_holds` that mirrors the model's invariant, and a test replays random sequences through the Rust and checks that invariant after every step. That is sampled invariant checking, not conformance: a change can keep the invariant while departing from the model's transitions, and the test won't notice. For V5 and V6, close both gaps. The Lean model itself runs a seeded set of a few thousand random event sequences (`#eval`) and writes the state after each step to a committed fixture. The Rust tests then replay those sequences through the production code and compare the states step by step, and the Lean CI job fails if the fixture no longer matches what the model produces. Then a change on either side, in the Rust or in the Lean, that shows up in any of those sequences fails a check. Like any test, this samples behaviour: it narrows drift a lot, but it doesn't rule out every change. A hand-written Rust copy of the model would only catch the Rust drifting from that copy, not the copy drifting from Lean. Once V1 is in place, CI re-checks the proof whenever a model changes.

## Recommendation

Take V1 to V4 now: about two days in all, every piece runs in CI, and V2 alone would have caught today's bug. Leave V5 and V6 for when My Devices and delivery stop moving, and the cryptography for an outside review before anything public.
