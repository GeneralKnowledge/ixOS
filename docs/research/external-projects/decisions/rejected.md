# Rejected

Status: 2026-11-17. Part of the [External Projects Atlas](../README.md). Projects ixOS decided against, with the reason. A rejected project doesn't return as a recommendation unless the reason stops being true, and the reopening says so.

| Project | For what | Reason | Decided |
|---|---|---|---|
| GRDB.swift | SQLite in Swift | Persistence belongs to the Rust core (forge-storage); a Swift database splits state across the boundary | 2026-11-17, [references.md](../../experience/references.md#libraries) (#297) |
| ScanCodes | QR and barcodes | Core Image makes QR codes; VisionKit scans | 2026-11-17, same |
| libp2p | Relay's transport | iroh does the same with fewer moving parts and a better iOS story | 2026-11-14, [relay-transfer-plan.md](../../../../examples/relay/docs/relay-transfer-plan.md) |
| magic-wormhole protocol, LocalSend protocol, wush, derphole, BitTorrent/IPFS | Relay's base | Internet rendezvous even on one Wi-Fi and EUPL crate; multicast and PIN auth; someone else's DERP; built for one-to-many | 2026-11-14, same |
| Maestro, Appium (for physical-iPhone testing) | UI automation | Maestro doesn't support physical iPhones; Appium off macOS needs iOS 18+ and a root tunnel | 2026-11-15, [reinvention-audit.md](../../reinvention-audit.md) |

**Not rejected, but not adopted:** Nuke. It was not judged a bad library; Reuben chose to extend ForgeImages instead (2026-11-17, #298). Kingfisher is covered by the same decision.
