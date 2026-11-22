#!/usr/bin/env bash
# Builds a compiled Python package for the iPhone (arm64, CPython 3.15) on
# Linux, for Python Lab: numpy, contourpy or matplotlib.
#
#   scripts/build-package.sh <package> <PYTHONHOME> <Python.xcframework> <out dir>
#
# prepare-python.sh runs it and copies what <out dir> holds into the app's
# site-packages; nobody needs to run it by hand. iOS only loads native code
# that was signed into the app, so these packages can't be pip-installed on
# the phone: they are compiled here, at build time, and prepare-python.sh
# turns each extension module into a framework the way it does the standard
# library's. (Packages with official iOS wheels, such as Pillow and
# kiwisolver, are not built: prepare-python.sh unpacks their wheels.)
#
# PyPI has no iOS wheels of these, and cibuildwheel builds them only on a Mac,
# so this cross-compiles each source release with the tools xtool already
# installed for the app's own build: Swift's clang, the Darwin SDK from
# Xcode.xip and its ld64.lld. Meson is told the target is iOS through a cross
# file; the one thing it can't ask the iPhone, "what does the target's Python
# look like", a small wrapper answers from the iOS build's sysconfig data.
# numpy leaves BLAS out (numpy's own fallback code), so linear algebra works
# but is not Accelerate-fast. matplotlib builds its own FreeType, HarfBuzz and
# Qhull (Meson subprojects, downloaded at configure) and uses the Agg backend.
#
# Each result is cached by version and this script's hash, so only the first
# build after a change pays for it (minutes). Needs python3 (3.12 or newer,
# to run Meson and the packages' code generators), and either uv or python3's
# venv to fetch the build tools into a private build environment.
set -euo pipefail

# Build-time tools, from PyPI into a private environment (exact versions).
BUILD_REQUIREMENTS=(cython==3.3.1 ninja==1.13.2 meson==1.12.1 pybind11==3.1.0
  setuptools_scm==9.2.2 setuptools==84.2.1)

package="$1"
# Each package: version, source release (URL, SHA-256), Meson setup options,
# the top-level names it installs into site-packages, and whether it uses its
# own copy of Meson (numpy does).
case "$package" in
  numpy)
    VERSION="2.5.5"
    URL="https://files.pythonhosted.org/packages/4e/3a/8770c288c47f0b5c2a786d3c20e79f66fd245f2d42cd232d7cd5e00e83e5/numpy-2.5.5.tar.gz"
    SHA256="5cddc243c75114ba845f01b1f87aca1572dbf7b6c2d88636ed3e1d2bccd7f1bf"
    MESON_OPTIONS=(-Dblas=none -Dlapack=none -Dallow-noblas=true)
    INSTALL_TAGS=""
    TOP_LEVEL=(numpy)
    MESON_SCRIPT="vendored-meson/meson/meson.py" ;;
  contourpy)
    VERSION="1.4.0"
    URL="https://files.pythonhosted.org/packages/83/5a/a55177dd22553a277388e8a1b3220e92de91bacb28356cdc73caa240121d/contourpy-1.4.0.tar.gz"
    SHA256="20156f5a1ac4f8ce02656e39a61e82164a3d359796dc8026f75b062783d500e1"
    MESON_OPTIONS=(-Dwerror=false)
    INSTALL_TAGS=""
    TOP_LEVEL=(contourpy)
    MESON_SCRIPT="" ;;
  matplotlib)
    VERSION="3.11.2"
    URL="https://files.pythonhosted.org/packages/e7/c8/9aa712a0afb882649424dd8de8ad9aa6235e796e84c6052e8f6dc1598d0d/matplotlib-3.11.2.tar.gz"
    SHA256="cec596316640f2b394b8f0daa0ea61a8eae82d017b620b9f202befb972a59ea4"
    # No Cocoa backend (macOS only); Agg draws PNGs, the phone has no GUI backend.
    MESON_OPTIONS=(-Dmacosx=false -DrcParams-backend=Agg -Dwerror=false)
    INSTALL_TAGS="data,python-runtime,runtime"
    TOP_LEVEL=(matplotlib mpl_toolkits pylab.py)
    MESON_SCRIPT="" ;;
  *) echo "[package] error: no recipe for '$package' (numpy, contourpy, matplotlib)" >&2; exit 1 ;;
esac

home="$2"
xcframework="$3"
out="$4"

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
say() { echo "[$package] $*"; }
fail() { echo "[$package] error: $*" >&2; exit 1; }

stamp_want="$VERSION $(sha256 "$0" | cut -c1-16)"
if [[ -f "$out/.built" && "$(cat "$out/.built")" == "$stamp_want" ]]; then
  exit 0
