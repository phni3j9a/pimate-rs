"""撮影用のドライバー。`linux-session.sh shots` の中で動く。shots は毎回、自分だけの新しい
ディレクトリ OUT（mktemp -d、権限 700）を作ってパスを表示し、命令の FIFO・ログ・画像をすべてそこに置く。
既存のディレクトリは使わないので、既存のファイルを消すことも、リンクをたどることもない。

    printf 'copylink\nclick 640 500\nsnap 01-first\n' > OUT/cmd; cat OUT/ack

命令: snap NAME / click X Y / rclick X Y / dclick X Y / move X Y / drag X1 Y1 X2 Y2 / wheel N /
key ctrl+k / type TEXT / copy TEXT / copylink（ペアリングのリンク）/ gs SCHEMA KEY VALUE / sleep S / sh CMD / quit
座標は仮想モニター（MONITOR、既定 1280x800）の画素。隔離した GNOME の中だけを操作する。
"""
import os, stat, subprocess, sys, time
from pathlib import Path
import dbus
assert os.environ['XDG_RUNTIME_DIR'] == str(Path.home()/'run')
out = Path(sys.argv[1])
bus = dbus.SessionBus()
remote = 'org.gnome.Mutter.RemoteDesktop'
root = dbus.Interface(bus.get_object(remote, '/org/gnome/Mutter/RemoteDesktop'), remote)
seat = dbus.Interface(bus.get_object(remote, root.CreateSession()), remote + '.Session')
seat.Start()
bus.request_name('org.gnome.Screenshot')
ss = 'org.gnome.Shell.Screenshot'
shot = dbus.Interface(bus.get_object(ss, '/org/gnome/Shell/Screenshot'), ss)
W, H = (int(v) for v in os.environ.get('MONITOR', '1280x800').split('x'))
MODS = dict(ctrl=0xffe3, shift=0xffe1, alt=0xffe9, super=0xffeb)
NAMED = dict(esc=0xff1b, enter=0xff0d, tab=0xff09, up=0xff52, down=0xff54, left=0xff51, right=0xff53,
             pgup=0xff55, pgdn=0xff56, home=0xff50, end=0xff57, bs=0xff08, space=32, f10=0xffc7, menu=0xff67, delete=0xffff)
def key(sym):
    seat.NotifyKeyboardKeysym(sym, True); time.sleep(.05); seat.NotifyKeyboardKeysym(sym, False); time.sleep(.08)
def move(x, y):
    seat.NotifyPointerMotionRelative(5000., 5000.); time.sleep(.12)
    seat.NotifyPointerMotionRelative(float(x-(W-1)), float(y-(H-1))); time.sleep(.15)
def button(code, x, y):
    move(x, y); seat.NotifyPointerButton(code, True); seat.NotifyPointerButton(code, False); time.sleep(.4)
def run(line):
    a = line.split()
    if not a: return
    c = a[0]
    if c == 'snap':
        path = out/(a[1] + '.png'); path.unlink(missing_ok=True)  # 同じ名前で撮り直すとき、この回の前の1枚だけを置き換える
        ok, _ = shot.Screenshot(False, False, str(path)); assert ok
    elif c == 'click': button(272, int(a[1]), int(a[2]))
    elif c == 'rclick': button(273, int(a[1]), int(a[2]))
    elif c == 'dclick':
        move(int(a[1]), int(a[2]))
        for _ in range(2): seat.NotifyPointerButton(272, True); seat.NotifyPointerButton(272, False); time.sleep(.08)
    elif c == 'move': move(int(a[1]), int(a[2]))
    elif c == 'drag':
        move(int(a[1]), int(a[2])); seat.NotifyPointerButton(272, True); time.sleep(.1)
        for _ in range(10): seat.NotifyPointerMotionRelative((int(a[3])-int(a[1]))/10., (int(a[4])-int(a[2]))/10.); time.sleep(.03)
        seat.NotifyPointerButton(272, False); time.sleep(.3)
    elif c == 'wheel': seat.NotifyPointerAxisDiscrete(0, int(a[1])); time.sleep(.3)
    elif c == 'key':  # key ctrl+shift+k / key esc
        parts = a[1].split('+'); mods = [MODS[p] for p in parts[:-1]]
        sym = NAMED.get(parts[-1]) or ord(parts[-1])
        for m in mods: seat.NotifyKeyboardKeysym(m, True)
        key(sym)
        for m in reversed(mods): seat.NotifyKeyboardKeysym(m, False)
        time.sleep(.2)
    elif c == 'type':
        for ch in line.split(' ', 1)[1]: key(ord(ch))
    elif c == 'copy': subprocess.run(['wl-copy', '--type', 'text/plain;charset=utf-8'], input=line.split(' ', 1)[1].encode(), check=True)
    elif c == 'copylink':
        link = next(s for s in Path('demo.log').read_text().splitlines() if s.startswith('pimate://pair?'))
        subprocess.run(['wl-copy', '--type', 'text/plain;charset=utf-8'], input=link.encode(), check=True)
    elif c == 'gs': subprocess.run(['gsettings', 'set', *a[1:]], check=True); time.sleep(.6)
    elif c == 'sleep': time.sleep(float(a[1]))
    elif c == 'sh': subprocess.run(line.split(' ', 1)[1], shell=True)
    elif c == 'quit': raise SystemExit
    else: print('unknown', line, flush=True)
fifo, ack = out/'cmd', out/'ack'
# FIFO は linux-session.sh がパスを表示する前に作る（表示直後の書き込みも準備まで待つ）。
for p in (fifo, ack):
    if not stat.S_ISFIFO(os.lstat(p).st_mode): sys.exit(f'{p} is not a FIFO')
(out/'ready').touch()
while True:
    with open(fifo) as f: lines = f.read().splitlines()
    msg = 'ok'
    try:
        for line in lines: run(line)
    except SystemExit:
        open(ack, 'w').write('bye\n'); break
    except Exception as e:
        msg = f'error: {e!r}'
    open(ack, 'w').write(msg + '\n')
seat.Stop()
