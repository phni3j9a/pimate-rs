"""Locate the same Markdown table header in every frame of the paused-drag interval."""
from pathlib import Path
import csv
import subprocess

import numpy as np
from numpy.lib.stride_tricks import sliding_window_view

ROOT = Path(__file__).resolve().parent
reference = None
for label in ['before', 'after']:
    video = ROOT / f'p2-{label}.mp4'
    time = subprocess.check_output([
        'ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries',
        'frame=best_effort_timestamp_time', '-of', 'csv=p=0', video,
    ], text=True)
    time = np.array([float(line.rstrip(',')) for line in time.splitlines() if line.strip(',')])
    pixels = subprocess.check_output([
        'ffmpeg', '-v', 'error', '-i', video, '-vf', 'format=gray,crop=116:1080:363:120',
        '-fps_mode', 'passthrough', '-enc_time_base', '1:1000000', '-pix_fmt', 'gray', '-f', 'rawvideo', '-',
    ])
    frames = np.frombuffer(pixels, np.uint8).reshape(-1, 1080, 116)
    assert len(frames) == len(time)
    if reference is None:
        # Crop the table's 「変更内容」 header from the BEFORE recording, at x=363, y=712.
        reference = frames[np.argmin(abs(time - 8.0)), 712-120:736-120].astype(float)
        assert reference.std() > 25
    with (ROOT / f'p2-{label}.csv').open('w') as output:
        writer = csv.writer(output, lineterminator='\n')
        writer.writerow(['seconds', 'y', 'mean_absolute_error'])
        for sec, frame in zip(time, frames):
            if not 8.0 <= sec <= 9.8:
                continue
            windows = sliding_window_view(frame, 24, axis=0).transpose(0, 2, 1)
            diff = np.abs(windows - reference).mean(axis=(1, 2))
            i = np.argmin(diff)
            writer.writerow([f'{sec:.6f}', i + 120, f'{diff[i]:.3f}'])
