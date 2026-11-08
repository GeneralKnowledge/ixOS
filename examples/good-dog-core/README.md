# good-dog-core

The Rust core of the Good Dog demo ([plan](../../docs/good-dog-demo.md)): the plan engine
ported from the Good Dog web app, its dog-training policy, and the exercise content.
This is milestone GD0. There is no app, storage or FFI yet.

| Module | What |
|---|---|
| `coaching` | Domain-neutral engine: daily plan, "something different" swaps, the skill-progress state machine, welfare cooldowns. Never names the domain (`tests/boundaries.rs`) |
| `dog_training` | `DogTrainingPolicy` (life stages, daily budget, starters, goal keywords, "why today" wording) and the bundled content |

The engine is pure. The caller passes the time in (Unix milliseconds), and storage gives
plan items their ids.

## Tests

```bash
cargo test -p good-dog-core
```

| Test | Proves |
|---|---|
| `golden.rs` | The web app's own golden fixture: 100 recorded plans, reproduced exactly |
| `parity.rs` | All 900 golden scenarios plus a swap for each, and 315 simulated weeks (plans, swaps, outcomes fed back into progress, welfare concerns), matching the TypeScript engine |
| `engine.rs` | Engine and progression behaviour with a made-up domain, ported from the web app's tests |
| `content.rs` | The bundled content parses and its references resolve |
| `boundaries.rs` | The engine stays domain-neutral |

When the web app's engine or exercises change, re-export and re-record from a checkout:

```bash
examples/good-dog-core/scripts/export-from-web.sh /path/to/Good-Dog
cargo test -p good-dog-core   # fails where the Rust port must follow the change
```

The script needs Node 22.6 or later and no `npm install`.
