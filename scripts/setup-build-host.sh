#!/usr/bin/env bash
# One-time setup for a machine that several people or sessions build on (the
# device-host laptop), so builds can never freeze it. See
# docs/builds.md#the-build-queue.
#
# Usage:
#   scripts/setup-build-host.sh                  set up (asks for your sudo password)
#   scripts/setup-build-host.sh --check          only report; changes nothing
#   scripts/setup-build-host.sh --protect NAME   also never let earlyoom stop
#                                                processes named NAME (repeatable;
#                                                remembered for later runs)
#
# What it sets up:
#   1. Memory caps for builds: lingering for your user and the memory
#      controller delegated to your user's systemd, so `forge` can run each
#      build in a `systemd-run --user --scope` with MemoryMax (sudo).
#   2. A safety net for anything that bypasses the queue: earlyoom, which
#      stops the biggest compiler before the machine runs out of memory. It
#      never stops a protected process: anything using a GPU when the script
#      runs, common ML runtimes (python, ollama, llama.cpp, jupyter), the
#      names in ~/.config/forge/protected-processes (one per line; --protect
#      adds to it), and sshd, tailscaled, usbmuxd and Claude (sudo, apt).
#   3. The current forge in ~/.cargo/bin, and a list of other forge copies
#      that are too old to queue.
#   4. ~/.config/forge/build-host.toml, which marks this machine as a shared
#      build host: forge then refuses to build without the memory cap rather
#      than risk the machine. Written only once the cap is proven to work.
# Safe to run again.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECK_ONLY=0
ADD_PROTECT=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --check) CHECK_ONLY=1 ;;
    --protect) ADD_PROTECT+=("${2:?--protect needs a process name}"); shift ;;
    -h | --help) sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "error: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done
PROTECT_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/forge/protected-processes"

UID_NUM="$(id -u)"
USER_NAME="$(id -un)"
CONTROLLERS="/sys/fs/cgroup/user.slice/user-$UID_NUM.slice/user@$UID_NUM.service/cgroup.controllers"
HOST_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/forge/build-host.toml"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$UID_NUM}"
NEEDS_RESTART=0
PROBLEMS=0

say() { echo "[build-host] $*"; }
ok() { echo "[build-host]   ok: $*"; }
todo() { echo "[build-host]   TO DO: $*"; PROBLEMS=$((PROBLEMS + 1)); }
run_sudo() {
  if [[ $CHECK_ONLY -eq 1 ]]; then
    todo "$*"
    return 1
  fi
  say "  sudo $*"
  sudo "$@"
}

if ! command -v systemctl >/dev/null || [[ ! -d /run/systemd/system ]]; then
  echo "error: this machine doesn't run systemd; builds can't be memory-capped here" >&2
  exit 1
fi

# 1. Memory caps --------------------------------------------------------------
say "1. Memory caps for builds"
if [[ "$(loginctl show-user "$USER_NAME" -p Linger --value 2>/dev/null)" == yes ]]; then
  ok "lingering is on for $USER_NAME (your systemd runs without a login, e.g. for Remote Control)"
else
  run_sudo loginctl enable-linger "$USER_NAME" && ok "lingering turned on"
fi

if grep -qw memory "$CONTROLLERS" 2>/dev/null; then
  ok "the memory controller is delegated to your systemd"
else
  dropin=/etc/systemd/system/user@.service.d/forge-delegate.conf
  if [[ -f $dropin ]]; then
    say "  $dropin is in place but not active yet"
  elif [[ $CHECK_ONLY -eq 1 ]]; then
    todo "delegate the memory controller to user systemd ($dropin)"
  else
    say "  writing $dropin"
    sudo mkdir -p "$(dirname "$dropin")"
    printf '[Service]\nDelegate=cpu cpuset io memory pids\n' | sudo tee "$dropin" >/dev/null
    sudo systemctl daemon-reload
  fi
  NEEDS_RESTART=1
fi

