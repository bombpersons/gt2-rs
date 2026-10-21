#!/usr/bin/env python3
"""Record the B-1 camera at its body-pose/control boundary using psx.

No native camera code produces expected values. Complete traces stay private;
only numeric body inputs, effective controls and original camera poses are
exported. Run the holdout after implementing the rules.
"""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--checkpoint", default=".psx/runs/b1-state1200/state.bin")
parser.add_argument("--scenario", choices=["coast", "views", "turn", "holdout"], default="coast")
parser.add_argument("--frames", type=int, default=96)
args = parser.parse_args()
scenarios = {
    "coast": [],
    "views": [{"frame":2,"buttons":["R1"],"until":6}, {"frame":16,"buttons":["R1"],"until":20},
              {"frame":32,"buttons":["R1"],"until":36}, {"frame":44,"buttons":["L1"],"until":60}],
    "turn": [{"frame":0,"buttons":["CROSS","LEFT"],"until":30},
             {"frame":32,"buttons":["R1","CROSS","RIGHT"],"until":38},
             {"frame":38,"buttons":["CROSS","RIGHT"],"until":64},
             {"frame":66,"buttons":["L1","CROSS"],"until":82}],
    "holdout": [{"frame":0,"buttons":["R1","CROSS","RIGHT"],"until":8},
                {"frame":8,"buttons":["CROSS","LEFT"],"until":24},
                {"frame":26,"buttons":["R1","SQUARE","L1"],"until":34},
                {"frame":38,"buttons":["CROSS","LEFT"],"until":62},
                {"frame":64,"buttons":["R1","CROSS"],"until":72}],
}
run = Path(".psx/runs") / f"camera-{args.scenario}"
command = ["psx","run","--from-state",args.checkpoint,"--frames",str(args.frames),
           "--input",json.dumps(scenarios[args.scenario]),
           "--watch-reads","800A9DEA-800A9DF4", "--watch-reads","800A9EA4-800A9EC4",
           "--watch-reads","801FFA8A:1", "--watch","801FFA20:32",
           "--trace","--trace-range","800102F4-80010344",
           "--max-events","30000","--out",str(run),"--summary"]
subprocess.run(command, check=True)
report = json.loads((run/"report.json").read_text())
assert report["status"] == "complete", report
reads = defaultdict(dict)
for line in (run/"reads.jsonl").read_text().splitlines():
    event = json.loads(line)
    pc = event["pc"]
    frame = event["run_frame"]
    if pc in [0x8001023C,0x80010240,0x80010244,0x80010248,0x8001025C,0x80010260,0x80010264,0x80010268]:
        reads[frame][event["address"]-0x800A9EA4] = event["value"]
    elif pc in [0x80010484,0x80010494]:
        reads[frame][pc] = event["value"]
    elif pc == 0x8001036C:
        reads[frame]["mode"] = event["value"]
controls = defaultdict(dict)
for line in (run/"trace.txt").read_text().splitlines():
    match = re.search(r"^(\d+) \d+ (800102fc|80010300).*=>\s*([0-9a-f]+)",line)
    if match:
        frame, pc, value = match.groups()
        controls[int(frame)-report["frames"]["start"]][pc] = int(value,16)
initial = Path(args.checkpoint).with_name("ram.bin").read_bytes()
pose = bytearray(initial[0x1FFA20:0x1FFA40])
writes = defaultdict(list)
for line in (run/"writes.jsonl").read_text().splitlines():
    event = json.loads(line)
    if 0x801FFA20 <= event["address"] < 0x801FFA40:
        writes[event["run_frame"]].append(event)
rows = []
for frame in sorted(reads):
    data = reads[frame]
    if 0 not in data: continue
    body = b"".join(data[i].to_bytes(4,"little") for i in range(0,32,4))
    for event in writes[frame]:
        offset = event["address"]-0x801FFA20
        pose[offset:offset+event["width"]] = event["value"].to_bytes(event["width"],"little")
    signed = lambda value: value if value < 32768 else value-65536
    rows.append(dict(run_frame=frame, mode=data["mode"],
        cycle=bool(controls[frame]["80010300"] & 0x100), rear=bool(controls[frame]["800102fc"] & 0x200),
        body_matrix=struct.unpack_from("<9h",body), body_position=struct.unpack_from("<3i",body,20),
        yaw_correction=signed(data.get(0x80010484,0)), pitch_correction=signed(data.get(0x80010494,0)),
        matrix=struct.unpack_from("<9h",pose), position=struct.unpack_from("<3i",pose,20)))
assert len(rows) == args.frames//2, (len(rows),args.frames)
target = Path("game/tests/data/original/camera")/f"{args.scenario}.json"
target.parent.mkdir(parents=True,exist_ok=True)
target.write_text(json.dumps(dict(command=command, checkpoint_sha256=hashlib.sha256(Path(args.checkpoint).read_bytes()).hexdigest(),
    source="0x800103C0 ordinary single-player camera, effective pad/body pose boundary", rows=rows), indent=2)+"\n")
print(target)
