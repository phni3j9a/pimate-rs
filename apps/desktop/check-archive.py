#!/usr/bin/env python3
"""Check final archives as data; never run tag-built code in the publish job."""
import hashlib
import plistlib
import sys
import tarfile
import zipfile
from pathlib import Path


def verify(archive, version, receipts):
    if archive.suffix == '.zip':
        with zipfile.ZipFile(archive) as z:
            info = plistlib.loads(z.read('PiMate Desktop.app/Contents/Info.plist'))
        if (info['CFBundleVersion'], info['CFBundleShortVersionString']) != (version, version):
            raise ValueError(f'{archive}: bundle version mismatch')
    else:
        # Native runners record --version and SHA256 of the same executable before upload.
        root = archive.name.removesuffix('.tar.gz')
        arch = root.split('-')[-1]
        actual_version, digest = (receipts / f'{arch}.txt').read_text().splitlines()
        with tarfile.open(archive, 'r:gz') as tar:
            member = tar.getmember(f'{root}/bin/pimate-desktop')
            if not member.isfile() or actual_version != f'pimate-desktop {version}':
                raise ValueError(f'{archive}: desktop binary/version mismatch')
            with tar.extractfile(member) as binary:
                if hashlib.file_digest(binary, 'sha256').hexdigest() != digest:
                    raise ValueError(f'{archive}: different binary from --version check')


if __name__ == '__main__':
    version, receipts, *archives = sys.argv[1:]
    for path in archives:
        verify(Path(path), version, Path(receipts))
