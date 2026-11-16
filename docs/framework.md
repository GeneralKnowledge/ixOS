# Framework

What apps run on: the Rust runtime and foundation crates, ForgeKit, ForgeUI, ForgeAppKit and the native services. Read this area to build an app's logic and screens, or to change the framework itself. Which code belongs here is in [areas.md](areas.md).

Read in this order.

| Page | What it's for |
|---|---|
| [architecture.md](architecture.md) | How the Rust core, the Swift layer and the generated bindings fit together |
| [app-crate.md](app-crate.md) | Your app's Rust crate: state, rules, lifecycle, saved documents and validation |
| [ffi-policy.md](ffi-policy.md) | The rules for calls between Swift and Rust: async, events, cancellation, errors |
| [persistence.md](persistence.md) | What Forge promises about saved data at each crash point, and what an app does |
| [ios-ui.md](ios-ui.md) | The SwiftUI side: how a Forge app should look and how to show Rust state |
| [forgeui.md](forgeui.md) | ForgeUI, the design system; usable in any SwiftUI app |
| [forgeappkit.md](forgeappkit.md) | ForgeAppKit: alerts, confirmations and destinations as model values, load and submission state, form errors |
| [native-services.md](native-services.md) | Keychain, notifications, permissions, location and other iPhone features |
| [forge-reader.md](forge-reader.md) | ForgeReader: documents as styled blocks, reading positions, the SwiftUI reading view |
| [app-config.md](app-config.md) | `forge.toml`: name, version, icon, permission prompts, entitlements |
| [localization.md](localization.md) | Translating an app: String Catalogs, plurals, permission texts |

## Plans and reviews

| Page | What it's for |
|---|---|
| [native-capability-roadmap.md](native-capability-roadmap.md) | Which Apple frameworks ixOS wraps next, in what order, and how each is proven |
| [app-model-review.md](app-model-review.md) | What an ixOS application is: the model `forge-app` and ForgeKit implement |
| [foundation-review.md](foundation-review.md) | Open findings in the foundation crates and the `gap_*` tests that pin them |
| [local-ai.md](local-ai.md), [forge-ai-plan.md](forge-ai-plan.md) | On-device AI: the stages so far and the proposed shared AI module |
| [adr/](adr/) | Architecture decisions |
