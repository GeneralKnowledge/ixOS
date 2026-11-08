//! Domain core of the Good Dog demo ([docs/good-dog-demo.md]).
//!
//! A port of the Good Dog web app's plan engine (GD0). It has two parts with a
//! one-way dependency, as in the web app:
//!
//! - [`coaching`]: a domain-neutral engine. It builds a daily plan, swaps one
//!   item for another, and moves a skill through its progress states. It never
//!   imports [`dog_training`] and never uses dog vocabulary
//!   (`tests/boundaries.rs` checks this).
//! - [`dog_training`]: the dog-specific policy (eligibility by life stage, the
//!   daily budget, starter exercises, goal matching, "why today" wording) and
//!   the exercise content bundled from the web app.
//!
//! Everything here is pure: no storage, network, clock or randomness. The
//! caller passes the time in and gives plan items their ids when it stores
//! them. `tests/golden.rs` and `tests/parity.rs` check the output against
//! plans recorded from the TypeScript engine.
//!
//! [docs/good-dog-demo.md]: https://github.com/GeneralKnowledge/ixOS/blob/main/docs/good-dog-demo.md

pub mod coaching;
pub mod dog_training;
