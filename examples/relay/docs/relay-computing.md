# Relay as a personal computing network (one possible direction)

How this fits the rest of Relay: [relay-network-architecture.md](relay-network-architecture.md)
(one secure communication layer; 2026-11-21).

Status: an idea from Reuben, 2026-11-17 21:46, recorded so it isn't lost.
This is **one possible direction emerging from the project, a hypothesis
and not a plan** to turn Relay into a computing platform (see
[relay-open-ended-design-principles.md](relay-open-ended-design-principles.md)).
The positioning, milestones and "strategic decisions" below are the
proposal as written, not adopted decisions.
Decided 21:48: the direction after M2 is chosen once M1 and M2 are done.
**Nothing here is built until he says so.** It proposes a **different
direction** from [relay-social.md](relay-social.md) (the Relay Social idea, recorded 21:28)
and a **reordering** of the milestones after M2 in
[relay-future-directions.md](relay-future-directions.md). Which direction
Relay takes after M2 is Reuben's open decision; M1 and M2 are the same in
every version. One level below this sits the
[foundations study](relay-foundations-study.md).

## Positioning

> Relay is an open, owner-controlled computing network. It connects
> devices, services and compute resources into one environment where
> applications and people can discover capabilities, delegate work and
> share results.

Not a social network, not only a messaging layer, not a replacement
operating system, and not a "distributed operating system" in the strict
sense: Relay doesn't control kernels, schedulers or memory. It gives
independent machines a common, secure way to cooperate. Relay coordinates
**resources and work**, not just devices and messages.

What a person could ask: run a workload on the best-suited machine; use a
GPU on another device without SSH tunnels; reach files and services across
devices; start a long job and collect the result later; route an AI request
to a local model, a remote GPU or an approved provider; automate work when
a device appears, a file arrives or a task ends; give someone access to one
resource, not the whole machine.

Resources: personal devices (phone, laptop, desktop, iMac); owned servers
(home lab, VPS, dedicated); compute (CPU, GPU, RAM, inference); services
and storage (databases, file stores, APIs, backups, apps). Relay's flow:
discovery, permissions, scheduling, execution, results.

## Six layers (modules in the Relay Rust core at first, not services)

| Layer | What |
|---|---|
| F Applications and developer platform | Relay AI, Compute, Storage, Automate, Work; third-party extensions |
| E Work orchestration | Task planning, scheduling, execution, results |
| C Resource discovery | Devices, CPU/GPU, storage, services |
| D Capability system | Interfaces, permissions, contracts |
| B Network and delivery | Discovery, routing, connections, reliable delivery (what M1 and M2 build) |
| A Identity, trust and policy | Device, owner, service, application and workload identity; grants, revocation, audit |

- **Trust never means unrestricted access.** Trusting a server doesn't
  grant its terminal, GPU, files or apps: each is a capability with its own
  grant. Grants support specific operations and callers, resource limits,
  expiry, revocation, approval and an audit history.
- **Resource inventory:** each node advertises hardware capacity and
  currently available capacity separately, with timestamps, measurement
  confidence and its own policy. Advertisements are leases that expire;
  nothing is assumed available because it was once advertised.
- **Capability:** a named, versioned operation a node agrees to perform
  (`compute.execute`, `ai.inference`, `storage.read`, `storage.write`,
  `terminal.execute`, `media.transcode`, `backup.create`), with a contract:
  inputs, outputs, requirements, permissions, limits, failure modes,
  version. New hardware and services don't need core rewrites.
- **Workload:** id, declared input, requirements, requested capability,
  execution policy, deadline, status and cancellation, output location,
  record of what happened. Lifecycle: create, validate permissions, find
  eligible resources, select a node, prepare inputs, execute, then store
  and deliver the result, or retry, fail or ask for approval. The scheduler
  starts with simple rules (requirements, availability, ownership policy,
  cost, latency), not AI.
- **No single virtual computer:** no pooled GPU memory or merged RAM. A GPU
  job on the P40 is a job on that server; Relay handles submission,
  authentication, scheduling, monitoring and results.

## Hybrid ownership: owner-first, provider-optional

| Resource | Ownership | Default policy |
|---|---|---|
| Personal | Your own devices and servers | Available to you under your policy |
| Trusted external | A friend's machine, a trusted organisation | Only explicitly granted capabilities |
| Commercial provider | A rented GPU, VPS or API | Under a declared price and provider policy |

