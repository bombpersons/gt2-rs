#!/usr/bin/env python3
"""Record original flat-textured road-quad refinement trees with psx.

Only numeric renderer inputs/decisions are exported; raw traces stay in .psx/.
The expected values come from CPU loads/stores, not a replacement renderer.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--checkpoint", default=".psx/runs/b1-state1200/state.bin")
parser.add_argument("--name", default="bumper")
args = parser.parse_args()
run = Path(".psx/runs")/f"subdivision-{args.name}"
command = ["psx","run","--from-state",args.checkpoint,"--frames","2","--trace"]
for region in ["80025100-8002512C","800251B8-80025368","80025438-80025488","80025528-800255F8"]:
    command += ["--trace-range",region]
command += ["--max-events","300000","--out",str(run),"--summary"]
subprocess.run(command,check=True)
report = json.loads((run/"report.json").read_text())
assert report["status"] == "complete" and report["frames"]["reached"] == 2
assert report["trace"]["lines_dropped"] == 0, "refuse a truncated reference trace"
roots, root, node = [], None, None
pending_leaf, pending_stitch = {}, {}
def packet(values, corners):
    signed = lambda v: v if v < 32768 else v-65536
    return dict(pixels=[[signed(values[8+i*8]&65535),signed(values[8+i*8]>>16)] for i in range(corners)],
                uv=[[values[12+i*8]&255,(values[12+i*8]>>8)&255] for i in range(corners)])
position_pcs = [0x800251CC,0x800251D0,0x800251D4,0x800251E4,
                0x80025218,0x8002521C,0x80025220,0x80025230,
                0x80025264,0x80025268,0x8002526C,0x8002527C]
uv_pcs = [0x800252C0,0x800252C4,0x800252C8,0x800252D8,
          0x8002530C,0x80025310,0x80025314,0x80025324]
for line in (run/"trace.txt").read_text().splitlines():
    match = re.match(r"(\d+) \d+ ([0-9a-f]{8}).*; \[([0-9a-f]{8})\](=>|=)([0-9a-f]+)",line)
    if not match: continue
    frame,pc,address,_,value = match.groups()
    pc,address,value = (int(v,16) for v in [pc,address,value])
    if pc == 0x80025104:
        root = dict(source_address=f"0x{value:08X}", nodes=[], leaves=0, stitches=0, leaf_packets=[], stitch_packets=[])
        roots.append(root)
    elif pc == 0x800251BC:
        node = dict(depth=(address-0x1F8000E8)//96, positions=[[None]*3 for _ in range(4)], uv=[[None]*2 for _ in range(4)])
        root["nodes"].append(node)
    elif pc in position_pcs:
        index = position_pcs.index(pc)
        node["positions"][index%4][index//4] = value if value < 32768 else value-65536
    elif pc in uv_pcs:
        index = uv_pcs.index(pc)
        node["uv"][index%4][index//4] = value
    elif pc == 0x80025358: root["focal_px"] = value*2
    elif pc == 0x8002535C: root["width"] = value
    elif pc == 0x80025360: root["height"] = value
    elif pc == 0x80025444:
        stitch_base = address-8
        pending_stitch = {8:value}
    elif pc in [0x80025448,0x80025458,0x80025460,0x80025468,0x80025474]:
        pending_stitch[address-stitch_base] = value
    elif pc == 0x80025480:
        root["stitches"] += 1
        root["stitch_packets"].append(packet(pending_stitch,3))
    elif pc == 0x80025528:
        leaf_base = value
        pending_leaf = {}
    elif pc in [0x80025538,0x8002553C,0x8002557C,0x80025584,0x8002558C,0x80025590,0x80025598,0x800255A4]:
        pending_leaf[address-leaf_base] = value
    elif pc == 0x8002555C: root["max_twice_area_px2"] = value
    elif pc == 0x800255B8:
        root["leaves"] += 1
        root["leaf_packets"].append(packet(pending_leaf,4))
roots = [root for root in roots if root.get("width") == 320]
assert roots
for root in roots:
    assert root["nodes"][0]["depth"] == 0
    for node in root["nodes"]:
        assert all(v is not None for corner in node["positions"]+node["uv"] for v in corner)
target = Path("game/tests/data/original/subdivision")/f"{args.name}.json"
target.parent.mkdir(parents=True,exist_ok=True)
target.write_text(json.dumps(dict(command=command, checkpoint_sha256=hashlib.sha256(Path(args.checkpoint).read_bytes()).hexdigest(),
    build=report["build"], trace_lines=report["trace"]["lines"], trace_lines_dropped=report["trace"]["lines_dropped"],
    source="0x800251B8 split inputs; 0x800255B8 leaf commits; 0x80025480 stitch commits; main viewport only",roots=roots), indent=2)+"\n")
print(f"{target}: {len(roots)} roots, {sum(len(r['nodes']) for r in roots)} splits, {sum(r['leaves'] for r in roots)} leaves, {sum(r['stitches'] for r in roots)} stitches")
