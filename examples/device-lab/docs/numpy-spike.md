# numpy in Python Lab (spike)

Can a compiled Python package ship inside Device Lab's Python Lab, built on Linux with xtool? This spike does it for numpy.

## How

iOS loads only native code that was signed into the app, so numpy can't be installed from the phone. It is built into the app bundle instead, as part of a curated set.

- PyPI has numpy 2.5.5 wheels for CPython 3.15 on macOS and Linux, but none for iOS. cibuildwheel builds iOS wheels only on a Mac, and BeeWare's mobile-forge recipe is for numpy 1.26.
- `scripts/build-package.sh numpy` (formerly `build-numpy.sh`) cross-compiles numpy's source release with the tools xtool installed for the app build: Swift's clang, the iPhoneOS SDK from Xcode.xip, and its ld64.lld.
  - A Meson cross file tells numpy's own copy of Meson that the target is iOS.
  - A wrapper answers Meson's questions about the target Python from python.org's iOS sysconfig data.
  - Cython and ninja come into a private build environment.
- `scripts/prepare-python.sh` runs the build and caches it under `~/.cache/forge/python-packages`. It copies numpy into the bundled `lib/python3.15/site-packages` and turns each extension module into a framework named after its dotted module name, as PEP 730 requires (`Frameworks/numpy._core._multiarray_umath.framework`). The `.fwork` file sits where the `.so` was, and xtool signs the frameworks with the rest of the app.
- No Swift changes: Python adds PYTHONHOME's site-packages at startup.

Two flags are needed on Linux:

- `-mlinker-version=907`: Swift's clang otherwise passes ld64.lld old ld64 flags (`-arch`, `-iphoneos_version_min`) that lld rejects.
- pkg-config files for the iOS Python: without them, Meson's Cython check found the build machine's own Python headers.

BLAS is left out, so linear algebra uses numpy's own code and not Accelerate. Accelerate is the obvious next step if linear algebra speed matters.

## Results (laptop, 2026-11-22)

| | |
|---|---|
| numpy build | 125 s for the first build (2 min 12 s for the whole prepare step); cached afterwards, so later builds add nothing |
| App growth | about 15 MB: Python 32 MB → 47 MB (numpy's Python side 4.1 MB, 13 extension module frameworks, `_multiarray_umath` 3.9 MB) |
| Binaries | Mach-O arm64 dylibs, iOS minos 17.0, linked to `@rpath/Python.framework/Python`, libc++ and libSystem |
| Phone | PASS, reported by Reuben on 2026-11-22 on Device Lab 1.29922692 (combined 4b062bb5, iPhone 15 Pro Max, iOS 26.3.1): the check script below worked. Import time not recorded |

The 13 frameworks are `numpy._core._multiarray_umath`, `numpy.fft._pocketfft_umath`, `numpy.linalg.lapack_lite` and `numpy.linalg._umath_linalg`, plus nine in `numpy.random`. numpy's tests and test-only modules (`_multiarray_tests`, `_simd` and others) are left out.

## Phone check

This check runs in the combined Device Lab build. Paste this into Python Lab:

```python
import time
t = time.perf_counter()
import numpy as np
print("import", round(time.perf_counter() - t, 2), "s", np.__version__)
a = np.arange(1_000_000, dtype=np.float64)
print(a.sum())                       # 499999500000.0
print(np.linalg.solve([[3, 1], [1, 2]], [9, 8]))   # [2. 3.]
print(np.random.default_rng(1).integers(0, 10, 5))
print(np.fft.fft([1, 0, 0, 0]))      # [1.+0.j 1.+0.j 1.+0.j 1.+0.j]
```

It passes if every line prints without an error and the values match the comments. Record the import time: numpy's `.py` files are compiled at each launch, because the signed bundle is read-only.

## Pillow

Pillow 13.0.0 publishes official iOS wheels on PyPI (`cp315-ios_13_0_arm64_iphoneos`), so nothing is compiled. `prepare-python.sh` downloads the wheel once and checks its SHA-256 (the `WHEELS` list). It unpacks the wheel into site-packages, and the same loop turns its 7 extension modules into frameworks (`PIL._imaging` and others). Pillow's own copies of libjpeg, libpng, FreeType, WebP, AVIF and LittleCMS are built into those modules, so they depend only on Python.framework, libSystem and libbz2. The Tk bridge (`_imagingtk`, `ImageTk`) is left out. The bundle grows by about 11 MB.

Phone check, in Python Lab:

```python
import io
from PIL import Image, ImageDraw, ImageFont, features
img = Image.new("RGB", (200, 100), "white")
d = ImageDraw.Draw(img)
d.ellipse((10, 10, 90, 90), fill="red")
d.text((110, 40), "Hi", fill="black", font=ImageFont.load_default(size=24))
for fmt in ("PNG", "JPEG", "WEBP"):
    buf = io.BytesIO(); img.save(buf, fmt); print(fmt, len(buf.getvalue()), "bytes")
print(Image.open(io.BytesIO(buf.getvalue())).size)   # (200, 100)
print(features.check("freetype2"), features.check("jpg"), features.check("webp"))  # True True True
```

## matplotlib

matplotlib 3.11.2 and contourpy 1.4.0 have no iOS wheels, so `scripts/build-package.sh` builds them the same way as numpy. Each package has a recipe (version, source SHA-256, Meson options) in the script. Both use pybind11, which the build environment provides through a `pybind11.pc` file. matplotlib downloads and builds its own FreeType, HarfBuzz, libraqm, SheenBidi and Qhull as Meson subprojects. It is built without the Cocoa backend (`-Dmacosx=false`) and draws with Agg. kiwisolver comes from its official iOS wheel. The pure-Python dependencies (cycler, fonttools, packaging, pyparsing, python-dateutil, six) come from their wheels in the same `WHEELS` list as Pillow.

A `sitecustomize.py` in the standard library sets `MPLCONFIGDIR` to `~/Library/Caches/matplotlib` and `MPLBACKEND` to `Agg`. matplotlib's default config folder, `~/.matplotlib`, is not writable in an iOS app, so without this it would rebuild its font cache at every launch.

Phone check, in Python Lab:

```python
import io, time
t = time.perf_counter()
import matplotlib
import matplotlib.pyplot as plt
import numpy as np
print("import", round(time.perf_counter() - t, 2), "s", matplotlib.__version__, matplotlib.get_backend())  # ... agg
x = np.linspace(0, 2 * np.pi, 200)
fig, ax = plt.subplots()
ax.plot(x, np.sin(x), label="sin")
ax.contourf(*np.meshgrid(x, x), np.sin(np.add.outer(x, x)), alpha=0.3)
ax.set_title("Hello from the iPhone"); ax.legend()
buf = io.BytesIO(); fig.savefig(buf, format="png", dpi=100)
print("PNG", len(buf.getvalue()), "bytes")   # tens of kB
```

## Open questions

- Python's iOS frameworks use the `APPL` package type and a relative install name, the same as the standard library ones that already pass on the phone. App Store validation may want `FMWK` and `@rpath` ids. That doesn't matter for sideloading.
- The next packages (Pillow, matplotlib) need their C libraries (libjpeg, freetype) cross-compiled the same way.
