#!/usr/bin/env python3
"""Record original B-1 controls and integration; no replacement physics model.

Requires the local psx CLI (format-9 state support) and the checkpoint described
in docs/driving-evidence.md. RAM, states and instruction traces stay in .psx/;
only numeric samples, effective controls and observation metadata are exported.
"""

import argparse
import csv
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


BASE = 0x800A96B4
SAMPLES = {
    "front_left_steering": (0x46C, "i16"), "front_right_steering": (0x4D4, "i16"),
    "rear_left_steering": (0x53C, "i16"), "rear_right_steering": (0x5A4, "i16"),
    "front_left_slip": (0x4B0, "i16"), "front_right_slip": (0x518, "i16"),
    "steering": (0x60C, "i16"), "steering_rate": (0x60E, "i16"),
    "throttle": (0x610, "i16"), "brake": (0x612, "i16"),
    "handbrake": (0x614, "i16"), "vx": (0x628, "i32"),
    "vz": (0x62C, "i32"), "vy": (0x630, "i32"),
    "heading": (0x648, "i16"), "yaw_rate": (0x64C, "i32"),
    "x": (0x65C, "i32"), "z": (0x660, "i32"), "y": (0x664, "i32"),
    "forward_velocity": (0x6A4, "i32"), "sideways_velocity": (0x6A8, "i32"),
    "engine_rpm": (0x6AC, "i16"), "speed_readout": (0x6AE, "u16"),
    "maximum_speed_readout": (0x6F8, "u16"),
    "step_duration": (0x6FE, "i16"),
    "steering_negative_limit": (0x70A, "i16"), "steering_positive_limit": (0x70C, "i16"),
}
SCENARIOS = {
    "gas": [{"frame": 0, "buttons": ["CROSS"]}],
    "coast": [{"frame": 0, "buttons": []}],
    "brake": [{"frame": 0, "buttons": ["SQUARE"]}],
    "left": [{"frame": 0, "buttons": ["CROSS", "LEFT"]}],
    "right": [{"frame": 0, "buttons": ["CROSS", "RIGHT"]}],
    "handbrake": [{"frame": 0, "buttons": ["CIRCLE"]}],
    # Record this only after writing the rules; it is a separate holdout.
    "mixed_holdout": [
        {"frame": 0, "buttons": ["SQUARE", "CIRCLE"]},
        {"frame": 14, "buttons": ["CROSS", "SQUARE"]},
        {"frame": 30, "buttons": []},
        {"frame": 46, "buttons": ["CROSS"]},
        {"frame": 62, "buttons": ["CIRCLE"]},
        {"frame": 78, "buttons": ["SQUARE"]},
        {"frame": 94, "buttons": ["CROSS"]},
    ],
    "steering_holdout": [
        {"frame": 0, "buttons": ["CROSS", "LEFT"]},
        {"frame": 14, "buttons": ["CROSS", "RIGHT"]},
        {"frame": 28, "buttons": ["CROSS"]},
        {"frame": 42, "buttons": ["SQUARE", "LEFT"]},
    ],
    # Independent holdout recorded after porting velocity-to-step construction.
    "motion_holdout": [
        {"frame": 0, "buttons": ["CROSS", "RIGHT"]},
        {"frame": 10, "buttons": ["SQUARE", "RIGHT"]},
        {"frame": 20, "buttons": ["CROSS", "LEFT"]},
        {"frame": 30, "buttons": ["LEFT"]},
        {"frame": 42, "buttons": ["CIRCLE", "RIGHT"]},
        {"frame": 54, "buttons": ["CROSS"]},
    ],
}


