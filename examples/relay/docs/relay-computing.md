# Relay and the future of computing (ideas)

Status: three ideas from Reuben, 2026-11-17 evening, recorded so they
aren't lost. **Nothing here is built until he says so**, and none of it
changes Relay's roadmap yet. The current milestone stays
[M1 Reliable Trusted-Device Delivery](relay-reliable-delivery.md), then
M2 Personal Device Network. The architectural parent stays
[relay-platform-plan.md](relay-platform-plan.md). The other parked ideas are
[relay-future-directions.md](relay-future-directions.md) (the personal
network operating system, M1 to M8) and [relay-social.md](relay-social.md)
(open social infrastructure).

The three ideas zoom out in turn:

1. [Relay as a personal computing network](#1-relay-as-a-personal-computing-network):
   devices, servers and services used as one coordinated computing
   environment, with capabilities and remote execution moved earlier.
2. [A foundational computing study](#2-a-foundational-computing-study):
   before committing to that roadmap, study what lets independent
   computers cooperate at all (Plan 9, seL4, Erlang, distributed systems)
   and test the abstractions with small prototypes.
3. [The next computing environment](#3-the-next-computing-environment):
   zoom out further, to what computing is and could become, with Relay as
   one possible outcome.

They stack: (3) is the research question, (2) the method, (1) one concrete
product it could lead to. The open question Reuben's last note ends on is
in [Open question](#open-question).

## 1. Relay as a personal computing network

**Statement.** Relay is an open, owner-controlled computing network. It
connects devices, services and compute resources into one environment where
applications and people can discover capabilities, delegate work and share
results. Not a social network, not only a messaging layer, not a replacement
operating system, and not "one virtual computer": Relay coordinates
independent machines with explicit capabilities.

What a person could eventually ask Relay to do: run a workload on the
machine best suited to it; use another device's GPU without hand-built SSH
tunnels; reach files and services across their devices; start a long job on
a server and collect the result later; route an AI request to a local model,
a remote GPU or an approved provider; automate work when a device appears, a
file arrives or a task finishes; give someone access to one resource without
the whole machine.

### Layers

Conceptually separate, but at first modules in `examples/relay/rust`, not
new services or crates:

| Layer | What it does |
|---|---|
| Applications and experiences | AI, automation, remote work, files, developer tools |
| Work and resource orchestration | Task planning, scheduling, execution, results |
| Resource layer | Devices, CPU and GPU, memory, storage, services |
| Capability layer | Named, versioned operations with contracts and permissions |
| Network and delivery | Discovery, routing, connections, reliable delivery (M1, M2) |
| Identity, trust and policy | Device, owner, service, application and workload identity; grants, revocation, audit |

- **Trust never means unrestricted access.** Trusting a server doesn't grant
  its terminal, GPU or files; each is a separate capability with its own
  grant, limits, expiry, approval rule and audit record.
- **Resource inventory.** Each node advertises hardware capacity separately
  from what is available now, with timestamps, confidence and its own
  policy. Availability is a lease that expires, never assumed permanent.
- **Capabilities are contracts:** inputs, outputs, requirements,
  permissions, resource limits, failure modes and a version. Examples:
  `compute.execute`, `ai.inference`, `storage.read`, `storage.write`,
  `terminal.execute`, `media.transcode`, `backup.create`.
- **A task** has an id, declared inputs, requirements, a requested
  capability, an execution policy, a deadline, status and cancellation, an
  output location and a record of what actually happened. Its lifecycle:
  create, check permissions, find eligible nodes, choose one, stage inputs,
  run, then store and deliver the result, or retry, fail or ask for approval.
- **The scheduler starts with simple rules**, not AI: requirements,
  availability, owner policy, cost and latency.

### Hybrid ownership: owner first, provider optional

| Resource | Ownership | Default |
|---|---|---|
| Personal | Your devices and servers | Yours, under your policy |
| Trusted external | A friend's machine, a trusted organisation | Only the capabilities they grant |
| Commercial | A rented GPU or VPS, an external API | Under a declared price and provider policy |

Rules: the owner of a node controls its resources and participation; there
is no mandatory central coordinator, so a personal network keeps working
locally when Relay's optional servers are down; external operators choose
what they expose, to whom and on what terms; identity is portable across
providers; paid resources show their cost or limits before work is sent;
delegation can be withdrawn, and then no new work is accepted.

### The practical first use

Reuben's own mix of hardware: the iPhone as control (submit, approve, watch,
collect), the GTX 1060 machine for small models and background jobs, a P40
workstation for larger models and long jobs, and a VPS (Hetzner) for
always-on coordination, storage and public endpoints. Example: "summarise
these 300 documents and build a searchable index" keeps the documents on
the storage node, splits the work, sends light classification to the 1060
and heavy inference to the P40, keeps the intermediate results, and returns
the finished index to the phone.

### Proposed milestones (not adopted)

| | Milestone | Exit |
|---|---|---|
| M1 | Reliable Delivery (current) | Work is delivered safely and exactly once across interruptions and relaunches |
| M2 | Personal Network | Devices find and reach one another with no addresses or tunnels set by hand |
| M3 | Resource and Capability Model (new priority) | A node discovers another's capabilities and knows exactly what it may request |
| M4 | Remote Execution (new priority) | A task submitted on one device runs on another and returns a verified result, even if the connection drops while it runs |
| M5 | Personal Resource Orchestration | Relay chooses between several eligible machines and manages a workload without the person picking each one |
| M6 | Distributed Data and State | Multi-stage jobs share inputs and results without copying everything by hand |
| M7 | Developer Platform and Extensions | A developer adds a new capability without changing Relay's core |
| M8 | Distributed Intelligence | An AI workload uses the best permitted model and compute across the network |
| M9 | Open Resource Network | Relay safely uses resources outside the owner's own network |

How it maps onto [the current future directions](relay-future-directions.md):
M1 and M2 stay as planned; M3 Universal Exchange becomes a capability built
on delivery and storage; M4 People and Rooms becomes an optional
application-level feature; M5 Capabilities and Extensions splits into the
new M3 (contracts) and the new M7 (ecosystem); M6 Distributed Intelligence
moves after execution and orchestration; M7 Private Service Network is
absorbed into resources, execution and storage; M8 Open Network Ecosystem
becomes the new M9. Chat stays useful as an interface to people and agents,
but doesn't define the platform. Execution comes before intelligence.

### The first architectural proof

Run a remote command through Relay: pair two Linux nodes; the second
advertises `compute.execute`; the first discovers it; the person authorises
one specific task; it runs on the second node in a restricted working
directory; the result and an execution record come back. **Accepted when**,
with the network cut after submission, the task finishes remotely and its
result is collected after reconnecting without running it twice, and after
revocation the next task is refused. For this proof, only a small set of
approved workloads, never arbitrary scripts.

Safeguards that are not negotiable: no unrestricted remote shell as the
default primitive; jobs run with restricted files and processes and with CPU,
memory, storage and time limits; credentials never in task payloads or logs;
cancellation and revocation actually stop work; every task records which
node ran it, under whose authority and what it touched; hardware a node
reports about itself is an unverified claim; availability leases expire.

Possible module boundaries, extracted only when the code warrants it:
`identity.rs`, `trust.rs`, `network/`, `delivery/`, `capabilities.rs`,
`resources.rs`, `tasks.rs`, `executor.rs` (abstract from day one: local
process, remote process, container, GPU worker), `scheduler.rs`,
`artifacts.rs`, `policy.rs`.

Products that could share this one foundation: Relay Compute, Relay
Intelligence, Relay Storage, Relay Automate (for example: a video arrives,
Automate invokes a transcoding capability, Compute picks the machine,
Storage keeps the result, Intelligence classifies it first).

## 2. A foundational computing study

Before committing to the roadmap above, go one level down: **what are the
fundamental abstractions Relay should provide so that higher-level
computing systems can be built on it?** Study distributed systems,
operating systems, networking, storage, scheduling, security, language
runtimes and computer architecture together. Relay stays a user-space
system on ordinary operating systems: no kernel, no replacing Linux.

### Ten areas, most foundational first

| Area | The key question |
|---|---|
| Naming and namespaces | Can an application see a consistent set of resources without knowing where they physically are? |
| Authority and capabilities | Can permission be an object that is passed, narrowed and revoked, rather than a row in a table? |
| Communication | One model (messages, RPC, streams, pub/sub, backpressure) for local processes, other devices and external services? |
| Time, ordering and causality | What does "newer" mean for a task, message, permission or resource state across machines without a shared clock? |
| State and consistency | What happens when two machines change the same thing while disconnected (transactions, event sourcing, CRDTs, logs)? |
| Resource management and scheduling | Decisions from real constraints (CPU, memory, GPU, storage separately), with quotas, admission control, fairness and locality |
| Execution and isolation | The smallest unit of work Relay can run safely and return with a verifiable result (processes, sandboxes, microVMs, WebAssembly, checkpoints) |
| Fault tolerance and recovery | Telling apart work never started, still running, and finished but whose result never arrived (idempotency, durable queues, replication) |
| Composition and interfaces | Combining capabilities without knowing how they are built (dataflow, dependency graphs, declarative workloads, versioning) |
| Observability and control | Can the owner see why a machine was chosen, what ran there, what it used and why it failed (tracing, accounting, audit, provenance)? |

### Systems and papers to learn from (not to copy)

| System | Lesson |
|---|---|
| Plan 9 | One computing environment of terminals, CPU servers and file servers; per-process namespaces; 9P |
| Inferno | A portable runtime across devices (Limbo, Dis, Styx), namespaces |
| seL4 | Capabilities as authority (an object reference plus rights), IPC, policy separated from mechanism |
| Erlang/BEAM | Lightweight processes, message passing, supervision trees, failure isolation |
| Kubernetes | Declarative desired state and reconciliation, scheduling, node health (without its operational weight) |
| Unikraft | Fast-starting, small, isolated workloads |

Reading: Lamport, *Time, Clocks, and the Ordering of Events in a
Distributed System*; the CAP theorem and consistency models; the
impossibility results for consensus and failure detection in asynchronous
systems (what is hard versus what is impossible under given assumptions);
van Steen and Tanenbaum's definition of a distributed system (autonomous
elements that appear as one coherent system, failing independently, with no
shared clock).

### A possible common substrate

Seven primitives from which the rest might be composed: **object**, **name**,
**capability**, **channel**, **process**, **resource** and **state**. A file
is a persistent object; a terminal is a channel to a process; a GPU is a
resource exposed through capabilities; a model server is a process with an
inference interface; a task is an execution request with inputs, constraints
and outputs; a permission is authority over an object or operation; a remote
machine is a collection of objects, processes and resources.

Three questions to look at first:
- **Plan 9's namespaces, modernised:** a per-application view where local
  and remote resources appear under one naming system (an illustration, not
  a literal filesystem: `/relay/devices`, `/compute`, `/storage`,
  `/models`, `/services`, `/processes`).
- **Capability authority across machines:** every meaningful operation an
  authorised invocation of an object, with delegation and narrowing.
- **A portable distributed process model:** start, talk to, watch, cancel and
  recover processes on any machine. Possibly the most important abstraction.

### Research stages, each with a working prototype

| Stage | Study | Prototype |
|---|---|---|
| R0 | Plan 9, Inferno, seL4, Erlang, distributed systems | Architecture comparison |
| R1 | Object identity, naming, namespaces | Reach a file on another machine by a stable logical name, even after its address changes |
| R2 | Capabilities and authority | Give a process temporary read access to that file without its parent folder |
| R3 | Communication and ordering | Send a request to a remote process, disconnect, reconnect, recover the reply |
| R4 | State, persistence, recovery | Two machines change shared state offline and reconcile it under an explicitly chosen consistency model |
| R5 | Processes, resources, execution | Run an isolated workload on another machine within declared limits |
| R6 | Composition and scheduling | Compose several remote processes into one workflow |
| R7 | Integration with Relay | Foundational Relay architecture |

The difference this makes: a scheduler decides where a job runs; a
computing platform provides the abstractions that let jobs, resources,
data and processes cooperate; a distributed substrate defines the rules that
make that platform possible. The substrate is the most interesting thing to
study, with the platform as its eventual product: something like Plan 9's
namespaces, Erlang's process model and capability security together, on
ordinary Linux machines. Learn from them; build only the minimum the product
needs. The next artefact would be a **Relay Foundational Computing Study**
that compares these architectures and ends in tested design decisions,
before the roadmap is restructured.

## 3. The next computing environment

Zoom out further than distributed computing, operating systems or Relay:
**what is computing fundamentally, what might it become, and what kind of
system would be worth building without today's assumptions?** Relay is one
possible outcome, not the starting point.

| Direction | Central idea | What it challenges |
|---|---|---|
| Ambient computing | Computing is everywhere; the phone is an interface, devices contribute resources, applications exist apart from any one machine | The device |
| Living computing | Components appear, adapt and cooperate; agents maintain and repair; workflows emerge (complex adaptive systems, artificial life, multi-agent systems), without becoming uncontrollable | Static architecture |
| Universal computing medium | An environment where people build and reshape their own tools (Kay's Dynabook, Smalltalk, Lisp machines, Unix, *The Humane Interface*): the smallest set of concepts that yields the widest range of experiences | The application |
| Distributed intelligence | Intelligence as infrastructure: resources with intelligent interfaces, local models for immediate work, larger ones when needed, deterministic computing and specialised models alongside general reasoning | Centralised AI |
| Dynamic computing | Capabilities assemble on demand (a video source, a processing capability, an analysis model and a renderer combine into a workflow whose output is a new resource); applications become packaged experiences on a general environment | Prebuilt software |

They aren't exclusive; the most interesting future may combine all five.

Questions to investigate before returning to architecture:
- **What is a computer?** A machine, an execution environment, a network, or
  a collection of capabilities?
- **What is software?** A prewritten program, or something assembled from
  available components?
- **What is an operating system?** Must it manage hardware, or is its real
  job organising computation, resources and interaction?
- **What is an application?** Inherent, or a historical convention?
- **What is intelligence?** A service, a participant, an infrastructure layer,
  or a property of the environment?
- **What should belong to the person?** Devices, data, identity, compute,
  software, agents, perhaps the whole environment?

The research question: **can we create a general-purpose computational
environment in which machines, software, data, intelligence and people
interact as parts of one coherent, adaptable system, without centralised
ownership or dependence on any particular device?** Relay could eventually
implement it, but its current architecture shouldn't be assumed to be the
right foundation.

## Open question

Reuben's last note ends with a choice that decides which research programme
follows:

> Are you more interested in discovering a fundamentally better way to
> organise computing, or in imagining an entirely new kind of computer that
> could eventually replace the way we use computers today?

Unanswered as of 2026-11-17.
