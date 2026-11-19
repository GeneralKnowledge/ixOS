#!/usr/bin/env python3
"""Record Calculator's answers to the R1 sequence table.

Run on the laptop with the iPhone on USB, unlocked, and Calculator open in
front (Forge can't open a system app itself: R0 finding 22). Calculator must
be at `full` in ~/.config/forge/device-control.toml, so Forge presses Delete
without asking (at `operate` the commit gate stops at its "Delete" label).
That is the person's decision: back up the file, set only Calculator's line to
`full`, and set it back to `operate` straight after the passes (see the plan,
docs/research/app-as-specification.md, R1).

    python3 run_sequences.py --pass a
    python3 run_sequences.py --pass b
    python3 run_sequences.py --compare a b

The same sums on the recreation (Sums, inside Device Lab, R2): no policy
change, because Forge built it. And the held-out sums on either app:

    python3 run_sequences.py --app sums --pass sums-a
    python3 run_sequences.py --pass b2 --sequences evidence/sequences-2.toml
    python3 run_sequences.py --compare a sums-a
    python3 run_sequences.py --compare evidence/predictions-2.toml b2

Each pass writes one `forge ui run` test file from evidence/sequences.toml,
runs it with --any-app (Forge presses every key and opens and closes
History; landscape is left out for now), then reads the display from the tree Forge saved
beside each screenshot and writes `results-<pass>.toml`. Everything goes to
~/expeditions/calculator/r1-<pass>/ unless --out says otherwise; the results
file is what gets committed (it holds no personal data, only the answers to
our own sums). Uses only the forge CLI and the Python standard library.
"""

import argparse
import json
import pathlib
import subprocess
import sys
import time
import tomllib

HERE = pathlib.Path(__file__).resolve().parent
SEQUENCES = HERE / "evidence" / "sequences.toml"
APPS = {
    # The reference: keys by its own ids, the display in StandardInputView.
    "reference": {
        "bundle": "com.apple.calculator",
        "key": lambda name: name,
        "display": "StandardInputView",
        "expression": "StandardResultView",
    },
    # The recreation, inside Device Lab: sums.key.<id in lower case>.
    "sums": {
        "bundle": "org.forgemobile.devicelab",
        "launch_args": ["-ForgeTemporaryData", "-launchApp", "sums"],
        "key": lambda name: "sums.key." + ("clear" if name == "AllClear" else name.lower()),
        "display": "sums.display",
        "expression": "sums.expression",
    },
}
LRM = "‎"  # left-to-right mark, before every part of a number (R0 finding 4)


def step(**action):
    key, value = next(iter(action.items()))
    if value == {}:
        return f"[[step]]\n{key} = {{}}\n"
    if isinstance(value, dict):
        inner = ", ".join(f'{k} = "{v}"' for k, v in value.items())
        return f"[[step]]\n{key} = {{ {inner} }}\n"
    return f'[[step]]\n{key} = "{value}"\n'


def test_file(sequences, extras, app):
    parts = [
        'name = "Calculator sequences"\n',
        f'app = "{app["bundle"]}"\n',
        "timeout = 10\n\n",
    ]
    if "launch_args" in app:
        args = ", ".join(f'"{a}"' for a in app["launch_args"])
        parts.append(f"[[step]]\nlaunch = {{ args = [{args}] }}\n")
    parts += [step(activate={}), step(orientation="portrait")]
    for number, seq in enumerate(sequences, 1):
        parts.append(f"\n# {seq['name']}: {seq['why']}\n")
        # Bring Calculator to the front before every sum: `forge ui run`
        # doesn't, and a person glancing at another app would stop the run.
        parts.append(step(activate={}))
        # The clear key is id Clear ("Clear") while there is something to
        # clear and AllClear ("All Clear") after: press whichever is showing.
        parts.append(step(tap={"label_contains": "Clear"}))
        parts.append(step(tap={"label_contains": "Clear"}))
        for key in seq["keys"]:
            parts.append(step(tap={"id": app["key"](key)}))
        parts.append(step(screenshot=f"{number:02d}-{seq['name']}"))
    if extras:
        parts.append("\n# History, opened and closed by Forge\n")
        parts.append(step(tap={"id": "SidebarButton"}))
        parts.append(step(screenshot="zz-1-history"))
        parts.append(step(tap={"id": "CloseButton"}))
    return "".join(parts)


def find(node, identifier):
    if node.get("identifier") == identifier:
        return node
    for child in node.get("children", []):
        hit = find(child, identifier)
        if hit:
            return hit
    return None


