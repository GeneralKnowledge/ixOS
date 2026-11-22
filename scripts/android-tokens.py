#!/usr/bin/env python3
"""Generates the Android app's ForgeUI tokens from ForgeUI's Swift tokens.

ForgeUI's Swift token files (forgekit/Sources/ForgeUI/Tokens) are the one source: the palette,
the accents, the text roles, spacing and radii. This reads their values and writes them as
Kotlin for each Android app (Pocket Library and Relay), so the platforms can't drift apart. Swift is never
written, so the iPhone apps don't change.

    scripts/android-tokens.py           # rewrite the Kotlin file
    scripts/android-tokens.py --check   # fail if it is out of date (make validate)
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOKENS = ROOT / "forgekit/Sources/ForgeUI/Tokens"
# Each Android app's theme package, and the file written there. The theme moves into a shared
# Android ForgeKit after Relay's first Android stage (examples/relay/docs/relay-android.md).
OUTS = {
    "org.forgemobile.pocketlibrary.ui.theme":
        ROOT / "examples/pocket-library-android/app/src/main/java/org/forgemobile/pocketlibrary/ui/theme/ForgeTokens.kt",
    "org.forgemobile.relay.ui.theme":
        ROOT / "examples/relay-android/app/src/main/java/org/forgemobile/relay/ui/theme/ForgeTokens.kt",
}

ROLES = ["background", "groupedBackground", "surface", "surfaceRaised",
         "textPrimary", "textSecondary", "textTertiary", "separator"]
TONES = ["accent", "neutral", "success", "warning", "danger", "info"]


def fail(message):
    sys.exit(f"android-tokens: {message}")


def read(name):
    return (TOKENS / name).read_text()


def block(text, start, end):
    """The text between the first `start` and the next `end` after it."""
    i = text.find(start)
    if i < 0:
        fail(f"can't find {start!r}")
    j = text.find(end, i + len(start))
    if j < 0:
        fail(f"can't find {end!r} after {start!r}")
    return text[i + len(start):j]


def colors(text):
    """One appearance's roles and tones from a `ForgeColors(...)` literal."""
    out = {}
    for role in ROLES:
        m = re.search(rf"\b{role}: ForgeRGBA\(hex: 0x([0-9A-Fa-f]{{6}})\)", text)
        if not m:
            fail(f"no hex for {role}")
        out[role] = m.group(1).upper()
    for tone in TONES:
        m = re.search(
            rf"\b{tone}: ForgeToneColors\(solid: ForgeRGBA\(hex: 0x(\w{{6}})\), onSolid: ForgeRGBA\(hex: 0x(\w{{6}})\), "
            rf"text: ForgeRGBA\(hex: 0x(\w{{6}})\), softOpacity: ForgePalette\.(lightSoft|darkSoft)\)",
            text,
        )
        if not m:
            fail(f"no tone {tone}")
        out[tone] = tuple(g.upper() for g in m.groups()[:3])
    return out


def number(text, pattern, what):
    m = re.search(pattern, text)
    if not m:
        fail(f"no value for {what}")
    return m.group(1)


def parse():
    c = read("ForgeColors.swift")
    standard = block(c, "public static let standard = ForgePalette(", "\n    )\n")
    light = colors(block(standard, "light: ForgeColors(", "\n        ),"))
    dark = colors(block(standard, "dark: ForgeColors(", "\n        )"))
    soft = {
        "light": number(c, r"static let lightSoft = ([\d.]+)", "lightSoft"),
        "dark": number(c, r"static let darkSoft = ([\d.]+)", "darkSoft"),
    }
    contrast = number(c, r"static let increasedContrastAmount = ([\d.]+)", "increasedContrastAmount")

    a = read("ForgeAccent.swift")
    accents = re.findall(r"^    case (\w+)$", a, re.M)
    names = dict(re.findall(r'case \.(\w+): "([^"]+)"', block(a, "public var name: String", "\n    }\n")))
    def accent_pairs(start):
        body = block(a, start, "return ForgeToneColors(")
        pairs = dict((k, (s.upper(), t.upper())) for k, s, t in
                     re.findall(r"case \.(\w+): \(0x(\w{6}), 0x(\w{6})\)", body))
        on = number(a[a.find(start):], r"onSolid: ForgeRGBA\(hex: 0x(\w{6})\)", f"{start} onSolid").upper()
        return pairs, on
    accent_light, on_light = accent_pairs("public var light: ForgeToneColors")
    accent_dark, on_dark = accent_pairs("public var dark: ForgeToneColors")
    for name in accents:
        if name not in accent_light or name not in accent_dark or name not in names:
            fail(f"accent {name} is incomplete")

    t = read("ForgeTextRole.swift")
    roles = re.findall(r"^    case (\w+)$", block(t, "public enum ForgeTextRole", "public var id"), re.M)
    def switch(prop):
        body = block(t, f"public var {prop}:", "\n    }\n")
        out = {}
        for cases, value in re.findall(r"case ((?:\.\w+(?:, )?)+): (\.?\w+)", body):
            for case in re.findall(r"\.(\w+)", cases):
                out[case] = value.lstrip(".")
        default = re.search(r"default: (\w+)", body)
        for role in roles:
            if role not in out:
                if not default:
                    fail(f"text role {role} has no {prop}")
                out[role] = default.group(1)
        return out
    text = {prop: switch(prop) for prop in ["style", "weight", "emphasis", "monospacedDigits"]}

    th = read("ForgeTheme.swift")
    def defaults(struct):
        sig = block(th, f"public struct {struct}", "public static let standard")
        init = block(sig, "public init(", ")")
        return re.findall(r"(\w+): Double = ([\d.]+)", init)
    spacing = defaults("ForgeSpacing")
    radius = defaults("ForgeRadius")
    if not spacing or not radius:
        fail("no spacing or radius defaults")
    return light, dark, soft, contrast, accents, names, accent_light, accent_dark, on_light, on_dark, roles, text, spacing, radius


