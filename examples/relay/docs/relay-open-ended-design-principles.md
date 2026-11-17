# Relay: open-ended design principles

Status: agreed by Reuben, 2026-11-17 22:31. This is a standing principle
for Relay, not a plan. It applies to every Relay document and PR.

**Relay is allowed to discover what it is becoming.** Deciding too early
that it is a messaging app, a distributed operating system, a personal
cloud or a computing platform could damage it. It may turn into something
that fits none of those categories.

## Three levels

| Level | What it is | What it needs |
|---|---|---|
| What Relay is today | A working system for connecting trusted devices and exchanging information | Concrete engineering requirements, tests and phone checks ([M1](relay-reliable-delivery.md), then M2) |
| What Relay is exploring | New ways for devices, people, software, services and intelligence to interact | Experiments and research |
| What Relay might become | Deliberately undecided | Nothing yet: it stays open |

The proposed future must not dictate the present architecture.

## Rules

- Relay has no fixed final product category.
- Its current functionality is not necessarily its ultimate purpose.
- Research directions are hypotheses, not commitments. That includes
  [Relay Social](relay-social.md), [Relay as a personal computing
  network](relay-computing.md) and the [foundations
  study](relay-foundations-study.md).
- No future direction becomes a roadmap commitment without practical
  evidence: a prototype, a real use, or a limit found in practice.
- Architecture preserves useful optionality without speculative
  over-engineering.
- Contributors don't have to resolve what Relay ultimately is when they
  propose a feature or an architecture change.
- Optionality is not a compatibility obligation. Experiments are
  disposable: keeping a future possible never means supporting every
  experiment forever.

## Design principles

- **Small primitives, not a giant platform.** Build useful capabilities
  without deciding their ultimate purpose.
- **Composability.** Capabilities should work together in ways nobody
  anticipated.
- **Owner control.** People keep authority over their devices, data and
  resources.
- **Replaceability.** No single protocol, model, vendor or device becomes
  indispensable.
- **Local-first, not local-only.** Relay stays useful without a central
  service where practical. External infrastructure, remote computation
  and shared services are welcome when they help, as long as Relay
  doesn't depend on them.
- **Emergence.** New uses can grow from combinations of existing
  capabilities.
- **Reversibility.** An experiment can be tried without committing the
  whole project to it.

Openness does not mean building abstractions for every imaginable future.
It means avoiding unnecessary decisions that close off plausible ones.
In practice, today's code already follows this where it counts: trust,
delivery and conversation are kept separate
([relay-reliable-delivery.md](relay-reliable-delivery.md#three-separate-concerns)),
and the Rust core stays a set of modules until real use shows where the
boundaries belong.

## Reviewing a Relay proposal

This page is the conceptual reference for Relay proposals and PRs. Ask:

1. Does this solve a real problem today?
2. Does it preserve useful possibilities?
3. Is it committing us to an assumed future?
4. Can we reverse the decision if we learn something different?

## How Relay moves

| When | What |
|---|---|
| Now | Build Relay: reliable delivery, device networking, real-world use |
| Alongside | Explore computing: other architectures, historical ideas, new possibilities |
| Eventually | Follow the discoveries: prototypes, unexpected uses and practical limits decide what Relay becomes |

Relay starts by connecting devices. That may turn out to be the first
step towards a different way of computing, and nobody needs to know yet
what that looks like.
