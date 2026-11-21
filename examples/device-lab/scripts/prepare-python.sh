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
#     Frameworks/<module>.framework       the extension modules (math, _json,
#                                         _sqlite3, _ssl ...), one framework
#                                         each, as CPython's iOS support
#                                         expects (Apple's rule: no loose
#                                         binaries in an app); xtool signs them
#
# forge runs it before every iOS build ([ios] prepare in forge.toml); with
# everything in place it exits at once. Uses curl, tar and sha256sum (or
# shasum on macOS).
set -euo pipefail

VERSION="3.15.0"
MINOR="3.15"
URL="https://www.python.org/ftp/python/${VERSION}/python-${VERSION}-iOS-XCframework.tar.gz"
SHA256="68e98ab35cc0643ba3a227a8c7768da45c2a5e67dbe1bc26598537e96680fd19"
# The extension modules' framework bundle ids start with this.
BUNDLE_PREFIX="org.forgemobile.devicelab.python"

app_dir="${FORGE_APP_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
out="$app_dir/Generated/Python"
# What this script makes: a new version or a changed script starts over.
stamp_want="$VERSION $(sha256sum "$0" 2>/dev/null | cut -c1-16 || shasum -a 256 "$0" | cut -c1-16)"
if [[ -f "$out/.prepared" && "$(cat "$out/.prepared")" == "$stamp_want" ]]; then
  exit 0
fi

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

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
find "$lib" -name __pycache__ -type d -prune -exec rm -rf {} +
# CPython's own test modules.
rm -f "$lib"/lib-dynload/{_test,xx,_ctypes_test,_xxtestfuzz}*.so

# Each extension module becomes Frameworks/<module>.framework/<module>, and
# lib-dynload/<module>.<suffix>.fwork names it (relative to the app bundle).
count=0
for so in "$lib"/lib-dynload/*.so; do
  file="$(basename "$so")"
  module="${file%%.*}"
  framework="$home/Frameworks/${module}.framework"
  mkdir -p "$framework"
  mv "$so" "$framework/$module"
  # _json -> org.forgemobile.devicelab.python.json (a bundle id has no _).
  bundle_id="${BUNDLE_PREFIX}.$(echo "${module#"${module%%[!_]*}"}" | tr '_' '-')"
  # CPython's template, with the module's executable name and bundle id.
  awk -v exe="$module" -v id="$bundle_id" '
    /<key>CFBundleExecutable<\/key>/ { print; getline; print "\t<string>" exe "</string>"; next }
    /<key>CFBundleIdentifier<\/key>/ { print; getline; print "\t<string>" id "</string>"; next }
    { print }' "$src/build/iOS-dylib-Info-template.plist" > "$framework/Info.plist"
  if [[ -f "$lib/lib-dynload/${module}.xcprivacy" ]]; then
    mv "$lib/lib-dynload/${module}.xcprivacy" "$framework/PrivacyInfo.xcprivacy"
  fi
  echo "python/Frameworks/${module}.framework/${module}" > "${so%.so}.fwork"
  echo "python/lib/python${MINOR}/lib-dynload/${file%.so}.fwork" > "$framework/${module}.origin"
  count=$((count + 1))
done

echo "$stamp_want" > "$out/.prepared"
echo "[python] CPython ${VERSION}: $(du -sh "$home" | cut -f1) standard library with ${count} extension modules, $(du -sh "$xcf" | cut -f1) interpreter"