def kotlin(package):
    (light, dark, soft, contrast, accents, names, accent_light, accent_dark,
     on_light, on_dark, roles, text, spacing, radius) = parse()

    def rgb(hex6):
        return f"ForgeRGBA(0x{hex6})"

    def palette(name, values, soft_name):
        lines = [f"    val {name} = ForgeColors("]
        for role in ROLES:
            lines.append(f"        {role} = {rgb(values[role])},")
        for tone in TONES:
            s, on, tx = values[tone]
            lines.append(f"        {tone} = ForgeToneColors({rgb(s)}, {rgb(on)}, {rgb(tx)}, {soft_name}),")
        lines.append("    )")
        return lines

    def kname(name):
        return re.sub(r"(?<!^)(?=[A-Z])", "_", name).upper()

    out = [
        "// GENERATED by scripts/android-tokens.py from ForgeUI's Swift tokens",
        "// (forgekit/Sources/ForgeUI/Tokens). Don't edit: change the Swift and run the script.",
        f"package {package}",
        "",
        "/** ForgeUI's token values, the same as on iOS. Plain Kotlin, so JVM tests check them. */",
        "object ForgeTokens {",
        f"    const val LIGHT_SOFT = {soft['light']}",
        f"    const val DARK_SOFT = {soft['dark']}",
        f"    const val INCREASED_CONTRAST_AMOUNT = {contrast}",
        "",
    ]
    out += palette("light", light, "LIGHT_SOFT")
    out.append("")
    out += palette("dark", dark, "DARK_SOFT")
    out += [
        "",
        f"    val spacing = ForgeSpacing({', '.join(f'{k} = {v}f' for k, v in spacing)})",
        f"    val radius = ForgeRadius({', '.join(f'{k} = {v}f' for k, v in radius)})",
        "}",
        "",
        "/** The accents an app can choose from, each checked for contrast in light and dark. */",
        "enum class ForgeAccent(val title: String, val light: ForgeToneColors, val dark: ForgeToneColors) {",
    ]
    for name in accents:
        ls, lt = accent_light[name]
        ds, dt = accent_dark[name]
        out.append(
            f"    {kname(name)}(\"{names[name]}\", "
            f"ForgeToneColors({rgb(ls)}, {rgb(on_light)}, {rgb(lt)}, ForgeTokens.LIGHT_SOFT), "
            f"ForgeToneColors({rgb(ds)}, {rgb(on_dark)}, {rgb(dt)}, ForgeTokens.DARK_SOFT)),"
        )
    out += [
        "}",
        "",
        "/** What a piece of text is (a row's title, its metadata), defined once, as ForgeTextRole on iOS. */",
        "enum class ForgeTextRole(",
        "    val style: ForgeTextStyle,",
        "    val weight: ForgeTextWeight,",
        "    val emphasis: ForgeTextEmphasis,",
        "    val monospacedDigits: Boolean,",
        ") {",
    ]
    for role in roles:
        out.append(
            f"    {kname(role)}(ForgeTextStyle.{kname(text['style'][role])}, ForgeTextWeight.{kname(text['weight'][role])}, "
            f"ForgeTextEmphasis.{kname(text['emphasis'][role])}, {text['monospacedDigits'][role]}),"
        )
    out += [
        "}",
        "",
    ]
    return "\n".join(out)


def main():
    for package, out in OUTS.items():
        generated = kotlin(package)
        if "--check" in sys.argv:
            current = out.read_text() if out.exists() else ""
            if current != generated:
                fail(f"{out.relative_to(ROOT)} is out of date with ForgeUI's Swift tokens; run scripts/android-tokens.py")
            continue
        out.write_text(generated)
        print(f"wrote {out.relative_to(ROOT)}")
    if "--check" in sys.argv:
        print("android-tokens: Kotlin tokens match ForgeUI's Swift tokens")


if __name__ == "__main__":
    main()
