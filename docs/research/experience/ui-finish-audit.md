# UI Finish Audit: P0 pass (errors and forms)

Status: first pass, 2026-11-22, on `main` at `c2b6d155` plus PR #407. Part of [ixOS Experience Research](README.md). Asked for by Reuben (2026-11-22, recorded in [forgeui-0.2.md section 8](forgeui-0.2.md#8-after-02-the-ui-finish-audit)): after ForgeUI 0.2, check every app for the interaction details that make it feel finished, starting with the P0 areas, **error, retry and recovery** and **form validation**. Each area first asks what ForgeUI and ForgeAppKit already cover; nothing new is proposed unless the apps show it repeating.

Evidence is the source, read file by file, plus a scan of every app's Swift (12 apps, about 39,000 lines). Nothing here was checked on the phone, so each finding says what the code does, not what a person saw. The P1 and P2 areas (success and Undo, accessibility beyond VoiceOver, layout, keyboard, offline) get their own pass next; section 4 lists what this pass already noticed for them.

## The answer

1. **The framework already covers almost all of P0, and most apps use it.** `ForgeFailure` turns errors into words a person can act on and already tells *offline*, *timed out* and *can't reach the server* apart. `ForgeAlert.failure(_:retry:)` adds Try Again when retrying might help. `ForgeStateView.failed(retry:)` and `ForgeLoadView` cover a screen that failed to load, `ForgeNotice` covers a problem in a screen that still works, and `ForgeFieldErrors` puts Rust's validation under the field. **No `ForgeErrorView` is needed.**
2. **The gaps are a handful of specific screens, not a missing pattern.** Section 2 lists them. The two that repeat are:
   - **Permission denied leads nowhere.** Weather and FieldKit say "allow it in Settings" with no button. Weather's only button asks for the location again, which iOS ignores after a denial. `ForgePermissions.openSettings()` exists, and Device Lab and Relay use it.
   - **Text typed in a sheet could be swiped away** in two forms. That is fixed in #407.
3. **Forms are better than expected.** Pocket Library's book form is the reference: Rust validates, each problem shows under its field, numbers get a number pad, and Return moves on. The other forms are one or two fields, where a disabled Save is the iOS convention. One thing is missing everywhere: **after a failed save, nothing moves focus to the field with the problem** (F2).
4. **Retry exists but is used unevenly.** Seven apps describe their errors with `ForgeDescribedError`, and failed *screens* offer Try Again in seven apps. But no app uses the alert's Try Again (`ForgeAlert.failure(_:retry:)`), so a failed *action* that waits on the network ends in an alert with only OK (E2).
5. **No new component and no new parity rule from P0.** The fixes are adoption: a recovery button where a permission is off, Try Again on network actions, focus on the first invalid field. One small shared helper is worth considering, `ForgeStateAction.openSettings` (section 3).

## 1. What already exists, and who uses it

| Piece | What it does | Apps using it |
|---|---|---|
| `ForgeFailure(error)` / `ForgeDescribedError` | Title, message and `isRetryable` in plain words. URLSession errors become "You're Offline", "No Response" or "Can't Reach the Server" | Commonplace, FieldKit, Good Dog, Pocket Library, Relay, Sketchbook, Soundscape. Weather has its own `WeatherClientError.userMessage`, which also separates offline, timeout and server trouble. Checklist and Urdu Naturally have their own `describe`. Sums and Device Lab have no user-facing network errors. |
| `forgeAlert` / `ForgeAlert.failure(_:retry:)` | Error alert, with Try Again when retryable | Most apps use the alert. No app uses the `retry:` form; Relay retries a failed message from its own row |
| `ForgeStateView.failed(retry:)`, `ForgeLoadView` | A screen that failed to load, with Try Again | Weather, Good Dog, Pocket Library, Relay, Soundscape, Commonplace (article), Device Lab |
| `ForgeNotice` | A problem in a screen that still works | Relay, Soundscape, FieldKit, Weather, Pocket Library |
| `ForgeFieldErrors`, `ForgeFieldMessageView` | Rust's validation under each field | Pocket Library only (the only form with fields Rust can refuse) |
| `forgeFormToolbar(hasChanges:)`, `forgeSwipeGuard` | Sheet can't be swiped away holding typed text | Every form sheet after #407 |
| `ForgePermissions.openSettings()` | Opens the app's page in Settings | Device Lab, Relay |
| `ForgeRemoteImage` placeholder `failed` | A failed image falls back to a placeholder | Pocket Library covers (title placeholder), Soundscape artwork (coloured stand-in) |

