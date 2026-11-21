//! Python Lab's graphics samples (docs/visual-labs-spec.md, P1): standard
//! library only, so they run on the phone today. Kept in Rust so the tests
//! run the same code the Examples menu shows.

/// A sample script for Python Lab's Examples menu.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PythonLabSampleCode {
    pub id: String,
    pub title: String,
    pub code: String,
}

pub const PNG_GRADIENT: &str = r##"# Writes gradient.png in this project with zlib and struct (no packages).
import struct, zlib
width, height = 256, 128
rows = []
for y in range(height):
    row = bytearray([0])  # each row starts with its filter type
    for x in range(width):
        row += bytes([x, y * 2, 255 - x])
    rows.append(bytes(row))

def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9))
png += chunk(b"IEND", b"")
with open("gradient.png", "wb") as f:
    f.write(png)
print(f"Wrote gradient.png, {width} by {height}, {len(png)} bytes")
"##;

pub const SVG_CHART: &str = r##"# Writes chart.svg: a bar chart of the numbers below.
values = {"Mon": 3.2, "Tue": 5.1, "Wed": 2.4, "Thu": 4.0, "Fri": 6.3}
width, height, gap = 320, 200, 12
bar = (width - gap * (len(values) + 1)) / len(values)
top = max(values.values())
parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height + 30}" '
         f'font-family="sans-serif" font-size="12">']
for i, (day, km) in enumerate(values.items()):
    h = km / top * (height - 20)
    x = gap + i * (bar + gap)
    parts.append(f'<rect x="{x:.1f}" y="{height - h:.1f}" width="{bar:.1f}" height="{h:.1f}" fill="#2a9d8f"/>')
    parts.append(f'<text x="{x + bar / 2:.1f}" y="{height - h - 4:.1f}" text-anchor="middle">{km}</text>')
    parts.append(f'<text x="{x + bar / 2:.1f}" y="{height + 18}" text-anchor="middle">{day}</text>')
parts.append("</svg>")
with open("chart.svg", "w") as f:
    f.write("\n".join(parts))
print("Wrote chart.svg with", len(values), "bars")
"##;

pub const SVG_SAFETY: &str = r##"# An SVG that tries to load a remote image and run a script. The viewer
# must show the red square and its text, and neither the image nor the
# script.
svg = """<svg xmlns="http://www.w3.org/2000/svg" width="240" height="160">
<rect width="240" height="160" fill="crimson"/>
<image href="https://example.com/tracker.png" x="10" y="10" width="40" height="40"/>
<script>document.documentElement.innerHTML = '<text y="80">SCRIPT RAN</text>'</script>
<text x="20" y="90" fill="white" font-size="16">No script, no network</text>
</svg>"""
with open("safety.svg", "w") as f:
    f.write(svg)
print("Wrote safety.svg: open it under Artifacts")
"##;

/// The graphics samples, for the end of Python Lab's Examples menu.
#[uniffi::export]
pub fn python_lab_graphics_samples() -> Vec<PythonLabSampleCode> {
    [
        ("png-gradient", "PNG gradient", PNG_GRADIENT),
        ("svg-chart", "SVG chart", SVG_CHART),
        ("svg-safety", "SVG safety test", SVG_SAFETY),
    ]
    .into_iter()
    .map(|(id, title, code)| PythonLabSampleCode {
        id: id.into(),
        title: title.into(),
        code: code.into(),
    })
    .collect()
}
