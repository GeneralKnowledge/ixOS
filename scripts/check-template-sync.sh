#!/usr/bin/env bash
# Checks that the iOS template's Swift sources match the Counter example, so a
# newly generated app gets the same bridge architecture as the reference.
# Allowed differences: the app module name (CounterApp vs AppHost) and the
# Application Support folder name placeholder.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXAMPLE="$ROOT/examples/counter"
TEMPLATE="$ROOT/templates/ios-app"
status=0

compare() {
  local example_file="$1" template_file="$2"
  if ! diff -u \
    <(sed -e 's/^import CounterApp$/import AppHost/' \
      -e 's/appendingPathComponent("Counter", isDirectory: true)/appendingPathComponent("FORGE_APP_SUPPORT_DIR", isDirectory: true)/' \
      "$EXAMPLE/$example_file") \
    "$TEMPLATE/$template_file"; then
    echo "error: $template_file differs from examples/counter/$example_file" >&2
    status=1
  fi
}

for file in "$EXAMPLE"/Sources/CounterBridge/*.swift; do
  name="$(basename "$file")"
  compare "Sources/CounterBridge/$name" "Sources/CounterBridge/$name"
done
for name in ContentView.swift CounterViewModel.swift; do
  compare "Sources/CounterApp/$name" "Sources/AppHost/$name"
done
for file in "$EXAMPLE"/Tests/CounterAppTests/*.swift; do
  name="$(basename "$file")"
  compare "Tests/CounterAppTests/$name" "Tests/AppHostTests/$name"
done

# No extra files in the template that the example does not have.
for file in "$TEMPLATE"/Sources/CounterBridge/*.swift "$TEMPLATE"/Tests/AppHostTests/*.swift; do
  name="$(basename "$file")"
  if [[ ! -f "$EXAMPLE/Sources/CounterBridge/$name" && ! -f "$EXAMPLE/Tests/CounterAppTests/$name" ]]; then
    echo "error: template has $name, which the example does not" >&2
    status=1
  fi
done

if [[ $status -eq 0 ]]; then
  echo "[forge] iOS template Swift sources match the Counter example."
fi
exit $status
