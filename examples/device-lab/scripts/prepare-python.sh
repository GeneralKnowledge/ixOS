#!/usr/bin/env bash
# Prepares CPython for Device Lab's Python Lab: downloads the official iOS
# build from python.org (once, checked against its SHA-256) and lays it out
# under Generated/Python, which Package.swift and forge.toml name:
#
#   Generated/Python/Python.xcframework   the interpreter (Python.framework),
#                                         device slice only: a SwiftPM binary
#                                         target, embedded and signed by xtool
#   Generated/Python/python               PYTHONHOME, copied to the app
#                                         bundle's root ([ios] resources):
#     lib/python3.15/                     the standard library, without its
#                                         tests, IDLE, Tk and ensurepip
#     lib/python3.15/lib-dynload/*.fwork  where each extension module's binary is
#     etc/cacert.pem                      the CA certificates Python's ssl
#                                         checks HTTPS against (Mozilla's, from
#                                         certifi): iOS has no OpenSSL trust
#                                         store, so without it every HTTPS
#                                         request fails to verify
#     lib/python3.15/site-packages/numpy  numpy, contourpy and matplotlib,
#                                         built for iOS by
#                                         scripts/build-package.sh (cached)
#     lib/python3.15/site-packages/PIL    Pillow, kiwisolver and
#                                         matplotlib's pure-Python
#                                         dependencies, from their wheels on
#                                         PyPI (WHEELS below)
#     Frameworks/<module>.framework       the extension modules (math, _json,
#                                         _sqlite3, _ssl ..., and numpy's as
#                                         numpy._core._multiarray_umath ...),
#                                         one framework each, as CPython's
#                                         iOS support expects (Apple's rule:
#                                         no loose binaries in an app); xtool
#                                         signs them
#
# forge runs it before every iOS build ([ios] prepare in forge.toml); with
# everything in place it exits at once. Uses curl, tar, python3 (to read
# certifi's wheel) and sha256sum (or shasum on macOS).
set -euo pipefail

VERSION="3.15.0"
MINOR="3.15"
URL="https://www.python.org/ftp/python/${VERSION}/python-${VERSION}-iOS-XCframework.tar.gz"
SHA256="68e98ab35cc0643ba3a227a8c7768da45c2a5e67dbe1bc26598537e96680fd19"
# Mozilla's CA certificates, from certifi's wheel (a release at least a few
# weeks old; bump both lines together).
CERTIFI_URL="https://files.pythonhosted.org/packages/32/d1/a3652fe2c19134d0d15922177821d863cbd7e842d53a21bbe7e8e3a1b5ac/certifi-2026.10.23-py3-none-any.whl"
CERTIFI_SHA256="e9b54de76e530bb6cf0ccd036787103acb58fd42ba930ea8bf948ddbaa2f61d1"
# Packages with official iOS wheels on PyPI (iphoneos arm64, CPython 3.15),
# unpacked into site-packages as they are: "<url> <sha256>", one per line.
WHEELS="
https://files.pythonhosted.org/packages/6e/eb/e86a41962a3a31ad495a3d6a912d9002b01bba4a6fa71c28703abdca7f69/pillow-13.0.0-cp315-cp315-ios_13_0_arm64_iphoneos.whl 7d5e097eabd5d4915c27ac9f6e9325bb1e01a7c94d7348b5c4451d3fc30e8521
https://files.pythonhosted.org/packages/75/62/f786c3a27f181fa339d851a77e266d208e776b9883cabc40a5b041a31b5a/kiwisolver-1.5.1-cp315-cp315-ios_13_0_arm64_iphoneos.whl bb7c99f0673c03017a3ee01e54a5c2617a05468b11eabe513b0080e063ed95b1
https://files.pythonhosted.org/packages/e7/05/c19819d5e3d95294a6f5947fb9b9629efb316b96de511b418c53d245aae6/cycler-0.12.1-py3-none-any.whl 85cef7cff222d8644161529808465972e51340599459b8ac3ccbac5a854e0d30
https://files.pythonhosted.org/packages/35/74/4ea331ecc65ba05a58b1e4e8ccd604c8a15565f876dfbf93fc2a2ea2e22b/fonttools-4.68.0-py3-none-any.whl d76f8fdcb6936f5e1045a0c9dd721a4bf11112e60d1e243bfa1d39f06a8b3104
https://files.pythonhosted.org/packages/63/34/ba1c580383c9eada3711951fef0795c80b829a078d72188184bcab9dd527/packaging-26.3-py3-none-any.whl d7193f7c8e4e93f444fde0262bf90af30e16fa0ad0ad44cb553c87339b23cd1c
https://files.pythonhosted.org/packages/38/bb/d215ee7c73b61497b28a5503f9f53523f294fcc936762b7caf90e0c1c2b5/pyparsing-3.3.3-py3-none-any.whl ece8c00a69cf01b45d0b1dedabb469c90d8caf996d4fda40f147627a122849a4
https://files.pythonhosted.org/packages/2f/a9/52b89563d154433d79b51ed1eb4842bf7334cb2696891a7fe135618853d7/python_dateutil-4.0.0-py3-none-any.whl a5e246f79eb80475e691bf93862cead98decd8a498cf53ba0516fcdc0f50dc1c
https://files.pythonhosted.org/packages/b7/ce/149a00dd41f10bc29e5921b496af8b574d8413afcd5e30dfa0ed46c2cc5e/six-1.17.0-py2.py3-none-any.whl 4721f391ed90541fddacab5acf947aa0d3dc7d27b2e1e8eda2be8970586c3274
"
# Wheel files left out of the app: typing stubs, and modules for a desktop
# toolkit iOS doesn't have (Pillow's Tk bridge).
WHEEL_SKIP='\.pyi$|/py\.typed$|^PIL/_imagingtk\.|^PIL/ImageTk\.py$'
# The extension modules' framework bundle ids start with this.
BUNDLE_PREFIX="org.forgemobile.devicelab.python"

