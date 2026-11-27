#!/usr/bin/env bash
# Start both the legacy owner and the probe on a new display; never touch the login clipboard.
set -eu
lab=$(mktemp -d); trap 'rm -rf "$lab"' EXIT
mkdir -m 700 "$lab/home" "$lab/run"
export HOME="$lab/home" XDG_RUNTIME_DIR="$lab/run" XDG_CONFIG_HOME="$lab/config"
export PIMATE_CONFIG_DIR="$lab/pimate" PI_CODING_AGENT_DIR="$lab/pi" PIMATE_RUN_DIR="$lab/pimate-run"
cc "$(dirname "$0")/fixtures/legacy-selection.c" -lX11 -o "$lab/owner"
mkfifo "$lab/ready"
timeout 30s env -u WAYLAND_DISPLAY -u ZED_HEADLESS xvfb-run -a dbus-run-session -- bash -eu -c '
  "$1/owner" >"$1/ready" & owner=$!; trap "kill $owner 2>/dev/null || :" EXIT
  read -r ready <"$1/ready"; test "$ready" = ready
  "$2"
' bash "$lab" "$1"
