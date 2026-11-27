"""python3 track.py (NumPy, ffmpeg, ffprobe). Run adjacent to both 720×1440 videos."""
import csv
from pathlib import Path
import subprocess

import numpy as np
from numpy.lib.stride_tricks import sliding_window_view

ROOT = Path(__file__).resolve().parent


def decode(name):
    video = ROOT / f'{name}-stream.mp4'
    timestamps = subprocess.check_output([
        'ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries',
        'frame=best_effort_timestamp_time', '-of', 'csv=p=0', video,
    ], text=True)
    time = np.array([float(line.rstrip(',')) for line in timestamps.splitlines() if line.strip(',')])
    raw = subprocess.check_output([
        'ffmpeg', '-v', 'error', '-i', video, '-vf', 'format=gray,crop=24:1440:69:0',
        '-fps_mode', 'passthrough', '-enc_time_base', '1:1000000', '-f', 'rawvideo', '-pix_fmt', 'gray', '-',
    ])
    frames = np.frombuffer(raw, np.uint8).reshape(-1, 1440, 24)
    assert len(time) == len(frames)
    return time, frames


before = decode('before')
# Issue #99's stationary copy icon at x=69, y=1100 in the first of its three frames.
reference = before[1][np.argmin(abs(before[0] - 10.101811)), 1100:1108].astype(float)
for label, (times, frames) in [('before', before), ('after', decode('after'))]:
    with (ROOT / f'{label}-positions.csv').open('w') as output:
        writer = csv.writer(output, lineterminator='\n')
        writer.writerow(['seconds', 'y', 'mean_absolute_error'])
        for timestamp, frame in zip(times, frames):
            windows = sliding_window_view(frame[650:1300], 8, axis=0).transpose(0, 2, 1)
            error = np.abs(windows - reference).mean(axis=(1, 2))
            i = np.argmin(error)
            writer.writerow([f'{timestamp:.6f}', 650 + i, f'{error[i]:.3f}'])
