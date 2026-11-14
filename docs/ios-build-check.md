# Checking the iOS build on Linux

Every app's SwiftUI has to compile and link against Apple's iOS SDK, and GitHub's Linux runners can't do that (the Darwin SDK can't be redistributed). This used to be the macOS workflow's job, about 35 minutes with the queue. It is now `scripts/check-ios-xtool.sh`, run on a Linux machine that has xtool and its Darwin SDK, such as the laptop the apps are installed from. It takes minutes, and needs no iPhone and no Apple sign-in: the builds are unsigned, like the old `xcodebuild CODE_SIGNING_ALLOWED=NO` ones.

## Running it

From the checkout, on a machine set up for `forge build ios` ([linux-setup.md](linux-setup.md); `forge doctor` checks it):

```bash
# The apps your branch touches
./scripts/check-ios-xtool.sh affected "$(git merge-base origin/main HEAD)"

# Named apps, or all of them
./scripts/check-ios-xtool.sh pocket-library chat
./scripts/check-ios-xtool.sh all

# A pushed branch, without touching this checkout (built in a reused worktree
# under ~/.cache/forge/ios-check, so the second run is warm)
./scripts/check-ios-xtool.sh --ref claude/my-branch affected origin/main

# Only print which apps would be built
./scripts/check-ios-xtool.sh --list affected origin/main
```

It builds every selected app even when one fails, prints the last 40 lines of each failure, and ends with a table of results and the directory holding the full logs. The exit status is 1 if anything failed.

The apps are every `examples/<app>` with an `xtool.yml`, plus three made fresh for the run: `template` (`forge init`), `minimal` (`forge init --minimal`) and `services-check` ([native-services-device-check.md](native-services-device-check.md)). Forge apps build through `forge build ios` (the Rust library for `aarch64-apple-ios`, then `xtool dev build`); the ForgeUI showcase, which has no Rust core, with `xtool dev build` directly. Each build has 20 minutes (`FORGE_IOS_CHECK_TIMEOUT`, in seconds).

`affected` picks apps from the diff the same way the macOS script did: a change to one example builds that example, ForgeKit builds the showcase, Checklist and the template, the CLI or templates build both starters, the native services add the services check app, anything else shared builds the template and Device Lab, and docs, Android and CI files build nothing.

## From a coding agent

A cloud session has no Darwin SDK. It pushes its branch and runs the check on the laptop through Remote Control, with `--ref` so the laptop's own checkout is left alone:

```bash
cd ~/ixOS && git fetch -q origin && ./scripts/check-ios-xtool.sh --ref <branch> affected origin/main
```

Report the results table. If the laptop can't be reached, say the Swift side wasn't compiled; start the macOS workflow only if the person asks.

When the change also needs a phone run, install from the build the check just made: the script ends with one `forge run ios --project …` line per app it built, pointing into the `--ref` worktree. That run uses the same configuration (debug Swift on release Rust), so it recompiles nothing, then looks for the iPhone; with none it stops with the build ready, and the run that installs, once the person has the phone, takes seconds. Building again in another checkout, or with `--release` on one side only, starts from scratch.

## What it covers, and what it doesn't

| Check | Before (macOS workflow) | Now |
|---|---|---|
| SwiftUI views compile and link for an iPhone | Every app, weekly or by hand, `xcodebuild` | Every app, on the laptop, `xtool dev build` |
| View models and their tests against the real Rust core, without SwiftUI | macOS XCTest | Linux CI: `forge check swift` for every app with a `requirements.toml` (Good Dog, Pocket Library) and the fresh starter app (`check-fresh-app.sh`), and plain `swift test` for Checklist, ForgeUI and the showcase |
| Swift host tests that need Apple frameworks (Chat, Counter, Weather, Device Lab, Commonplace, FieldKit) | macOS XCTest | **Not run.** The macOS workflow still runs them by hand. Giving an app a `requirements.toml` (a headless twin, [evidence.md](evidence.md)) brings its portable tests back on Linux CI |
| The Xcode projects (`Xcode/*.xcodeproj`) build | `xcodebuild` | **Not built.** xtool builds from `Package.swift`. Linux CI still checks their layout (`validate-xcode-project.sh`); they matter only to someone with a Mac ([ios-xcode.md](ios-xcode.md)) |
| Independent of one machine | GitHub runners | Depends on the laptop being set up and awake |

The macOS workflow stays as a manual fallback (Actions, **macOS**, **Run workflow**). Its weekly run is gone, so nothing runs it on its own.

The other workflows with a macOS leg are short and path-filtered, and are unchanged: ForgeUI (an `xcodebuild` of ForgeAppKit and the showcase, on ForgeKit changes), App model (Checklist, on pushes to `main`) and the local AI check app (on changes to it).
