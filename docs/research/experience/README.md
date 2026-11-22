# ixOS Experience Research

Status: 2026-11-17, phase 1 started. Research only: nothing here changes an app, ForgeUI or ForgeAppKit. Implementation comes in its own PRs after the decisions at the end of the [audit](audit.md#decisions-for-reuben).

## Why

ixOS has spent its effort on what the apps can do: the Rust core, persistence, networking, device services, reliability. The experience of using the apps has had much less attention. The result is a set of technically capable apps that are not yet consistently enjoyable, intuitive or coherent.

The goal: ixOS doesn't just have apps that work. It has apps people enjoy using.

The larger aim is an ixOS product-design philosophy: not one appearance, but a recognisable standard for how an ixOS app welcomes someone, explains itself, handles mistakes, communicates success and makes its most important task easy. A new app made with `forge init` should start with a good experience, not only a working Rust core and some screens.

## The three questions

Every app is judged against these, in this order:

1. Can a new user understand what this app does without being taught?
2. Can an experienced user do its common tasks quickly and naturally?
3. Does using it feel polished, responsive and satisfying?

Modern-looking is not the same as good. A beautiful screen can still be frustrating, an animation doesn't fix poor navigation, and a design system doesn't make a product intuitive by itself.

## The six dimensions

Every finding records its evidence: source verified, visually verified (screenshot or phone) or user validated (the workflow performed). Statements about how an app feels stay judgements until user validated.

Each app is examined in six dimensions, which cover the five areas of the brief (visual identity, interaction, states and feedback, accessibility and ergonomics, product coherence):

| # | Dimension | What it covers |
|---|---|---|
| 01 | Visual design | Typography, spacing, colour, hierarchy, iconography, motion, consistency |
| 02 | Navigation | Information architecture, discoverability, back navigation |
| 03 | Interaction | Taps, gestures, menus, editing, selection, undo |
| 04 | Feedback | Loading, empty, error, success, progress, offline, permissions, confirmation |
| 05 | Accessibility | VoiceOver, Dynamic Type, contrast, touch targets, haptics, dark mode, one-handed use |
| 06 | Product experience | Onboarding, common workflows, friction, confidence; personality and coherence across ixOS |

## Phases

| Phase | What | Output |
|---|---|---|
| 1. Audit | What we have: every app, its screens and main journeys, and what each app is trying to be | [audit.md](audit.md), [references.md](references.md) (this PR) |
| 1b. Reference deep audits | What mature apps do, and why: LocalSend, Ice Cubes, NetNewsWire, then four transfer apps for Relay | One PR each, `references/<app>.md` |
| 2. Design direction | An ixOS visual language, interaction principles and a small set of foundations | A design-direction doc PR |
| 3. Forge Experience System | The shared foundations and components (ForgeUI and ForgeAppKit), plus quality checks | Implementation PRs |
| 4. Product UX implementation | Each app's main journey redesigned end to end, not only its components: what the first screen offers, how the main task flows, what happens when it fails or is repeated. Each journey states a measurable success criterion. **Relay's complete transfer journey is first** (Reuben, 2026-11-17); the order after it comes from the reference audits and Reuben's priorities, not from the audit's order | One PR per app |
| 5. Experience validation | Whole workflows on the iPhone: usability, consistency, accessibility, responsiveness | Phone sittings, ledger records |

Research PRs stay separate from implementation PRs, so nothing is changed for looks before the problem is understood. Phase 3 serves phase 4 and is not a substitute for it: a banner makes one interaction better, a redesigned journey makes the product better.

## Principles

- **Don't copy appearance; reverse-engineer decisions.** Why is an action prominent, why is something hidden, how does the app say it is unsure, what happens after a mistake.
- **Shared infrastructure gives good defaults; it doesn't dictate each app's design.** Each app keeps its own personality inside one recognisable ecosystem.
- **Native first.** Wrap Apple's frameworks and follow iOS conventions; never replace them ([architecture.md](../../architecture.md)).
- **Guidelines, not compliance rules.** Interaction principles are starting points with stated exceptions: feedback in proportion to the action, sample data only where it isn't personal, settings placed by each app's information architecture.
- **Extract on the second use.** A shared component exists because two apps needed it, not because a catalogue should have it. No giant component library.
- **Borrow ideas, not dependencies.** The ForgeUI proposal already set this for design systems; it holds for the reference apps too.

## Related

- [ForgeUI coverage audit](forgeui-coverage.md) (2026-11-18): which ForgeUI and ForgeAppKit pieces each app uses, duplicates or lacks, and the checks that would keep it that way.
- [App UI review](app-ui-review-2026-11.md) (2026-11-21): what is still plain or unfinished in each app, and the three pieces (share a file, rename, notice) that three or more apps hand-build.
- [ForgeUI 0.2: component-demand audit and specification](forgeui-0.2.md) (2026-11-22, proposal): which components the apps build by hand, what stays native, the 0.2 scope, and the Forge A2UI catalogue contract.
- [UI Finish Audit: P0 pass](ui-finish-audit.md) (2026-11-22): errors, retry and recovery, and form validation across the apps; what ForgeUI and ForgeAppKit already cover, and the specific screens that don't use it yet.
- [App friction and small-defect audit](https://claude.ai/artifact/HYycziassRTwYkXkwRCF4C) (2026-11-16): 23 functional and 10 visual defects, most now fixed or queued. This research does not repeat them; it links to them where they matter.
- [better-by-default.md](../better-by-default.md): defaults and parity checks so apps get things right without per-app fixes (PR #282).
- [forgeui.md](../../forgeui.md), [forgeappkit.md](../../forgeappkit.md), [ios-ui.md](../../ios-ui.md), [visual-regression.md](../../visual-regression.md), [ax-audit.md](../ax-audit.md).
