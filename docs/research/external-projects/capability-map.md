# Capability map

Status: v0.1, 2026-11-17. Part of the [External Projects Atlas](README.md). **A proposed longer-term direction, not a commitment to build everything.**

The atlas is organised by project. This page is organised by capability: what ixOS could do across apps, whether or not an external project is reused. It is a second way to read the roadmap, next to the per-app one, and it follows the existing rules: apps first, extract on a real second use ([AGENTS.md](../../../AGENTS.md) rule 1), expand an existing app before adding a new one.

```
                    ixOS applications
 Relay (with Chat) · Commonplace · Pocket Library · Soundscape
        FieldKit · Sketchbook · Checklist · Good Dog · Weather
 ─────────────────────────────────────────────────────────────
 Identity & trust      Connectivity        Personal intelligence
 devices, permissions, P2P, discovery,     AI, search,
 keys                  sync                classification

 Media                 System integration  Personal data
 audio, images,        Shortcuts, widgets, storage, indexing,
 documents             actions             history

 Accessibility         Automation
 VoiceOver,            events, workflows,
 adaptive UX           triggers
 ─────────────────────────────────────────────────────────────
                  Existing ixOS foundations
 ForgeUI · ForgeAppKit · ForgeImages · Rust services
 (forge-runtime, forge-storage, forge-net, forge-services) · Device Lab
```

## Capabilities

For each: what exists today, which apps would use it, which atlas projects bear on it, and the open question. "First and second user" follows the evidence rule: a capability is extracted into the framework only once two apps use it.

| Capability | Exists today | First and second user | Atlas projects | Open question |
|---|---|---|---|---|
| **Identity and trust** | Relay's device key (`device.key`), SPAKE2 codes, pairing policies; trusted devices in Relay's brief PR 3 | Relay; Chat inside Relay | Signal-iOS, Bitwarden, age | How does a person see which devices are trusted, revoke one and recover? |
| **Connectivity** | iroh in Relay; outbox for messages and files (#299); Bonjour discovery | Relay; AI over Relay ([forge-ai-plan.md](../../forge-ai-plan.md)) | Lightning P2P, libp2p | Can Relay be a general device-to-device transport other apps call? |
| **Sync between own devices** | None; each app keeps its own data in forge-storage | Commonplace; Checklist or Pocket Library | Automerge, Syncthing, Yjs, ElectricSQL | What does a sync layer add over forge-storage plus Relay's transport, without a second persistence architecture? |
| **Personal intelligence** | Foundation Models in Chat (L1); downloadable models planned (L2); AI Lab in Device Lab | Relay's Chat; Soundscape or FieldKit | FluidAudio, WhisperKit, MLX Swift, llama.cpp | Which capabilities are shared (transcription, search, classification), and does every app work without them? |
| **Media** | ForgeImages (files; web pictures in #298); Soundscape's player; Sketchbook | Pocket Library, Soundscape, Commonplace (images); Relay (previews) | Offscan, OpenScanner, Brightroom, Essentia | Which processing belongs in ForgeImages, and which stays in one app? |
| **System integration** | App Intents compile; metadata step missing on Linux ([app-intents-on-linux.md](../app-intents-on-linux.md)) | Checklist; Pocket Library (candidates from the 2026-11-16 roadmap rule) | App Intents sample, WidgetExamples, Ice Cubes | One action vocabulary across apps, once one intent appears in Shortcuts from an xtool build |
| **Personal data** | forge-storage (JSON, SQLite) per app | Commonplace; FieldKit (timeline candidates) | Anytype, Ente, Readest | Search and history across one app first; across apps later, if ever |
| **Accessibility** | Identifiers on every control (rule 4), `forge inspect` lists them, `forge ui` saves trees, ax audit spike | Every app | a11y-check, AccessibilitySnapshot | Can a rule check run on Linux before the phone? |
| **Automation** | None beyond Shortcuts candidates | Checklist (recurring); Good Dog (schedules) | Shortcuts, App Intents | Depends on system integration first |

## An example of why this layer matters

One local intelligence capability could later give transcription to Chat, document text to Pocket Library, audio analysis to Soundscape and searchable field notes to FieldKit. Four separate AI integrations would each solve storage, cancellation, model downloads and memory limits again. The capability is still built in one app first, and moves into the framework only when a second app uses it.