## 2. P0 findings

### Errors, retry and recovery

| # | App | What happens now (source) | Fix |
|---|---|---|---|
| E1 | **Weather** | With location denied, the page shows `ForgeStateView.failed` with "Location access is off for Weather. Search for a city, or allow location in Settings." Its main button is **Use My Location**, which can't help: iOS won't ask again after a denial (`ForecastPage.swift`, `LocationProvider.swift`) | When the error is `.location(.denied)`, the main button becomes **Open Settings** (`ForgePermissions.openSettings()`), with Search for a City kept as the second |
| E1 | **FieldKit** | New Trip says "Location is off for FieldKit, so the track won't record. Turn it on in Settings." It offers no button, and Start still starts a trip that won't record (`StartTripView.swift`) | An **Open Settings** button under that text |
| E2 | **Relay, Soundscape, Commonplace, Pocket Library** | Network actions started from a button (Trust Again, add from Discover, Download, catalogue add, full text) fail into a plain alert. `ForgeFailure` already marks network errors retryable, but the alert offers only OK, so the person has to find the button again | Use `ForgeAlert.failure(error, retry: <the same action>)` where the action is a model command. This rides on each app's silent-wait move (0.2 item 9) because it touches the same buttons |
| E3 | **Starter** (`templates/ios-app`) | A note saves when its screen closes. If that save fails, the list shows an alert, but the edited text is gone, because the screen that held it has closed (`NoteDetailView.swift`, "A failure shows in the list's alert") | Keep the unsaved text in the model when the save fails, and offer it back from the alert (Try Again re-sends it). The starter is what new apps copy, so this matters more than its size suggests |
| E4 | **Urdu Naturally** | The name in Settings is saved when the screen closes. A refusal shows an alert, but the typed name is lost. The field already refuses names Rust would reject (`fitsName`), so only a storage failure can reach this | Low risk; note only. Moves with the app's next Settings change |
| E5 | **Checklist** | A failed search returns `nil` and shows nothing, the same as a cancelled one (`ChecklistModel.search`) | Treat a failure that isn't a cancellation like the other commands: set `errorMessage` |

What's already right and stays: Weather's forecast failures (offline, timeout and server each say so, with Try Again and Search). Commonplace's per-feed refresh problem ("Couldn't reach the site", shown on the feed's row, plus "1 feed couldn't refresh" in the list footer). Relay keeping a message's text when sending fails before it was kept. Soundscape's catalogue problems as `ForgeNotice`. Failed images falling back to a placeholder in Pocket Library and Soundscape, which `ForgeThumbnail` (0.2 item 5) will make the same everywhere.

### Form validation and input

