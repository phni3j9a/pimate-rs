"""Drive the isolated 1280x800 Mutter monitor and inspect only the mock RC."""
import csv
import io
import json
import os
import signal
from pathlib import Path
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
def session():
    device = dbus.Interface(bus.get_object(remote, root.CreateSession()), remote + '.Session')
    device.Start()
    return device


seat = session()
pointer = session() if mode == 'hotplug' else seat


def key(sym):
    seat.NotifyKeyboardKeysym(sym, True)
    time.sleep(.05)
    seat.NotifyKeyboardKeysym(sym, False)
    time.sleep(.1)


def click(x, y):
    pointer.NotifyPointerMotionRelative(3000., 3000.)
    time.sleep(.15)
    pointer.NotifyPointerMotionRelative(float(x-1279), float(y-799))
    time.sleep(.15)
    pointer.NotifyPointerButton(272, True)
    pointer.NotifyPointerButton(272, False)
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


def prompts():
    path = Path('lab/run/received-prompts.jsonl')
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


key(0xff1b)  # dismiss shell overview
link = next(s for s in Path('demo.log').read_text().splitlines() if s.startswith('pimate://pair?'))
subprocess.run(['wl-copy', '--type', 'text/plain;charset=utf-8'], input=link.encode(), check=True)
image = snap('initial')
# Find the flat sidebar instead of assuming a Mutter window placement.
pixels = image.load()
coords = [(x, y) for y in range(800) for x in range(1280) if pixels[x, y] in ((241,243,244),(30,35,38))]
x = min(x for x, y in coords) + 120
y = min(y for x, y in coords)
if mode == 'check':
    # Stop only the daemon started by our namespace. Paste must still redraw while reload waits.
    daemon = int(os.environ['PRIVATE_KEYRING_PID'])
    os.kill(daemon, signal.SIGSTOP)
    try:
        started = time.monotonic()
        click(x, y+95)  # reload
        click(x, y+55)  # paste
        snap('confirm')
        def ocr(name):
            return subprocess.check_output(['tesseract', name+'.png', 'stdout', '--psm', '11'], text=True)
        assert '127.0.0.1' in ocr('confirm') and time.monotonic() - started < 5, 'UI blocked on reload'
        time.sleep(5)
        snap('storage-timeout')
        assert 'storage' in ocr('storage-timeout').lower(), 'missing timeout notice'
    finally:
        os.kill(daemon, signal.SIGCONT)
else:
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
assert prompts() == []
empty_editor = snap('empty-editor')
click(x+320, y+615)
if mode == 'hotplug':
    # Really losing the keyboard must cancel the marked 'か', not carry it into the next draft.
    key(0xff26)
    for char in 'ka':
        key(ord(char))
    marked = snap('before-keyboard-loss')
    seat.Stop()
    time.sleep(.5)
    cleared = snap('after-keyboard-loss')
    # Compare the placeholder glyphs, excluding the caret, pointer and input border.
    placeholder = (x+258, y+589, x+350, y+616)
    assert ImageChops.difference(empty_editor, marked).crop(placeholder).getbbox()
    assert not ImageChops.difference(empty_editor, cleared).crop(placeholder).getbbox(), 'marked text remained after keyboard loss'
    seat = session()
    key(0xffe1)
    key(0xff1b)  # dismiss any candidate restored by the input method itself
    click(x+320, y+615)
    pointer.Stop()  # keep only the keyboard until conversion is live
    time.sleep(.5)
else:
    key(0xff26)  # Hiragana
for char in 'nihongo':
    key(ord(char))
key(32); key(32)
snap('candidate')
if mode == 'hotplug':
    pointer = session()
    pointer.NotifyPointerMotionRelative(1., 0.)
    time.sleep(.5)
    snap('candidate-after-hotplug')
key(0xff0d)
snap('commit')
key(0xff0d)  # ordinary Enter adds newline, not a prompt
time.sleep(3.5)
assert prompts() == []
click(x+335,y+694)
time.sleep(4)
assert prompts() == [['demo', 'ニホンゴ\n']], 'explicit send did not deliver exactly one committed line'
snap('sent')
print(f'{mode}: paired, demo listed, stream changed, Enter: 0 prompts; button: 1 prompt = ニホンゴ + newline')
seat.Stop()
