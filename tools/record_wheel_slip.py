#!/usr/bin/env python3
"""Record independent executions of the original wheel-slip preprocessing.

Each checkpoint intervention supplies four local velocity pairs. Read watches
prove the consumed inputs; write watches collect only 80039DE8's outputs.
No reference-model predictions are exported. Run the holdout after writing
the native rule: python3 tools/record_wheel_slip.py --holdout
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import struct
import subprocess

BASE = 0x800A96B4
STATE = Path(".psx/runs/b1-state1200/state.bin")
OUTPUT = Path("game/tests/data/original/wheel_slip")


def export_tuning(path):
    """Compress the original mathematical lookup; no fitted trig coefficients."""
    ram = STATE.with_name("ram.bin").read_bytes()
    original = ram[0xA4AC8:0xA4AC8 + 8194]
    table = struct.unpack("<4097h", original)
    assert table[0] == 0 and table[-1] == 512
    assert all(a <= b for a, b in zip(table, table[1:]))
    thresholds = [next(i for i, value in enumerate(table) if value >= angle)
                  for angle in range(1, 513)]
    # Verify the compression against every source entry, including endpoints.
    assert all(sum(index >= start for start in thresholds) == value
               for index, value in enumerate(table))
    header = """// Vitz parameters from 80039DE8; speeds in metres/second.
// At low speed the source truncates squared components before the square root.
(
    stationary_speed_mps: 0.27783203125, // 1138/4096
    full_speed_mps: 2.7783203125, // 11380/4096
    activation_per_mps: 0.39990234375, // 1638/4096
    // Minimum quantized |minor/major| ratio for each 1/4096-turn angle
    // from 1 through 512. Exact monotone compression of 4097 signed
    // halfwords at original RAM 800A4AC8; ratio is dimensionless.
    // SHA-256 of the original little-endian table: """
    text = header + hashlib.sha256(original).hexdigest() + "\n    angle_threshold_ratios: [\n"
    for start in range(0, 512, 8):
        text += "        " + ", ".join(str(value / 4096) for value in thresholds[start:start + 8]) + ",\n"
    Path(path).write_text(text + "    ],\n)\n")
    print(f"Exported exact 512-threshold angle curve to {path}")


def vectors(holdout):
    if holdout:
        # Fresh mixed values and unequal sign-sensitive normalization cases,
        # deliberately absent from the initial thresholds/quadrants recording.
        return [(12345, -6789), (-12345, 6789), (-12345, -6789), (12345, 6789),
                (1142, 21), (804, 811), (11200, 2087), (-11379, -144),
                (32767, 139), (32768, 139), (32769, 139), (-32768, -139),
                (131071, 1923), (131072, 1923), (-131071, 1923), (-131072, 1923),
                (192671, -53291), (-101773, 52811), (345671, 98761), (-278191, -33219)]
    values = [(0, 0), (1, 1), (1000, 0), (1138, 0), (1139, 0), (1140, 0),
              (1141, 0), (1142, 0), (1143, 0), (11379, 0), (11380, 0), (11381, 0),
              (-11380, 0), (-11381, 0), (0, -11380), (0, -11381),
              (804, 804), (805, 805), (806, 806), (807, 807),
              (8000, 8000), (8046, 8046), (8047, 8047), (8048, 8048),
              (16383, 1099), (16384, 1099), (-16384, 1099), (-16385, 1099),
              (32767, 1087), (32768, 1087), (-32768, 1087), (-32769, 1087)]
    for forward, lateral in ((40960, 0), (0, 40960), (40960, 40960),
                             (40960, 160), (40960, 220), (40960, 600),
                             (40960, 660), (40960, 1040), (40960, 1100),
                             (16383, 6001), (16384, 6001), (16385, 6001)):
        for sign_forward, sign_lateral in ((1, 1), (1, -1), (-1, 1), (-1, -1)):
            values.append((forward * sign_forward, lateral * sign_lateral))
    return values


def signed(event):
    value, bits = event["value"], event["width"] * 8
    return value - (1 << bits) if value >= (1 << (bits - 1)) else value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--holdout", action="store_true")
    parser.add_argument("--export-tuning", metavar="PATH", help="Export the original curve/tuning and exit")
    args = parser.parse_args()
    if args.export_tuning:
        export_tuning(args.export_tuning)
        return
    name = "holdout" if args.holdout else "boundaries"
    pairs = vectors(args.holdout)
    assert len(pairs) % 4 == 0
    OUTPUT.mkdir(parents=True, exist_ok=True)
    rows, records = [], []
    for batch in range(0, len(pairs), 4):
        group = pairs[batch:batch + 4]
        run = Path(f".psx/runs/wheel-slip-{name}-{batch // 4:02}")
        command = ["psx", "run", "--from-state", str(STATE), "--frames", "2",
                   "--out", str(run), "--summary", "--max-events", "100000"]
        addresses = {}
        for wheel, (forward, lateral) in enumerate(group):
            start = BASE + 0x460 + wheel * 0x68
            command.extend(["--poke", f"{start + 0x2C:08X}={forward}:4@0",
                            "--poke", f"{start + 0x30:08X}={lateral}:4@0",
                            "--watch-reads", f"{start + 0x2C:08X}:8",
                            "--watch", f"{start + 0x50:08X}:4"])
            addresses[start + 0x2C] = (wheel, "forward")
            addresses[start + 0x30] = (wheel, "lateral")
            addresses[start + 0x50] = (wheel, "angle")
            addresses[start + 0x52] = (wheel, "activation")
        subprocess.run(command, check=True, capture_output=True, text=True)
        report = json.loads((run / "report.json").read_text())
        assert report["status"] == "complete", report
        assert report["watch"]["complete"] and report["read_watch"]["complete"], report
        observed = [dict(case=batch + wheel, wheel=wheel) for wheel in range(4)]
        for log, pcs in (("reads", {0x80039E48, 0x80039E4C}),
                         ("writes", {0x80039EE8, 0x80039F00})):
            for line in (run / f"{log}.jsonl").open():
                event = json.loads(line)
                if event["pc"] not in pcs or event["run_frame"] != 0:
                    continue
                wheel, field = addresses[event["address"]]
                assert field not in observed[wheel], (run, event)
                observed[wheel][field] = signed(event)
        for wheel, row in enumerate(observed):
            assert set(row) == {"case", "wheel", "forward", "lateral", "angle", "activation"}, row
            assert (row["forward"], row["lateral"]) == group[wheel], (row, group[wheel])
            rows.append(row)
        records.append(dict(inputs=group, command=command, report=str(run / "report.json")))
    with (OUTPUT / f"{name}.csv").open("w", newline="") as target:
        writer = csv.DictWriter(target, list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    (OUTPUT / f"{name}.json").write_text(json.dumps(dict(
        state_sha256=hashlib.sha256(STATE.read_bytes()).hexdigest(),
        ram_sha256=hashlib.sha256(STATE.with_name("ram.bin").read_bytes()).hexdigest(),
        angle_table_sha256=hashlib.sha256(STATE.with_name("ram.bin").read_bytes()[0xA4AC8:0xA4AC8 + 8194]).hexdigest(),
        psx_version=subprocess.check_output(["psx", "--version"], text=True).strip(),
        scope="Controlled original wheel-slip executions; not a contact/force trajectory",
        units="forward/lateral 4096 per m/s; angle 4096 per turn; activation 4096 per full scale",
        holdout=args.holdout, cases=records), indent=2) + "\n")
    print(f"{len(rows)} original {name} wheel-slip samples exported")


if __name__ == "__main__":
    main()
