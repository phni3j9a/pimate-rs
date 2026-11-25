#!/usr/bin/env bash
# Pass the compiled Rust test binary; never address the login keyring or bus.
set -euo pipefail
home=$(mktemp -d); trap 'rm -rf -- "$home"' EXIT
mkdir -m700 "$home/run"
timeout -k 5 60 env -i PIMATE_PRIVATE_STORE_TEST=1 PATH=/usr/bin:/bin HOME="$home" XDG_RUNTIME_DIR="$home/run" \
  XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" \
  dbus-run-session -- bash -eu -c '
    bin=$1; failure=store::desktop::secret_service_failure_is_not_empty_registration
    DBUS_SESSION_BUS_ADDRESS="unix:path=$HOME/missing" "$bin" --ignored --exact "$failure"
    gnome-keyring-daemon --foreground --unlock --components=secrets < <(printf disposable) >"$HOME/daemon.log" 2>&1 & daemon=$!
    trap '\''kill -CONT "$daemon" 2>/dev/null || :; kill "$daemon" 2>/dev/null || :; wait "$daemon" || :'\'' EXIT
    gdbus wait --session --timeout 10 org.freedesktop.secrets
    "$bin" --ignored --exact store::desktop::replacement_survives_old_invalidation
    "$bin" --ignored --exact store::desktop::alias_change_during_edit_does_not_redirect_write
    PRIVATE_KEYRING_PID=$daemon "$bin" --ignored --exact store::desktop::delayed_create_keeps_the_edit_lock_until_reply
    kill -STOP "$daemon"
    timeout 14 "$bin" --ignored --exact "$failure"
    kill -CONT "$daemon"
    gdbus call --session -d org.freedesktop.secrets -o /org/freedesktop/secrets -m org.freedesktop.Secret.Service.Lock "[\"/org/freedesktop/secrets/collection/login\"]"
    "$bin" --ignored --exact "$failure"
  ' bash "$(realpath "$1")"
