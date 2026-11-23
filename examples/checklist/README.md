# Checklist

A list of things to do, kept on the phone. The smallest app on the
application runtime (`forge-app` in Rust, ForgeKit's `ForgeHost` in Swift),
so it is also the shortest worked example of every runtime rule: commands,
queries, on-screen and finishing work, cancellation and the launch report.
Hosted in [Device Lab](../device-lab/README.md) (Apps, Checklist).

## The app

- **Add, tick, rename, delete.** Titles are tidied (runs of spaces become
  one) and refused when empty or over 200 characters, in plain words.
- **Due dates.** Any item can have a day it is due, from its menu (Due
  Date). The row says "Due today", "Due tomorrow", "Due 24 Nov", or
  "Overdue: 20 Nov" in the warning tone.
- **Agenda.** All or Agenda at the top. The agenda lists the items not yet
  done that have a due day, under Overdue, Today and Upcoming, in day order.
- **Search** every word of a query, as you type; leaving the search stops
  it in Rust.
- **Share as Text or as a Spreadsheet** (Item, Done, Due), and **Import from
  Spreadsheet**: rows it can't use are skipped and listed by line, and items
  already on the list aren't added twice.
- **Clear Done Items**, after a confirmation.

## The Rust core

`rust/` is the app's own crate (`checklist_core`):

- `src/checklist.rs`: what a checklist is (items, due days, the agenda,
  search, the text export). Plain Rust, no runtime and no FFI.
- `src/lib.rs`: the API Swift calls, on the runtime. Commands (`add`,
  `toggle`, `rename`, `set_due`, `remove`, `clear_done`, `import_csv`) run
  one at a time and save before they return; queries (`items`, `summary`,
  `agenda`) never wait for a command.

A due day is a local day, `YYYY-MM-DD`, not a moment: Swift says which day
is today in the phone's time zone (always on the Gregorian calendar), and
Rust compares days. Saved data is `checklist.json` through `forge_app`'s
`Document`; every version the app has shipped is in `rust/fixtures/` and is
opened by the tests.

## Checking it

```bash
cargo test --manifest-path examples/checklist/rust/Cargo.toml
forge check swift --project examples/checklist   # the model tests, on Linux
```

What it must do and what proves it is in [requirements.toml](requirements.toml)
(CL-1 to CL-4). The phone scenarios are in `uitests/`, run against Device
Lab with `forge ui run examples/checklist/uitests/<name>.toml`.

## Next

From the [app plan](../../docs/app-plan.md#checklist): reminders for due
items, recurring items, several lists, Shortcuts, and a widget.