| # | App | What happens now | Fix |
|---|---|---|---|
| F1 | Every form | A form with one required field (trip title, dog name, feed address, conversation title, book title) disables its confirm button until the field has text, and says nothing about why. This is iOS's own convention (Reminders, Contacts), and the field is the only one on screen | No change. The 0.2 showcase gets a form example that says this is the rule: disable for an empty required field; explain under the field for anything else |
| F2 | **Pocket Library** (reference form) | After a refused save, each problem shows under its field, but focus stays where it was. A VoiceOver or keyboard user isn't taken to the problem | After `errors` is set, move `focused` to the first field with an error. Make it a `ForgeFieldErrors` recipe in `forgeui.md` so the next app's form copies it |
| F3 | **FieldKit New Trip, Commonplace Add Feed** | A swipe down closed the sheet and lost a typed title or address | **Fixed in #407**: the swipe is blocked once the text differs from what the sheet opened with (FieldKit suggests a title, so an untouched sheet still swipes away). Pocket Library's add form now counts every field, not only title, author and notes |
| F4 | **Relay sign-up** | Name and server address are checked only when submitted, and a refusal is an alert ("Couldn't Create the Account") | When the server answers with a field-level reason (address unreachable, name taken), show it under that field with `ForgeFieldErrors`, as Pocket Library does. Needs Relay's Rust error to name the field; Relay's thread decides when |
| F5 | Keyboards | Number, URL and name fields already use the right keyboard and content type where they exist: Pocket Library pages (`numberEntry`), cover URL, Commonplace feed address, Relay server address and name | No change |

**Required versus optional.** No app marks fields as required or optional, and none needs to yet: every form's required field is its only text field, or (Pocket Library) its first, named in its placeholder. The rule for when a form gets a second required field: write "Optional" in the optional fields' placeholders, as Apple's apps do, rather than asterisks.

## 3. One helper worth considering

`ForgeStateAction.openSettings` (ForgeUI, under ten lines): a `ForgeStateAction("Open Settings", identifier: "<id>.openSettings")` that calls `ForgePermissions.openSettings()`, so a failed or empty state caused by a denied permission offers the way out in one line. Weather and FieldKit need it now (E1), and Device Lab and Relay each build the same button by hand. That's four uses, so it passes the "second use" bar. Its parity rule would be weak, since a check can't tell a denial from other failures, so this is adoption plus a line in `forgeui.md`, not a rule.

## 4. Noticed for the P1 and P2 passes

From the same scan, not yet judged:

- **Success and Undo:** the `forgeBanner` undo is used by Commonplace (and shown in Device Lab's patterns); Sketchbook has its own `UndoManager` for drawing. Destructive actions are mostly confirmed (confirmation dialogs in seven apps). The next pass checks which reversible actions (remove from library, delete a note, mark all read) offer Undo instead of a confirmation.
- **Accessibility beyond VoiceOver:** no app reads `colorSchemeContrast` or Differentiate Without Color, and only Weather (its animated sky) reads Reduce Motion; ForgeUI's own components handle both. Selection is marked with the `.isSelected` trait in four apps (Device Lab, Good Dog, Soundscape, Weather). The next pass looks for states shown by colour alone (Relay delivery ticks, Good Dog stage dots, Urdu skill bars).
- **Layout:** 41 fixed-width frames and 36 `lineLimit(1)` across the apps are candidates for checking at AX3. Weather's two-column grid is the known case.
- **Keyboard:** `scrollDismissesKeyboard` and keyboard toolbars are in the long forms. Hardware keyboard shortcuts (`.keyboardShortcut`) appear in no app.
- **Offline:** only Commonplace and Device Lab use the `connectivity` service, and Weather, Relay and Soundscape learn they're offline from a failed request. That matches Reuben's "only where it affects the task", so the next pass checks the moment of failure, not a banner.

## 5. Order

1. **E1** (Open Settings in Weather and FieldKit, with `ForgeStateAction.openSettings`): one small PR.
2. **F2** (focus to the first invalid field): Pocket Library plus the `forgeui.md` recipe.
3. **E3** (the starter keeps a failed save's text): goes with the next ForgeKit release, like the other starter changes.
4. **E2** with each app's silent-wait move (0.2 item 9), Relay first. **E5** and **F4** when those apps are next touched.
5. Then the P1 pass.

Phone time for 1 to 3: about 10 minutes. Deny location for Weather and FieldKit and use Open Settings. Submit a bad page number in Pocket Library and check where focus lands, with VoiceOver on.
