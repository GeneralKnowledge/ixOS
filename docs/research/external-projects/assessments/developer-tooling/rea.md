# REA: what can it give App Studio's Recreate on Linux?

Status: 2026-11-19. Part of the [External Projects Atlas](../../README.md). Milestone X0 of [App Studio](../../../app-studio.md#recreate-milestones). Research only: nothing is added to ixOS or forge-lab by this page.

## Question

REA is App Studio's integration candidate for investigating reference apps (Reuben, 2026-11-19). Before X1 (Spotify's Library, investigated on the web player and the iPhone app), what does REA actually do on a Linux machine like the ones ixOS uses, what does it see in an app ixOS built and in a real web page, and is its output usable as the evidence for a Recreate specification?

## Evidence

| Field | Value |
|---|---|
| Date | 2026-11-19 |
| Inspected | npm package `rea-agents` **6.9.0** (latest tag on 2026-11-19), source repository [morluto/rea](https://github.com/morluto/rea) |
| Licence | MIT (npm metadata and repository) |
| Activity | Releases often (its own README: "REA changes quickly"). Star counts differ wildly between its GitHub pages, so popularity numbers were ignored |
| Read or run | README, `DISCLAIMER.md`, the Apple, Android and installation guides read; the package installed and run in a cloud session |
| Build evidence | `npm install --ignore-scripts rea-agents@6.9.0` in an empty folder: 30 packages, 220 MB, 11 s, Node 22.23.5 (its `engines` field needs `^22.23.4`, `^24.22.1` or `>=26.12.1`). No install script was needed. `rea mcp doctor`: healthy, **255 MCP tools and 6 prompts**; the CLI lists 198 commands |
| Platform evidence | Ubuntu 24.04 x86_64 cloud container. No Ghidra, Hopper, IDA, JADX or pwntools installed (`rea doctor` lists each as missing). Nothing on the phone |
| ixOS baseline | main at `a4b3b4f`; Device Lab's `Info.plist` (from `forge.toml`) |

## What it did

### An app ixOS built

A real Device Lab IPA can't be built in the cloud (no Darwin SDK; xtool lives on the laptop), and the Rust app crates need Apple's `xcrun` for their C dependencies, so a stand-in was made from ixOS's own build inputs: an IPA with `Payload/DeviceLab.app/Info.plist` (Device Lab's real one) and an arm64 iOS Mach-O object from Rust's own `aarch64-apple-ios` standard library as the executable. Enough to try REA's Apple path; a real Device Lab IPA from the laptop is the follow-up.

| Command | Result |
|---|---|
| `inspect-artifact DeviceLab.ipa` | **Works on Linux.** A hashed inventory: zip, `Payload`, the `.app` directory, the plist, the Mach-O, each with SHA-256 and format |
| `project-apple-application-graph` (from that inventory) | **Works.** Platform `ios`, application root, `Info.plist` path, executable candidate, runtime family `native`, coverage "complete-within-inventory". It reports **paths and hashes only**: its own limitations say bundle identifiers and signing need dedicated plist and CMS parsing |
| `inspect-plist` | **Refused on Linux**: "Native macOS utilities require macOS." |
| `inspect-macho` | **Refused on Linux**, same reason |
| `inspect-native-dispatch-metadata` (Swift and Objective-C metadata) | Needs Ghidra, Hopper or IDA; none installed |
| `inspect-binary-layout` | Needs a caller-supplied Python with pwntools |

So on Linux without a decompiler, REA tells you an iOS app's file structure and nothing about what's inside the plist or the binary. With Ghidra 12.1.4 and JDK 21 installed it can decompile and read Swift and Objective-C metadata; that wasn't tried (a large install, and not what Recreate needs first).

### A real web page

Headless Chromium with a CDP port on Apple's Human Interface Guidelines page (one of the few public sites this cloud session's network allows), then:

| Command | Result |
|---|---|
| `list-browser-targets` | The page found by its CDP target id |
| `inspect-web-page --observation-ms 3000 --include-accessibility-text` | **Rich.** 1,576 DOM nodes, the **accessibility tree (1,100 nodes with roles, names and states)**, 15 scripts with hashes, resources with types and sizes, one console error (location only, text not approved), storage summary with values redacted |
| `capture-web-screenshot` | A PNG as evidence, origin-scoped |

