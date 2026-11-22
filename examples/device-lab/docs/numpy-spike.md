# numpy in Python Lab (spike)

Can a compiled Python package ship inside Device Lab's Python Lab, built on Linux with xtool? This spike does it for numpy.

## How

iOS loads only native code that was signed into the app, so numpy can't be installed from the phone. It is built into the app bundle instead, as part of a curated set.

- PyPI has numpy 2.5.5 wheels for CPython 3.15 on macOS and Linux, but none for iOS. cibuildwheel builds iOS wheels only on a Mac, and BeeWare's mobile-forge recipe is for numpy 1.26.
- `scripts/build-numpy.sh` cross-compiles numpy's source release with the tools xtool installed for the app build: Swift's clang, the iPhoneOS SDK from Xcode.xip, and its ld64.lld.
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

## Open questions

- Python's iOS frameworks use the `APPL` package type and a relative install name, the same as the standard library ones that already pass on the phone. App Store validation may want `FMWK` and `@rpath` ids. That doesn't matter for sideloading.
- The next packages (Pillow, matplotlib) need their C libraries (libjpeg, freetype) cross-compiled the same way.
