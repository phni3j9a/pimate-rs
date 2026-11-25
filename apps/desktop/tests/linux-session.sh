#!/usr/bin/env bash
# GNOME/Mozc end-to-end on a virtual monitor; no login session or user manager.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); root=$(realpath "$here/../../..")
if [[ ${1:-} != inner ]]; then
  [[ ${1:-} == check || ${1:-} == hotplug ]] || { echo "usage: $0 check|hotplug OUTPUT_DIRECTORY" >&2; exit 2; }
  bin=$(realpath "${CARGO_TARGET_DIR:-$root/target}/debug"); [[ -x $bin/pimate && -x $bin/pimate-desktop ]]
  mkdir -p "$2"; out=$(realpath "$2"); home=$(mktemp -d); trap 'rm -rf -- "$home"' EXIT
  mkdir -m700 "$home/run" "$home/Desktop" "$home/tmp"
  timeout -k 5 150 unshare --user --map-current-user --pid --fork --kill-child --mount-proc \
    env -i TMPDIR="$home/tmp" HOME="$home" PATH=/usr/bin:/bin USER="$(id -un)" LANG=C.UTF-8 XDG_RUNTIME_DIR="$home/run" \
    XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" \
    dbus-run-session -- "$here/linux-session.sh" inner "$1" "$bin" "$out"
  exit
fi
mode=$2; bin=$3; out=$4
[[ $XDG_RUNTIME_DIR == "$HOME/run" && $(cat /proc/1/comm) == dbus-run-sessio ]] || exit 2
cd "$HOME"; umask 077; ulimit -c 0
export XDG_CURRENT_DESKTOP=GNOME XDG_SESSION_TYPE=wayland WAYLAND_DISPLAY=wayland-0 DISPLAY=:19731
printf disposable | gnome-keyring-daemon --unlock --components=secrets >/dev/null
gsettings set org.gnome.shell disable-user-extensions true
gsettings set org.gnome.desktop.session idle-delay 0
gsettings set org.gnome.desktop.input-sources sources "[('ibus', 'mozc-jp'), ('xkb', 'us')]"
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800 >shell.log 2>&1 &
sleep 8
ibus engine mozc-jp || :
ibus engine
mkdir lab
PIMATE_DEMO_NAME=Demo54 PIMATE_DEMO_DESKTOP_REVIEW=1 python3 -B - "$root/scripts/android-demo.py" "$bin/pimate" >demo.log 2>&1 <<'PY' &
import asyncio, pathlib, runpy, sys
asyncio.run(runpy.run_path(sys.argv[1])['demo'](pathlib.Path.cwd()/'lab', sys.argv[2]))
PY
if [[ $mode == check ]]; then
  python3 "$here/linux-drive.py" prime &
  for _ in {1..50}; do [[ ! -f ready ]] || break; sleep .1; done
  test -f ready
fi
unset DISPLAY
WAYLAND_DEBUG=client "$bin/pimate-desktop" --demo >app.log 2>&1 & app=$!
sleep 4; kill -0 "$app"
python3 "$here/linux-drive.py" "$mode" "$out"
test "$(grep -Fc '.commit_string("ニホンゴ")' app.log)" = 1
if [[ $mode == hotplug ]]; then
  # The old object must be destroyed before the Japanese commit.
  sed '/\.commit_string("ニホンゴ")/q' app.log | grep 'zwp_text_input_v3@[0-9]*\.destroy()' >/dev/null
fi
printf '%s: Wayland commit_string exactly once\n' "$mode"
