#!/usr/bin/env python3
"""Compose only named UI test shots (never automatic diagnostics or input screens)."""
import json
from pathlib import Path
from PIL import Image

here = Path(__file__).resolve().parent
out = here / 'build/comparison'
out.mkdir(exist_ok=True)
for screen in ('Pair', 'Drawer', 'Main', 'Chat-Waiting'):
    paths = [here.parents[1] / 'docs/design/png' / f'{screen}.png']
    for variant in ('light', 'dark', 'large'):
        folder = here / 'build/attachments' / variant
        shot, = [a for test in json.loads((folder / 'manifest.json').read_text()) for a in test['attachments']
                 if a['suggestedHumanReadableName'].startswith(f'{screen}-{variant}_')]
        paths.append(folder / shot['exportedFileName'])
    canvas = Image.new('RGB', (4 * 390 + 3 * 4, 860), '#777777')
    for i, path in enumerate(paths):
        with Image.open(path) as image:
            image.thumbnail((390, 860), Image.Resampling.LANCZOS)
            canvas.paste(image.convert('RGB'), (394 * i + (390 - image.width) // 2, 0))
    canvas.save(out / f'{screen}.webp', quality=86, method=6)
