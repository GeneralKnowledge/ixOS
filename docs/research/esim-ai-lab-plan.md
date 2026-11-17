# Focus plan: eSIM product and AI Lab

Status: **proposed working plan, written by Reuben** (2026-11-17). Planning only: no code yet. All product and company names are provisional.

Priority: eSIM commercial readiness, and AI Lab technical validation.

This page is Reuben's focus plan, kept close to his wording. Notes marked **From the code** are additions from reading ixOS and the VPN repo. They record what already exists or what will get in the way; they don't change the plan's scope. It replaces an earlier draft of this PR that also planned the VPN app.

## 1. Overall direction

Two independent workstreams:

1. **eSIM:** turn the existing eSIM backend and provider integration into a reliable, commercially viable travel eSIM product, then a native iOS app.
2. **AI Lab:** an experimental AI environment inside Device Lab, using Apple's on-device models first and the existing Dell laptop's GTX 1060 as the first external inference machine.

They share infrastructure only where there is a demonstrated need. Neither needs the native VPN app, Relay, or a new general-purpose framework.

### Out of scope for this phase

- A native VPN tunnel.
- A VPN App Store launch.
- Relay integration.
- Permanent brand identity or naming.
- Shared Rust crates built ahead of need.
- Large-scale AI infrastructure or multi-server orchestration.
- Growing Device Lab into a general-purpose AI platform before the first experiments work.

### Names used elsewhere

