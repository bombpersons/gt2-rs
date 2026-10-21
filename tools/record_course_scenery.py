#!/usr/bin/env python3
"""Record original scenery matrices; no native-port code produces expected data.

Requires the existing B-1 checkpoint and the psx CLI. Replays are two fields,
with explicit pokes into scenery instance 1 for the two rotated cases.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--checkpoint", default=".psx/runs/b1-state1200/state.bin")
parser.add_argument("--out", default=".psx/runs/course-scenery-evidence")
parser.add_argument("--fixture", default="game/tests/data/original/course/scenery_transforms.json")
args = parser.parse_args()
cases = [
    ("identity", [0, 0, 0], [4096, 4096, 4096]),
    ("rotation_regression", [84, -1173, 213], [3907, 3295, 4055]),
    ("rotation_holdout", [-67, 731, -317], [4055, 4096, 3907]),
]
records = []
for name, angles, scales in cases:
    output = Path(args.out) / name
    command = ["psx", "run", "--from-state", args.checkpoint, "--frames", "2"]
    for offset, values in [(0, angles), (8, scales)]:
        for axis, value in enumerate(values):
            command += ["--poke", f"{0x800B4BF0 + offset + axis * 2:08X}={value}:2"]
    command += ["--trace", "--trace-range", "8001F874-8001F8C0",
                "--trace-range", "80081428-80081478",
                "--trace-range", "8007BA24-8007BA70",
                "--max-events", "20000", "--out", str(output), "--summary"]
    subprocess.run(command, check=True)
    report = json.loads((output / "report.json").read_text())
    assert report["status"] == "complete", report
    matrices, matrix = [], {}
    for line in (output / "trace.txt").read_text().splitlines():
        if "8001f874 " in line:
            matrix = {}
        match = re.search(r"\[(1f8000[9a][0-9a-f])\]=([0-9a-f]{4})\b", line)
        if match:
            address, value = (int(v, 16) for v in match.groups())
            matrix[address] = value if value < 32768 else value - 65536
        if "8001f8bc " in line and matrix:
            matrices.append([matrix[0x1F80009C + i * 2] for i in range(9)])
    assert len(matrices) == 13, matrices
    records.append(dict(name=name, list=0, instance=1, angles=angles, scales=scales,
                        matrix=[matrices[1][i:i + 3] for i in range(0, 9, 3)],
                        command=command))
    ram = (output / "ram.bin").read_bytes()
    table = struct.unpack_from("<5120h", ram, 0x93150)
    import math
    # Validate the expression against the complete original table, including
    # the extra quarter turn addressed as cosine by the original function.
    assert all(value == int(math.sin(i * math.tau / 4096) * 4096 + 0.5)
               for i, value in enumerate(table))

fixture = Path(args.fixture)
fixture.parent.mkdir(parents=True, exist_ok=True)
fixture.write_text(json.dumps(dict(
    source="PSX B-1 checkpoint; matrix after 0x8001F8B4 scale call",
    checkpoint_sha256=hashlib.sha256(Path(args.checkpoint).read_bytes()).hexdigest(),
    sine_table_sha256=hashlib.sha256(ram[0x93150:0x95950]).hexdigest(),
    cases=records), indent=2) + "\n")
print(fixture)
