# Relay Foundational Computing Study (research plan, parked)

How this fits the rest of Relay: [relay-network-architecture.md](relay-network-architecture.md)
(one secure communication layer; 2026-11-21).

Status: proposed by Reuben, 2026-11-17 21:48. Recorded so it isn't lost.
**Not started.** M1 comes first, then M2; the direction after M2 is
decided once both are done (Reuben's choice, 21:48). This study sits one
level below [relay-computing.md](relay-computing.md) and
[relay-social.md](relay-social.md): it is meant to produce tested design decisions
*before* the milestones after M2 are restructured.

Under the [open-ended design principles](relay-open-ended-design-principles.md)
the research is organised around questions, not a predetermined product.
Some may lead nowhere; others may reveal something more interesting.

## Open questions (Reuben, 22:31)

| Area | Question |
|---|---|
| Computing | What could a computer become if it wasn't tied to one device? |
| Identity | What do ownership and identity mean across machines, people and software? |
| Communication | What happens when everything can communicate directly? |
| Intelligence | Could intelligence become a capability of an environment rather than an application? |
| Resources | Could computing, storage and other resources be shared without central ownership? |
| Experience | What replaces the conventional distinction between apps, operating systems and services? |
| Autonomy | How much should a system organise itself, and how much control should its owner keep? |

The sections below are one way into these questions: the abstractions
underneath, and a programme of small prototypes.

## Research question

What fundamental abstractions should Relay provide so that higher-level
computing systems (social, computing, AI, storage, automation) can be
built on top of it?

Scope: user space only. No kernel, no scheduler or memory control. Don't
build all of the studied systems inside Relay; take their ideas.

## Ten areas

1. Naming and namespaces
2. Authority and capabilities
3. Communication
4. Time, ordering and causality
5. State and consistency
6. Resource management and scheduling
7. Execution and isolation
8. Fault tolerance and recovery
9. Composition and interfaces
10. Observability

## What to study

- **Systems:** Plan 9 (per-process namespaces, 9P, everything as a file
  server), Inferno (Styx, portable Dis processes), seL4 (capability-based
  authority, formal guarantees), Erlang/BEAM (processes, mailboxes, links,
  supervision, "let it crash"), Kubernetes (declarative desired state and
  reconciliation loops, without its complexity), Unikraft (minimal,
  specialised execution units).
- **Theory:** Lamport, "Time, Clocks, and the Ordering of Events in a
  Distributed System"; CAP and the spectrum of consistency models;
  consensus and the failure-detection impossibility results (FLP and
  related).

## Seven candidate primitives

Object, Name, Capability, Channel, Process, Resource, State.

## Programme

| | Topic | Practical prototype |
|---|---|---|
| R0 | Architecture comparison | Written comparison of the systems above against the ten areas; no code |
| R1 | Naming | Reach a file by a stable logical name after its address changes |
| R2 | Capabilities | Grant temporary read of one file, not its parent directory |
| R3 | Communication | Request, disconnect, reconnect, recover the response |
| R4 | State | Offline changes on two machines reconciled under a chosen consistency model |
| R5 | Execution | An isolated workload with resource limits |
| R6 | Composition | Several remote processes run as one workflow |
| R7 | Integration with Relay | Fold the chosen primitives into the Relay core |

## Three questions first

1. **A modernised Plan 9 namespace.** Could Relay expose a per-person
   namespace such as `/relay/devices`, `/relay/compute`, `/relay/storage`,
   `/relay/models`, `/relay/services`, `/relay/processes` (illustrative
   only)? What is a name bound to, and who sees which view?
2. **Capability authority across machines.** How is a capability created,
   delegated, attenuated, expired and revoked when the holder and the
   resource are on different machines that may be offline?
3. **A portable distributed process model.** What is a Relay "process":
   identity, mailbox, supervision, where it runs, and what survives a
   disconnect or relaunch?

## How it relates to work already done (Claude's notes)

- R3 is largely what M1 already proves: the outbox delivers by id exactly
  once across disconnections and relaunches, and the receiver remembers
  ids. The study would describe it in these terms rather than rebuild it.
- R2 has a seed in the trust model's per-device grant (`receives_from`),
  which is a coarse capability; R2 asks for something finer (one object,
  time-limited, revocable).
- R1 overlaps with M2 (devices found without manual addresses): device
  names are the first layer of a namespace.
- R0 is docs only and could be done without touching code; the rest are
  prototypes and wait for the decision after M2.

## Zoomed out further: the next computing environment (Reuben, 21:50)

Reuben proposed going above Relay altogether: study what computing is and
might become, with Relay as one possible outcome rather than the starting
point. Research question: *can we create a general-purpose computational
environment in which machines, software, data, intelligence and people
interact as parts of one coherent, adaptable system, without centralised
ownership or dependence on any particular device?*

Five directions (not exclusive): ambient computing (challenges the device),
living computing (static architecture), universal computing medium (the
application; Kay/Dynabook, Smalltalk, Lisp machines, Unix, The Humane
Interface), distributed intelligence (centralised AI; deterministic code,
specialised models and general reasoning combined), dynamic computing
(prebuilt software; capabilities assembled into workflows on demand).

Six questions: what is a computer, software, an operating system, an
application, intelligence, and what should belong to the user.

Open framing question he posed: a fundamentally better way to *organise*
computing, or an entirely new kind of *computer* that replaces today's.
Claude recommended the first (21:51): history favours systems that absorb existing machines over ones that replace them, it produces testable claims, it fits the hardware at hand, and a new kind of computer can still grow out of it. M1 (#321) still finishes
first; this stays a study until he says otherwise.
