#!/usr/bin/env bash
# Linux-side regression: the connection probe must not consume a piped remote script.
set -eu
d=$(mktemp -d)
bash -c 'exec -a "devin forward (test)" sleep 30' & pid=$!
trap 'kill "$pid" 2>/dev/null || :; rm -rf "$d"' EXIT
export DEVIN_MAC_STATE_DIR="$d" DEVIN_MAC_SESSION=devin-00000000000000000000000000000000
printf '%s\n' "$pid" >"$d/tunnel.pid"
printf '%s\n' "$DEVIN_MAC_SESSION" >"$d/tunnel.session"
printf '#!/bin/sh\ncat\n' >"$d/ssh"
chmod +x "$d/ssh"
export PATH="$d:$PATH"
result=$(printf 'payload' | "$(dirname "$0")/devin-mac" ssh cat)
[[ $result == payload ]]
