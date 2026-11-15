#!/usr/bin/env bash
# The iOS "linker" for Rust builds on Linux (scripts/build-rust-ios.sh).
#
# Forge builds an app's Rust as a static library, which never runs a linker:
# Swift's build links it into the app. But a dependency that declares a
# cdylib (iroh, iroh-relay, irpc, n0-dns-resolver do) makes Cargo link that
# dylib for iOS too, and no Linux linker can link iOS dylibs (ld64.lld:
# "does not support linking for platform iOS"). Nothing uses those dylibs, so
# this writes an empty file where the linker would and says so.
set -euo pipefail
out=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "-o" && $# -gt 1 ]]; then
    out="$2"
    shift 2
  else
    shift
  fi
done
if [[ -n "$out" ]]; then
  : > "$out"
  echo "[forge] skipped linking $(basename "$out") for iOS (an unused dependency dylib)" >&2
fi
