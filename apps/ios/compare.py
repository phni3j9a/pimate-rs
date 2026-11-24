#!/usr/bin/env python3
"""Compose only named UI test shots (never automatic diagnostics or input screens)."""
import argparse
import json
from pathlib import Path
from PIL import Image

SCREENS = ('Pair', 'Drawer', 'Main', 'Chat-Waiting', 'Chat-Attach', 'Chat-Edge', 'Attachments', 'Images', 'Viewer', 'Table', 'Commands', 'Sheet-Model', 'Sheet-Usage', 'Sheet-Session')


def captures(root, screen, partial):
    paths = []
    for variant in ('light', 'dark', 'large'):
        folder = root / variant
        manifest = folder / 'manifest.json'
        tests = json.loads(manifest.read_text()) if manifest.exists() else []
        shots = [a for test in tests for a in test['attachments']
                 if a['suggestedHumanReadableName'].startswith(f'{screen}-{variant}_')]
        if len(shots) != 1:
            if partial:
                return []
            raise ValueError(f'{screen}-{variant}: expected exactly one capture, found {len(shots)}')
        paths.append(folder / shots[0]['exportedFileName'])
    return paths


def compose(partial):
    here = Path(__file__).resolve().parent
    out = here / 'build/comparison'
    out.mkdir(exist_ok=True)
    for screen in SCREENS:
        paths = captures(here / 'build/attachments', screen, partial)
        if not paths:
            continue
        mock = here.parents[1] / 'docs/design/png' / f'{screen}.png'
        if mock.exists():
            paths.insert(0, mock)
        canvas = Image.new('RGB', (len(paths) * 394 - 4, 860), '#777777')
        for i, path in enumerate(paths):
            with Image.open(path) as image:
                image.thumbnail((390, 860), Image.Resampling.LANCZOS)
                canvas.paste(image.convert('RGB'), (394 * i + (390 - image.width) // 2, 0))
        canvas.save(out / f'{screen}.webp', quality=86, method=6)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--partial', action='store_true', help='recover complete comparisons from a failed UI test run')
    compose(parser.parse_args().partial)
