# First App Store release: plan

Status: plan, 2026-11-08. Nothing on this page is built yet. It turns Reuben's review "shipping a real ixOS app" into a staged plan. The constraint is to do as much as possible on Linux, accepting that a Mac is needed at some point.

The goal is a new measure of success for ixOS: **someone builds an app with ixOS, releases it on the App Store and keeps maintaining it, without needing to understand Forge's internals.** The first release is an experiment that forces the framework through every step from build to an approved listing. The point is not to earn money from it.

This plan doesn't repeat two pieces of work happening alongside it:

- **The foundation review** covers lifecycle, storage recovery and migrations, security and resource limits. The release depends on its storage and migration findings, because an App Store update has to carry users' data forward.
- **Forge M6** ([forge-m6.md](forge-m6.md)) covers the evidence-driven dev loop, run records, and a person's acceptance through `forge verify --accept`. `forge release check` reads M6's records and doesn't rerun them.

## The short answer

- **Pocket Library is the first release candidate.** It is offline-first, has no accounts, payments or AI, already runs on the iPhone, and has the most finished product surface of the existing apps.
- **Linux can do most of the pipeline.** It can build the release app with the iOS 26 SDK, sign it, package the IPA, and upload it through the App Store Connect API, which has had a build upload endpoint since 2025. Metadata, screenshots (from the iPhone with `forge device screenshot`) and submission can also be done from Linux.
- **Three things are blocked or unproven on Linux:**
  1. **App icon asset catalog.** Apple requires the icon in a compiled `Assets.car` with `CFBundleIconName`, and only Apple's `actool` builds that. A Mac is needed once per icon change, and the output can be reused in Linux builds.
  2. **The Xcode build metadata Apple checks.** Xcode writes `DT*` keys (SDK name, Xcode version, platform build) into Info.plist and a build version into the binary, and xtool doesn't write the same set. Whether App Store validation accepts an xtool build is unknown until we upload one.
  3. **The fallback path.** `xcodebuild archive` plus `-exportArchive` is Apple's supported route and needs macOS.
- **The cheapest Mac is the one we already have: GitHub's macOS runners.** The repository is public, so they cost nothing, and we already run a macOS workflow on demand. A release workflow that runs only when started by hand fits the CI policy. A borrowed or second-hand Mac mini is only needed if CI can't do a step.
- **The paid Apple Developer Program is needed from stage R2 onwards.** Everything before R2 works on the free team. App Store Connect, distribution certificates, App Store profiles, uploads and TestFlight all need the paid membership. Pay when R0 and R1 are done, not before.

## Which app

| App | For | Against | Verdict |
|---|---|---|---|
| **Pocket Library** | Offline-first; no accounts, payments or AI; EPUB reader and read-aloud make it a useful product (guideline 4.2, minimum functionality); on the iPhone and has acceptance tests and UI tests; JSON export | Online catalogue search sends search text to Open Library (a privacy disclosure, and search can be switched off in Settings); no icon yet; the name may already be taken on the App Store | **First release** |
| Good Dog | A real product from Reuben's existing web app; the most commercially interesting | Still being built (GD1 just passed); about to add notifications and the first widget extension; training advice brings welfare and safety review questions | **Second release**, once Pocket Library has proven the pipeline |
| Weather | Small, on the iPhone | Weather apps are a saturated category (guideline 4.3, spam); depends on a weather API's commercial terms; asks for location | No |
| Chat, Murmur | Big feature set | Accounts, a server, user-generated content (moderation, reporting and account deletion rules) | No |
| Counter, Device Lab, Tipster | — | Demos and test tools; would fail minimum functionality | No |
| Kataster | Game | A port of someone else's open-source game; licence and originality questions | No |

Pocket Library choices to confirm before stage R2, because two of them can't be undone after the first upload:

1. **Bundle ID.** `org.forgemobile.pocketlibrary` is permanent once uploaded. Use a reverse domain Reuben controls.
2. **App name.** App Store names are unique; check that "Pocket Library" is free in App Store Connect when creating the record.
3. **iPhone only.** Leaving out iPad avoids iPad screenshots and iPad layout review. Check `UIDeviceFamily` in the xtool build.

## What Linux can do and what needs a Mac

