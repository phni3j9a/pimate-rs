"""Drive the isolated 1280x800 Mutter monitor and inspect only the mock RC."""
import csv
import io
import json
import os
import signal
import socket
from pathlib import Path
import subprocess
import sys
import time

import dbus
from PIL import Image, ImageChops

assert Path.cwd() == Path.home() and os.environ['XDG_RUNTIME_DIR'] == str(Path.home()/'run')
# Small OCR crops are faster single-threaded, especially alongside other isolated clients.
os.environ['OMP_THREAD_LIMIT'] = '1'
# A separate Wayland surface advertises text/uri-list, like a file manager.
if sys.argv[1] == 'drag-source':
    import gi
    gi.require_version('Gtk', '3.0')
    from gi.repository import Gtk, Gdk
    image = Path('dropped.png').resolve()
    Image.new('RGB', (96, 96), (26, 192, 107)).save(image)
    win = Gtk.Window(title='Image drop fixture')
    area = Gtk.EventBox(); area.set_size_request(190, 100)
    color = Gdk.RGBA(); color.parse('#c31d68')
    area.override_background_color(Gtk.StateFlags.NORMAL, color)
    area.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY)
    area.drag_source_add_uri_targets()
    area.connect('drag-data-get', lambda _,ctx,data,info,t: data.set_uris([image.as_uri()]))
    win.add(area); win.show_all(); Gtk.main(); sys.exit()

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


def move(x, y):
    pointer.NotifyPointerMotionRelative(3000., 3000.)
    time.sleep(.15)
    pointer.NotifyPointerMotionRelative(float(x-1279), float(y-799))
    time.sleep(.15)


def click(x, y):
    move(x, y)
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
ACCENT = (27, 107, 145)


def blob(image, color, box=(0, 0, 1280, 800)):
    """Centre and left edge of the pixels painted in exactly this colour (a filled button)."""
    pixels = image.load()
    found = [(px, py) for py in range(box[1], box[3]) for px in range(box[0], box[2]) if pixels[px, py] == color]
    assert found, f'no {color} in {box}'
    xs, ys = [f[0] for f in found], [f[1] for f in found]
    return (min(xs)+max(xs))//2, (min(ys)+max(ys))//2, min(xs)


# Find the window by the title bar the app draws on GNOME, not by a Mutter window placement.
pixels = image.load()
bar = [(x, y) for y in range(800) for x in range(1280) if pixels[x, y] in ((230,233,235),(41,48,53))]
x = min(x for x, y in bar)
y = min(y for x, y in bar) + 34  # top of the panes, below the title bar
# First screen: the only filled button is step 03 "リンクを貼り付ける"; "登録を読み直す" sits below its right.
paste_x, paste_y, _ = blob(image, ACCENT)


def confirm():
    """The dialog's filled button (the paste button behind the scrim is dimmed), Cancel on its left,
    when it was drawn, and the name and address above them (small type: enlarge it for OCR)."""
    image = snap('confirm')
    drawn = time.monotonic()
    cx, cy, left = blob(image, ACCENT)
    image.crop((cx-290, cy-100, cx+90, cy-25)).resize((1140, 225)).save('target.png')
    target = subprocess.check_output(['tesseract', 'target.png', 'stdout', '--psm', '6'], text=True)
    return (cx, cy), (left-62, cy), drawn, target


if mode == 'check':
    # Stop only the daemon started by our namespace. Paste must still redraw while reload waits.
    daemon = int(os.environ['PRIVATE_KEYRING_PID'])
    os.kill(daemon, signal.SIGSTOP)
    try:
        started = time.monotonic()
        click(paste_x+322, paste_y+78)  # reload
        click(paste_x, paste_y)  # paste
        _, cancel, drawn, target = confirm()
        drawn_after = drawn - started
        def ocr(name):
            return subprocess.check_output(['tesseract', name+'.png', 'stdout', '--psm', '11'], text=True)
        assert '127.0.0.1' in target and drawn_after < 5, 'UI blocked on reload'
        click(*cancel)  # the notice appears under the paste button, behind the dialog
        time.sleep(5)
        # The notice makes the centred steps taller, so look for the paste button again.
        paste_x, paste_y, _ = blob(snap('storage-timeout'), ACCENT)
        assert 'storage' in ocr('storage-timeout').lower(), 'missing timeout notice'
    finally:
        os.kill(daemon, signal.SIGCONT)
