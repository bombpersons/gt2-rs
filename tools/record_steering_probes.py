#!/usr/bin/env python3
"""Probe the original steering curve/spring with explicit state interventions.

These are controlled original executions, not normal driving trajectories.
Each case restores the same checkpoint. Only values actually read by the
control routine are used as test inputs; no formula here predicts its output.
"""
import csv
import hashlib
import json
from pathlib import Path
import subprocess

from record_driving import BASE, SAMPLES


def cases():
    # Both sides of each curve knot, reverse, and the 16-bit speed ceiling.
    speeds = {0, 8192, -8192, 34139, 34140, -102605, 400000}
    for knot in (4096, 10240, 17920, 25856, 32640):
        raw = knot * 4096 // 461
        speeds.update((raw, raw + 1))
    for i, speed in enumerate(sorted(speeds)):
        yield dict(name=f"curve_{i}", speed=speed, lateral=0,
                   angle=(-190 if i % 2 else 210), rate=(-1200 if i % 3 else 1300),
                   buttons=(["LEFT"] if i % 2 else ["RIGHT"]))
    # Slip extension threshold, both signs, its cap, and its forward-only gate.
    for speed in (34139, 34140, 102605, -102605):
        for lateral in (-10000, 10000, -60000, 60000):
            yield dict(name=f"slip_{speed}_{lateral}", speed=speed,
                       lateral=lateral, angle=0, rate=0, buttons=["LEFT" if lateral < 0 else "RIGHT"])
    # Target crossing, return to centre and nonzero stored rate at the target.
    for angle, rate, buttons in ((1, -1300, []), (-1, 1300, []),
                                  (170, 500, ["LEFT"]), (-170, -500, ["RIGHT"]),
                                  (0, 1300, []), (0, -1300, [])):
        yield dict(name=f"spring_{angle}_{rate}", speed=102605,
                   lateral=0, angle=angle, rate=rate, buttons=buttons)


def main():
    state = Path(".psx/runs/b1-state1200/state.bin")
    output = Path("game/tests/data/original/driving")
    rows, records = [], []
    for case in cases():
        run = Path(".psx/runs") / f"steering-probe-{case['name']}"
        command = ["psx", "run", "--from-state", str(state), "--frames", "4",
                   "--input", json.dumps([dict(frame=0, buttons=case["buttons"])]),
                   "--out", str(run), "--summary", "--max-events", "200000"]
        # Apply at the start of the second logic tick, after the pad has
        # received the replay. The front wheel local velocities are consumed
        # by the original slip calculation before the steering controller.
        for offset, value, width in ((0x6A4, case["speed"], 4),
                                     (0x60C, case["angle"], 2), (0x60E, case["rate"], 2),
                                     (0x48C, 40960, 4), (0x490, case["lateral"], 4),
                                     (0x4F4, 40960, 4), (0x4F8, case["lateral"], 4)):
            command.extend(["--poke", f"{BASE + offset:08X}={value}:{width}@2"])
        for watch in ("801FF750:12", "800A9B64:2", "800A9BCC:2", "800A9D58:4", "800A9CC0:4"):
            command.extend(["--watch-reads", watch])
        for name, (offset, fmt) in SAMPLES.items():
            command.extend(["--sample", f"{BASE + offset:08X}:{fmt}:{name}"])
        subprocess.run(command, check=True, capture_output=True, text=True)
        report = json.loads((run / "report.json").read_text())
        assert report["status"] == "complete", report
        assert report["read_watch"]["complete"], report["read_watch"]
        inputs = {}
        pcs = {0x8002FB54: "left_slip", 0x8002FB58: "right_slip",
               0x8002FB5C: "speed", 0x8002FCC4: "direction",
               0x8002FCF8: "angle", 0x8002FD0C: "rate"}
        for line in (run / "reads.jsonl").open():
            event = json.loads(line)
            if event["run_frame"] != 2 or event["pc"] not in pcs:
                continue
            bits = event["width"] * 8
            value = event["value"]
            inputs[pcs[event["pc"]]] = value - (1 << bits) if value >= 1 << (bits - 1) else value
        assert set(inputs) == set(pcs.values()), (case, inputs)
        assert all(inputs[k] == case[k] for k in ("speed", "angle", "rate")), (case, inputs)
        samples = list(csv.DictReader((run / "samples.csv").open()))
        after = samples[3]
        keys = ("steering", "steering_rate", "steering_negative_limit", "steering_positive_limit",
                "front_left_steering", "front_right_steering", "rear_left_steering", "rear_right_steering")
        rows.append(dict(case=case["name"], **inputs, **{k: after[k] for k in keys}))
        records.append(dict(case=case, command=command, report=str(run / "report.json")))
    with (output / "steering_probes.csv").open("w", newline="") as target:
        writer = csv.DictWriter(target, list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    metadata = dict(state_sha256=hashlib.sha256(state.read_bytes()).hexdigest(),
                    psx_version=subprocess.check_output(["psx", "--version"], text=True).strip(),
                    scope="Independent original control ticks with documented state interventions; no dynamics parity claim",
                    cases=records)
    (output / "steering_probes.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"{len(rows)} original steering probes exported")


if __name__ == "__main__":
    main()