fi

# What a first (uncached) build needs from this machine, named if missing.
python3 -c 'import sys; sys.exit(sys.version_info < (3, 12))' 2>/dev/null \
  || fail "the build needs python3 3.12 or newer (to run Meson and $package's code generators)"
if ! command -v uv >/dev/null 2>&1 && ! python3 -c 'import venv, ensurepip' 2>/dev/null; then
  fail "the build needs uv or python3's venv module (Ubuntu: sudo apt install python3-venv)"
fi
command -v curl >/dev/null || fail "the build needs curl (and network access to PyPI, once)"

slice="$xcframework/ios-arm64"
headers="$slice/Python.framework/Headers"
[[ -f "$headers/Python.h" ]] || fail "no Python.h in $headers"
sysconfigdata="$(find "$home/lib" -maxdepth 2 -name '_sysconfigdata_*ios*.py' | head -1)"
[[ -n "$sysconfigdata" ]] || fail "no iOS _sysconfigdata in $home/lib"

# The tools xtool's Darwin SDK build uses: Swift's clang, the iPhoneOS SDK
# and ld64.lld. Each can be overridden (FORGE_IOS_CC, SDKROOT, FORGE_IOS_LD).
sdk="${SDKROOT:-}"
toolset_dir=""
if [[ -z "$sdk" ]] && command -v xcrun >/dev/null 2>&1; then
  sdk="$(xcrun --sdk iphoneos --show-sdk-path 2>/dev/null || true)"
fi
if [[ -z "$sdk" ]] && command -v swift >/dev/null 2>&1; then
  config="$(swift sdk configure darwin aarch64-apple-ios --show-configuration 2>/dev/null \
    || swift sdk configure darwin arm64-apple-ios --show-configuration 2>/dev/null || true)"
  sdk="$(echo "$config" | awk -F': ' '/sdkRootPath/ {print $2; exit}')"
  toolset="$(echo "$config" | awk -F': ' '/toolsetPaths/ {print $2; exit}' | tr -d '[]"' | cut -d, -f1)"
  [[ -n "$toolset" ]] && toolset_dir="$(dirname "$toolset")"
fi
[[ -n "$sdk" && -d "$sdk" ]] || fail "no iPhoneOS SDK (install xtool's Darwin SDK: xtool setup)"
if [[ -z "$toolset_dir" ]]; then
  # The SDK sits at <bundle>/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/<sdk>.
  toolset_dir="$(cd "$sdk/../../../../../.." 2>/dev/null && pwd || true)"
fi

cc="${FORGE_IOS_CC:-}"
if [[ -z "$cc" ]] && command -v swift >/dev/null 2>&1; then
  swift_bin="$(dirname "$(readlink -f "$(command -v swift)")")"
  [[ -x "$swift_bin/clang" ]] && cc="$swift_bin/clang"
fi
[[ -n "$cc" ]] || cc="$(command -v clang || true)"
[[ -n "$cc" ]] || fail "no clang (Swift's, from the toolchain xtool uses)"
cxx="${cc%clang}clang++"
[[ -x "$cxx" ]] || cxx="$cc"
tools_bin="$(dirname "$cc")"

# On Linux: ld64.lld, and -mlinker-version because Swift's clang can't ask
# ld64.lld its version and would pass old ld64 flags (-arch,
# -iphoneos_version_min) that lld rejects; from 520 on it passes
# -platform_version. On macOS Xcode's own linker and ar need neither.
ld=""
ar=""
link_args=""
if [[ "$(uname -s)" == "Darwin" ]]; then
  ar="$(xcrun --find ar 2>/dev/null || command -v ar)"
else
  ld="${FORGE_IOS_LD:-}"
  for candidate in "$toolset_dir/toolset/bin/ld64.lld" "$tools_bin/ld64.lld" "$(command -v ld64.lld || true)"; do
    if [[ -z "$ld" && -n "$candidate" && -x "$candidate" ]]; then ld="$candidate"; fi
  done
  [[ -n "$ld" ]] || fail "no ld64.lld (xtool's Darwin SDK toolset has one: xtool setup)"
  link_args="'--ld-path=$ld', '-mlinker-version=907', "
  for candidate in "$tools_bin/llvm-ar" "$(command -v llvm-ar || true)"; do
    if [[ -z "$ar" && -n "$candidate" && -x "$candidate" ]]; then ar="$candidate"; fi
  done
fi
[[ -n "$ar" ]] || fail "no llvm-ar (GNU ar can't index Mach-O archives; it comes with Swift's toolchain)"
strip_tool=""
for candidate in "$tools_bin/llvm-strip" "$(command -v llvm-strip || true)"; do
  if [[ -z "$strip_tool" && -n "$candidate" && -x "$candidate" ]]; then strip_tool="$candidate"; fi
