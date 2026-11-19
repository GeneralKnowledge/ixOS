# Apps

The products built with Forge. Each one is its own SwiftUI app on its own Rust crate, and each is reachable from [Device Lab](device-lab/README.md), the app that goes on the phone for testing. Which apps keep up with the framework is in [app-parity.md](../docs/app-parity.md); which area each folder belongs to is in [areas.md](../docs/areas.md).

| App | What it is | Read |
|---|---|---|
| [Pocket Library](pocket-library/README.md) | Reading tracker: books, progress, EPUB import, summaries. The reference app | [README](pocket-library/README.md), [acceptance](pocket-library/ACCEPTANCE.md) |
| [Good Dog](good-dog/README.md) | Dog training plans, with its content core in `good-dog-core` | [README](good-dog/README.md), [design](good-dog/docs/good-dog-demo.md), [pain points](good-dog/docs/good-dog-pain-points.md) |
| [Commonplace](commonplace/README.md) | Feed reader growing into a personal reading system | [README](commonplace/README.md), [NetNewsWire benchmark](commonplace/docs/benchmark/README.md) |
| [FieldKit](fieldkit/README.md) | Field notebook: GPS tracks, notes, photos | [README](fieldkit/README.md) |
| [Weather](weather/README.md) | Weather in the style of the iPhone's own app | [README](weather/README.md) |
| [Checklist](checklist/) | The smallest app on the runtime: lists with due dates | its `rust/` and `Sources/` |
| [Relay](relay/README.md) | Your devices, one network: messages and files between your devices and the people you trust. Chat merged into it (its Chats tab, with `chat-server`, `chat-cli` and on-device AI) | [README](relay/README.md), [messaging](relay/docs/messaging.md), [messaging acceptance](relay/ACCEPTANCE.md), [chat merge](relay/docs/relay-chat-merge.md), [transfer journey](relay/docs/relay-transfer-journey.md), [delivery outbox](relay/docs/relay-delivery-outbox.md), [platform plan](relay/docs/relay-platform-plan.md), [next steps brief](relay/docs/relay-m1-m2-brief.md), [transfer plan](relay/docs/relay-transfer-plan.md) |
| [Sketchbook](sketchbook/README.md) | Drawing notebook with layers | [README](sketchbook/README.md) |
| [Sums](sums/README.md) | A calculator rebuilt from the outside: the Calculator expedition's recreation, tested against answers recorded from the iPhone's own | [README](sums/README.md), [expedition](../docs/research/expeditions/calculator/profile.md), [reconstruction](../docs/research/expeditions/calculator/reconstruction.md) |
| [Soundscape](soundscape/README.md) | Multi-track sound sketchpad | [README](soundscape/README.md) |
| [Pocket Library for Android](pocket-library-android/README.md) | Android, work in progress | [README](pocket-library-android/README.md) |

Not apps, but kept here for now: [Device Lab](device-lab/README.md) (the test bed, a tool) and the [ForgeUI showcase](forgeui-showcase/README.md) (ForgeUI's gallery).

An app's design plans, acceptance records and notes live in its own folder (`<app>/docs/`), not in the top-level `docs/`.
