#!/usr/bin/env python3
"""Record original engine torque at consumers, including controlled limiter probes.

No engine-force model is used: watches capture curve/drag outputs and the
engine-speed input; instruction trace captures effective throttle and returned
torque. The limiter scenarios poke integer RPM only, preserving the real latch.
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
    "gas": (60, [{"frame": 0, "buttons": ["CROSS"]}], []),
    "coast": (60, [{"frame": 0, "buttons": []}], []),
    "brake": (60, [{"frame": 0, "buttons": ["SQUARE"]}], []),
    "limiter": (36, [{"frame": 0, "buttons": ["CROSS"]}], [6799, 6800, 6799, 6300, 6299, 6800]),
    # Record this after implementation, independently of derivation cases.
    "limiter_holdout": (40, [{"frame": 0, "buttons": ["CROSS"]}],
                        [6801, 6301, 6300, 6299, 6799, 6800, 6300, 6299]),
    "mixed_holdout": (60, [{"frame": 0, "buttons": ["SQUARE"]},
                           {"frame": 16, "buttons": ["CROSS", "SQUARE"]},
                           {"frame": 30, "buttons": []},
                           {"frame": 42, "buttons": ["CROSS"]}], []),
    "curve_holdout": (20, [{"frame": 0, "buttons": ["CROSS"]}], []),
}

# Controlled positive engine-speed probes cover end clamping, exact knots,
# and interpolation on both sides of the torque peak after implementation.
CURVE_SPEED_PROBES = [4096, 54613, 70000, 109226, 273066, 286000, 382293, 450000, 464213, 491520]


def signed(event):
    value = event["value"]
    bits = event["width"] * 8
    return value - (1 << bits) if value >= 1 << (bits - 1) else value


def export(run, target, frames, start_frame):
    ticks = {}
    def put(frame, key, value):
        row = ticks.setdefault(frame, {"run_frame": frame})
        if key in row:
            raise RuntimeError(f"Multiple engine calls/observations for {key} at {frame}")
        row[key] = value

    for line in (run / "reads.jsonl").open():
        event = json.loads(line)
        key = {0x8003E1FC: "curve_speed_q12", 0x80035354: "before_cut",
               0x80035364: "limiter_rpm", 0x80035388: "limiter_rpm",
               0x80039438: "refresh_speed_q12"}.get(event["pc"])
        if key:
            put(event["run_frame"], key, signed(event))
    for line in (run / "writes.jsonl").open():
        event = json.loads(line)
        key = {0x8003E218: "full_torque", 0x8003E220: "drag",
               0x80039458: "refreshed_rpm", 0x80035380: "after_cut",
               0x8003539C: "after_cut"}.get(event["pc"])
        if key:
            put(event["run_frame"], key, signed(event))
    pattern = re.compile(r"^(\d+) \d+ (80035348|800353d0) [0-9a-f]+ .*; (a0|v0)=([0-9a-f]+)$")
    for line in (run / "trace.txt").open():
        match = pattern.match(line.strip())
        if match:
            absolute, pc, register, raw = match.groups()
            value = int(raw, 16)
            if value >= 1 << 31:
                value -= 1 << 32
            put(int(absolute) - start_frame, "throttle_q12" if pc == "80035348" else "net_torque", value)
    required = {"run_frame", "curve_speed_q12", "before_cut", "limiter_rpm", "refresh_speed_q12",
                "full_torque", "drag", "refreshed_rpm", "after_cut", "throttle_q12", "net_torque"}
    if sorted(ticks) != list(range(0, frames, 2)):
        raise RuntimeError(f"Missing 30 Hz engine calls: {sorted(ticks)}")
    rows = []
    for frame, row in sorted(ticks.items()):
        row.setdefault("after_cut", row.get("before_cut"))
        if set(row) != required:
            raise RuntimeError(f"Incomplete engine tick {frame}: {row}")
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
    parser.add_argument("--out", type=Path, default=Path("game/tests/data/original/engine"))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    for scenario in args.scenario or SCENARIOS:
        frames, replay, rpm_probes = SCENARIOS[scenario]
        run = Path(".psx/runs") / f"engine-{scenario}"
        command = ["psx", "run", "--from-state", str(args.state), "--frames", str(frames),
                   "--input", json.dumps(replay), "--trace", "--trace-range", "8003533C-800353DC",
                   "--watch-reads", "800A96B4-800A9E00", "--watch", "800A9DC4:8",
                   "--watch", "800A9D60:2", "--watch", "800A9CD1:1",
                   "--max-events", "2000000", "--out", str(run), "--summary"]
        if rpm_probes:
            command.extend(["--poke", "800A9CD1=0:1"])
            # Let the checkpoint's existing shift finish before exercising
            # the limiter, so the probes include positive combustion torque.
            for index, rpm in enumerate(rpm_probes):
                command.extend(["--poke", f"800A9D60={rpm}:2@{24 + index * 2}"])
        speed_probes = CURVE_SPEED_PROBES if scenario == "curve_holdout" else []
        for index, speed in enumerate(speed_probes):
            command.extend(["--poke", f"800A9CD8={speed}:4@{index * 2}"])
        subprocess.run(command, check=True, text=True, capture_output=True)
        report = json.loads((run / "report.json").read_text())
        if report["status"] != "complete":
            raise RuntimeError(f"Incomplete run: {run}")
        for observation in ("watch", "read_watch"):
            if report.get(observation, {}).get("complete") is not True:
                raise RuntimeError(f"Incomplete {observation}: {run}")
        if report.get("trace", {}).get("lines_dropped") != 0:
            raise RuntimeError(f"Incomplete trace: {run}")
        rows = export(run, args.out / f"{scenario}.ticks.csv", frames, report["frames"]["start"])
        metadata = {"scenario": scenario, "frames": frames, "ticks": len(rows), "command": command,
                    "state_sha256": hashlib.sha256(args.state.read_bytes()).hexdigest(),
                    "ram_sha256": hashlib.sha256(args.state.with_name("ram.bin").read_bytes()).hexdigest(),
                    "psx_version": subprocess.check_output(["psx", "--version"], text=True).strip(),
                    "source_report": str(run / "report.json"), "controlled_rpm_probes": rpm_probes,
                    "controlled_probe_start_frame": 24 if rpm_probes else None,
                    "controlled_speed_probes_q12": speed_probes,
                    "scope": "Torque-curve sampling, drag, effective throttle, stateful fuel cut and RPM refresh; no drivetrain integration",
                    "units": {"speed": "revolutions/second *4096", "rpm": "integer RPM",
                              "throttle": "fraction *4096", "torque": "original internal unit, N m calibration unproven"}}
        (args.out / f"{scenario}.json").write_text(json.dumps(metadata, indent=2) + "\n")
        print(f"{scenario}: {len(rows)} complete engine ticks")


if __name__ == "__main__":
    main()
