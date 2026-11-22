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
#     lib/python3.15/site-packages/numpy  numpy, built for iOS by
#                                         scripts/build-numpy.sh (cached)
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
# The extension modules' framework bundle ids start with this.
BUNDLE_PREFIX="org.forgemobile.devicelab.python"

app_dir="${FORGE_APP_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
out="$app_dir/Generated/Python"
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# What this script makes: a new version or a changed script starts over.
numpy_script="$(dirname "$0")/build-numpy.sh"
stamp_want="$VERSION $(sha256 "$0" | cut -c1-16) $(sha256 "$numpy_script" | cut -c1-16)"
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

# numpy, compiled for iOS once and cached (scripts/build-numpy.sh).
numpy_cache="${XDG_CACHE_HOME:-$HOME/.cache}/forge/python-packages/numpy-ios-cp315"
"$numpy_script" "$home" "$xcf" "$numpy_cache"
mkdir -p "$lib/site-packages"
cp -R "$numpy_cache/numpy" "$lib/site-packages/"

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
      minos="17.0" ;;  # build-numpy.sh builds for the app's iOS 17
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