click(paste_x, paste_y)
click(*confirm()[0])
time.sleep(2)
image = snap('list')
# English OCR locates the demo in the (sorted, variable-length) list.
words = csv.DictReader(io.StringIO(subprocess.check_output(['tesseract','list.png','stdout','--psm','11','tsv'], text=True)), delimiter='\t')
word = next(w for w in words if 'android' in w['text'].lower())
if mode == 'package':
    print('Package: paired with loopback Host, demo /android visible')
    sys.exit(0)
click(int(word['left'])+8, int(word['top'])+6)
time.sleep(.8)
# Select only the first characters in the heading, while 50 ms deltas are arriving.
# Read the beginning deliberately, even if wrapping tables have already pushed it out of view.
move(x+550, y+190)
deadline = time.monotonic() + 7
while time.monotonic() < deadline:
    pointer.NotifyPointerAxisDiscrete(0, -16)
    time.sleep(.25)
    image = snap('heading')
    image.crop((x+350,y+48,x+1050,y+280)).resize((1400,464)).save('heading-ocr.png')
    words = csv.DictReader(io.StringIO(subprocess.check_output(['tesseract','heading-ocr.png','stdout','-l','jpn','--psm','11','tsv'],text=True)),delimiter='\t')
    heading = next((w for w in words if '開発' in w['text']), None)
    if heading:
        break
else:
    raise AssertionError('stream heading not visible after scrolling to the beginning')