Rules: local ownership; no mandatory central coordinator (the personal
network works when Relay's optional infrastructure is down); explicit
external participation; portable identity; costs shown before work is
dispatched; revocable delegation. A provider can expose a GPU without its
filesystem or shell.

## The first use case: Reuben's own hardware

iPhone (control: submit, approve, watch, collect); GTX 1060 machine (small
models, light inference, background tasks); P40 workstation (larger
models, image generation, long jobs); VPS and providers (always-on
coordination, storage, public endpoints). Example: summarise and index 300
documents, with documents kept on the storage node, light classification
on the 1060, heavy inference on the P40, intermediate results stored, the
index returned to the phone.

## Proposed milestones (replacing M3 onwards)

| | Milestone | Exit |
|---|---|---|
| M1 | Reliable Delivery (unchanged) | Work is delivered safely across interruptions and relaunches: retried for up to 30 days (then marked Failed), with duplicates dropped by id |
| M2 | Personal Network (unchanged) | Devices find and reach each other with no manual addresses or tunnels |
| M3 | Resource and Capability Model | A node discovers another's capabilities and knows exactly what it may request |
| M4 | Remote Execution | Submit on one device, run on another, get a verified result even if the connection drops during execution |
| M5 | Personal Resource Orchestration | Relay chooses between eligible machines and manages a workload without the person picking each one |
| M6 | Distributed Data and State | Multi-stage jobs share inputs and results without manual copying |
| M7 | Developer Platform and Extensions | An outside developer registers a new capability without changing Relay Core |
| M8 | Distributed Intelligence | An AI workload uses the best permitted model and compute across the network |
| M9 | Open Resource Network | Relay safely uses resources outside the owner's network |

What happens to the current future directions: M1 and M2 kept; M3
Universal Exchange kept as a capability on delivery and storage; **M4 People
and Rooms demoted to an optional application-level feature**; M5
Capabilities split (contracts to new M3, ecosystem to new M7); M6
Distributed Intelligence kept but after execution and orchestration; M7
Private Service Network absorbed into resources, execution and storage; M8
Open Network becomes new M9. Chat stays as an interface for people and
agents, without defining the platform. Execution comes before intelligence:
AI is one workload among video processing, backups, builds, indexing and
data processing.

## The first architectural proof

Run a remote command through Relay: pair two Linux nodes; the second
advertises `compute.execute`; the first discovers it; the person authorises
one specific task; Relay submits it; the second runs it in a restricted
working directory; the result and execution record come back.

Acceptance: disconnect after submission, let the task finish, reconnect,
and get the result without running the task twice; revoke the permission,
and later tasks are refused. Only a small set of approved workloads, never
arbitrary scripts, in the first execution milestone.

Module boundaries (conceptual, inside `examples/relay/rust`, extracted only
when warranted): identity, trust, network, delivery, capabilities,
resources, tasks, executor (abstract from day one: local process, remote
process, container, GPU worker), scheduler, artifacts, policy.

## Safeguards

No unrestricted remote shell as the default primitive; jobs run with
restricted filesystem and process permissions; an explicit grant for every
capability; CPU, memory, storage and time limits; no credentials in
payloads or logs; cancellation and revocation that take effect; a record of
which node ran a task, under whose authority, and what it touched; external
nodes' hardware claims untrusted until verified; availability leases that
expire.

## Three strategic decisions proposed

1. Relay is a computing platform, not a communications product; messaging
   and transfer are supporting capabilities.
2. The capability model is the central abstraction.
3. Execution comes before intelligence.

**Next step, when Reuben says so:** a Relay Computing Architecture
specification (node, resource, capability, permission, task and artifact
contracts), mapping M3 to M9 onto the M1 and M2 code without disturbing
the work under way.

## Notes for the decision (Claude, 2026-11-17)

- **What's common:** M1 and M2 are unchanged in this, in
  `relay-future-directions.md` and in [relay-social.md](relay-social.md), so nothing in
  progress changes whichever direction wins.
- **It conflicts with Relay Social as written:** that doc's S1, S3 and S5
  wait for M4 People and Rooms, which this proposal demotes, and it says
  "not a social network". Both could live as layers on one capability
  platform (Social as an application, as Relay Social already proposes),
  but the milestone order after M2 has to pick one.
- **It builds on what exists:** the trust model's per-device
  `receives_from` grant is already the first per-capability permission;
  M1's outbox already retries delivery for up to 30 days (then marks it
  Failed) and drops duplicates by id; "result without running twice" in the remote-execution
  proof needs more than that (the job's own id, checked before it runs,
  and its result kept to answer a repeat); AI Lab
  (A1 on the GTX 1060) and `relay serve-ai` are a natural first
  `ai.inference` capability.
- **It touches the merged Chat plan:** Chat merged into Relay and the front
  door is Chats, Send, Devices. Under this direction Chats stays, but as an
  interface rather than the product's centre.
