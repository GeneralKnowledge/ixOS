# Replication benchmarks

A replication benchmark takes a real iOS app as a behavioural reference and rebuilds it with ixOS, from scratch, to find out what ixOS is missing for a real app. The point is the platform, not the clone: every place where the rebuild needs a workaround is recorded, and the reusable patterns it uses are inventoried, so ixOS work is chosen from evidence rather than guesses.

| Benchmark | Reference | Our app | Status |
|---|---|---|---|
| [netnewswire/](netnewswire/README.md) | NetNewsWire (MIT, App Store) | [Commonplace](../../examples/commonplace/README.md) | B0: slice 1 built, reference not yet walked |

## Rules

- **Behaviour, not implementation.** Our app is written from scratch, with its own name, icon and look. No code, assets, icons or names are taken from the reference, whatever its licence.
- **iOS only, on the real iPhone.** The reference is walked on the phone over USB from Linux, not in a simulator (simulators need a Mac).
- **Rust for state and rules, SwiftUI/ForgeUI for presentation, Apple's frameworks where they fit**, as in any ixOS app.
- **Evidence, not interpretation.** The explorer (`forge ui explore --app <bundle id> --any-app`, [forge-explore.md](../forge-explore.md#apps-forge-didnt-build)) records what the reference does without an AI model. A person or an agent writes the specification from that evidence and says where a claim comes from.

## What each benchmark folder holds

| File | What |
|---|---|
| `README.md` | The target, its scope, what is left out, and how to run the comparison |
| `spec.md` | Screens, flows and states of the reference, each with its evidence, and how our screens map to its screens |
| `gaps.toml` | Every place our rebuild needed a workaround or couldn't follow the reference ([format](#gap-records)) |
| `patterns.toml` | Every reusable pattern the rebuild used ([format](#pattern-records)) |
| `fixtures/` | The test data both apps load (for a feed reader: feeds served from the laptop) |

## Gap records

One `[[gap]]` per thing the reference does that our app couldn't do cleanly:

```toml
[[gap]]
id = "swipe-gestures-in-explore"
reference = "What the reference app does"
ixos_today = "What ixOS offers for it now"
workaround = "What the app did instead (or 'none: not done yet')"
missing = "The reusable capability that would remove the workaround"
layer = "tooling"     # rust | swift | forgeui | appkit | forge-cli | tooling | docs
cost = "small"        # small | medium | large
blocks = false        # did it stop the feature, or only slow it down
second_use = "Another app that needs it, or 'none yet'"
```

A gap becomes ixOS work only with a second use, the same test as extracting framework code ([architecture.md](../architecture.md#framework-principles)). Things plain SwiftUI or Apple's frameworks already do are not gaps.

## Pattern records

One `[[pattern]]` per reusable pattern the rebuild relied on, whether or not ixOS provides it:

```toml
[[pattern]]
id = "command-then-snapshot"
does = "What the pattern does for the app"
provided_by = "Which ixOS component provides it now (or 'nothing')"
app_code = "What app-specific code remains, with paths"
duplicated = "What had to be duplicated or worked around, and from where"
shared = "candidate"  # yes (already shared) | candidate | no (app-specific)
why = "Why it is or isn't general enough to become a shared building block"
```

The inventory is the long-term output of each benchmark: patterns that recur across benchmarks are the strongest case for new ixOS building blocks.
