#!/usr/bin/env bash
# Builds numpy for the iPhone (arm64, CPython 3.15) on Linux, for Python Lab.
#
#   scripts/build-numpy.sh <PYTHONHOME> <Python.xcframework> <out dir>
#
# prepare-python.sh runs it (and copies <out dir>/numpy into the app's
# site-packages); nobody needs to run it by hand. iOS only loads native code
# that was signed into the app, so numpy can't be pip-installed on the phone:
# it is compiled here, at build time, and prepare-python.sh turns each of its
# extension modules into a framework the way it does the standard library's.
#
# PyPI has no iOS wheels of numpy, and cibuildwheel builds them only on a Mac,
# so this cross-compiles numpy's source release with the tools xtool already
# installed for the app's own build: Swift's clang, the Darwin SDK from
# Xcode.xip and its ld64.lld. Meson (numpy's own copy) is told the target is
# iOS through a cross file; the one thing it can't ask the iPhone, "what does
# the target's Python look like", a small wrapper answers from the iOS
# build's sysconfig data. BLAS is left out (numpy's own fallback code), so
# linear algebra works but is not Accelerate-fast.
#
# The result is cached by numpy version and this script's hash, so only the
# first build after a change pays for it (a few minutes). Needs python3 (3.12
# or newer, to run numpy's code generators and Meson), and either uv or
# python3's venv to fetch Cython and ninja into a private build environment.
set -euo pipefail

NUMPY_VERSION="2.5.5"
NUMPY_URL="https://files.pythonhosted.org/packages/4e/3a/8770c288c47f0b5c2a786d3c20e79f66fd245f2d42cd232d7cd5e00e83e5/numpy-${NUMPY_VERSION}.tar.gz"
NUMPY_SHA256="5cddc243c75114ba845f01b1f87aca1572dbf7b6c2d88636ed3e1d2bccd7f1bf"
# Build-time tools, from PyPI into a private environment (exact versions).
BUILD_REQUIREMENTS=(cython==3.3.1 ninja==1.13.2)

home="$1"
xcframework="$2"
out="$3"

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
say() { echo "[numpy] $*"; }
fail() { echo "[numpy] error: $*" >&2; exit 1; }

stamp_want="$NUMPY_VERSION $(sha256 "$0" | cut -c1-16)"
if [[ -f "$out/.built" && "$(cat "$out/.built")" == "$stamp_want" ]]; then
  exit 0
fi

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

ld="${FORGE_IOS_LD:-}"
for candidate in "$toolset_dir/toolset/bin/ld64.lld" "$tools_bin/ld64.lld" "$(command -v ld64.lld || true)"; do
  if [[ -z "$ld" && -n "$candidate" && -x "$candidate" ]]; then ld="$candidate"; fi
done
[[ -n "$ld" ]] || fail "no ld64.lld (xtool's Darwin SDK toolset has one)"

ar=""
for candidate in "$tools_bin/llvm-ar" "$(command -v llvm-ar || true)"; do
  if [[ -z "$ar" && -n "$candidate" && -x "$candidate" ]]; then ar="$candidate"; fi
done
[[ -n "$ar" ]] || fail "no llvm-ar (GNU ar can't index Mach-O archives)"
strip_tool=""
for candidate in "$tools_bin/llvm-strip" "$(command -v llvm-strip || true)"; do
  if [[ -z "$strip_tool" && -n "$candidate" && -x "$candidate" ]]; then strip_tool="$candidate"; fi
done

# The deployment target CPython's iOS build was made for.
min_ios="$(python3 -I -c 'import sys; ns = {}; exec(open(sys.argv[1]).read(), ns); print(ns["build_time_vars"].get("IPHONEOS_DEPLOYMENT_TARGET") or "13.0")' "$sysconfigdata")"
say "clang: $cc"
say "SDK: $sdk"
say "linker: $ld"
say "target: arm64-apple-ios$min_ios"

