# Better by default: what the friction log says the framework should prevent

Written 2026-11-17 from the [app friction log](app-friction-log.md) (A1 to
A42, plus A36 and A37 from #277 and A43 from #280), checked against the apps,
the starter and `scripts/check-app-parity.sh` on main (dfb046c). It is a plan
only: nothing here is built until Reuben agrees.

The question for each entry was: did more than one app (or the starter) hit
it, and could a check, the starter, ForgeKit or `forge` have stopped it
without each app fixing it again? Per-app defects stay with the friction
audit thread; this plan only adds the defaults and checks that keep them
from coming back.

The rules it follows: a bug in one app means checking every app and the
starter and adding an automatic check; extract to the framework only when two
apps need it; wrap Apple's frameworks, never replace them; one item at a
time.

## Recommended first

1. **Picked files are copied off the main thread, by the framework**
   (item 1). Two apps have the same freeze, and ForgeKit already has the
   helper that should prevent it.
2. **A check for alerts hidden under sheets** (item 2). The flaw has been
   fixed by hand in two apps and the starter, and a third case is open.

Both are small, each is one PR, and neither needs the phone to prove the
check; the phone sitting for the audit's fixes can cover the rest.

## The plan, most useful first

| # | Default to change | Kind | Evidence | Apps |
|---|---|---|---|---|
| 1 | Picked files are copied off the main thread, and cleaned up | Framework (existing helper) + check | A40, A2, A14 | Relay, Chat; Checklist and Pocket Library already fine |
| 2 | An alert never sits under an open sheet | Check | A39, A34, A33 | Commonplace, Relay, starter (fixed); Checklist (open) |
| 3 | Plain http is a `forge.toml` key that hosts merge | `forge` tooling + check | A35, A26 | Soundscape, Chat, Device Lab, starter |
| 4 | A PR shows whether its Swift was compiled for iPhone | Infrastructure | A28, A37, A43 | Every app |
| 5 | Bindings come out the same on every machine | `forge` tooling | A36 | Device Lab, Chat (every app on regenerate) |
| 6 | An error's kind decides retry and cancel, not a Swift switch | Framework, later | A36, A19 | Seven apps and the starter |

### 1. Picked files: copied off the main thread, and cleaned up

**Default to change.** ForgeKit's `ForgeFiles.importCopy` and
`forgeFileImporter` already copy a picked file so Rust can read it later.
No app uses them, and they copy on the main thread and never delete the
copy, which is exactly Relay's bug. Make the copy `async` and off the main
actor, return a handle the app releases (or a folder ForgeKit clears at
launch), and move the apps onto it.

**Evidence.** A40: Relay froze for seconds on a large video and kept the
copy in Caches for good; #264 fixed it by hand, twice (`PeerView`,
`SendCodeView`). Chat has the same flaw today:
`AttachmentFiles.prepareDocument` copies a picked document inside the
picker's callback on the main thread (`ChatView.stageDocuments`), so a large
document freezes the compose screen the same way (from the code; not seen on
the phone). Checklist and Pocket Library are fine: Rust reads the file in
place inside the security scope, off the main thread, with no copy.

**Check.** A parity rule, `picked-files`: no `FileManager.copyItem` of a
picked URL in app code; use ForgeFiles. Run on main today it would name
Relay and Chat.

**Done (2026-11-17, Reuben agreed items 1 and 2).** The copy went into
ForgeAppKit as `ForgePickedFile`, not into the native services'
`ForgeFiles.importCopy`: Relay doesn't use the native services' Swift, and
every app uses ForgeAppKit, whose model code tests on Linux. `importCopy`
is left as it is (no app calls it) and can go when the native services get
one route (everyday capabilities Phase 4). Chat and Relay use
`ForgePickedFile`; `picked-files` is in `check-app-parity.sh`. Chat's
staged attachment owns its copy and releases it when the attachment is
removed or once the send has finished.

**Not included.** The photo picker extraction (wave 5) stays where it is;
it should use this copy when it comes.

### 2. A check for alerts under sheets