| Step | Linux with xtool | Mac | Paid team? | Status |
|---|---|---|---|---|
| Release build (`forge build ios --release`) | Yes | — | No | Built |
| Built with the iOS 26 SDK (required for uploads since 28 April 2026) | Yes. xtool uses the SDK from Xcode 26. The binary's SDK stamp needs the fix that is in our xtool fork (upstream PR #392) | — | No | Needs checking on a built binary |
| Xcode build metadata (`DTXcode`, `DTSDKName`, `DTPlatformBuild`, …) | Not written by xtool. We could copy it from a Mac build, but writing values we didn't build with is risky | `xcodebuild` writes it | No | **Unknown: R1 compares the two builds, R3 tests the upload** |
| App icon in `Assets.car` | No `actool` on Linux, and no maintained open-source replacement | `actool`, once per icon change; Linux builds reuse the output | No | R1 |
| Launch screen | Yes: an empty `UILaunchScreen` dictionary (iOS's default) is accepted | Needed only for custom colours or images | No | Check that the build writes it |
| Privacy manifest (`PrivacyInfo.xcprivacy`) | Yes. It is a plist that Forge can generate from `forge.toml` | — | No | R0 |
| Distribution certificate and App Store profile | Yes, through the App Store Connect API with an API key: generate the key pair with openssl, create the certificate, then create the profile | Xcode can do it automatically | **Yes** | R2 |
| Distribution signing | Probably, with `rcodesign` (apple-codesign, written in Rust, runs on Linux) or an xtool change. Not tried for App Store builds | `xcodebuild -exportArchive` | **Yes** | R3 |
| Package the IPA | Yes (`Payload/App.app` zipped) | Done by the export | No | Trivial |
| Upload | Yes, through the App Store Connect API build upload endpoint (2025) or Transporter. Not tried here | Xcode, `altool` or Transporter | **Yes** | R3 |
| Apple's processing and validation | Done on Apple's servers, whichever machine uploaded | — | **Yes** | R3 |
| TestFlight install | On the iPhone | — | **Yes** | R3 |
| Screenshots | Yes: `forge device screenshot` on Cati gives 1290×2796, which should fit the 6.9" iPhone slot (check in R4) | — | No | R4 |
| Metadata, age rating, privacy answers, submission | App Store Connect website or API, from any machine | — | **Yes** | R4, R5 |

## Stages

Each stage ends with something checked. The free team is enough until R2.

### R0: Linux preparation (free team, no Mac)

- `docs/distribution.md`: what Forge can and can't do for releases, the paid-team line, and both upload routes. The roadmap already promises this page.
- `forge release check` levels 1 and 2 (below), on Linux.
- `[ios.privacy]` in `forge.toml` generates `PrivacyInfo.xcprivacy`, and the ledger gets a row for it.
- Run the check on Pocket Library and fix what it finds: add an icon, choose the bundle ID, and declare in the privacy manifest the reasons for using `UserDefaults` (read-aloud settings) and file timestamps (the Rust standard library uses `stat`).
- Confirm that the release build contains none of the devtools server code, and that the UI-test launch argument changes nothing for a normal launch.

### R1: one macOS CI run (free team, GitHub's macOS runner)

A `release-mac.yml` workflow, started only by hand:

1. Compile the icon with `actool` and keep the resulting `Assets.car` and generated Info.plist keys as a workflow output. Forge copies them into Linux builds, and a Linux check fails if the icon changes and the compiled copy is stale.
2. Run an unsigned `xcodebuild archive` of Pocket Library with Xcode 26 (our current workflow uses `macos-15` with Xcode 16.4, so it has to switch to Xcode 26).
3. Compare that archive with the Linux release build: Info.plist keys, the binary's build version and SDK stamp, the architectures, the frameworks it links, and the bundle layout. The difference list is what R3 has to fix or test.

This answers most of "what does a Linux build lack" without paying.

### R2: pay and set up (paid team)

- Join the Apple Developer Program as an individual ($99 a year). Switch xtool's team: `xtool auth logout`, `xtool auth login`, then set `team_type = "paid"` ([ios-signing.md](ios-signing.md)).
- Create an App Store Connect API key. Store it outside the repository on the laptop, and as a GitHub secret for the macOS workflow.
- Register the bundle ID, create the app record, the distribution certificate and the App Store profile.

### R3: first upload, two routes

- **Route A (Linux):** a release build, the compiled icon from R1, distribution signing with `rcodesign`, then upload through the API. Note every validation error Apple returns. This is the experiment that matters for "Linux-first".
- **Route B (Mac, the fallback):** the R1 workflow archives, signs, exports and uploads with the API key.
- Whichever route is accepted first goes to TestFlight and is installed on Cati. **iPhone step:** install from TestFlight and run Pocket Library's acceptance journey. This proves that the store-signed build behaves like the development build. Reuben reports the result.
- If route A fails on something we can't fix, route B ships the first release, and route A stays a tracked gap in the ledger.

### R4: release candidate hardening and store material

- Device acceptance and accessibility evidence come from M6 (`forge verify`, the run record, `--accept`). Storage, migration and recovery come from the foundation review. Release-specific additions:
  - **An update test:** install version N with real data, update to N+1 through TestFlight, and check the data survives. This needs the iPhone.
  - Clean first launch on a phone that has never had the app, and with no network.
  - No placeholders, debug screens or developer-only settings visible.
- A privacy policy page (GitHub Pages is enough), App Store description, keywords, support URL, age rating, and the App Privacy questions.
- Screenshots from Cati in light mode, taken by Forge.

### R5: submit and respond

Submit the TestFlight build that passed R3 and R4. Record any rejection and its fix in the ledger and in this page. Approval completes the first release.

After R5, maintenance is the same loop: raise the build number, run `forge release check`, upload, TestFlight, submit. A second release of Pocket Library within a month or so tests whether the loop works without help.

## `forge release check`

One command with one report. It sorts each finding into **blocker** (Apple will reject it or it will break for users), **warning**, or **not run**, and says why for each. It writes `release-check.md` and a JSON file. Like `forge verify`, it never counts a skipped check as a pass.

| Level | Needs | Checks |
|---|---|---|
| 1. Configuration | `forge.toml` only, Linux | Version and build number are set, and the build number is higher than the last upload Forge recorded; the bundle ID isn't the sandbox `org.forgemobile.agentlab` or a `com.example` placeholder; the icon is 1024×1024 PNG with no alpha; a compiled icon catalog exists and is current; every permission the app uses has a usage description; `ITSAppUsesNonExemptEncryption` is set; the privacy manifest exists; a privacy policy URL is set; device family and orientations are set; no paid-only entitlement is missing a paid team |
| 2. The built app | The release `.app` or `.ipa`, Linux | arm64 only, no simulator slice; the minimum OS matches `forge.toml`; the SDK stamp is 26 or newer; no `get-task-allow` in a distribution-signed app; no embedded development profile; no devtools server code (look for `forge-devtools` symbols); the required-reason APIs the binary references, such as `stat` or `UserDefaults`, are declared in the privacy manifest; `Assets.car` and `CFBundleIconName` are present; `UILaunchScreen` is present; the Info.plist keys from the Mac build that the Linux build lacks (the list from R1); size |
| 3. Evidence | M6's records | Requirement status comes from `forge verify --json` for the current files, not from an older run record, because an acceptance goes stale when the app's files change. It needs every requirement in the app's `requirements.toml` proven on the device and accepted by a person (`data.requirements[].person`, written by `forge verify --accept`). From the latest run record (`.forge/runs/<id>-run/evidence.json`, kind `run`) it reads `crashes`, `new_accessibility_findings`, `failures` and `not_run`, plus whether the visual baselines were accepted and whether the update test passed. It reads these records and doesn't run anything |
| 4. App Store Connect | API key, paid team | The app record and bundle ID exist; the version isn't already used; the processing state of the last upload; TestFlight status; metadata and privacy answers are complete |

Levels 1 and 2 are cheap and Linux-only, so they belong in R0. Level 3 waits on M6.4 merging. Level 4 waits on R2. The first version could be `forge check release`, next to `forge check swift`, if that reads better in the command list.

## App Review and privacy checklist for Pocket Library

| Area | What Apple expects | Pocket Library |
|---|---|---|
| Completeness (2.1) | No placeholders, broken links or unfinished screens; the reviewer can reach every feature | Check every screen with an empty library. Include a short sample EPUB note in the review notes, because the reader needs an imported book |
| Minimum functionality (4.2) | More than a repackaged website or a trivial app | A reading tracker with a native EPUB reader and read-aloud should pass |
| Privacy policy (5.1.1) | A URL in App Store Connect and inside the app | Add one. It says all data stays on the device, catalogue search sends the search text to Open Library, and nothing goes to the developer |
| App Privacy answers | What data is collected, by you or partners | Most likely "Data Not Collected", because Open Library is not a partner of ours. Record the reasoning in the release notes |
| Privacy manifest | `PrivacyInfo.xcprivacy`, listing reasons for the required-reason APIs | `UserDefaults`, file timestamps, and possibly disk space for "storage used". Confirm from the binary in R0 |
| Permissions (5.1.1) | Ask only when needed, with a clear explanation | Uses none today. Read-aloud uses background audio, which is a capability, not a permission |
| Background audio (2.5.4) | Used only for audible content | Reading aloud qualifies |
| Content and copyright (5.2) | No infringing content | Users import their own EPUBs. Covers and descriptions from Open Library follow its terms (attribution) |
| Encryption export | `ITSAppUsesNonExemptEncryption` | Already `false`. HTTPS only, which is exempt |
| Accounts, payments, moderation | Account deletion, in-app purchase rules, reporting tools | None apply |
| Metadata (2.3) | Screenshots of the real app, accurate description, age rating | Screenshots from Cati; age rating 4+ |

## Decisions for Reuben

1. **Pocket Library first, Good Dog second.** Recommended. Pocket Library proves the pipeline without Good Dog's notifications and widget, then Good Dog becomes the real product release.
2. **When to pay.** Recommended after R1, when the Linux and Mac builds have been compared. Before that, paying unlocks nothing that we can use.
3. **The Mac.** Recommended: GitHub's macOS runner for R1 and route B. Buy or borrow a Mac mini only if a step needs an interactive Mac. App Store Connect works from a browser, so very little should need one.
4. **Bundle ID and name for Pocket Library.** A reverse domain he controls, decided before the first upload.

## Not known yet

- Whether App Store validation accepts an xtool-built binary without Xcode's `DT*` keys. R1 and R3 answer this.
- Whether `rcodesign` signs an iOS app with the result App Store validation expects. R3 answers this.
- Whether the App Store Connect build upload API works as documented for a first upload. R3 answers this, and Transporter or route B are the fallbacks.
- Whether App Store Connect accepts 6.7" screenshots from Cati in the 6.9" slot. Check this when uploading them in R4.