done

# The app's deployment target (Package.swift: iOS 17; CPython's own is lower).
min_ios="${FORGE_IOS_DEPLOYMENT_TARGET:-17.0}"
say "clang: $cc"
say "SDK: $sdk"
say "linker: ${ld:-Xcode ld}"
say "target: arm64-apple-ios$min_ios"

cache="${XDG_CACHE_HOME:-$HOME/.cache}/forge"
mkdir -p "$cache/downloads"
tarball="$cache/downloads/$(basename "$URL")"
if [[ ! -f "$tarball" || "$(sha256 "$tarball")" != "$SHA256" ]]; then
  say "Downloading $package ${VERSION}'s source from PyPI"
  curl -fL --retry 3 -o "$tarball.part" "$URL"
  got="$(sha256 "$tarball.part")"
  if [[ "$got" != "$SHA256" ]]; then
    rm -f "$tarball.part"
    fail "the download's SHA-256 is $got, not $SHA256: not using it"
  fi
  mv "$tarball.part" "$tarball"
fi

# Cython, ninja, Meson and pybind11, in a build environment of their own.
venv="$cache/python-package-build-env"
venv_stamp="${BUILD_REQUIREMENTS[*]}"
if [[ ! -f "$venv/.ready" || "$(cat "$venv/.ready")" != "$venv_stamp" ]]; then
  say "Setting up the build environment (${BUILD_REQUIREMENTS[*]})"
  rm -rf "$venv"
  if command -v uv >/dev/null 2>&1; then
    uv venv -q --python python3 "$venv"
    VIRTUAL_ENV="$venv" uv pip install -q "${BUILD_REQUIREMENTS[@]}"
  else
    python3 -m venv "$venv" || fail "python3 -m venv failed (install python3-venv, or uv)"
    "$venv/bin/python" -m pip install -q "${BUILD_REQUIREMENTS[@]}"
  fi
  echo "$venv_stamp" > "$venv/.ready"
fi
py="$venv/bin/python"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar xzf "$tarball" -C "$work"
srcdir="$work/$package-$VERSION"

# What Meson asks "python" about the target: python_info.py's answers from
# this machine's Python, with the iOS build's sysconfig data, headers, version
# and extension suffix in place of the host's.
cat > "$work/ios_python_info.py" <<'PYEOF'
import contextlib, io, json, runpy, sys
script, sysconfigdata, headers, platform = sys.argv[1:5]
sys.argv = [script]
captured = io.StringIO()
with contextlib.redirect_stdout(captured):
    runpy.run_path(script, run_name="__main__")
info = json.loads(captured.getvalue())
ns = {}
exec(open(sysconfigdata).read(), ns)
variables = dict(ns["build_time_vars"])
variables["INCLUDEPY"] = headers
variables["base_prefix"] = info["variables"]["base_prefix"]
info.update(
    variables=variables,
    version=variables["VERSION"],
    platform=platform,
    suffix=variables["EXT_SUFFIX"],
    limited_api_suffix=".abi3.so",
    link_libpython=False,
    is_pypy=False,
    is_venv=False,
    is_freethreaded=bool(variables.get("Py_GIL_DISABLED")),
)
for key in ("paths", "sysconfig_paths"):
    info[key]["include"] = info[key]["platinclude"] = headers
print(json.dumps(info))
PYEOF
cat > "$work/python" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
  *python_info.py) exec "$py" -I "$work/ios_python_info.py" "\$1" "$sysconfigdata" "$headers" "ios-$min_ios-arm64-iphoneos" ;;
esac
exec "$py" "\$@"
EOF
chmod +x "$work/python"
# pkg-config sees only the iOS Python: Meson's Cython check links against the
# "python3" dependency, which could otherwise find this machine's own headers.
py_version="$(python3 -I -c 'import sys; ns = {}; exec(open(sys.argv[1]).read(), ns); print(ns["build_time_vars"]["VERSION"])' "$sysconfigdata")"
mkdir -p "$work/pkgconfig"
for name in python3 "python-$py_version"; do
  printf '%s\n' "Name: Python" "Description: CPython for iOS (python.org's iOS build)" \
    "Version: $py_version" "Cflags: -I$headers" "Libs: -F$slice -framework Python" \
    > "$work/pkgconfig/$name.pc"
done
# pybind11 (header-only, contourpy and matplotlib) from the build environment.
pybind11_include="$("$py" -c 'import pybind11; print(pybind11.get_include())')"
pybind11_version="$("$py" -c 'import pybind11; print(pybind11.__version__)')"
printf '%s\n' "Name: pybind11" "Description: pybind11 headers" \
  "Version: $pybind11_version" "Cflags: -I$pybind11_include" > "$work/pkgconfig/pybind11.pc"