**Default to change.** An alert attached under an open sheet can't present,
so a failure from inside the sheet goes unseen. The pattern that works is
already in ForgeAppKit: the sheet keeps its own `ForgeSubmission` and
`.forgeAlert(failure:)` (FieldKit's Start Trip sheet), and the screen under
it gates the model's alert while a sheet is up (the starter since #271).
What's missing is the check, so new apps learn it from CI rather than from
the phone.

**Evidence.** A39: Commonplace's Settings sheet and Relay's sheets, fixed
by hand; the starter fixed in #271. A34 found the Commonplace case in a
manual sweep. Checklist's rename sheet still reports a failed rename on the
list underneath (audit item 16, the friction audit thread's to fix).

**Check.** A parity rule, `alerts-over-sheets`: a sheet whose content runs
an action shows that action's failure itself, and a model alert on a screen
that presents sheets is gated while one is open. Like `alerts-on-top`, it
runs on every app and both starters; Checklist goes in the lag list until
the audit thread's fix lands.

**Done (2026-11-17).** `alerts-over-sheets` is in `check-app-parity.sh`.
It checks the first half (the model alert held back while a sheet is up);
whether a sheet shows its own failures is left to review, since a script
can't tell which sheet raises which failure. On main it named FieldKit's
trip list (now held back while Start Trip is up) and Checklist (listed as
lag); on the code before the A39 fixes it names the starter and
Commonplace.

### 3. Plain http as a `forge.toml` key that hosts merge

**Default to change.** Plain http is set today through
`[ios.info.NSAppTransportSecurity]`, which hosts don't merge, so a hosted
app's exception never reaches Device Lab and Device Lab writes it by hand.
Give it a key of its own, `[ios] plain_http = true`, written to
`NSAllowsArbitraryLoads` and merged by hosts like `bonjour_services` (A26),
and have `forge config` say what it costs at App Store review.

**Evidence.** A35: Soundscape's take server, numeric addresses refused too
(#263). Device Lab and the starter carry the setting by hand (#271). Chat
accepts an `http://` server address and sends through URLSession, but its
own `forge.toml` has no exception: it works inside Device Lab because
Device Lab has one, and would be refused if Chat were installed on its own
(from the code; not checked on the phone).

**Check.** A parity rule, `plain-http`: an app whose Rust accepts `http://`
addresses and uses the `http` service sets `plain_http` itself.

### 4. Show on the PR whether the Swift was compiled for iPhone

**Default to change.** Cloud sessions can't compile SwiftUI, so Swift has
merged uncompiled more than once. `scripts/check-ios-xtool.sh` already
builds a branch on the laptop; it could report its result to the PR as a
commit status ("iPhone build"), so a PR with uncompiled Swift says so
instead of relying on the description.

**Evidence.** A28 (two errors only the iOS SDK showed), A37 (#196 and #257
merged with Swift not compiled), A38 (AVFoundation rules seen only on the
phone, which a compile wouldn't catch, so this is not a cure for those).
A43: a cloud session has no Swift at all; a line in the cloud setup notes
for the `swift:6.2` image would let `forge check swift` run there too.

**Needs Reuben.** The laptop would need a GitHub token that can post commit
statuses. The build queue itself is frozen; this only reads its result.

### 5. Bindings that are the same on every machine

**Default to change.** UniFFI formats the Swift it writes when a
formatter happens to be installed, so the laptop and the cloud generate
different text for the same Rust. `scripts/generate-bindings.sh` (which
`forge bindings generate` runs) should always pass `uniffi-bindgen`'s
`--no-format`, and every app's bindings be regenerated once.

**Evidence.** A36: regenerating Device Lab's bindings in the cloud for one
new type made an 8,800-line diff; Chat's copy is unformatted. The drift
check (`check-app-crates.sh`) then compares text that depends on the
machine.

### 6. Error kinds decide retry and cancel (later)

**Default to change.** Every app writes a Swift `ForgeDescribedError`
switch that decides again whether an error is retryable or a cancellation,
which Rust's `ErrorKind` already says. When an app exports its error's
kind, ForgeAppKit could default `isRetryable` and `isCancellation` from it,
leaving the app only the words.

**Evidence.** A36 ("every app with errors does this"), A19 (the kinds
exist now). Sketchbook, FieldKit, Relay, Good Dog, Pocket Library,
Soundscape, Commonplace and the starter have the switch.

**Why later.** It touches the same code as the everyday capabilities plan's
Phase 1 (typed open failure to ForgeRoot), so it should be designed with
it, not beside it.

## Guardrails, not guarantees

The parity rules read the Swift source for known shapes (a security scope
next to `copyItem`, a model alert next to `.sheet(`); they don't analyse
Swift. They stop the mistakes the apps actually made from coming back, and
a new shape of the same mistake can still get past them, so review and the
phone remain the proof.

## Looked at and left alone

- **Done already:** CSV row numbering (A42, `CsvImport::row_lines`), alerts
  behind pushed screens (A33, `alerts-on-top`), dropped failures (A34,
  `errors-shown`), ids (A3), `forge init` names and leftovers (A7, A21),
  hashed file extensions (A2, A14).
- **One app, or Apple's rule to live with:** AVAudioEngine traps (A38),
  drag gestures in a hosted sheet (A41), PencilKit's undo (A23), `#if
  os(iOS)` for host tests (A22), audio in Rust (A20).
- **Small, and only if it comes up again:** a `forge config` warning when
  hosted apps' permission texts differ (A25); defaults for UniFFI records
  and `CaseIterable` enums (A32, Good Dog 6); test fixtures in `forge check
  swift` (A29).
- **Already planned elsewhere:** native services once per process, including
  `ai` (A36; everyday capabilities Phase 4), the `on_transition` hook (A31;
  waits for Relay), the service loop (A9, A18).
