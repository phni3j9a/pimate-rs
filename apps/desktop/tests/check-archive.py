"""A correct filename must not mask a stale plist or an unverified executable."""
import hashlib
import io
import plistlib
import runpy
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

verify = runpy.run_path(str(Path(__file__).resolve().parents[1] / 'check-archive.py'))['verify']


class ArchiveTest(unittest.TestCase):
    def test_bundle(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'PiMate-Desktop-0.3.1-macos-arm64.zip'
            for build, short in [('0.3.1', '0.3.1'), ('1', '0.3.1'), ('0.3.1', '0.1.0')]:
                with zipfile.ZipFile(path, 'w') as z:
                    z.writestr('PiMate Desktop.app/Contents/Info.plist', plistlib.dumps(
                        dict(CFBundleVersion=build, CFBundleShortVersionString=short)))
                if build == short:
                    verify(path, '0.3.1', None)
                else:
                    with self.assertRaises(ValueError):
                        verify(path, '0.3.1', None)

    def test_linux(self):
        with tempfile.TemporaryDirectory() as temp:
            root = 'PiMate-Desktop-0.3.1-linux-aarch64'
            path = Path(temp) / (root + '.tar.gz')
            with tarfile.open(path, 'w:gz') as t:
                member = tarfile.TarInfo(root + '/bin/pimate-desktop'); member.size = 1
                t.addfile(member, io.BytesIO(b'x'))
            for version, binary in [('0.3.1', b'x'), ('0.1.0', b'x'), ('0.3.1', b'y')]:
                (path.parent / 'aarch64.txt').write_text(f'pimate-desktop {version}\n{hashlib.sha256(binary).hexdigest()}\n')
                if version == '0.3.1' and binary == b'x':
                    verify(path, '0.3.1', path.parent)
                else:
                    with self.assertRaises(ValueError):
                        verify(path, '0.3.1', path.parent)


if __name__ == '__main__':
    unittest.main()