app_dir="${FORGE_APP_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
out="$app_dir/Generated/Python"
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# What this script makes: a new version or a changed script starts over.
package_script="$(dirname "$0")/build-package.sh"
stamp_want="$VERSION $(sha256 "$0" | cut -c1-16) $(sha256 "$package_script" | cut -c1-16)"
if [[ -f "$out/.prepared" && "$(cat "$out/.prepared")" == "$stamp_want" ]]; then
  exit 0
fi

cache="${XDG_CACHE_HOME:-$HOME/.cache}/forge/downloads"
tarball="$cache/python-${VERSION}-iOS-XCframework.tar.gz"
mkdir -p "$cache"
if [[ ! -f "$tarball" || "$(sha256 "$tarball")" != "$SHA256" ]]; then
  echo "[python] Downloading CPython ${VERSION} for iOS (85 MB) from python.org"
  curl -fL --retry 3 -o "$tarball.part" "$URL"
  got="$(sha256 "$tarball.part")"
  if [[ "$got" != "$SHA256" ]]; then
    rm -f "$tarball.part"
    echo "[python] The download's SHA-256 is $got, not $SHA256: not using it" >&2
    exit 1
  fi
  mv "$tarball.part" "$tarball"
fi

wheel="$cache/$(basename "$CERTIFI_URL")"
if [[ ! -f "$wheel" || "$(sha256 "$wheel")" != "$CERTIFI_SHA256" ]]; then
  echo "[python] Downloading certifi's CA certificates from PyPI"
  curl -fL --retry 3 -o "$wheel.part" "$CERTIFI_URL"
  got="$(sha256 "$wheel.part")"
  if [[ "$got" != "$CERTIFI_SHA256" ]]; then
    rm -f "$wheel.part"
    echo "[python] certifi's SHA-256 is $got, not $CERTIFI_SHA256: not using it" >&2
    exit 1
  fi
  mv "$wheel.part" "$wheel"
fi

echo "[python] Laying out CPython ${VERSION} in Generated/Python"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar xzf "$tarball" -C "$work" ./Python.xcframework
src="$work/Python.xcframework"

rm -rf "$out"
mkdir -p "$out"

# The interpreter: an XCFramework with only the device slice.
xcf="$out/Python.xcframework"
mkdir -p "$xcf/ios-arm64"
cp -R "$src/ios-arm64/Python.framework" "$xcf/ios-arm64/"
cat > "$xcf/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AvailableLibraries</key>
	<array>
		<dict>
			<key>BinaryPath</key>
			<string>Python.framework/Python</string>
			<key>LibraryIdentifier</key>
			<string>ios-arm64</string>
			<key>LibraryPath</key>
			<string>Python.framework</string>
			<key>SupportedArchitectures</key>
			<array>
				<string>arm64</string>
			</array>
			<key>SupportedPlatform</key>
			<string>ios</string>
		</dict>
	</array>
	<key>CFBundlePackageType</key>
	<string>XFWK</string>
	<key>XCFrameworkFormatVersion</key>
	<string>1.0</string>
</dict>
</plist>
PLIST

# PYTHONHOME: the standard library, then the device's compiled parts
# (lib-dynload, sysconfig data).
home="$out/python"
lib="$home/lib/python${MINOR}"
mkdir -p "$home/lib" "$home/Frameworks"
cp -R "$src/lib/python${MINOR}" "$home/lib/"
cp -R "$src/ios-arm64/lib-arm64/python${MINOR}/." "$lib/"
rm -rf "$lib"/{test,idlelib,tkinter,turtledemo,ensurepip,turtle.py,site-packages}
# PythonBridge points OpenSSL at this file (SSL_CERT_FILE) before Python starts.
mkdir -p "$home/etc"
python3 -I -c 'import sys, zipfile; sys.stdout.buffer.write(zipfile.ZipFile(sys.argv[1]).read("certifi/cacert.pem"))' \
  "$wheel" > "$home/etc/cacert.pem"
