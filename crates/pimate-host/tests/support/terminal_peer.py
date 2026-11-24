"""Exercise pimate attach as a real terminal user (including its controlling termios)."""
import json
import os
from pathlib import Path
import pty
import select
import signal
import socket
import stat
import subprocess
import sys
import termios
import time

host, root = sys.argv[1], Path.cwd()
name = 'pimate-12345678'
path = root / 'run/t' / name
enter, leave = b'\x1b[?1049h', b'\x1b[?1049l\x1b[?25h'


def listing():
    return json.loads(subprocess.check_output([host, 'attach'], timeout=3))


def until(test):
    end = time.monotonic() + 5
    while not test():
        assert time.monotonic() < end, 'condition timed out'
        time.sleep(.01)


def read_until(fd, token):
    data = b''
    end = time.monotonic() + 5
    while token not in data:
        assert select.select([fd], [], [], max(0, end - time.monotonic()))[0], (token, data)
        data += os.read(fd, 8192)
    return data


assert listing() == []
# A non-exec wrapper waits for a child that redraws *only* on SIGWINCH.
program = root / 'pi.py'
program.write_text('''import signal, sys
from pathlib import Path
def redraw(*_):
    with open('signals','a') as f: f.write('W')
    print('REDRAW', flush=True)
signal.signal(signal.SIGWINCH,redraw)
Path('ready').touch()
for line in sys.stdin:
    if line.strip()=='stop': sys.exit(42)
''')
r, w = os.pipe()
os.close(r)  # Ready notification fails deterministically, not a reason to kill Pi.
keeper = subprocess.Popen([host, '__hold', path, '/bin/sh', '-c', '"$1" "$2"; exit $?',
                           'wrapper', sys.executable, program], stdout=w)
os.close(w)
master, slave = pty.openpty()
saved = termios.tcgetattr(slave)
attach = None
try:
    until(lambda: (root / 'ready').exists())
    assert keeper.poll() is None
    assert stat.S_IMODE(path.stat().st_mode) == 0o600
    for why in ('detach', 'signal', 'exit'):
        until(lambda: listing()[0]['attached'] is False)
        attach = subprocess.Popen([host, 'attach', name], stdin=slave, stdout=slave, stderr=slave)
        screen = read_until(master, b'REDRAW')
        assert enter in screen and termios.tcgetattr(slave) != saved
        count = (root / 'signals').read_bytes()
        assert listing()[0]['attached'] is True
        assert (root / 'signals').read_bytes() == count
        if why == 'detach':
            # A symlink replacing the name must not redirect permission changes at its target.
            victim = root / 'victim'
            victim.touch(mode=0o640)
            original_mode = victim.stat().st_mode
            parked = path.with_suffix('.saved')
            path.rename(parked)
            path.symlink_to(victim)
            os.write(master, b'\x1c')
        elif why == 'signal':
            attach.send_signal(signal.SIGTERM)
        else:
            os.write(master, b'stop\n')
        read_until(master, leave)
        assert attach.wait(5) == (42 if why == 'exit' else 0)
        assert termios.tcgetattr(slave) == saved
        if why == 'detach':
            assert victim.stat().st_mode == original_mode
            path.unlink()
            parked.rename(path)
    assert keeper.wait(5) == 42 and not path.exists()
    # Unexpected EOF after the handshake is an error, not a clean detach.
    with socket.socket(socket.AF_UNIX) as listener:
        listener.bind(str(path))
        listener.listen(1)
        listener.settimeout(5)
        attach = subprocess.Popen([host, 'attach', name], stdin=slave, stdout=slave, stderr=slave)
        with listener.accept()[0] as conn:
            assert conn.recv(4, socket.MSG_WAITALL) == b'PMT2'
            conn.sendall(b'PMT2')
            read_until(master, enter)
        read_until(master, leave)
        assert attach.wait(5) != 0 and termios.tcgetattr(slave) == saved
finally:
    for child in (attach, keeper):
        if child is not None and child.poll() is None:
            child.kill()
            child.wait()
    os.close(master)
    os.close(slave)
