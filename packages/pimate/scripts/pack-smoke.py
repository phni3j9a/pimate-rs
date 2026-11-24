"""Exercise an actual tarball, never the user's Pi/npm/service installation."""
import hashlib
import json
import os
import pty
import re
import select
import shutil
import signal
import subprocess as sp
import sys
import tempfile
import threading
from contextlib import ExitStack
from pathlib import Path
from urllib.parse import urlsplit, parse_qs

pkg = Path(__file__).resolve().parents[1]
tarball = Path(sys.argv[1]).resolve(strict=True)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


def stop(child):
    try:
        os.killpg(child.pid, signal.SIGTERM)
        child.wait(timeout=5)
    except sp.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait()
    except ProcessLookupError:
        child.wait()


def smoke(lab, stack):
    os.chdir(lab)
    # Don't inherit service, credential, Pi, npm, or Host overrides from the developer.
    env = dict(PATH=f'{lab}/mock:{os.environ["PATH"]}', HOME=str(lab), TERM='xterm-256color',
               TMPDIR=str(lab), XDG_CONFIG_HOME=f'{lab}/xdg', PI_CODING_AGENT_DIR=f'{lab}/agent',
               npm_config_prefix=f'{lab}/prefix', npm_config_cache=f'{lab}/cache', PI_OFFLINE='1',
               PIMATE_RUN_DIR=f'{lab}/run', PIMATE_CONFIG_DIR=f'{lab}/config', PIMATE_SERVICE_PORT='47801',
               PIMATE_ORIGIN='mobile', PIMATE_TRUST='no', PIMATE_E2E='1', PIMATE_INSTANCE_ID='pack-smoke')
    Path('mock').mkdir()
    for name in ['systemctl', 'systemd-run', 'loginctl', 'launchctl', 'tailscale']:
        Path('mock', name).symlink_to('/usr/bin/false')

    def run(*args, timeout=60, **opts):
        return sp.run(args, env=env, check=True, timeout=timeout, stdout=sp.PIPE, text=True, **opts).stdout

    def start(*args, **stdio):
        child = sp.Popen(args, env=env, start_new_session=True, **stdio)
        stack.callback(stop, child)
        return child

    Path('package.json').write_text('{"private":true,"type":"module"}')
    run('npm', 'install', '--ignore-scripts', '--legacy-peer-deps', str(tarball),
        '@earendil-works/pi-coding-agent@1.6.0', timeout=240)
    installed = lab / 'node_modules/@phni3j9a/pimate'
    manifest = json.loads((installed / 'package.json').read_text())
    sums = json.loads((installed / 'bin/SHA256SUMS.json').read_text())
    assert len(sums) == 4
    for target in ['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64']:
        path = f'bin/{target}/pimate'
        assert digest(installed / path) == sums[path] and os.access(installed / path, os.X_OK)
    pi = lab / 'node_modules/.bin/pi'
    cli = lab / 'node_modules/.bin/pimate'
    run(pi, 'install', installed)
    assert 'pimate' in run(pi, 'list')
    run(cli, 'service', 'command-install')
    target = run('node', '-p', 'process.platform+"-"+process.arch').strip()
    linked = lab / '.local/bin/pimate'
    assert linked.resolve(strict=True) == installed / f'bin/{target}/pimate'
    host = start(cli, 'serve', '--listen', '127.0.0.1', '--port', '0', stdout=sp.PIPE)
    assert select.select([host.stdout], [], [], 15)[0], 'Host startup timed out'
    addr = re.fullmatch(rb'Listening http://127\.0\.0\.1:(\d+)\n', host.stdout.readline())
    assert addr and addr[1] != b'47800'
    port = addr[1].decode()
    env['PIMATE_SERVICE_PORT'] = port
    health = json.loads(run(linked, 'service', 'health'))
    assert health['pid'] == host.pid and health['version'] == run(cli, '--version').strip() == manifest['version']
    pair = json.loads(run(cli, 'pair', '--json', '--port', port))
    link = urlsplit(pair['link'])
    query = parse_qs(link.query)
    assert (link.scheme, link.netloc) == ('pimate', 'pair') and query['h'] == [f'127.0.0.1:{port}']

    shutil.copy(pkg / 'test/fixtures/faux.ts', 'faux.ts')
    master, slave = pty.openpty()
    stack.callback(os.close, master)
    try:
        start(pi, '--no-approve', '--no-mcp', '--no-skills', '--no-context-files', '--no-prompt-templates',
              '--no-themes', '-e', './faux.ts', '--model', 'pimate-faux/scripted', '--thinking', 'off',
              stdin=slave, stdout=slave, stderr=slave)
    finally:
        os.close(slave)

    def drain():
        try:
            while os.read(master, 65536):
                pass
        except OSError:
            pass

    threading.Thread(target=drain, daemon=True).start()
    # Observe discovery through the running Host, not just the CLI's filesystem scan.
    run('node', str(pkg / 'scripts/verify-registration.mjs'), port, input=query['c'][0], timeout=45)
    answer = json.loads(run(cli, 'prompt', 'pack-smoke', 'hello', '--wait', '--timeout', '20'))
    assert answer['reply'] == 'Hello from the PiMate faux model.'
    print('PASS: tarball hashes, isolated Pi install, both CLI paths, Host version, pairing, discovery and faux reply')


with tempfile.TemporaryDirectory(prefix='pm-', dir='/tmp') as lab:
    # A separate Pi is allowed to edit real settings while this test runs.
    settings = Path(lab) / '.pi/agent/settings.json'
    settings.parent.mkdir(parents=True)
    settings.write_text('{"packages":[]}\n')
    before = digest(settings)
    try:
        with ExitStack() as stack:
            smoke(Path(lab), stack)
    finally:
        assert digest(settings) == before, 'Pi wrote outside its isolated agent dir'