def texts(node):
    if node is None:
        return []
    found = []
    if node.get("kind") == "StaticText":
        found.append(node.get("value") or node.get("label") or "")
    for child in node.get("children", []):
        found.extend(texts(child))
    return found


def read_display(tree_file, app):
    if not tree_file.exists():
        return None, None, "no tree saved"
    tree = json.loads(tree_file.read_text())
    display = texts(find(tree, app["display"]))
    expression = [t for t in texts(find(tree, app["expression"])) if t.strip()]
    note = "" if display else "display not in the tree"
    return (display[0] if display else None), (expression[0] if expression else None), note


def toml_string(text):
    return json.dumps(text, ensure_ascii=True)  # JSON strings are valid TOML basic strings


def record(args):
    app = APPS[args.app]
    sequences = tomllib.loads(pathlib.Path(args.sequences).read_text())["sequence"]
    if args.start_at:
        names = [seq["name"] for seq in sequences]
        if args.start_at not in names:
            sys.exit(f"no sequence named {args.start_at}")
        sequences = sequences[names.index(args.start_at):]
    out = pathlib.Path(args.out or f"~/expeditions/calculator/r1-{args.pass_}").expanduser()
    out.mkdir(parents=True, exist_ok=True)
    run_file = out / "run.toml"
    # History is the reference's; Sums has none.
    run_file.write_text(test_file(sequences, args.app == "reference" and not args.no_extras, app))
    command = [args.forge, "ui", "run", str(run_file), "--out", str(out / "run")]
    if args.app == "reference":
        command.insert(4, "--any-app")
    print("$", " ".join(command), flush=True)
    started = time.monotonic()
    status = subprocess.call(command)
    seconds = time.monotonic() - started
    print(f"forge ui run: exit {status} in {seconds:.0f} s", flush=True)

    lines = [
        f"# Calculator, {args.app}, pass {args.pass_}: forge ui run exit {status}, {seconds:.0f} s.",
        "# display/expression have every U+200E removed; *_raw keep them escaped.",
        "",
    ]
    for number, seq in enumerate(sequences, 1):
        tree_file = out / "run" / f"{number:02d}-{seq['name']}.tree.json"
        display, expression, note = read_display(tree_file, app)
        lines += ["[[result]]", f"name = {toml_string(seq['name'])}"]
        lines.append("keys = [" + ", ".join(toml_string(k) for k in seq["keys"]) + "]")
        if display is not None:
            lines.append(f"display = {toml_string(display.replace(LRM, ''))}")
            lines.append(f"display_raw = {toml_string(display)}")
        if expression is not None:
            lines.append(f"expression = {toml_string(expression.replace(LRM, ''))}")
        if note:
            lines.append(f"note = {toml_string(note)}")
        lines.append("")
    results = out / f"results-{args.pass_}.toml"
    results.write_text("\n".join(lines))
    print(f"wrote {results}")
    return status


def compare(args):
    def load(label):
        path = pathlib.Path(label)
        if not path.is_file():
            path = pathlib.Path(args.out or "~/expeditions/calculator").expanduser()
            path = path / f"r1-{label}" / f"results-{label}.toml"
        return {r["name"]: r for r in tomllib.loads(path.read_text())["result"]}

    first, second = load(args.compare[0]), load(args.compare[1])
    differ = 0
    for name, a in first.items():
        b = second.get(name, {})
        if (a.get("display"), a.get("expression")) != (b.get("display"), b.get("expression")):
            differ += 1
            print(f"DIFFERENT {name}: {a.get('display')!r} / {b.get('display')!r}")
    print(f"{len(first) - differ} of {len(first)} sequences gave the same answer in both passes")
    return 1 if differ else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--pass", dest="pass_", help="a name for this recording, e.g. a")
    parser.add_argument("--compare", nargs=2, metavar=("A", "B"), help="compare two passes")
    parser.add_argument("--forge", default="forge", help="the forge binary (default: forge on PATH)")
    parser.add_argument("--out", help="output folder (default ~/expeditions/calculator/r1-<pass>)")
    parser.add_argument("--start-at", metavar="NAME", help="skip the sequences before this one (to finish a pass that stopped)")
    parser.add_argument("--no-extras", action="store_true", help="skip opening History at the end")
    parser.add_argument("--app", choices=sorted(APPS), default="reference", help="which calculator (default: the reference)")
    parser.add_argument("--sequences", default=str(SEQUENCES), help="the sequence file (default: evidence/sequences.toml)")
    args = parser.parse_args()
    if args.compare:
        sys.exit(compare(args))
    if not args.pass_:
        parser.error("give --pass NAME or --compare A B")
    sys.exit(record(args))


if __name__ == "__main__":
    main()
