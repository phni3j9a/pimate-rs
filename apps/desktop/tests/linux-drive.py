"""Drive the isolated 1280x800 Mutter monitor and inspect only the mock RC."""
import csv
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

import dbus
from PIL import Image, ImageChops

assert Path.cwd() == Path.home() and os.environ['XDG_RUNTIME_DIR'] == str(Path.home()/'run')
mode = sys.argv[1]
bus = dbus.SessionBus()
remote = 'org.gnome.Mutter.RemoteDesktop'
root = dbus.Interface(bus.get_object(remote, '/org/gnome/Mutter/RemoteDesktop'), remote)
seat = dbus.Interface(bus.get_object(remote, root.CreateSession()), remote + '.Session')
seat.Start()


def key(sym):
    seat.NotifyKeyboardKeysym(sym, True)
    time.sleep(.05)
    seat.NotifyKeyboardKeysym(sym, False)
    time.sleep(.1)


def click(x, y):
    seat.NotifyPointerMotionRelative(3000., 3000.)
    time.sleep(.15)
    seat.NotifyPointerMotionRelative(float(x-1279), float(y-799))
    time.sleep(.15)
    seat.NotifyPointerButton(272, True)
    seat.NotifyPointerButton(272, False)
    time.sleep(.35)


if mode == 'prime':
    key(0xffe1)
    Path('ready').touch()
    time.sleep(140)
    sys.exit()

out = Path(sys.argv[2])
bus.request_name('org.gnome.Screenshot')
ss = 'org.gnome.Shell.Screenshot'
shot = dbus.Interface(bus.get_object(ss, '/org/gnome/Shell/Screenshot'), ss)


def snap(name):
    path = Path(name + '.png').resolve()
    path.unlink(missing_ok=True)
    ok, _ = shot.Screenshot(False, False, str(path))
    assert ok
    image = Image.open(path).convert('RGB')
    image.thumbnail((960, 600))
    image.save(out / (name + '.webp'), quality=84)
    return Image.open(path).convert('RGB')


def users():
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(4)
        sock.connect('lab/run/demo.sock')
        sock.sendall(b'{"id":"verify","method":"history"}\n')
        for line in sock.makefile('rb'):
            r = json.loads(line)
            if r.get('id') == 'verify':
                return [i['text'] for i in r['result']['items'] if i['kind'] == 'user']
    raise AssertionError('no history reply')


key(0xff1b)  # dismiss shell overview
link = next(s for s in Path('demo.log').read_text().splitlines() if s.startswith('pimate://pair?'))
subprocess.run(['wl-copy', '--type', 'text/plain;charset=utf-8'], input=link.encode(), check=True)
image = snap('initial')
# Find the flat sidebar instead of assuming a Mutter window placement.
pixels = image.load()
coords = [(x, y) for y in range(800) for x in range(1280) if pixels[x, y] in ((241,243,244),(30,35,38))]
x = min(x for x, y in coords) + 120
y = min(y for x, y in coords)
click(x, y+55)
snap('confirm')
click(x, y+126)
time.sleep(2)
image = snap('list')
# English OCR locates the demo in the (sorted, variable-length) list.
words = csv.DictReader(io.StringIO(subprocess.check_output(['tesseract','list.png','stdout','--psm','11','tsv'], text=True)), delimiter='\t')
word = next(w for w in words if 'android' in w['text'].lower())
click(int(word['left'])+8, int(word['top'])+6)
time.sleep(.7)
a = snap('stream-start')
time.sleep(2)
b = snap('stream-later')
assert ImageChops.difference(a,b).crop((x+160,y+60,x+760,y+490)).getbbox(), 'no visible stream update'
time.sleep(12)
assert users() == []
click(x+320, y+615)
key(0xff26)  # Hiragana
for char in 'nihongo':
    key(ord(char))
key(32); key(32)
snap('candidate')
key(0xff0d)
snap('commit')
key(0xff0d)  # ordinary Enter adds newline, not a prompt
time.sleep(3.5)
assert users() == []
click(x+335,y+694)
time.sleep(4)
assert users() == ['ニホンゴ\n'], 'explicit send did not deliver exactly one committed line'
snap('sent')
print(f'{mode}: paired, demo listed, stream changed, Enter: 0 prompts; button: 1 prompt = ニホンゴ + newline')
seat.Stop()
