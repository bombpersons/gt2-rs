#!/usr/bin/env python3
"""Record original stability signals, traction throttle and brake consumers.

All expected results come from original execution, including signed/overflow
probes. Traces and numerical reference fixtures stay private and ignored.
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
OUT = Path('game/tests/data/original/driver_assist')
PREFIX = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
EFFECT = re.compile(r'; \[([0-9a-f]+)\](=>|=)([0-9a-f]+)$')
PHASES = {0x8003E4FC: 'before', 0x8003E68C: 'signals', 0x8003DE68: 'traction_before',
          0x8003DFCC: 'traction_after', 0x8003DBE8: 'brake_before', 0x8003DE64: 'brake_after',
          0x8003A198: 'prepared', 0x8003A278: 'allocated'}


def export(folder, frames, initial):
    report = json.loads((folder / 'report.json').read_text())
    assert report['status'] == 'complete' and report['watch']['complete']
    assert report['trace']['lines_dropped'] == 0
    start = report['frames']['start']
    ram, scratch = bytearray(initial), bytearray(1024)
    writes = iter(sorted((json.loads(line) for line in (folder / 'writes.jsonl').open()), key=lambda e: e['cycle']))
    pending = next(writes, None)
    def snapshot(cycle):
        nonlocal pending
        while pending and pending['cycle'] <= cycle:
            p, size = pending['address'] & 0x1FFFFF, pending['width']
            ram[p:p + size] = pending['value'].to_bytes(size, 'little')
            pending = next(writes, None)
        def car(offset, fmt='h'):
            return struct.unpack_from('<' + fmt, ram, (BASE & 0x1FFFFF) + offset)[0]
        values = {key: car(off, fmt) for key, off, fmt in [
            ('forward', 0x6A4, 'i'), ('lateral', 0x6A8, 'i'), ('yaw', 0x64C, 'i'),
            ('steering', 0x60C, 'h'), ('feedback', 0x64A, 'h'), ('wheelbase', 0x10, 'h'),
            ('layout', 0x370, 'B'), ('engagement', 0x619, 'B'), ('gear', 0x618, 'B'),
            ('throttle', 0x708, 'h'), ('available', 0x710, 'i'), ('source', 0x45D, 'b'),
            ('gain', 0x450, 'i'), ('fade', 0x454, 'h'), ('speed_gain', 0x456, 'h'),
            ('speed_response', 0x700, 'h'), ('correction_gain', 0x458, 'h'), ('correction_threshold', 0x45A, 'h'),
            ('target0', 0x1FC, 'h'), ('target1', 0x2D4, 'h'), ('brake_target0', 0x1FE, 'h'),
            ('brake_target1', 0x2D6, 'h'), ('slip_gain0', 0x424, 'i'), ('slip_gain1', 0x428, 'i'),
            ('grip_gain0', 0x42C, 'h'), ('grip_gain1', 0x42E, 'h'), ('handbrake', 0x614, 'h')
        ]}
        values['yaw_strength'] = ram[0x46EE8]
        values['yaw_shortfall'], values['lateral_correction'] = struct.unpack_from('<hh', scratch, 0x90)
        values['brake_capacity0'] = car(0x414, 'i')
        values['brake_capacity1'] = car(0x418, 'i')
        values['handbrake_capacity'] = car(0x41C, 'i')
        for i in range(4):
            for key, off, fmt in [('load', 8, 'i'), ('slip', 0x44, 'h'), ('brake', 0x60, 'h'),
                                  ('response', 0x2A, 'h'), ('grip', 0x34, 'i')]:
                values[f'{key}{i}'] = car(0x460 + 0x68 * i + off, fmt)
            values[f'demand{i}'] = struct.unpack_from('<i', scratch, 4 + 0x1C * i)[0]
        return values
    cars = {}
    cleared = {}
    for line in (folder / 'trace.txt').open():
        prefix = PREFIX.match(line)
        if not prefix:
            continue
        absolute, cycle, pc = prefix.groups()
        frame, cycle, pc = int(absolute) - start, int(cycle), int(pc, 16)
        effect = EFFECT.search(line.strip())
        if effect and effect[2] == '=':
            address, raw = int(effect[1], 16), effect[3]
            if 0x1F800000 <= address < 0x1F800400:
                data = bytes.fromhex(raw)[::-1]
                p = address - 0x1F800000
                scratch[p:p + len(data)] = data
            if pc in [0x8003E4FC, 0x8003E500]:
                cleared.setdefault(frame, set()).add(pc)
        if pc in PHASES:
            row = cars.setdefault(frame, {})
            phase = PHASES[pc]
            assert phase not in row, 'single car checkpoint required'
            row[phase] = snapshot(cycle)
    assert sorted(cars) == list(range(0, frames, 2))
    rows = []
    for frame, car in sorted(cars.items()):
        assert {'before', 'signals', 'prepared', 'allocated'} <= set(car)
        assert cleared[frame] == {0x8003E4FC, 0x8003E500}, 'observe both scratch signal clears'
        assert ('traction_before' in car) == ('traction_after' in car)
        assert ('brake_before' in car) == ('brake_after' in car)
        row = dict(run_frame=frame, traction_called=int('traction_before' in car), brake_called=int('brake_before' in car))
        for phase in PHASES.values():
            values = car.get(phase, car['signals'])
            row.update({f'{phase}_{key}': value for key, value in values.items()})
        rows.append(row)
    return rows


def cases(holdout):
    if holdout:
        for i in range(18):
            yield {0x45D: (1, 1), 0x6A4: (4, (i * 73991) % 1179173 - 197317),
                   0x6A8: (4, (i * 17391) % 171997 - 79173), 0x64C: (4, (i * 7919) % 349997 - 173911),
                   0x60C: (2, (i * 317) % 3971 - 1979)}
        for layout in range(7):
            yield {0x370: (1, layout), 0x619: (1, 1), 0x450: (4, 7319), 0x454: (2, 137),
                   0x456: (2, 1979), 0x700: (2, 791), 0x64A: (2, -1371), 0x618: (1, layout % 3),
                   0x48C: (4, 17391), 0x478: (4, 37917)}
        # Fresh severe-slip probes across every driven layout. The earlier
        # holdout exercised partial suppression but no complete throttle cut.
        for layout in range(7):
            case = {0x370: (1, layout), 0x619: (1, 1), 0x450: (4, 19031),
                    0x454: (2, 293), 0x456: (2, 1117), 0x700: (2, 677),
                    0x64A: (2, -1201), 0x618: (1, 2)}
            for wheel in range(4):
                base = 0x460 + wheel * 0x68
                case[base + 0x18] = (4, 197531 + wheel * 379)
                case[base + 0x2C] = (4, 13719 + wheel * 193)
            yield case
        return
    for speed in [-409601, -1, 0, 1, 4096, 409601]:
        for angle, yaw in [(0, 0), (317, 1), (-317, -1), (317, -17391), (-317, 17391), (2048, 7319)]:
            yield {0x45D: (1, 1), 0x6A4: (4, speed), 0x6A8: (4, 79173),
                   0x60C: (2, angle), 0x64C: (4, yaw)}
    for lateral in [-32769, -1, 0, 1, 32769]:
        for yaw in [-17391, 0, 17391]:
            yield {0x45D: (1, 1), 0x6A8: (4, lateral), 0x64C: (4, yaw)}
    for yaw in [-2147483648, -524289, 524289, 2147483647]:
        yield {0x45D: (1, 1), 0x60C: (2, 17391), 0x64C: (4, yaw)}
    for layout in range(7):
        for engagement in [0, 1, 2, 3, 4]:
            yield {0x370: (1, layout), 0x619: (1, engagement), 0x450: (4, 8192),
                   0x454: (2, 1024), 0x48C: (4, 65536), 0x478: (4, 131072),
                   0x55C: (4, 65536), 0x548: (4, 196608)}
    for fade in [-317, 1, 4096, 8192]:
        for feedback in [-2049, -1024, 1024, 2049]:
            yield {0x619: (1, 1), 0x450: (4, 17391), 0x454: (2, fade), 0x64A: (2, feedback),
                   0x456: (2, 4096), 0x700: (2, 8191), 0x478: (4, 131072)}
    for gear in [0, 1, 5]:
        for gain in [-8192, 0, 8192]:
            yield {0x619: (1, 1), 0x618: (1, gear), 0x450: (4, gain), 0x454: (2, 1),
                   0x478: (4, 131072), 0x48C: (4, 65536), 0x468: (4, 0)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    parser.add_argument('--holdout', action='store_true')
    parser.add_argument('--limit', type=int)
    args = parser.parse_args()
    initial = args.state.with_name('ram.bin').read_bytes()
    name = 'probe' if args.limit else 'holdout' if args.holdout else 'boundaries'
    jobs = [(str(i), 2, [], case) for i, case in enumerate(list(cases(args.holdout))[:args.limit])]
    if not args.limit:
        jobs += [('mixed', 80, [dict(frame=0, buttons=['CROSS', 'LEFT']), dict(frame=18, buttons=['SQUARE', 'RIGHT']),
                                dict(frame=42, buttons=['CROSS', 'CIRCLE']), dict(frame=62, buttons=[])], {}),
                 ('traction', 40, [dict(frame=0, buttons=['SQUARE']), dict(frame=16, buttons=['CROSS', 'RIGHT'])],
                  {0x619: (1, 1), 0x450: (4, 7913 if args.holdout else 8192), 0x454: (2, 197 if args.holdout else 1024),
                   0x458: (2, 137), 0x45A: (2, -173)})]
    rows, records = [], []
    for sequence, (label, frames, replay, case) in enumerate(jobs):
        folder = Path('.psx/runs') / f'driver-assist-{name}-{label}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames), '--input', json.dumps(replay),
                   '--trace', '--max-events', '1000000', '--watch', f'{BASE:08X}:1952', '--out', str(folder), '--summary']
        for region in ['8003E4FC-8003E7A8', '8003DBE8-8003E020', '8003A198-8003A28C']:
            command += ['--trace-range', region]
        ram = bytearray(initial)
        for offset, (width, value) in case.items():
            address = BASE + offset
            command += ['--poke', f'{address:08X}={value}:{width}']
            p = address & 0x1FFFFF
            ram[p:p + width] = (value % (1 << (width * 8))).to_bytes(width, 'little')
        subprocess.run(command, check=True, capture_output=True, text=True)
        actual = export(folder, frames, ram)
        for row in actual:
            row.update(sequence=sequence, case=len(rows))
            rows.append(row)
        records.append(dict(command=command, report=str(folder / 'report.json')))
    OUT.mkdir(parents=True, exist_ok=True)
    with (OUT / f'{name}.csv').open('w', newline='') as target:
        writer = csv.DictWriter(target, sorted(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    (OUT / f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows), records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(), ram_sha256=hashlib.sha256(initial).hexdigest(),
        psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
        scope='Original stability signals, traction reduction and per-wheel brake consumption; motion/load/slip inputs observed'), indent=2) + '\n')
    print(f'{len(rows)} original {name} driver assistance calls exported', flush=True)


if __name__ == '__main__':
    main()