- This plan's **A0 to A3** are AI Lab phases. They are not the milestones of the same names in [forge-ai-plan.md](../forge-ai-plan.md), where A0 (the shared `ai` module) and A1 (Relay asks) are already merged. This plan takes over from that plan's A2 onwards; its Relay steps are deferred.
- The VPN and eSIM plan of 2026-11-15 ([VPN and eSIM apps plan](https://claude.ai/artifact/HqXN9pQXSVi3BYZhYAJgYR)) is superseded where they differ. Most importantly, there is no shared eSIM and VPN Rust crate now.

## 2. Workstream A: eSIM

### Objective

A straightforward travel eSIM product: a customer buys a data package, receives an eSIM, installs it, and understands how to use it.

The VPN repository ([GeneralKnowledge/VPN](https://github.com/GeneralKnowledge/VPN)) stays the backend and commercial foundation. The native eSIM app is a separate product, not part of the future VPN client.

> **From the code** (VPN repo main `bbffa86`, 2026-10-12, still current on 2026-11-17):
> - eSIM sales are one-off orders (browse, pay once, issue), chosen by hostname next to the VPN product.
> - `ESIM_PROVIDER=mock` is the default; ResellPortal needs a wallet balance for live orders ([esim-resellportal.md](https://github.com/GeneralKnowledge/VPN/blob/bbffa86/docs/esim-resellportal.md)).
> - An order response carries `qr_code_url`, `activation_url`, `iccid` and `esim_status`. What format `activation_url` uses (Apple's setup link, an LPA string or a web page) is not documented and is a question for E1.
> - Retail prices are wholesale times a flat markup (`retailMarkup`, 1.4) until an explicit price map replaces it.
> - The mock checkout is a test path, not a production payment flow. Real eSIM payment is step 3 of E0.
> - PR #17 (branch `claude/project-thread-bvh8b8`, 2026-10-22, four commits) closes free-access holes. It refuses a production boot with mock crypto billing, or with mock billing next to a live eSIM provider. It refuses an eSIM checkout that can never take payment, trusts only a known proxy for the client IP, refuses cross-site API writes, and stops Stripe renewals being counted twice. It belongs at the start of E0.

### Phase E0: validate the existing business

Priority: immediate.

Before building a native app, prove that the backend can reliably fulfil a real purchase.

1. Review and resolve VPN repo PR #17.
2. Confirm the ResellPortal integration and wallet behaviour.
3. Implement Stripe one-time eSIM Checkout. It doesn't exist yet: with any billing provider other than mock, the checkout route returns HTTP 501 (since PR #17, before it creates an order). This is real implementation work, not configuration:
   - create a payment session for each eSIM order and track it against the order;
   - provision only after verified payment confirmation (the signed webhook, not the browser's return);
   - make webhook processing idempotent, so a repeated delivery never provisions twice;
   - handle payment success followed by provisioning failure (retry, then refund or manual resolution, never a silent loss);
   - handle refunds and reconciliation between Stripe and ResellPortal records.
4. Restore checkout only when those payment and provisioning safeguards are in place and tested.
5. Complete an actual paid eSIM purchase.
6. Verify provider provisioning and the delivery of activation information.
7. Test successful payments, declined payments, duplicate webhook delivery, failed provisioning, refunds, cancellations and expiry, and reconciliation between payment and provider records.

Exit criteria:

- A customer can complete a real purchase.
- The provider provisions the correct eSIM.
- The customer receives usable installation information.
- No eSIM can be issued through an unpaid or duplicated order.
- Payment and provisioning failures have recoverable states.
- The order lifecycle can be audited.

The existing dashboard and provider integration are not proof of a working commercial checkout until this is demonstrated.

### Phase E1: eSIM activation experiment

Priority: immediate; can run alongside E0.

Test the native iOS installation experience before committing to a complete app.

1. Obtain a test eSIM.
2. Test the provider's supported activation-link format on the iPhone.
3. Investigate QR-code installation.
4. Document manual activation details as a fallback.
5. Test what happens when:
   - the eSIM is already installed;
   - the activation link is opened on an unsupported device;
   - the user leaves the process halfway through;
   - the eSIM is already consumed or expired.
6. Record which installation methods work with the provider and supported devices.

Exit criteria: a reliable, documented installation journey with realistic fallback instructions.

Don't assume every provider supports the same activation methods.

> **From the code and Apple:**
> - A non-carrier app can't install an eSIM itself (`CTCellularPlanProvisioning` is carrier-only).
> - Travel eSIM apps open Apple's eSIM setup link (`esimsetup.apple.com/...?carddata=LPA:1$...`, iOS 17.4 and later), which hands the code to Settings. Apple doesn't document the link, so E1 is the place to prove it.
> - None of this needs an entitlement, so E1 works on the free Apple team. A throwaway screen in Device Lab, or simply opening the link from Notes or Safari, is enough. No new app is needed for E1.
> - The one iPhone 15 Pro Max can test "already installed" and "consumed", but not "unsupported device". That case needs a device without eSIM, or a written note of what Apple shows there.

### Phase E2: native eSIM application

Priority: after E0 and E1.

The native iOS app, built around the existing backend.

Core features:

- Browse available packages; see destination and coverage, data allowance, validity and price.
- Buy a package; see order and payment status.
- Receive installation instructions; launch supported activation flows.
- Show guidance for purchased and installed eSIMs; show remaining data and expiry where the provider reports them.
- Support and troubleshooting.

Technical principles:

- The existing backend stays the source of truth for purchases and orders.
- A small, explicit API contract.
- Payment and provisioning logic stays on the server.
- Installation and device-specific behaviour stays in the iOS app.
- eSIM-specific Rust lives in the app first.
- No shared Rust crate just because the future VPN app might use something similar. Shared account or session code is extracted only when a second, independent app actually uses the same implementation.

Exit criteria: a customer can complete the whole purchase-to-installation journey without the web dashboard.

> **From the code:**
> - Today's web auth is browser cookies, so the "small, explicit API contract" needs token auth for the app. It goes in the VPN repo.
> - The app lives in the VPN repo and depends on ixOS by git, as forge-lab does. ixOS must never depend on it ([areas.md](../areas.md)), so it can't sit inside Device Lab and takes one of the free team's three app slots while it is tested.
> - Card checkout in a web sheet that returns by a custom URL scheme needs no paid entitlement. Returning by universal link (associated domains) and Apple Pay both need the paid team. App Store rule 3.1.3(e) points to card payment rather than in-app purchase for data used outside the app; that is a reading of the rule, not Apple's wording.

### Phase E3: commercial launch preparation

Priority: after the native app works.

- Confirm provider pricing and margins.
- Set up customer support and refund procedures.
- Define package availability and destination coverage.
- Add operational monitoring.
- Review privacy, terms and customer-facing disclosures.
- Test the complete journey with a small group of users.
- A repeatable process for provider failures.

### Gate before commercial launch: the company

A new UK Ltd will be needed for the intended business structure and for future organisation-enrolled VPN distribution on Apple.

- **Not a blocker for technical validation.** E0's test purchases and E1's activation experiments go ahead without it.
- **A blocker for public commercial orders.** Before the product takes orders from the public under the intended structure, these are in place: the Ltd registered and its ongoing obligations understood, business banking, the payment provider's account in the company's name, and accounting and tax responsibilities.
- Apple organisation enrolment belongs to the same set-up, but it is only needed for an App Store release, and for the VPN app later.

Start it during E2 so it is ready by E3, not after.

## 3. Workstream B: AI Lab

### Objective

Make AI Lab a practical experimental environment inside the existing Device Lab app. The aim is to explore the boundary between Apple's on-device intelligence and external local inference, not to build another generic chatbot.

### Hardware

The primary external machine is the Dell G3 laptop with a GTX 1060 (6 GB VRAM).

The GPU is an experimental resource, not an always-available service. Don't start workloads on it or interfere with other GPU jobs without Reuben's explicit approval. If this is the same laptop that runs the build queue, note that the queue covers builds only: a model server running next to a build competes with it for memory.

The Hetzner and Oracle machines stay available for ordinary infrastructure, but the first AI Lab work doesn't depend on them.

> **From the code:** what already exists, so A0 and A1 are mostly wiring, not new framework:
> - `forge_services::ai` (merged in #196) has one request shape (OpenAI chat completions, text only), streaming into a `TextSink` with real stopping, availability reasons, and the answer checker for small models.
> - `ForgeAppleModel` is the Swift adapter for Foundation Models. Chat and Pocket Library already use it, and Foundation Models has worked on the phone on the free team since L0 (2026-11-07).
> - `OpenAiServer` talks to any OpenAI-compatible server (llama.cpp server, Ollama, LM Studio) with streamed answers. It is tested against a fake server only.
> - Device Lab already has the `device_status` service (memory, thermal state) for the resource numbers.

### Phase A0: AI Lab foundation

Priority: immediate.

AI Lab as a section of Device Lab.

1. The AI Lab screen and navigation.
2. Identify the available Apple on-device model capabilities.
3. A simple model availability and capability check.
4. A basic prompt-and-response interface.
5. Streaming output where the API permits it.
6. Cancellation and Stop.
7. Response timings and relevant diagnostics.
8. Clear states for unavailable models and unsupported devices.

First experiment, Apple models only, with no external hosting yet. Test text generation, instruction following, summarisation, structured output, streaming, cancellation, response latency, and memory and resource behaviour.

Exit criteria: the iPhone can run repeatable experiments through AI Lab, with reliable cancellation, clear diagnostics and useful timing information.

> **From the code:** structured output is the one item with a known risk, never tested. Foundation Models' `@Generable` is a Swift macro, and Apple ships its compiler plugin inside Xcode, built for macOS. A Linux compiler can't run that plugin, so the xtool build most likely fails ([local-ai.md](../local-ai.md) lists it as a risk). Plain Swift macros such as `@Observable` ship with the toolchain and already build with xtool. A0 settles it by trying: one `@Generable` struct in the AI Lab and one run of the laptop build check. If that fails, `DynamicGenerationSchema` gives the same guided generation with the shape described at run time, with no macro.

### Phase A1: GTX 1060 inference connection

Priority: after A0.

Connect AI Lab to the laptop's existing local inference environment. Treat the laptop as an external experimental model host, not part of a distributed platform.

1. Identify the existing inference service and its supported API.
2. Define a small, explicit connection contract.
3. Secure connectivity from the iPhone.
4. Show host availability and model status.
5. Send prompts and receive streaming responses.
6. Cancellation and connection-failure handling.
7. Record inference timings and resource usage.

Initial models: a small model that fits the GTX 1060's 6 GB VRAM and the machine's limited system memory. The existing Qwen3.5-4B direction can be evaluated, but its quantisation, memory use, context length and speed must be measured before it becomes the default.

Compare Apple on-device inference and GTX 1060 inference on response quality, time to first token, generation speed, memory use and reliability.

Exit criteria: AI Lab can switch between Apple on-device inference and the laptop's inference service, and shows meaningful performance differences.

> **From the code:**
> - The contract is OpenAI chat completions with streaming, already spoken by `OpenAiServer`, so A1 needs no new protocol.
> - Secure connectivity is meant to be Tailscale. **Prerequisite to verify at the start of A1:** the iPhone and the GTX 1060 laptop are both on the tailnet and reach each other. (The build laptop and the iPhone were on it on 2026-11-16; whether that is the same machine as the Dell G3 isn't recorded.) The model server listens only on the laptop's tailnet address, never on the LAN or the internet, and no GPU work starts without Reuben's approval.
> - Plain http inside the tailnet needs an App Transport Security exception, as Soundscape already has for its take server.
> - Task 1 ("identify the existing inference service") is a read-only look and is done before anything is started on the laptop.

### Phase A2: model experimentation

Priority: after A1.

A small experimental model catalogue once the connection works. Possible experiments: small general-purpose models, specialist models for classification and extraction, embeddings and retrieval, lightweight local vision models, and comparisons of quantisation and context window.

No model marketplace or orchestration framework. Each experiment answers a specific question and produces measurable results.

### Phase A3: background and continuous operation

Priority: deferred.

Only after A0 to A2 have shown value: background model downloads, model management, persistent experiment history, lightweight specialist models, larger external hosts, and background processing and task queues.

No Apple Background Assets, complex asset delivery or multi-machine orchestration until a concrete workload needs them. Background Assets, if it is ever needed:

- Ordinary downloads (`URLSession`, including background transfers) don't need Background Assets at all.
- Self-hosted asset packs: the framework and signing requirements still need checking before anyone relies on them.
- Apple-hosted asset packs: storage comes with the paid Apple Developer Program membership.

## 4. Shared architecture and boundaries

| Component | Responsibility |
|---|---|
| Device Lab | Hosts AI Lab experiments and device diagnostics |
| AI Lab | Model selection, prompts, experiments and inference diagnostics |
| GTX 1060 host | External local inference |
| Existing VPN backend | eSIM products, orders, payments and provisioning |
| Native eSIM app | Customer purchase and installation experience |
| Forge | Reusable platform capabilities proven by real apps |

Architecture rules:

1. No shared abstractions just because two future apps might need them.
2. eSIM business rules stay out of Forge.
3. AI model experiments stay out of the eSIM backend.
4. Relay stays independent.
5. API boundaries stay explicit.
6. A working end-to-end prototype beats more architectural layers.
7. Reusable code is extracted when real use shows the common need.

## 5. Implementation sequence

| Order | Milestone | Outcome |
|---|---|---|
| 1 | E0 | Stripe one-time eSIM checkout built; paid eSIM provisioning proven |
| 2 | A0 | A working Apple-model AI Lab |
| 3 | E1 | eSIM installation proven on the iPhone |
| 4 | A1 | AI Lab connected to the GTX 1060 |
| 5 | E2 | Native eSIM purchase and installation app |
| 6 | A2 | Repeatable model experiments |
| 7 | E3 | Commercial launch preparation, behind the company gate |
| 8 | A3 | Background and continuous AI experiments |

E0, A0 and E1 can overlap. The order shows dependencies; it doesn't require finishing one workstream before touching the other.

## 6. Definition of success

**eSIM:** a real customer can browse an available package, pay, receive a provisioned eSIM, install it on their iPhone, understand its validity and usage, and get help if something goes wrong. The business has a reliable, auditable fulfilment process.

**AI Lab:** a user can run Apple on-device experiments, connect to the GTX 1060 host, switch between inference locations, compare quality and performance, cancel work reliably, and inspect results and diagnostics. The result is a useful experimental tool, not a speculative AI platform.

## 7. Deferred roadmap

Still valid ideas that don't take current capacity:

- The native VPN app and tunnel. When it returns, it needs a paid Apple team enrolled as an organisation (Network Extension, App Review 5.4) and app-extension support in Forge.
- VPN subscription and server-location integration.
- Relay-based AI connectivity (forge-ai-plan's Relay steps).
- Multi-host inference orchestration.
- A general-purpose shared AI runtime.
- Shared eSIM and VPN Rust libraries.
- Permanent Northstar branding.
- Advanced background model lifecycle management.

Guiding principle: prove the eSIM business and prove the AI Lab on real hardware. Expand only when those two give evidence that the next layer is worth it.

## 8. Open points before starting

The plan's decisions are made. Each point below is a fact or a resource needed before a phase can start:

1. **E0:** VPN repo PR #17 needs Reuben's review and merge. The repo is outside ixOS, and this thread can only read it.
2. **E0 and E1:** ResellPortal credentials, a funded wallet and a real payment account are Reuben's. A test eSIM for E1 is a real purchase.
3. **A1:** the go-ahead to look at the laptop's existing inference service, and later to start a model on its GPU, comes from Reuben in the thread that does the work.
4. **A0:** comes first in ixOS, as one PR for the AI Lab screen in Device Lab, with a short phone check in the next combined Device Lab build.
