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
APP = "com.apple.calculator"
LRM = "‎"  # left-to-right mark, before every part of a number (R0 finding 4)


def step(**action):
    key, value = next(iter(action.items()))
    if isinstance(value, dict):
        inner = ", ".join(f'{k} = "{v}"' for k, v in value.items())
        return f"[[step]]\n{key} = {{ {inner} }}\n"
    return f'[[step]]\n{key} = "{value}"\n'


def test_file(sequences, extras):
    parts = [
        'name = "Calculator R1 sequences"\n',
        f'app = "{APP}"\n',
        "timeout = 10\n\n",
        step(orientation="portrait"),
    ]
    for number, seq in enumerate(sequences, 1):
        parts.append(f"\n# {seq['name']}: {seq['why']}\n")
        parts.append(step(tap={"id": "AllClear"}))
        parts.append(step(tap={"id": "AllClear"}))
        for key in seq["keys"]:
            parts.append(step(tap={"id": key}))
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


def read_display(tree_file):
    if not tree_file.exists():
        return None, None, "no tree saved"
    tree = json.loads(tree_file.read_text())
    display = texts(find(tree, "StandardInputView"))
    expression = texts(find(tree, "StandardResultView"))
    note = "" if display else "display not in the tree"
    return (display[0] if display else None), (expression[0] if expression else None), note


def toml_string(text):
    return json.dumps(text, ensure_ascii=True)  # JSON strings are valid TOML basic strings


def record(args):
    sequences = tomllib.loads(SEQUENCES.read_text())["sequence"]
    out = pathlib.Path(args.out or f"~/expeditions/calculator/r1-{args.pass_}").expanduser()
    out.mkdir(parents=True, exist_ok=True)
    run_file = out / "run.toml"
    run_file.write_text(test_file(sequences, not args.no_extras))
    command = [args.forge, "ui", "run", str(run_file), "--any-app", "--out", str(out / "run")]
    print("$", " ".join(command), flush=True)
    started = time.monotonic()
    status = subprocess.call(command)
    seconds = time.monotonic() - started
    print(f"forge ui run: exit {status} in {seconds:.0f} s", flush=True)

    lines = [
        f"# Calculator R1, pass {args.pass_}: forge ui run exit {status}, {seconds:.0f} s.",
        "# display/expression have every U+200E removed; *_raw keep them escaped.",
        "",
    ]
    for number, seq in enumerate(sequences, 1):
        tree_file = out / "run" / f"{number:02d}-{seq['name']}.tree.json"
        display, expression, note = read_display(tree_file)
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
    parser.add_argument("--no-extras", action="store_true", help="skip opening History at the end")
    args = parser.parse_args()
    if args.compare:
        sys.exit(compare(args))
    if not args.pass_:
        parser.error("give --pass NAME or --compare A B")
    sys.exit(record(args))


if __name__ == "__main__":
    main()
