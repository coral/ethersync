#!/usr/bin/env python3
"""Measure native leader sampling cadence through a PTY, excluding screen/compositor latency.
Run `cargo build -p libethersync --example leader` first. Uses only Python's standard library.
"""
import fcntl
import json
import os
import pty
import re
import select
import struct
import subprocess
import sys
import time
import datetime
import math
import termios
from pathlib import Path

root = Path(__file__).resolve().parent.parent
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 150, 0, 0))
process = subprocess.Popen(
    [str(root / "target/debug/examples/leader"), "--bind", "127.0.0.1:0", "--no-mdns", "--seconds", "5"],
    stdin=slave, stdout=slave, stderr=slave,
    env={**os.environ, "TERM": "xterm-256color"},
)
os.close(slave)
samples = []
ages = []
tod = "--tod" in sys.argv
buffer = ""
output_bytes = bytearray()
started = False
try:
    while process.poll() is None:
        if not select.select([master], [], [], 0.02)[0]:
            continue
        try:
            payload = os.read(master, 65536)
            received_wall = time.time()
            output_bytes.extend(payload)
            buffer += payload.decode(errors="replace")
        except OSError:
            break
        if not started and "ETHERSYNC / LEADER" in buffer:
            os.write(master, b"tod\r" if tod else b"play\r")
            started = True
        end = 0
        for match in re.finditer(r"frame ([-0-9]+\.[0-9]{3})", buffer):
            frames = float(match[1])
            if frames > 0:
                samples.append(frames)
                if tod:
                    wall = datetime.datetime.fromtimestamp(received_wall)
                    wall_ms = ((wall.hour * 60 + wall.minute) * 60 + wall.second) * 1000 + wall.microsecond / 1000
                    ages.append((wall_ms - frames / 30 * 1000 + 43_200_000) % 86_400_000 - 43_200_000)
            end = match.end()
        buffer = buffer[end:] if end else buffer[-4096:]
finally:
    if process.poll() is None:
        process.terminate()
    process.wait()
    os.close(master)

# Check complete drawing transactions, not just numeric samples. The shutdown path
# may emit an extra end marker defensively, so require paired ends for every begin.
sync_depth = 0
sync_frames = 0
for token in re.finditer(rb"\x1b\[\?2026([hl])", output_bytes):
    if token[1] == b"h":
        if sync_depth: raise RuntimeError("nested/incomplete terminal update")
        sync_depth = 1
    elif sync_depth:
        sync_depth = 0
        sync_frames += 1
if sync_depth: raise RuntimeError("terminal output ended inside a synchronized update")

intervals = sorted((b - a) / 30 * 1000 for a, b in zip(samples, samples[1:]) if b > a)
if len(intervals) < 100:
    raise RuntimeError(f"insufficient terminal samples: {len(intervals)}; check PTY availability")
late = sorted((b - math.floor(b)) / 30 * 1000 for a,b in zip(samples,samples[1:]) if math.floor(b) > math.floor(a))
leader_checks = sorted(float(m[1]) for m in re.finditer(rb"TOD check ([+-][0-9.]+) ms", output_bytes) if abs(float(m[1])) < 100) if tod else []
print(json.dumps({
    "samples": len(samples),
    "synchronizedOutputFrames": sync_frames,
    "leaderSameInstantTodDifferenceMs": None if not leader_checks else {"median": leader_checks[len(leader_checks)//2], "min": min(leader_checks), "max": max(leader_checks)},
    "samplingIntervalMs": {
        "median": intervals[len(intervals) // 2],
        "p95": intervals[int(len(intervals) * 0.95)],
        "max": max(intervals),
    },
    "frameChangeSampleLatenessMs": {"median": late[len(late)//2], "p95": late[int(len(late)*.95)], "max": max(late)} if late else None,
    "todSampleAgeAtPtyMs": None if not ages else {"median": sorted(ages)[len(ages)//2], "p95": sorted(ages)[int(len(ages)*.95)], "max": max(ages)},
    "note": "Derived from 30fps unwrapped positions, precision about 0.033ms. Excludes compositor latency.",
}, indent=2))
