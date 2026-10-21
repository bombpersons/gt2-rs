#!/usr/bin/env python3
"""Record original road-node parent texture selections, before refinement.

The original's actual material load address is the selection oracle. Projected
corners and transformed coordinates come from CPU loads. Raw traces remain in
.psx/runs; fixtures contain numeric renderer evidence only.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess


PATHS = [
    dict(kind="flat_triangle", group=4, start=0x80024468, word0=0x8002446C,
         word1=0x80024470, flags=[0x80024498, 0x8002449C, 0x800244A0],
         pixels=[0x800244DC, 0x800244E0, 0x800244E4],
         xy=[0x800245D8, 0x800245DC, 0x800245E0],
         threshold=0x800245B4, selected=[0x800245CC, 0x800245D0, 0x800245D4]),
    dict(kind="flat_quad", group=5, start=0x80024CD0, word0=0x80024CD4,
         word1=0x80024CF0, flags=[0x80024D08, 0x80024D20, 0x80024D38, 0x80024D50],
         pixels=[0x80024D88, 0x80024DA0, 0x80024DC8, 0x80024DE4],
         xy=[0x80024E00, 0x80024E20, 0x80024E38, 0x80024E50],
         threshold=0x80024E88, selected=[0x80024EB8, 0x80024EBC, 0x80024EC0]),
    dict(kind="gouraud_triangle", group=6, start=0x8002570C, word0=0x80025710,
         word1=0x80025714, flags=[0x8002573C, 0x80025740, 0x80025744],
         pixels=[0x80025780, 0x80025784, 0x80025788],
         xy=[0x80025894, 0x80025898, 0x8002589C],
         threshold=0x8002586C, selected=[0x80025888, 0x8002588C, 0x80025890]),
    dict(kind="gouraud_quad", group=7, start=0x8002604C, word0=0x80026050,
         word1=0x8002606C, flags=[0x80026084, 0x8002609C, 0x800260B4, 0x800260CC],
         pixels=[0x80026104, 0x8002611C, 0x80026144, 0x80026160],
         xy=[0x8002617C, 0x800261E4, 0x80026200, 0x8002621C],
         threshold=0x80026220, selected=[0x80026250, 0x80026254, 0x80026258]),
]


def signed16(value):
    value &= 65535
    return value if value < 32768 else value - 65536


def pair(value):
    return [signed16(value), signed16(value >> 16)]


def cross(a, b, c):
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def record(args):
    run = Path(".psx/runs") / f"material-variants-{args.name}"
    command = ["psx", "run", "--from-state", args.checkpoint, "--frames", "2", "--trace"]
    for region in ["80024468-800245F0", "80024CD0-80024ECC",
                   "8002570C-800258AC", "8002604C-80026260",
                   "8007B480-8007B48C"]:
        command += ["--trace-range", region]
    command += ["--max-events", "300000", "--out", str(run), "--summary"]
    subprocess.run(command, check=True)
    report = json.loads((run / "report.json").read_text())
    assert report["status"] == "complete" and report["frames"]["reached"] == 2
    assert report["trace"]["lines_dropped"] == 0, "refuse a truncated reference trace"
    ram = (run / "ram.bin").read_bytes()
    records, current, path, viewport = [], None, None, {}

    def finish():
        if current is None or "selected_address" not in current:
            return
        assert all(p is not None and all(v is not None for v in p)
                   for p in current["positions"] + current["pixels"])
        pixels = current["pixels"]
        # The original feeds triangle NCLIP in 0,2,1 order. For quads the
        # two NCLIPs are 1,2,0 and 2,0,3, with the first negated.
        area = cross(pixels[0], pixels[2], pixels[1]) if len(pixels) == 3 else (
            -cross(pixels[1], pixels[2], pixels[0]) + cross(pixels[2], pixels[0], pixels[3]))
        unsigned_area = area & 0xFFFFFFFF if len(pixels) == 3 else abs(area)
        current["signed_twice_area_px2"] = area
        current["selector_area_u32"] = unsigned_area
        base = current.pop("template_base")
        selected = current.pop("selected_address")
        current["template_address"] = f"0x{base:08X}"
        current["selected_variant"] = (selected - base) // 16
        assert selected - base in (0, 16)
        assert current["selected_variant"] == int(unsigned_area <= current["threshold"])
        variants = []
        for address in [base, base + 16]:
            words = struct.unpack_from("<III", ram, address & 0x1FFFFF)
            uv = [[words[i] & 255, (words[i] >> 8) & 255] for i in range(3)]
            if len(pixels) == 4:
                uv.append([(words[2] >> 16) & 255, words[2] >> 24])
            variants.append(dict(uv=uv, palette=words[0] >> 16, page=words[1] >> 16))
        assert current.pop("selected_words") == list(struct.unpack_from("<III", ram, selected & 0x1FFFFF))
        current["variants"] = variants
        current["projection_failed"] = any(flag & 32 for flag in current["flags"])
        current["double_sided"] = not bool(current["source_word0"] & 0x80000000)
        records.append(current.copy())

    for line in (run / "trace.txt").read_text().splitlines():
        match = re.match(r"(\d+) \d+ ([0-9a-f]{8}).*; \[([0-9a-f]{8})\](=>|=)([0-9a-f]+)", line)
        if not match:
            continue
        frame, pc, address, _, value = match.groups()
        pc, address, value = [int(x, 16) for x in [pc, address, value]]
        if pc in [0x8007B480, 0x8007B484, 0x8007B488]:
            viewport[{0x8007B480: "focal_px", 0x8007B484: "width", 0x8007B488: "height"}[pc]] = value
            continue
        for candidate in PATHS:
            if pc == candidate["start"]:
                finish()
                path = candidate
                count = len(path["pixels"])
                current = dict(frame=int(frame), primitive=path["kind"], group=path["group"],
                               viewport=viewport.copy(), positions=[None] * count,
                               pixels=[None] * count, flags=[None] * count,
                               selected_words=[None] * 3)
                break
        if current is None:
            continue
        if pc == path["word0"]:
            current["source_word0"] = value
            if len(path["pixels"]) == 3:
                current["source_address"] = f"0x{address:08X}"
        if pc == path["word1"]:
            current["source_word1"] = value
            current["source_address"] = f"0x{address - 4:08X}"
        if pc in path["flags"]:
            i = path["flags"].index(pc)
            current["flags"][i] = value & 65535
            current["positions"][i] = [None, None, signed16(value >> 16)]
        if pc in path["pixels"]:
            current["pixels"][path["pixels"].index(pc)] = pair(value)
        if pc in path["xy"]:
            current["positions"][path["xy"].index(pc)][:2] = pair(value)
        if pc == path["threshold"]:
            current["threshold"] = value
            current["template_base"] = address - 12
        if pc in path["selected"]:
            i = path["selected"].index(pc)
            current["selected_words"][i] = value
            if i == 0:
                current["selected_address"] = address
    finish()
    assert records
    target = Path("game/tests/data/original/material_variants") / f"{args.name}.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(dict(
        command=command, checkpoint_sha256=hashlib.sha256(Path(args.checkpoint).read_bytes()).hexdigest(),
        build=report["build"], trace_lines=report["trace"]["lines"],
        trace_lines_dropped=report["trace"]["lines_dropped"],
        source="Original road-node parent material loads (groups4..7); corners in compact source order; viewport includes mirror and main; selection precedes subdivision",
        records=records), indent=2) + "\n")
    counts = Counter((r["primitive"], r["viewport"].get("width"), r["selected_variant"]) for r in records)
    print(f"{target}: {len(records)} records; {dict(counts)}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkpoint", default=".psx/runs/b1-state1200/state.bin")
    parser.add_argument("--name", default="bumper")
    record(parser.parse_args())
