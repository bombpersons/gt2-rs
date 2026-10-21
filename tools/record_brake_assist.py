#!/usr/bin/env python3
"""Record the original wheel-brake assistance at its consumer instructions.

Natural sequences plus controlled parameter/velocity interventions. Expected
outputs come only from original stores; no Python model supplies predictions.
Run --holdout after writing the native rule and initial trajectory checks.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

BASE = 0x800A96B4
STATE = Path('.psx/runs/b1-state1200/state.bin')
OUTPUT = Path('game/tests/data/original/brake_assist')
EFFECT = re.compile(r'^(\d+) \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \[([0-9a-f]+)\](=>|=)([0-9a-f]+)$')


def scenarios(holdout):
    if holdout:
        return [
            ('mixed_holdout', 48, [{'frame': 0, 'buttons': ['SQUARE', 'RIGHT']},
                                 {'frame': 10, 'buttons': ['LEFT', 'CIRCLE']},
                                 {'frame': 22, 'buttons': ['SQUARE', 'CROSS']},
                                 {'frame': 34, 'buttons': []}], []),
            ('yaw_holdout', 24, [{'frame': 0, 'buttons': ['LEFT', 'SQUARE']}],
             [(0x458, 2, 137), (0x45A, 2, -173), (0x64C, 4, 317), ('global', 1, 37)]),
            ('zero_strength_holdout', 16, [{'frame': 0, 'buttons': ['RIGHT', 'SQUARE']}],
             [(0x458, 2, 91), (0x45A, 2, -97), (0x64C, 4, 239), ('global', 1, 0)]),
            ('feedback_holdout', 16, [{'frame': 0, 'buttons': ['SQUARE']}],
             [(0x424, 4, 23891), (0x428, 4, 51013), (0x42C, 2, 4096), (0x42E, 2, -103)]),
        ]
    return [
        ('brake', 60, [{'frame': 0, 'buttons': ['SQUARE']}], []),
        ('combined', 24, [{'frame': 0, 'buttons': ['SQUARE', 'CIRCLE']}], []),
        ('left_yaw', 24, [{'frame': 0, 'buttons': ['LEFT', 'SQUARE']}],
         [(0x458, 2, 100), (0x45A, 2, -512), (0x64C, 4, 100)]),
        ('right_yaw', 24, [{'frame': 0, 'buttons': ['RIGHT', 'SQUARE']}],
         [(0x458, 2, 100), (0x45A, 2, -512), (0x64C, 4, -100)]),
        ('upper_targets', 12, [{'frame': 0, 'buttons': ['LEFT', 'SQUARE']}],
         [(0x1FE, 2, 5000), (0x2D6, 2, 6000), (0x64C, 4, 100)]),
        ('zero_factor', 12, [{'frame': 0, 'buttons': ['SQUARE']}],
         [(0x42C, 2, 0), (0x42E, 2, 0)]),
    ]


def export(run, frames, start):
    ticks = {}
    for line in (run / 'trace.txt').open():
        m = EFFECT.match(line.strip())
        if not m:
            continue
        absolute, pc, address, op, raw = m.groups()
        frame, pc, address = int(absolute) - start, int(pc, 16), int(address, 16)
        value = int(raw, 16)
        if value >= 1 << (len(raw) * 4 - 1):
            value -= 1 << (len(raw) * 4)
        row = ticks.setdefault(frame, {'run_frame': frame})
        def put(key, v=value):
            if key in row and row[key] != v:
                raise RuntimeError(f'Conflicting input {run} {frame} {key}: {row[key]} vs {v}')
            row[key] = v
        if pc in (0x8003E740, 0x8003E744):
            put('correction_gain' if pc == 0x8003E740 else 'correction_threshold')
        elif pc == 0x8003E2E8:
            wheel = (address - BASE - 0x4C0) // 0x68
            put(f'pedal_{wheel}')
        elif pc == 0x8003A1B8:
            wheel = (address - BASE - 0x4C0) // 0x68
            put(f'allocated_input_{wheel}')
        elif pc == 0x8003A1BC:
            put(f'capacity_{(address - BASE - 0x414) // 4}')
        elif pc in (0x8003A1E8, 0x8003A1EC):
            put('handbrake' if pc == 0x8003A1E8 else 'handbrake_capacity')
        elif pc in (0x8003A1E4, 0x8003A22C):
            wheel = (address - 0x1F800004) // 0x1C
            put(f'{"ordinary" if pc == 0x8003A1E4 else "total"}_{wheel}')
        elif pc in (0x8003DBF4, 0x8003DC08):
            put('target_front' if pc == 0x8003DBF4 else 'target_rear')
        elif pc == 0x8003DBF8:
            put('yaw_strength')
        elif pc == 0x8003DC34:
            put('yaw_shortfall')
        elif pc == 0x8003DD2C:
            put('lateral_correction')
        elif pc in (0x8003DC7C, 0x8003DD40):
            put('yaw')
        elif pc == 0x8003DD58:
            wheel = (address - BASE - 0x4C0) // 0x68
            put(f'input_{wheel}')
        elif pc in (0x8003DDA4, 0x8003DDBC):
            offset = 0x424 if pc == 0x8003DDA4 else 0x42C
            stride = 4 if offset == 0x424 else 2
            axle = (address - BASE - offset) // stride
            put(f'{"slip_feedback" if offset == 0x424 else "grip_feedback"}_{axle}')
        elif pc in (0x8003DDA8, 0x8003DDC4, 0x8003DDD4, 0x8003DDE4, 0x8003DE4C):
            field, offset = {0x8003DDA8: ('speed', 0x2A), 0x8003DDC4: ('grip', 0x34),
                             0x8003DDD4: ('before_attenuation', 0x60),
                             0x8003DDE4: ('slip', 0x44), 0x8003DE4C: ('output', 0x60)}[pc]
            wheel = (address - BASE - 0x460 - offset) // 0x68
            put(f'{field}_{wheel}')
    if sorted(ticks) != list(range(0, frames, 2)):
        raise RuntimeError(f'Missing calls: {run} {sorted(ticks)}')
    rows = []
    for row in ticks.values():
        # These branches do not read their signals when disabled. Export
        # zero only with an explicit observation flag, never a fake reading.
        for field in ('yaw_shortfall', 'lateral_correction', 'yaw'):
            row[f'{field}_consumed'] = int(field in row)
            row.setdefault(field, 0)
        for i in range(4):
            before = row[f'before_attenuation_{i}']
            row.setdefault(f'input_{i}', before)
            row.setdefault(f'output_{i}', before)  # zero travel skips the store
            row[f'slip_consumed_{i}'] = int(f'slip_{i}' in row)
            row.setdefault(f'slip_{i}', 0)
            assert row[f'slip_consumed_{i}'] == int(before != 0)
            assert f'speed_{i}' in row and f'grip_{i}' in row
            assert row[f'allocated_input_{i}'] == row[f'output_{i}']
            assert row[f'input_{i}'] == row[f'pedal_{i}']
            if i < 2:
                row[f'total_{i}'] = row[f'ordinary_{i}']
        rows.append(row)
    assert all(set(r) == set(rows[0]) for r in rows)
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--holdout', action='store_true')
    args = parser.parse_args()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    ram = STATE.with_name('ram.bin').read_bytes()
    for name, frames, replay, interventions in scenarios(args.holdout):
        run = Path('.psx/runs') / f'brake-assist-{name}'
        command = ['psx', 'run', '--from-state', str(STATE), '--frames', str(frames),
                   '--input', json.dumps(replay), '--trace', '--trace-range', '8003DBE8-8003DE68',
                   '--trace-range', '8003E740-8003E74C', '--max-events', '100000',
                   '--trace-range', '8003E2D0-8003E2F8', '--trace-range', '8003A19C-8003A278',
                   '--out', str(run), '--summary']
        for offset, width, value in interventions:
            address = 0x80046EE8 if offset == 'global' else BASE + offset
            command.extend(['--poke', f'{address:08X}={value}:{width}@0..'])
        subprocess.run(command, check=True, capture_output=True, text=True)
        report = json.loads((run / 'report.json').read_text())
        assert report['status'] == 'complete' and report['trace']['lines_dropped'] == 0
        rows = export(run, frames, report['frames']['start'])
        with (OUTPUT / f'{name}.csv').open('w', newline='') as target:
            writer = csv.DictWriter(target, sorted(rows[0]))
            writer.writeheader()
            writer.writerows(rows)
        meta = dict(command=command, source_report=str(run / 'report.json'), ticks=len(rows),
                    holdout=args.holdout, state_sha256=hashlib.sha256(STATE.read_bytes()).hexdigest(),
                    ram_sha256=hashlib.sha256(ram).hexdigest(),
                    psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
                    scope='Original brake-assistance consumer executions; wheel/body inputs remain measured',
                    units='All responses/travel/targets divided by 4096; yaw sign only; yaw_strength percent',
                    source_parameters={hex(o): struct.unpack_from('<'+fmt, ram, (BASE & 0x1FFFFF)+o)[0]
                                       for o, fmt in [(0x1FE, 'h'), (0x2D6, 'h'), (0x424, 'i'),
                                                      (0x428, 'i'), (0x42C, 'h'), (0x42E, 'h')]})
        (OUTPUT / f'{name}.json').write_text(json.dumps(meta, indent=2) + '\n')
        print(f'{name}: {len(rows)} original assistance calls')


if __name__ == '__main__':
    main()
