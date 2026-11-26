#!/usr/bin/env bash
# GNOME/Mozc end-to-end on a virtual monitor; no login session or user manager.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); root=$(realpath "$here/../../..")
if [[ ${1:-} != inner ]]; then
  [[ ${1:-} == check || ${1:-} == hotplug || ${1:-} == package || ${1:-} == shots ]] || { echo "usage: $0 check|hotplug OUTPUT_DIRECTORY, package OUTPUT_DIRECTORY EXTRACTED_BIN, or shots" >&2; exit 2; }
  debug=${CARGO_TARGET_DIR:-$root/target}/debug
  if [[ $1 == package ]]; then
    desktop=$(realpath "${3:?pass the extracted bin/pimate-desktop}")
    [[ $desktop == */bin/pimate-desktop ]] || { echo 'expected extracted bin/pimate-desktop' >&2; exit 2; }
  else
    desktop=$(realpath "$debug/pimate-desktop")
  fi
  host=$(realpath "$debug/pimate"); [[ -x $host && -x $desktop ]]
  if [[ $1 == shots ]]; then
    # 撮影用。出力先から命令（sh を含む）を読むので、既存のディレクトリは受け付けない。毎回、自分だけの
    # 新しいディレクトリ（権限 700）を作ってパスを表示し、FIFO・ログ・画像をすべてそこに置く。
    [[ $# == 1 ]] || { echo 'shots takes no directory: it makes a new one and prints its path' >&2; exit 2; }
    out=$(realpath "$(mktemp -d "${TMPDIR:-/tmp}/pimate-shots.XXXXXX")"); echo "$out"
  else
    mkdir -p "$2"; out=$(realpath "$2")
  fi
  home=$(mktemp -d); trap 'rm -rf -- "$home"' EXIT
  mkdir -m700 "$home/run" "$home/Desktop" "$home/tmp"
  # shots は表示したディレクトリの cmd に linux-shots.py の命令を書いて操作する（SESSION_SECONDS で長さ）。
  limit=150; [[ $1 != shots ]] || limit=${SESSION_SECONDS:-1800}
  timeout -k 5 "$limit" unshare --user --map-current-user --pid --fork --kill-child --mount-proc \
    env -i TMPDIR="$home/tmp" HOME="$home" PATH=/usr/bin:/bin USER="$(id -un)" LANG=C.UTF-8 XDG_RUNTIME_DIR="$home/run" \
    XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" \
    MONITOR="${MONITOR:-1280x800}" DEMO_SCREENS="${DEMO_SCREENS:-}" \
    dbus-run-session -- "$here/linux-session.sh" inner "$1" "$host" "$desktop" "$out"
  exit
fi
mode=$2; host=$3; desktop=$4; out=$5
[[ $XDG_RUNTIME_DIR == "$HOME/run" && $(cat /proc/1/comm) == dbus-run-sessio ]] || exit 2
cd "$HOME"; umask 077; ulimit -c 0
export XDG_CURRENT_DESKTOP=GNOME XDG_SESSION_TYPE=wayland WAYLAND_DISPLAY=wayland-0 DISPLAY=:19731
gnome-keyring-daemon --foreground --unlock --components=secrets < <(printf disposable) >keyring.log 2>&1 &
export PRIVATE_KEYRING_PID=$!
# Only the private bus. The portal must see this Wayland session, not the login session.
dbus-update-activation-environment XDG_CURRENT_DESKTOP XDG_SESSION_TYPE WAYLAND_DISPLAY
gdbus wait --session --timeout 10 org.freedesktop.secrets
gsettings set org.gnome.shell disable-user-extensions true
gsettings set org.gnome.desktop.session idle-delay 0
gsettings set org.gnome.desktop.input-sources sources "[('ibus', 'mozc-jp'), ('xkb', 'us')]"
gnome-shell --headless --wayland --no-x11 --virtual-monitor "$MONITOR" >shell.log 2>&1 &
sleep 8
ibus engine mozc-jp || :
ibus engine
mkdir lab
demo=(PIMATE_DEMO_COUNT_PROMPTS=1 PIMATE_DEMO_NAME=Demo54 PIMATE_DEMO_DESKTOP_REVIEW=1)
# DEMO_SCREENS=list|empty|reconnect: モックに近い一覧の作り物（scripts/android-screen-fixture.cjs）。
[[ -z $DEMO_SCREENS ]] || demo=(PIMATE_DEMO_NAME=MacBook PIMATE_DEMO_SCREENS="$DEMO_SCREENS")
env "${demo[@]}" python3 -B - "$root/scripts/android-demo.py" "$host" >demo.log 2>&1 <<'PY' &
import asyncio, pathlib, runpy, sys
asyncio.run(runpy.run_path(sys.argv[1])['demo'](pathlib.Path.cwd()/'lab', sys.argv[2]))
PY
if [[ $mode == check ]]; then
  python3 "$here/linux-drive.py" prime &
  for _ in {1..50}; do [[ ! -f ready ]] || break; sleep .1; done
  test -f ready
fi
unset DISPLAY
WAYLAND_DEBUG=client "$desktop" --demo >app.log 2>&1 & app=$!
sleep 4; kill -0 "$app"
if [[ $mode == shots ]]; then
  cp demo.log "$out/demo.log"
  # ドライバーが起動できなかったら（D-Bus など）、ログを残して失敗で終わる。
  status=0; python3 "$here/linux-shots.py" "$out" || status=$?
  cp app.log "$out/app.log"
  exit "$status"
fi
python3 "$here/linux-drive.py" "$mode" "$out"
[[ $mode != package ]] || exit 0
test "$(grep -Fc '.commit_string("ニホンゴ")' app.log)" = 1
if [[ $mode == hotplug ]]; then
  # Assert pointer capability was added during live composition, on the same input object.
  python3 - <<'PY_TRACE'
import re
from pathlib import Path
log = Path('app.log').read_text()
start = re.search(r'(zwp_text_input_v3@\d+)\.preedit_string\("ニホンゴ"', log)
assert start, 'missing candidate'
old = re.search(r'(zwp_text_input_v3@\d+)\.preedit_string\("か"', log[:start.start()])
assert old and old[1] + '.destroy()' in log[old.end():start.start()], 'keyboard removal not exercised'
commit = log.index(start[1] + '.commit_string("ニホンゴ")', start.end())
during = log[start.end():commit]
assert re.search(r'wl_seat@\d+\.capabilities\(3\)', during), 'pointer not added during preedit'
assert '.destroy()' not in '\n'.join(l for l in during.splitlines() if start[1]+'.' in l)
assert '.get_text_input(' not in during
PY_TRACE
fi
printf '%s: Wayland commit_string exactly once\n' "$mode"
