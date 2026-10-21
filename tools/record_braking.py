#!/usr/bin/env python3
"""Record original brake preparation, not a guessed braking force model.

The short instruction trace preserves effective per-wheel travel, axle
coefficients, ordinary demand, additive rear demand and initial wheel flags
at their consuming/writing instructions. Scratchpad store watches do not log
these stores in this toolkit version, so trace store effects are required.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import subprocess


BASE = 0x800A96B4
SCENARIOS = {
    "brake": (60, [{"frame": 0, "buttons": ["SQUARE"]}]),
    "handbrake": (60, [{"frame": 0, "buttons": ["CIRCLE"]}]),
    "combined": (60, [{"frame": 0, "buttons": ["SQUARE", "CIRCLE"]}]),
    # The scenarios below are recorded after the rule is implemented.
    "mixed_holdout": (60, [{"frame": 0, "buttons": ["SQUARE", "CIRCLE"]},
                            {"frame": 8, "buttons": ["CIRCLE"]},
                            {"frame": 18, "buttons": []},
                            {"frame": 24, "buttons": ["SQUARE"]},
                            {"frame": 38, "buttons": ["CROSS", "SQUARE", "CIRCLE"]},
                            {"frame": 48, "buttons": []}]),
    "wheel_holdout": (16, []),
    "handbrake_alias_holdout": (10, [{"frame": 0, "buttons": ["CIRCLE"]}]),
}
WHEEL_PROBES = [
    ([1000, 2000, 3000, 4000], 1365),
    ([4096, 0, 1, 4095], 2730),
    ([17, 31, 2048, 1024], 4095),
    ([0, 0, 0, 0], 4096),
    ([1365, 2730, 4095, 4096], 0),
    ([4096, 4096, 4096, 4096], 4096),
    ([1, 2, 3, 4], 1),
    ([0, 0, 0, 0], 0),
]
ALIAS_CAPACITIES = [32767, 32768, 65535, 65536, 32767]


def export(run, target, frames, start_frame):
    ticks = {}
    def put(frame, key, value, repeated=False):
        row = ticks.setdefault(frame, {"run_frame": frame})
        if key in row and not (repeated and row[key] == value):
            raise RuntimeError(f"Duplicate observation {frame} {key}")
        row[key] = value

    pattern = re.compile(r"^(\d+) \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \[([0-9a-f]+)\](=>|=)([0-9a-f]+)$")
    for line in (run / "trace.txt").open():
        match = pattern.match(line.strip())
        if not match:
            continue
        absolute, pc_text, address_text, operation, raw = match.groups()
        frame, pc, address, value = int(absolute) - start_frame, int(pc_text, 16), int(address_text, 16), int(raw, 16)
        bits = len(raw) * 4
        if value >= 1 << (bits - 1):
            value -= 1 << bits
        if pc == 0x8003A1B8:
            wheel = (address - BASE - 0x4C0) // 0x68
            put(frame, f"travel_{wheel}", value)
        elif pc == 0x8003E2E8:
            wheel = (address - BASE - 0x4C0) // 0x68
            put(frame, f"copied_brake_{wheel}", value)
        elif pc == 0x8003A1BC:
            axle = (address - BASE - 0x414) // 4
            put(frame, f"capacity_{axle}", value, repeated=True)
        elif pc == 0x8003A1E8:
            put(frame, "handbrake_travel", value)
        elif pc == 0x8003A1EC:
            put(frame, "handbrake_capacity", value)
        elif pc in (0x8003A1E4, 0x8003A22C):
            wheel = (address - 0x1F800004) // 0x1C
            put(frame, f"{'ordinary' if pc == 0x8003A1E4 else 'total'}_{wheel}", value)
        elif pc == 0x8003A250:
            wheel = (address - BASE - 0x468) // 0x68
            put(frame, f"wheel_load_{wheel}", value)
        elif pc == 0x8003A264:
            wheel = (address - BASE - 0x4C3) // 0x68
            put(frame, f"initial_flags_{wheel}", value)

    required = {"run_frame", "capacity_0", "capacity_1", "handbrake_capacity", "handbrake_travel"}
    required.update(f"{key}_{wheel}" for key in ("travel", "copied_brake", "ordinary", "total", "wheel_load", "initial_flags") for wheel in range(4))
    if sorted(ticks) != list(range(0, frames, 2)):
        raise RuntimeError(f"Missing 30 Hz brake-preparation calls: {sorted(ticks)}")
    rows = []
    for frame, row in sorted(ticks.items()):
        # Controlled wheel-input injection skips the ordinary pedal-copy
        # branch. A sentinel distinguishes absence from zero pedal travel.
        copied = [f"copied_brake_{wheel}" in row for wheel in range(4)]
        if any(copied) and not all(copied):
            raise RuntimeError(f"Incomplete pedal copying at {frame}")
        for wheel in range(4):
            row.setdefault(f"copied_brake_{wheel}", -1)
        # The front wheels have no handbrake addition/store in the original.
        for wheel in range(2):
            row[f"total_{wheel}"] = row.get(f"ordinary_{wheel}")
        if set(row) != required or any(value is None for value in row.values()):
            raise RuntimeError(f"Incomplete braking call {frame}: {row}")
        rows.append(row)
    with target.open("w", newline="") as output:
        writer = csv.DictWriter(output, sorted(required))
        writer.writeheader()
        writer.writerows(rows)
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", choices=SCENARIOS, action="append")
    parser.add_argument("--state", type=Path, default=Path(".psx/runs/b1-state1200/state.bin"))
    parser.add_argument("--out", type=Path, default=Path("game/tests/data/original/braking"))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    for scenario in args.scenario or SCENARIOS:
        frames, replay = SCENARIOS[scenario]
        run = Path(".psx/runs") / f"braking-{scenario}"
        command = ["psx", "run", "--from-state", str(args.state), "--frames", str(frames),
                   "--input", json.dumps(replay), "--trace", "--trace-range", "8003A19C-8003A278",
                   "--trace-range", "8003E2D0-8003E2F8",
                   "--max-events", "200000", "--out", str(run), "--summary"]
        controlled = {}
        if scenario == "wheel_holdout":
            # The established 8003E270 branch skips live-control copying for
            # this mode. This is a controlled consumer-input experiment, not
            # a claim to understand the original mode enum or assist logic.
            command.extend(["--poke", "800A9B11=1:1"])
            for index, (channels, handbrake) in enumerate(WHEEL_PROBES):
                for wheel, travel in enumerate(channels):
                    command.extend(["--poke", f"{BASE + 0x4C0 + wheel * 0x68:08X}={travel}:2@{index * 2}"])
                command.extend(["--poke", f"800A9CC8={handbrake}:2@{index * 2}"])
            controlled = {"wheel_inputs": WHEEL_PROBES, "control_mode_poke": 1}
        elif scenario == "handbrake_alias_holdout":
            for index, capacity in enumerate(ALIAS_CAPACITIES):
                command.extend(["--poke", f"800A9AD0={capacity}:4@{index * 2}"])
            controlled = {"handbrake_capacity_probes": ALIAS_CAPACITIES}
        subprocess.run(command, check=True, capture_output=True, text=True)
        report = json.loads((run / "report.json").read_text())
        if report["status"] != "complete" or report.get("trace", {}).get("lines_dropped") != 0:
            raise RuntimeError(f"Incomplete original trace: {run}")
        rows = export(run, args.out / f"{scenario}.ticks.csv", frames, report["frames"]["start"])
        metadata = {"scenario": scenario, "frames": frames, "ticks": len(rows), "command": command,
                    "state_sha256": hashlib.sha256(args.state.read_bytes()).hexdigest(),
                    "ram_sha256": hashlib.sha256(args.state.with_name("ram.bin").read_bytes()).hexdigest(),
                    "psx_version": subprocess.check_output(["psx", "--version"], text=True).strip(),
                    "source_report": str(run / "report.json"), "controlled_probes": controlled,
                    "scope": "Effective wheel travel to prepared brake demand and initial brake-state flag; no tyre forces or speed model",
                    "units": {"travel": "fraction *4096", "demand": "original internal brake unit, N m calibration unproven"}}
        (args.out / f"{scenario}.json").write_text(json.dumps(metadata, indent=2) + "\n")
        print(f"{scenario}: {len(rows)} complete brake-preparation calls")


if __name__ == "__main__":
    main()