q() { printf "'%s'" "$1"; }
target="arm64-apple-ios$min_ios"
cat > "$work/ios.ini" <<EOF
[binaries]
c = [$(q "$cc"), '-target', $(q "$target"), '-isysroot', $(q "$sdk")]
cpp = [$(q "$cxx"), '-target', $(q "$target"), '-isysroot', $(q "$sdk")]
ar = $(q "$ar")
python = $(q "$work/python")
cython = $(q "$venv/bin/cython")
pkg-config = 'pkg-config'

[built-in options]
c_link_args = [${link_args}'-F', $(q "$slice"), '-framework', 'Python']
cpp_link_args = [${link_args}'-F', $(q "$slice"), '-framework', 'Python']

[properties]
# Apple arm64: long double is the same as double.
longdouble_format = 'IEEE_DOUBLE_LE'
needs_exe_wrapper = true
pkg_config_libdir = $(q "$work/pkgconfig")

[host_machine]
system = 'ios'
subsystem = 'ios'
kernel = 'xnu'
cpu_family = 'aarch64'
cpu = 'arm64'
endian = 'little'
EOF

say "Configuring $package $VERSION for iOS"
started=$(date +%s)
export PATH="$venv/bin:$PATH"
if [[ -n "$MESON_SCRIPT" ]]; then meson=("$py" "$srcdir/$MESON_SCRIPT"); else meson=("$venv/bin/meson"); fi
(cd "$srcdir" && "${meson[@]}" setup "$work/build" \
  --cross-file "$work/ios.ini" --prefix=/usr --buildtype=release -Db_ndebug=if-release \
  "${MESON_OPTIONS[@]}") > "$work/configure.log" 2>&1 \
  || { tail -60 "$work/configure.log" >&2
       log="$work/build/meson-logs/meson-log.txt"
       if [[ -f "$log" ]]; then echo "--- end of meson-log.txt" >&2; tail -120 "$log" >&2; fi
       fail "configure failed (log above)"; }
jobs="${FORGE_PACKAGE_JOBS:-${FORGE_NUMPY_JOBS:-4}}"
say "Compiling (ninja -j$jobs)"
"$venv/bin/ninja" -C "$work/build" -j "$jobs" > "$work/build.log" 2>&1 \
  || { tail -60 "$work/build.log" >&2; fail "compile failed (log above)"; }
install_args=(install -C "$work/build" --destdir "$work/stage" --quiet)
[[ -n "$INSTALL_TAGS" ]] && install_args+=(--tags "$INSTALL_TAGS")
"${meson[@]}" "${install_args[@]}" > "$work/install.log" 2>&1 \
  || { tail -40 "$work/install.log" >&2; fail "install failed"; }

site="$(find "$work/stage" -type d -path "*-packages/${TOP_LEVEL[0]}" | head -1)"
[[ -n "$site" ]] || fail "meson install made no ${TOP_LEVEL[0]} package"
site="$(dirname "$site")"
rm -rf "$out"
mkdir -p "$out"
for name in "${TOP_LEVEL[@]}"; do
  if [[ -e "$site/$name" ]]; then cp -R "$site/$name" "$out/"; fi
done
# Tests, typing stubs and build-only files stay off the phone.
find "$out" -type d \( -name tests -o -name __pycache__ -o -name _pyinstaller \) -prune -exec rm -rf {} +
find "$out" \( -name '*.pyi' -o -name '*.pxd' -o -name '*.c' -o -name '*.h' -o -name '*.a' -o -name '*.pc' -o -name py.typed \) -delete
case "$package" in
  numpy)
    rm -rf "$out"/numpy/_core/include "$out"/numpy/_core/lib "$out"/numpy/random/lib "$out"/numpy/random/_examples
    # Extension modules only numpy's own test suite imports.
    rm -f "$out"/numpy/_core/{_multiarray_tests,_umath_tests,_rational_tests,_struct_ufunc_tests,_operand_flag_tests,_simd}.*.so ;;
  matplotlib)
    # Example data for matplotlib's own gallery.
    rm -rf "$out"/matplotlib/mpl-data/sample_data ;;
esac
if [[ -n "$strip_tool" ]]; then
  find "$out" -name '*.so' -exec "$strip_tool" -x {} \;
fi

elapsed=$(( $(date +%s) - started ))
echo "$stamp_want" > "$out/.built"
say "$package $VERSION: $(du -sh "$out" | cut -f1), $(find "$out" -name '*.so' | wc -l) extension modules, built in ${elapsed}s"