find "$lib" -name __pycache__ -type d -prune -exec rm -rf {} +
# CPython's own test modules.
rm -f "$lib"/lib-dynload/{_test,xx,_ctypes_test,_xxtestfuzz}*.so

# The compiled packages, each built for iOS once and cached
# (scripts/build-package.sh).
mkdir -p "$lib/site-packages"
for package in numpy contourpy matplotlib; do
  package_cache="${XDG_CACHE_HOME:-$HOME/.cache}/forge/python-packages/$package-ios-cp315"
  "$package_script" "$package" "$home" "$xcf" "$package_cache"
  cp -R "$package_cache"/. "$lib/site-packages/"
  rm -f "$lib/site-packages/.built"
done

# The wheels: downloaded once (checked against their SHA-256) and unpacked.
while read -r url want; do
  [[ -n "$url" ]] || continue
  whl="$cache/$(basename "$url")"
  if [[ ! -f "$whl" || "$(sha256 "$whl")" != "$want" ]]; then
    echo "[python] Downloading $(basename "$url") from PyPI"
    curl -fL --retry 3 -o "$whl.part" "$url"
    got="$(sha256 "$whl.part")"
    if [[ "$got" != "$want" ]]; then
      rm -f "$whl.part"
      echo "[python] $(basename "$url")'s SHA-256 is $got, not $want: not using it" >&2
      exit 1
    fi
    mv "$whl.part" "$whl"
  fi
  python3 -I -c '
import re, sys, zipfile
wheel, dest, skip = sys.argv[1:4]
with zipfile.ZipFile(wheel) as z:
    for name in z.namelist():
        if not re.search(skip, name):
            z.extract(name, dest)
' "$whl" "$lib/site-packages" "$WHEEL_SKIP"
done <<< "$WHEELS"

# Started by site at every launch: matplotlib's settings and font cache go to
# the app's Caches folder (the default, ~/.matplotlib, is not writable on iOS,
# so it would rebuild the font cache in a temporary folder at every launch),
# and it draws with Agg (PNG), as the phone has no GUI backend.
cat > "$lib/sitecustomize.py" <<'PYEOF'
import os
os.environ.setdefault("MPLCONFIGDIR", os.path.expanduser("~/Library/Caches/matplotlib"))
os.environ.setdefault("MPLBACKEND", "Agg")
PYEOF

# Each extension module becomes Frameworks/<module>.framework/<module>, named
# by its full dotted name (numpy/_core/_multiarray_umath.<suffix>.so is
# numpy._core._multiarray_umath), and a .fwork file where the binary was
# names it (relative to the app bundle).
count=0
while IFS= read -r so; do
  file="$(basename "$so")"
  case "$so" in
    "$lib"/site-packages/*)
      rel="${so#"$lib/site-packages/"}"
      module="$(dirname "$rel" | tr '/' '.').${file%%.*}"
      minos="17.0" ;;  # build-package.sh builds for the app's iOS 17
    *) module="${file%%.*}"; minos="" ;;
  esac
  framework="$home/Frameworks/${module}.framework"
  mkdir -p "$framework"
  mv "$so" "$framework/$module"
  # _json -> org.forgemobile.devicelab.python.json, numpy._core._umath_linalg
  # -> ...python.numpy.core.umath-linalg (a bundle id has no _).
  bundle_id="${BUNDLE_PREFIX}.$(echo "$module" | sed -E 's/(^|\.)_+/\1/g' | tr '_' '-')"
  # CPython's template, with the module's executable name and bundle id.
  awk -v exe="$module" -v id="$bundle_id" -v minos="$minos" '
    /<key>CFBundleExecutable<\/key>/ { print; getline; print "\t<string>" exe "</string>"; next }
    /<key>MinimumOSVersion<\/key>/ && minos != "" { print; getline; print "\t<string>" minos "</string>"; next }
    /<key>CFBundleIdentifier<\/key>/ { print; getline; print "\t<string>" id "</string>"; next }
    { print }' "$src/build/iOS-dylib-Info-template.plist" > "$framework/Info.plist"
  if [[ -f "$lib/lib-dynload/${module}.xcprivacy" ]]; then
    mv "$lib/lib-dynload/${module}.xcprivacy" "$framework/PrivacyInfo.xcprivacy"
  fi
  echo "python/Frameworks/${module}.framework/${module}" > "${so%.so}.fwork"
  echo "python/${so#"$home/"}" | sed 's/\.so$/.fwork/' > "$framework/${module}.origin"
  count=$((count + 1))
done < <(find "$lib/lib-dynload" "$lib/site-packages" -name '*.so' | sort)

echo "$stamp_want" > "$out/.prepared"
echo "[python] CPython ${VERSION}: $(du -sh "$home" | cut -f1) standard library with ${count} extension modules, $(du -sh "$xcf" | cut -f1) interpreter"