What matters for Recreate:

- **Privacy is opt-in by default.** DOM text, console text, JSON body shapes, WebSocket shapes, script sources and storage keys are all filtered out unless a flag asks for them, and values are never kept. That suits a signed-in Spotify session.
- **Network only covers the observation window.** Its limitations say so: "prior network and console activity is unavailable." To learn which requests a library view makes, the person navigates while `observe-web-session` records, which fits "his own browser, signed in, driving it himself" (X1 runs on his computer).
- **The web accessibility tree has the same shape as `forge ui tree`** (roles, names, states). Comparing the web player's library with the iPhone app's is a tree-to-tree comparison ixOS already does for its own apps.

## Compared with ixOS

| Recreate needs | REA | ixOS today |
|---|---|---|
| iOS screens, controls, navigation on the phone | Nothing | `forge ui tree/screenshot/run --any-app` under the person's policy; the explorer (parked in forge-lab) |
| iOS package structure | Inventory and graph on Linux; contents need macOS or a decompiler | Nothing |
| App Store binary code | Out of bounds (FairPlay-encrypted; decrypting is circumvention) | Out of bounds |
| Web page structure, accessibility, screenshots | Yes, rich and privacy-filtered | Nothing |
| Network behaviour of a web view | Yes, during an observed window; opt-in shapes, no values | Nothing |
| Evidence records | Content-addressed Evidence JSON with ids, digests and limitations | Evidence bundles under `.forge/runs/` |
| Specification with Observed / Inferred / Unknown | `build-reconstruction-obligation-ledger`, `evaluate-reconstruction-coverage` (not tried) | The expedition format (#318), by hand |

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | Good for the web, weak for iOS | Web observation is the strongest part; iOS on Linux is file listing only, and there is no iOS device layer |
| Product value | Medium | Makes the web half of X1 possible without writing a browser tool |
| Architecture | Good | A separate MCP server and CLI; nothing links into ixOS or forge-lab |
| Platform | Mixed | Linux works for web and inventory; Apple file contents need macOS (refused on Linux) or Ghidra |
| Development | n/a | Not modified |
| Maintenance | Risky | 6.9.0 on 2026-11-19 after 4.x in its own docs; fast-moving, so pin the version |
| Licence | Good | MIT. Hopper, IDA and JEB are separately licensed and not needed |
| Security | **Needs restricting** | 255 tools in one server, including unrestricted GDB and Rizin, Frida instrumentation, process capture and adb install/uninstall. No tool allowlist of its own. The agent host must allow only the read-only web and inventory tools |
| Integration cost | Low | `npm install` in its own folder, one MCP entry |
| Reusability | Medium | Useful for any web reference; little for iOS |

## Verdict

**Adopt as an external MCP server for X1's web half, restricted, pinned at 6.9.0.** It gives the web player's structure, accessibility tree, screenshots and (opt-in) request shapes without ixOS writing any of it. It is **not** the iOS layer: on Linux it can't read an iOS plist or binary, and it can't see a phone. The iPhone half of X1 stays `forge ui --any-app`.

Restriction for App Studio: only `list_browser_targets`, `inspect_web_page`, `observe_web_session`, `capture_web_screenshot`, `compare_web_captures`, `compare_web_screenshots`, `inspect_artifact`, `project_apple_application_graph`, and the evidence tools are allowed (by the agent host's permission rules). Debugger, instrumentation, process capture, adb and any mutating tool are denied.

Reopen when: REA adds an iOS device layer, its Apple plist and Mach-O tools work off macOS, or X1 shows the restricted set isn't enough.

## Next step

X1 as planned in [App Studio](../../../app-studio.md#x1-spotify-library), after M0: REA on Reuben's computer with his signed-in browser (structure, accessibility tree, screenshots and request metadata, which he allowed on 2026-11-20; never values), `forge ui --any-app` on the phone. Before that, optional and small: run `inspect-artifact` and `project-apple-application-graph` on a real Device Lab IPA from the laptop's last build to confirm the stand-in result.
