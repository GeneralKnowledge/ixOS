# Adopted

Status: 2026-11-17. Part of the [External Projects Atlas](../README.md). External projects that are dependencies of shipped ixOS code, found through research. Toolchain basics (Rust crates like serde or tokio, UniFFI, rustls) are not listed.

| Project | Where | Decided | Evidence |
|---|---|---|---|
| iroh, iroh-blobs | Relay (`examples/relay/rust/Cargo.toml`) | Reuben, 2026-11-14 (#197) | M0 on two Linux machines (#198, #199); iPhone transfers at the start of M1 ([relay-stage0-spike.md](../../../../examples/relay/docs/relay-stage0-spike.md)); production core #267 |
| spake2 (RustCrypto) | Relay word codes | Reuben, 2026-11-14 (#197) | Same as above |
| Apple Foundation Models | Chat's Assistant, now in Relay | Reuben, 2026-11-07 ([local-ai.md](../../../local-ai.md)) | L1 passed on the phone |
| WebDriverAgent | `forge ui` | [forge-ui-testing.md](../../../forge-ui-testing.md) | Physical iPhone UI tests |
| xtool (GeneralKnowledge fork) | The iOS build | Project foundation | Every iPhone build |

No new entry has come from the atlas yet.
