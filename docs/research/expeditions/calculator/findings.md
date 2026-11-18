# Findings register: Calculator

Tags: **O** Observed, **I** Inferred, **U** Unknown. Representation: visual, structural, behavioural, navigational, temporal. Source names a file in [evidence/](evidence/) or a screenshot; "laptop" means the R0 folder `~/expeditions/calculator/r0-20261118T0044Z` on the laptop.

| # | Tag | Representation | Finding | Source |
|---|---|---|---|---|
| 1 | O | structural | All 20 basic keys are in the tree as type Key, each with a label (7, Divide, Change Sign, Point, All Clear…) and an English-word id (Seven, Divide, Negate, Decimal, AllClear) | main-tree.txt |
| 2 | I | behavioural | Every key can be pressed by id, with no coordinates | from 1 |
| 3 | O | structural | In portrait the display is a StaticText (no id) inside ScrollView `StandardInputView` "Edit field"; label and value equal the number | main-tree.txt, relaunch-tree.txt |
| 4 | O | structural | Every displayed number and expression has U+200E (left-to-right mark) before each part, e.g. `‎12‎+‎7`. Strip them all before comparing | relaunch-tree.txt |
| 5 | O | structural | **In landscape the result and expression are not in the tree at all**: both containers hold only invisible, empty children. Two reads, same result | landscape-tree.txt |
| 6 | O | visual | Landscape is the basic keypad in pill-shaped keys, not a scientific calculator. Rows: 7 8 9 ⌫ ÷ / 4 5 6 C × / 1 2 3 % − / ± 0 . = + | r0-landscape.png |
| 7 | I | navigational | Scientific mode is behind Change Mode, whose id carries the state: `Mode: basic; unitConversion: false` | main-tree.txt |
| 8 | O | visual, structural | The clear key shows C or AC depending on state. **Corrected 2026-11-18 (R1):** the tree does show it: id `Clear` / "Clear" while there is something to clear, `AllClear` / "All Clear" after. R0 only saw the AllClear state | r0-landscape.png, R1 attempt 3 |
| 9 | O | structural | All Clear's place in the tree changes: last (round 1, display 0) or second after Delete (display 19), matching the screen | main-tree.txt, relaunch-tree.txt |
| 10 | I | structural | The tree order depends on the clear key's AC/C state | from 9 |
| 11 | U | visual | Whether the key read AC or C in round 1 | not recorded |
| 12 | O | structural | "Last Expression" (`StandardResultView`) appears only once there is a last expression; it holds e.g. "12+7" above the result | relaunch-tree.txt |
| 13 | O | temporal | After 12 + 7 =, closing from the app switcher and reopening, the display still shows 12+7 and 19 | r0-after-relaunch.png, relaunch-tree.txt |
| 14 | I | temporal | The last calculation is saved, not just held in memory | from 13 |
| 15 | O | navigational | History (`SidebarButton`) opens a half-height sheet; in the tree it is an Alert `HistoryView` with Edit (no id) and Close (`CloseButton`); the keypad stays in the tree, hidden | laptop history-tree.txt |
| 16 | O | structural | Each History entry is one `HistoryRecordCell` labelled like "Input, 12 + 7, result, 19", containing `InputRecord` and `ResultRecord`. All entries share these ids (9 duplicate-id warnings) | laptop history-tree.txt |
| 17 | O | temporal | History keeps 12 + 7 after the relaunch, under "Today"; older entries under "Previous 30 Days"; a scroll bar says 9 pages | laptop history-tree.txt |
| 18 | O | visual | History rounds results (three decimal places in one entry, "…" in the spoken label) and uses a thousands separator | laptop history-tree.txt |
| 19 | U | behavioural | Whether the main display rounds and separates thousands the same way as History | R1 |
| 20 | O | visual | A ">" tab on the left edge in every screenshot, never in the tree in four readings | main.png, r0-landscape.png |
| 21 | U | navigational | What the ">" tab does | |
| 22 | O | equipment | `forge ui tree --app com.apple.calculator` failed: "not installed on the phone". **Fixed 2026-11-18** (#318): Forge looks system apps up too, and opened Calculator itself on the phone | laptop |
| 23 | O | equipment | Forge reads an app it didn't build only with `--any-app` and a line in the person's policy file; the laptop session would not add either without Reuben's words | field notes 00:44 |
| 24 | O | equipment | Tree reads 1.8 to 8.5 s, screenshots 1.1 s; the phone dropped off USB once | field notes |
| 25 | O | structural | Forge's accessibility audit: nothing on the keypad; 12 warnings with History open (9 duplicate ids, Sheet Grabber and Edit without ids, Sheet Grabber 74 × 26 pt) | laptop history-tree.json |
| 26 | O | behavioural | 0.1 + 0.2 = 0.3, not 0.30000000000000004 | reference-results.toml |
| 27 | I | behavioural | Arithmetic is decimal, or rounded before display; plain `f64` printed as is would differ | from 26 |
| 28 | O | behavioural | Results show up to 11 decimal places, last digit rounded (2÷3 = 0.66666666667) | reference-results.toml |
| 29 | O | behavioural | Large and small results switch to scientific notation with nine significant digits (9.99999998e17, 1.000000002e-18) | reference-results.toml |
| 30 | O | behavioural | Typing has no 9-digit limit: 18 digits are kept, with thousands separators | reference-results.toml |
| 31 | O | behavioural | Operator precedence is respected (2+3×4 = 14) | reference-results.toml |
| 32 | O | behavioural | Repeated equals repeats the last operation and operand (2+3=== gives 11, expression "8+3") | reference-results.toml |
| 33 | O | behavioural | A second operator replaces the first; an operator first starts from 0; = alone gives 0 | reference-results.toml |
| 34 | O | behavioural | Percent is displayed, not evaluated ("50%"); in a sum it is relative to the left operand (200+10% = 220; 50×10% = 5) | reference-results.toml |
| 35 | O | visual | A negative typed number shows in brackets, "(-5)"; ± on 0 stays 0 | reference-results.toml |
| 36 | O | behavioural | 5÷0 shows "Undefined"; 0÷0 shows "Indeterminate"; neither string has U+200E | reference-results.toml |
| 37 | O | behavioural | Delete removes the last digit, the point, or the pending operator (7 + ⌫ 5 = gives 75); after a result it clears to 0 | reference-results.toml |
| 38 | O | equipment | `forge ui run` doesn't bring the app to the front; a run that started with another app in front stopped at its first tap, correctly. Fixed in the script with `activate` before each sum | field notes 16:16 |
| 39 | O | equipment | At a failure with another app in front, Forge saved that app's screen, which the docs say it doesn't | field notes 16:16 |
| 40 | O | equipment | Delete can only be pressed unattended at `full`: at `operate` the commit gate stops at its label | field notes 09:46 |
| 41 | U | behavioural | Whether Calculator answers the same way twice (no second pass recorded; recheck any sum the rebuild disagrees with) | |