cache="${XDG_CACHE_HOME:-$HOME/.cache}/forge"
mkdir -p "$cache/downloads"
tarball="$cache/downloads/numpy-${NUMPY_VERSION}.tar.gz"
if [[ ! -f "$tarball" || "$(sha256 "$tarball")" != "$NUMPY_SHA256" ]]; then
  say "Downloading numpy ${NUMPY_VERSION}'s source from PyPI"
  curl -fL --retry 3 -o "$tarball.part" "$NUMPY_URL"
  got="$(sha256 "$tarball.part")"
  if [[ "$got" != "$NUMPY_SHA256" ]]; then
    rm -f "$tarball.part"
    fail "the download's SHA-256 is $got, not $NUMPY_SHA256: not using it"
  fi
  mv "$tarball.part" "$tarball"
fi

# Cython and ninja, in a build environment of their own.
venv="$cache/numpy-build-env"
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
srcdir="$work/numpy-${NUMPY_VERSION}"

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
mkdir -p "$work/no-pkgconfig"

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
c_link_args = [$(q "--ld-path=$ld"), '-F', $(q "$slice"), '-framework', 'Python']
cpp_link_args = [$(q "--ld-path=$ld"), '-F', $(q "$slice"), '-framework', 'Python']

[properties]
# Apple arm64: long double is the same as double.
longdouble_format = 'IEEE_DOUBLE_LE'
needs_exe_wrapper = true
pkg_config_libdir = $(q "$work/no-pkgconfig")

[host_machine]
system = 'ios'
subsystem = 'ios'
kernel = 'xnu'
cpu_family = 'aarch64'
cpu = 'arm64'
endian = 'little'
EOF

say "Configuring numpy ${NUMPY_VERSION} for iOS"
started=$(date +%s)
export PATH="$venv/bin:$PATH"
(cd "$srcdir" && "$py" vendored-meson/meson/meson.py setup "$work/build" \
  --cross-file "$work/ios.ini" --prefix=/usr --buildtype=release -Db_ndebug=if-release \
  -Dblas=none -Dlapack=none -Dallow-noblas=true) > "$work/configure.log" 2>&1 \
  || { tail -60 "$work/configure.log" >&2; fail "configure failed (log above)"; }
jobs="${FORGE_NUMPY_JOBS:-4}"
say "Compiling (ninja -j$jobs)"
"$venv/bin/ninja" -C "$work/build" -j "$jobs" > "$work/build.log" 2>&1 \
  || { tail -60 "$work/build.log" >&2; fail "compile failed (log above)"; }
"$py" "$srcdir/vendored-meson/meson/meson.py" install -C "$work/build" --destdir "$work/stage" --quiet \
  > "$work/install.log" 2>&1 || { tail -40 "$work/install.log" >&2; fail "install failed"; }

pkg="$(find "$work/stage" -type d -path '*-packages/numpy' | head -1)"
[[ -n "$pkg" ]] || fail "meson install made no numpy package"
# Tests, typing stubs and build-only files stay off the phone.
find "$pkg" -type d \( -name tests -o -name __pycache__ -o -name _pyinstaller \) -prune -exec rm -rf {} +
find "$pkg" \( -name '*.pyi' -o -name '*.pxd' -o -name '*.c' -o -name '*.h' -o -name '*.a' -o -name '*.pc' \) -delete
rm -rf "$pkg/_core/include" "$pkg/_core/lib" "$pkg/random/lib" "$pkg/random/_examples" "$pkg/f2py/tests"
# Extension modules only numpy's own test suite imports.
rm -f "$pkg"/_core/{_multiarray_tests,_umath_tests,_rational_tests,_struct_ufunc_tests,_operand_flag_tests,_simd}.*.so
if [[ -n "$strip_tool" ]]; then
  find "$pkg" -name '*.so' -exec "$strip_tool" -x {} \;
fi

rm -rf "$out"
mkdir -p "$out"
cp -R "$pkg" "$out/numpy"
elapsed=$(( $(date +%s) - started ))
echo "$stamp_want" > "$out/.built"
say "numpy ${NUMPY_VERSION}: $(du -sh "$out/numpy" | cut -f1), $(find "$out/numpy" -name '*.so' | wc -l) extension modules, built in ${elapsed}s"