def export_ticks(run, target):
    samples = list(csv.DictReader((run / "samples.csv").open()))
    observations = {}
    for line in (run / "reads.jsonl").open():
        event = json.loads(line)
        # Normalized input at 0x801ff750 is a call-local stack object. Read
        # watches, filtered by consuming instructions, establish its values
        # at the control call rather than trusting a later stack snapshot.
        key = {
            0x8002FCB0: "input_flags", 0x8002FCC4: "input_steering",
            0x8002FFF4: "input_throttle", 0x800300A8: "input_brake",
            0x80030178: "input_handbrake",
            0x8002FB54: "input_front_left_slip", 0x8002FB58: "input_front_right_slip",
            0x80030390: "step_dt", 0x800303D8: "step_yaw_rate",
        }.get(event["pc"])
        if event["pc"] == 0x800303A8:
            key = {0: "step_vx", 4: "step_vz", 8: "step_vy"}.get(event["address"] - (BASE + 0x628))
        if event["pc"] in (0x80039E48, 0x80039E4C):
            offset = event["address"] - (BASE + 0x48C)
            wheel, field = divmod(offset, 0x68)
            if 0 <= wheel < 4 and field in (0, 4):
                key = f"wheel{wheel}_" + ("forward" if field == 0 else "lateral")
        if event["pc"] in (0x80033EAC, 0x80033ED4):
            offset = event["address"] - 0x1F8000B4
            key = {0: "dx", 4: "dz", 8: "dy", 12: "dheading"}.get(offset)
        if key:
            value = event["value"]
            bits = event["width"] * 8
            if value >= 1 << (bits - 1):
                value -= 1 << bits
            observations.setdefault(event["run_frame"], {})[key] = value
    ticks = []
    for frame, observed in sorted(observations.items()):
        required = {"input_flags", "input_steering", "input_throttle", "input_brake",
                    "input_handbrake", "input_front_left_slip", "input_front_right_slip", "dx", "dz", "dy", "dheading",
                    "step_dt", "step_yaw_rate", "step_vx", "step_vz", "step_vy"}
        required.update(f"wheel{wheel}_{axis}" for wheel in range(4) for axis in ("forward", "lateral"))
        if not required <= observed.keys():
            raise RuntimeError(f"Incomplete tick {frame}: {observed}")
        if observed["input_flags"] != 0:
            raise RuntimeError("This recorder's tick export supports digital input only")
        row = {"tick": len(ticks), "run_frame": frame, **observed}
        row.update({f"before_{name}": samples[frame][name] for name in SAMPLES})
        row.update({f"after_{name}": samples[frame + 1][name] for name in SAMPLES})
        ticks.append(row)
    if not ticks:
        raise RuntimeError("No observed car ticks")
    with target.open("w", newline="") as output:
        writer = csv.DictWriter(output, list(ticks[0]))
        writer.writeheader()
        writer.writerows(ticks)
    return len(ticks)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state", type=Path, default=Path(".psx/runs/b1-state1200/state.bin"))
    parser.add_argument("--out", type=Path, default=Path("game/tests/data/original/driving"))
    parser.add_argument("--scenario", choices=SCENARIOS, action="append")
    parser.add_argument("--frames", type=int, help="Default 60; 120 for mixed_holdout")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    for scenario in args.scenario or SCENARIOS:
        replay = SCENARIOS[scenario]
        frames = args.frames if args.frames is not None else (120 if scenario == "mixed_holdout" else 60)
        run = Path(".psx/runs") / f"driving-{scenario}"
        command = ["psx", "run", "--from-state", str(args.state), "--frames", str(frames),
                   "--input", json.dumps(replay), "--watch-reads", "1F8000B4:16",
                   "--watch-reads", "801FF750:12", "--max-events", "2000000",
                   "--watch-reads", "800A9B64:2", "--watch-reads", "800A9BCC:2",
                   "--watch-reads", "800A9CDC:12", "--watch-reads", "800A9D00:4", "--watch-reads", "800A9DB2:2",
                   "--out", str(run), "--summary"]
        for name, (offset, fmt) in SAMPLES.items():
            command.extend(["--sample", f"{BASE + offset:08X}:{fmt}:{name}"])
        for wheel in range(4):
            command.extend(["--watch-reads", f"{BASE + 0x48C + wheel * 0x68:08X}:8"])
        result = subprocess.run(command, check=True, text=True, capture_output=True)
        report = json.loads((run / "report.json").read_text())
        if report["status"] != "complete":
            raise RuntimeError(result.stdout)
        # A capped trace is insufficient evidence even when the run completes.
        for observation in ("watch", "read_watch"):
            if report.get(observation, {}).get("complete") is False:
                raise RuntimeError(f"Truncated {observation}: {run}")
        shutil.copyfile(run / "samples.csv", args.out / f"{scenario}.samples.csv")
        count = export_ticks(run, args.out / f"{scenario}.ticks.csv")
        metadata = {
            "scenario": scenario, "command": command, "input": replay,
            "state_sha256": hashlib.sha256(args.state.read_bytes()).hexdigest(),
            "ram_sha256": hashlib.sha256(args.state.with_name("ram.bin").read_bytes()).hexdigest(),
            "psx_version": subprocess.check_output(["psx", "--version"], text=True).strip(),
            "ticks": count, "frames": frames, "source_report": str(run / "report.json"),
            "units": {"position": "1/4096 metre", "displacement": "1/4096 metre per tick",
                      "heading": "1/4096 turn", "pedals": "1/4096 of full travel",
                      "step_duration": "1/65536 second"},
            "scope": "Observed controls and explicit integration deltas; force model is unported",
        }
        (args.out / f"{scenario}.json").write_text(json.dumps(metadata, indent=2) + "\n")
        print(f"{scenario}: {count} complete observed car ticks")


if __name__ == "__main__":
    main()
