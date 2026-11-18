# Specimen profile: Calculator

| | |
|---|---|
| App | Apple Calculator, ships with iOS |
| Bundle id | `com.apple.calculator` (Inferred: not in `forge device apps --all`, which leaves out system apps; the trees confirm the app in front) |
| Phone | Cati, iPhone 15 Pro Max (iPhone16,2), iOS 26.3.1, on USB to the Linux laptop |
| Equipment | `forge` 0.1.0 (`~/.cargo/bin/forge` on the laptop), WebDriverAgent already running, `forge ui tree --any-app` and `forge ui screenshot --any-app` |
| Policy | `"com.apple.calculator" = "observe"` in the person's `~/.config/forge/device-control.toml`, added with Reuben's OK on 2026-11-18 00:47. Observe means read only: every tap and key press was made by Reuben's hand |
| Installed by | Nothing to install; takes no sideload slot |
| Constraints | Closed source. We recreate behaviour only, with our own name and look; no Apple art, icons or names. History holds the person's own past calculations: not committed beyond what they show about formatting |
| Expedition | [Plan](../../app-as-specification.md), [field notes](../../../blog/calculator/log.md) |