cap_works() {
  # A capped scope must start, and a process that grows past its 64 MiB cap
  # must be killed (`tail` keeps the whole newline-free stream in memory).
  systemd-run --user --scope --quiet --collect -p MemoryMax=64M -- true >/dev/null 2>&1 || return 1
  ! systemd-run --user --scope --quiet --collect -p MemoryMax=64M -p MemorySwapMax=0 \
    -- sh -c 'head -c 512M /dev/zero | tail -n 1 >/dev/null' >/dev/null 2>&1
}
CAP_OK=0
if [[ $NEEDS_RESTART -eq 0 ]]; then
  if cap_works; then
    CAP_OK=1
    ok "a capped scope really stops a process at its memory limit"
  else
    todo "\`systemd-run --user --scope -p MemoryMax=64M\` did not stop a 512 MiB process; check \`systemctl --user status\` and $CONTROLLERS"
  fi
fi

# 2. Safety net ---------------------------------------------------------------
say "2. Out-of-memory safety net (earlyoom)"
# earlyoom matches the process name (/proc/<pid>/comm, at most 15
# characters). Protected names go in --ignore: never stopped, whatever
# happens. Compilers go in --prefer: stopped first.
if [[ ${#ADD_PROTECT[@]} -gt 0 && $CHECK_ONLY -eq 0 ]]; then
  mkdir -p "$(dirname "$PROTECT_FILE")"
  printf '%s\n' "${ADD_PROTECT[@]}" >>"$PROTECT_FILE"
fi
gpu_users() {
  # Every process with a GPU device open (NVIDIA, AMD/Intel render nodes,
  # ROCm), and what nvidia-smi lists as compute jobs.
  local dev pid
  for dev in /dev/nvidia[0-9]* /dev/dri/renderD* /dev/kfd; do
    [[ -e $dev ]] || continue
    for pid in $(fuser "$dev" 2>/dev/null; sudo -n fuser "$dev" 2>/dev/null); do
      cat "/proc/${pid//[^0-9]/}/comm" 2>/dev/null
    done
  done
  if command -v nvidia-smi >/dev/null; then
    nvidia-smi --query-compute-apps=process_name --format=csv,noheader 2>/dev/null |
      while IFS= read -r name; do basename "$name" | cut -c1-15; done
  fi
}
mapfile -t PROTECTED < <( {
  printf '%s\n' sshd systemd tailscaled usbmuxd claude node dbus-daemon init \
    python python3 ollama llama-server llama-cli jupyter-lab torchrun
  gpu_users
  [[ -f $PROTECT_FILE ]] && grep -v '^\s*\(#\|$\)' "$PROTECT_FILE"
  printf '%s\n' "${ADD_PROTECT[@]}"
} | sed 's/[^A-Za-z0-9_.+-]//g' | cut -c1-15 | grep -v '^$' | sort -u)
GPU_NOW="$(gpu_users | sort -u | tr '\n' ' ')"
[[ -n $GPU_NOW ]] && say "  using a GPU now, so protected: $GPU_NOW"
say "  protected (never stopped): ${PROTECTED[*]}"
# Prefix matches, no "$": a name cut to 15 characters still matches.
ignore="^($(IFS='|'; echo "${PROTECTED[*]}" | sed 's/\./\\./g; s/+/\\+/g'))"
# No quotes or spaces in the regexes: the service passes $EARLYOOM_ARGS split
# on whitespace.
EARLYOOM_ARGS="-m 6 -s 10 -r 3600 --prefer ^(swift-frontend|swift-build|swift-driver|swiftc|rustc|cargo|clang|ld|lld|xtool|cc1) --ignore $ignore"
if [[ $CHECK_ONLY -eq 1 ]]; then
  if ! systemctl is-active --quiet earlyoom; then
    todo "install and start earlyoom"
  elif [[ "$(cat /etc/default/earlyoom 2>/dev/null)" != *"--ignore $ignore"* ]]; then
    todo "earlyoom runs, but without today's protected list"
  else
    ok "earlyoom is running and protects them"
  fi
else
  command -v earlyoom >/dev/null || sudo apt-get install -y earlyoom
  if ! earlyoom -h 2>&1 | grep -q -- '--ignore'; then
    todo "this earlyoom ($(earlyoom -v 2>&1 | head -1)) has no --ignore, so it can't promise to spare protected processes; leaving it off"
    sudo systemctl disable --now earlyoom 2>/dev/null
  else
    echo "EARLYOOM_ARGS=\"$EARLYOOM_ARGS\"" | sudo tee /etc/default/earlyoom >/dev/null
    sudo systemctl enable earlyoom >/dev/null 2>&1
    sudo systemctl restart earlyoom
    if systemctl is-active --quiet earlyoom; then
      ok "earlyoom is running: it stops compilers first and never a protected process"
    else
      todo "earlyoom didn't start: \`systemctl status earlyoom\`"
    fi
  fi
fi
say "  A job started later with a name not in the list: add it with --protect <name>,"
say "  or protect that one process from any out-of-memory kill: sudo choom -n -1000 -p <pid>"

# 3. forge --------------------------------------------------------------------
say "3. forge"
if [[ $CHECK_ONLY -eq 0 ]]; then
  say "  installing this checkout's forge into ~/.cargo/bin (at low priority)"
  # Waits its turn behind any build running now (the queue's lock file).
  lock="${FORGE_BUILD_LOCK:-${XDG_CACHE_HOME:-$HOME/.cache}/forge/build.lock}"
  mkdir -p "$(dirname "$lock")"
  flock -w 3600 "$lock" nice -n 10 cargo install --quiet --locked --force --path "$ROOT/crates/forge-cli" ||
    todo "cargo install of forge failed"
fi
old=()
while IFS= read -r bin; do
  [[ -x "$bin" ]] || continue
  "$bin" queue status >/dev/null 2>&1 || old+=("$bin")
done < <( { command -v forge; ls "$HOME"/.cargo/bin/forge "$HOME"/*/target/{debug,release}/forge "$HOME"/.cache/forge/ios-check/*/target/{debug,release}/forge 2>/dev/null; } | sort -u)
if [[ ${#old[@]} -eq 0 ]]; then
  ok "every forge found here queues its builds"
else
  say "  these forge copies are too old to queue builds; rebuild them from main (or delete them):"
  printf '[build-host]     %s\n' "${old[@]}"
  PROBLEMS=$((PROBLEMS + 1))
fi

# 4. Mark the machine ---------------------------------------------------------
say "4. Shared build host marker"
if [[ -f $HOST_FILE ]]; then
  ok "$HOST_FILE exists: forge refuses to build here without the memory cap"
elif [[ $CAP_OK -eq 1 && $CHECK_ONLY -eq 0 ]]; then
  mkdir -p "$(dirname "$HOST_FILE")"
  cat >"$HOST_FILE" <<'EOF'
# This machine is a shared build host: forge builds only run in the build
# queue with a memory cap (docs/builds.md#the-build-queue). Uncomment to
# change the caps; the defaults leave a quarter of the memory (at least
# 2 GiB) and one core for everything else.
# memory_max_gib = 10
# cpu_quota_percent = 300
# jobs = 3
EOF
  ok "wrote $HOST_FILE"
else
  todo "write $HOST_FILE once the memory cap works (run this script again)"
fi

echo
if [[ $NEEDS_RESTART -eq 1 ]]; then
  say "The memory controller is delegated from the next start of your user's systemd."
  say "Restart it when no build or Remote Control session needs to keep running"
  say "(or reboot), then run this script again:"
  say "    sudo systemctl restart user@$UID_NUM.service"
fi
if [[ $PROBLEMS -eq 0 && $NEEDS_RESTART -eq 0 ]]; then
  say "Done: builds here queue, run capped, and earlyoom guards the rest. \`forge queue status\` shows the queue."
else
  say "$PROBLEMS step(s) still to do (above)."
  exit 1
fi