move(x+350+int(heading['left'])//2+2, y+48+(int(heading['top'])+int(heading['height'])//2)//2)
pointer.NotifyPointerButton(272, True)
pointer.NotifyPointerMotionRelative(56.,0.)
time.sleep(.12)
pointer.NotifyPointerButton(272, False)
a = snap('stream-start')
time.sleep(2)
b = snap('stream-later')
assert ImageChops.difference(a,b).crop((x+400,y+55,x+1140,y+570)).getbbox(), 'no visible stream update'
seat.NotifyKeyboardKeysym(0xffe3, True)
key(ord('c'))
seat.NotifyKeyboardKeysym(0xffe3, False)
time.sleep(.3)  # let the compositor see the new selection before reading it back
copied = subprocess.check_output(['wl-paste','--no-newline'],text=True,timeout=2)
assert copied.strip() and copied in '開発用の会話' and copied != '開発用の会話', 'partial selection or copy focus lost during streaming'
# On the mock PC, a second RC client steers the running answer. The mock appends this
# user entry before message_end(assistant); no focus changes in the desktop window.
external = [['demo', 'PC-side interruption']]
with socket.socket(socket.AF_UNIX) as peer:
    peer.settimeout(6)
    peer.connect(str(Path.home()/'lab/run/demo.sock'))
    with peer.makefile('rb') as incoming:
        peer.sendall((json.dumps(dict(id='pc-steer',method='prompt',params=dict(text=external[0][1],deliverAs='steer')))+'\n').encode())
        while (reply := json.loads(incoming.readline())).get('id') != 'pc-steer':
            pass  # skip interleaved stream events
        assert reply['ok'] and reply['result']['deliveredAs'] == 'steer'
# Only switch the isolated GNOME settings. The test namespace and temporary HOME own dconf.
# Switch twice while updates and cursor/selection rendering can re-enter the platform.
for _ in range(2):
    for scheme, name, rgb in [('prefer-dark', 'dark', (30,35,38)), ('default', 'light', (241,243,244))]:
        subprocess.run(['gsettings','set','org.gnome.desktop.interface','color-scheme',scheme],check=True)
        deadline = time.monotonic() + 5
        while snap(name).getpixel((x+12,y+600)) != rgb:
            assert time.monotonic() < deadline, f'desktop did not follow GNOME {name} appearance'
            time.sleep(.2)
# message_end precedes the idle status. Wait for the actual rendered header
# and answer (not mock time or a fixed delay); never scroll the selection away.
deadline = time.monotonic() + 25
while time.monotonic() < deadline:
    image = snap('committed-row')
    image.crop((x+800,y+8,x+1170,y+44)).resize((1110,108)).save('header.png')
    status = subprocess.check_output(['tesseract','header.png','stdout','-l','jpn','--psm','11'],text=True)
    if '待機' in ''.join(status.split()):
        image.crop((x+400,y+55,x+1140,y+570)).save('transcript.png')
        painted = subprocess.check_output(['tesseract','transcript.png','stdout','--psm','11'],text=True)
        if any(marker in painted for marker in ('Android', 'greet', 'shout', 'PiMate')):
            break
    time.sleep(.3)
else:
    raise AssertionError('settled header and committed answer not painted')
assert prompts() == external
# Discard the old clipboard contents so losing focus can't give a false positive.
subprocess.run(['wl-copy', 'sentinel'],check=True)
seat.NotifyKeyboardKeysym(0xffe3, True)
key(ord('c'))
seat.NotifyKeyboardKeysym(0xffe3, False)
time.sleep(.3)
assert subprocess.check_output(['wl-paste','--no-newline'],text=True,timeout=2) == copied, 'selection lost at interrupted message_end'
empty_editor = snap('empty-editor')
editor = (x+700, y+624)
click(*editor)
if mode == 'hotplug':
    # Really losing the keyboard must cancel the marked 'か', not carry it into the next draft.
    key(0xff26)
    for char in 'ka':
        key(ord(char))
    marked = snap('before-keyboard-loss')
    seat.NotifyKeyboardKeysym(0xffe1, True)  # remove with Shift held
    time.sleep(.15)
    seat.Stop()
    time.sleep(.5)
    cleared = snap('after-keyboard-loss')
    # Compare the placeholder glyphs, excluding the caret, pointer and input border.
    placeholder = (x+444, y+610, x+510, y+632)
    assert ImageChops.difference(empty_editor, marked).crop(placeholder).getbbox()
    assert not ImageChops.difference(empty_editor, cleared).crop(placeholder).getbbox(), 'marked text remained after keyboard loss'
    # Without a keyboard, the vertical wheel must not be interpreted as Shift+wheel.
    pointer.NotifyPointerMotionRelative(0., -280.)
    time.sleep(.2)
    area = (x+400, y+60, x+1140, y+560)
    views = [snap('wheel-start').crop(area)]
    for step in (-6, 6):
        pointer.NotifyPointerAxisDiscrete(0, step)
        time.sleep(.4)
        views.append(snap(f'wheel-{step}').crop(area))
    assert any(ImageChops.difference(views[0], v).getbbox() for v in views[1:]), 'stuck Shift after keyboard loss'
    seat = session()
    key(0xffe1)
    key(0xff1b)  # dismiss any candidate restored by the input method itself
    click(*editor)
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
time.sleep(1)
assert prompts() == external, 'conversion Enter sent a prompt'
key(0xff0d)
time.sleep(4)
assert prompts() == external + [['demo', 'ニホンゴ']], 'Enter did not send the committed text exactly once'
snap('sent')
print(f'{mode}: conversion Enter: 0 prompts; committed Enter: 1 prompt = ニホンゴ')
if mode == 'check':
    source = subprocess.Popen([sys.executable, __file__, 'drag-source'])
    try:
        deadline = time.monotonic() + 5
        while True:
            try:
                sx, sy, _ = blob(snap('drop-source'), (195, 29, 104)); break
            except AssertionError:
                if time.monotonic() >= deadline: raise
        move(sx, sy); pointer.NotifyPointerButton(272, True)
        for _ in range(12):
            pointer.NotifyPointerMotionRelative((editor[0]-sx)/12., (editor[1]-sy)/12.); time.sleep(.05)
        pointer.NotifyPointerButton(272, False)
    finally:
        source.terminate(); source.wait(timeout=3)
    deadline = time.monotonic() + 5
    while True:
        crop = snap('dropped-image').crop((x+320, y+495, x+1130, y+695))
        if sum(n for n,(r,g,b) in crop.getcolors(crop.width*crop.height) if abs(r-26)+abs(g-192)+abs(b-107)<10) > 150: break
        assert time.monotonic() < deadline, 'dropped PNG did not appear in the composer'
    assert prompts() == external + [['demo', 'ニホンゴ']], 'drop must not send'
    print('check: external Wayland drop attached one PNG without sending')
    # Exercise the actual wl_data_offer pipe (GPUI unit tests use an in-memory clipboard).
    Image.new('RGB', (96, 96), (201, 41, 117)).save('clipboard.png')
    with open('clipboard.png', 'rb') as payload:
        subprocess.run(['wl-copy', '--type', 'image/png'], stdin=payload, check=True)
    click(*editor)
    seat.NotifyKeyboardKeysym(0xffe3, True); key(ord('v')); seat.NotifyKeyboardKeysym(0xffe3, False)
    deadline = time.monotonic() + 5
    while True:
        crop = snap('pasted-image').crop((x+320, y+495, x+1130, y+695))
        if sum(n for n,(r,g,b) in crop.getcolors(crop.width*crop.height) if abs(r-201)+abs(g-41)+abs(b-117)<10) > 150: break
        assert time.monotonic() < deadline, 'clipboard PNG did not appear in the composer'
    assert prompts() == external + [['demo', 'ニホンゴ']], 'paste must not send'
    print('check: external clipboard PNG attached')
seat.Stop()
