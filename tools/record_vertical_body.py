#!/usr/bin/env python3
"""Record original body tilt, vertical impulses and suspension-stop correction.

Outputs are snapshots of original execution, never reference-model results.
Road/suspension inputs are observed; the native test can also evaluate the
spring and damper producer from the earlier suspension boundary.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

from record_suspension import AXLE_FIELDS, BASE, PREFIX, EFFECT

OUT = Path('game/tests/data/original/vertical_body')
PHASES = {0x80043AE0: 'suspension', 0x80043F18: 'before',
          0x80044464: 'integrated', 0x80044898: 'after'}


def export(folder, frames, initial):
    report = json.loads((folder/'report.json').read_text())
    assert report['status'] == 'complete' and report['watch']['complete']
    assert report['trace']['lines_dropped'] == 0
    start = report['frames']['start']
    ram = bytearray(initial)
    writes = iter(sorted((json.loads(l) for l in (folder/'writes.jsonl').open()), key=lambda w: w['cycle']))
    pending = next(writes, None)

    def snapshot(cycle):
        nonlocal pending
        while pending and pending['cycle'] <= cycle:
            p, size = pending['address'] & 0x1FFFFF, pending['width']
            ram[p:p+size] = pending['value'].to_bytes(size, 'little')
            pending = next(writes, None)
        def read(address, fmt='i'):
            return struct.unpack_from('<'+fmt, ram, address & 0x1FFFFF)[0]
        def car(off, fmt='i'):
            return read(BASE+off, fmt)
        fields = [('height', 0x664, 'i'), ('road_height', 0x688, 'i'),
                  ('pitch', 0x644, 'h'), ('roll', 0x646, 'h'), ('heading', 0x648, 'h'),
                  ('pitch_rate', 0x650, 'h'), ('roll_rate', 0x652, 'h'),
                  ('duration', 0x6FE, 'h'), ('inverse_duration', 0x704, 'i'),
                  ('mode', 0x786, 'B'), ('reset_ticks', 0x78E, 'H'), ('airborne', 0x6B1, 'b'),
                  ('mask', 0x719, 'B'), ('normal_up', 0x6A0, 'h'), ('body_up', 0x67C, 'h'),
                  ('inverse_mass', 0x2C, 'h'), ('pitch_response0', 0x6C, 'i'),
                  ('pitch_response1', 0x70, 'i'), ('roll_response0', 0x74, 'i'), ('roll_response1', 0x78, 'i'),
                  ('wheelbase', 0x10, 'h'), ('track', 0x12, 'h'), ('rear_fraction', 0x14, 'h'),
                  ('front_lever', 0x0C, 'h'), ('rear_lever', 0x0E, 'h'),
                  ('front_side', 0x18, 'h'), ('rear_side', 0x1A, 'h'),
                  ('lateral_lever', 0x1E, 'h'), ('forward_lever', 0x20, 'h'),
                  ('front_force', 0x71C, 'i'), ('rear_force', 0x720, 'i'),
                  ('lateral_force', 0x728, 'i'), ('vertical_force', 0x72C, 'i'), ('forward_force', 0x734, 'i')]
        result = {k: car(o, f) for k, o, f in fields}
        result['tick_rate'] = read(0x801C8570)
        record = read(read(0x801C8568)+24)+car(0x6B2, 'b')*40
        result['reset_heading'] = read(record+40)
        for i in range(3):
            result[f'velocity{i}'] = car(0x628+4*i)
            for k, o in [('body_forward', 0x668), ('body_side', 0x670), ('body_normal', 0x678),
                         ('road_forward', 0x68C), ('road_side', 0x694)]:
                result[f'{k}{i}'] = car(o+2*i, 'h')
        for i in range(4):
            for k, o, f in [('road', 0, 'i'), ('load', 8, 'i'), ('travel', 16, 'h'), ('rate', 18, 'h'),
                            ('attachment', 40, 'h'), ('offset', 66, 'h'), ('spring', 72, 'i'), ('damper', 76, 'i')]:
                result[f'{k}{i}'] = car(0x460+i*0x68+o, f)
        for i in range(2):
            result[f'radius{i}'] = car(0x3E4+i*2, 'h')
            for k, o, f in AXLE_FIELDS:
                result[f'axle_{k}{i}'] = car(0x12C+i*0x34+o, f)
        return result

    cars = {}
    for line in (folder/'trace.txt').open():
        m = PREFIX.match(line)
        if not m:
            continue
        absolute, cycle, pc = m.groups()
        frame, cycle, pc = int(absolute)-start, int(cycle), int(pc, 16)
        row = cars.setdefault(frame, {})
        if pc in PHASES:
            phase = PHASES[pc]
            assert phase not in row
            row[phase] = snapshot(cycle)
        effect = EFFECT.search(line.strip())
        if effect:
            address, value = [int(v, 16) for v in effect.groups()]
            value = value if value < 0x80000000 else value-0x100000000
            if pc in [0x80043E64, 0x80043E8C, 0x80043EB4, 0x80043EDC]:
                row[f'body_force{(address-0x1F800018)//4}'] = value
    assert sorted(cars) == list(range(0, frames, 2))
    results = []
    for frame, car in sorted(cars.items()):
        assert set(PHASES.values()) <= set(car), (folder, frame)
        row = dict(run_frame=frame)
        for phase in PHASES.values():
            row.update({f'{phase}_{k}': v for k, v in car[phase].items()})
        row.update({k: car.get(k, 0) for k in [f'body_force{i}' for i in range(4)]})
        results.append(row)
    return results


def cases(ram, holdout):
    if holdout:
        for i in range(18):
            yield {0x644: (2, (i*79)%1103-551), 0x646: (2, (i*137)%1027-513),
                   0x650: (2, (i*7019)%65536-32768), 0x652: (2, (i*3907)%65536-32768),
                   0x664: (4, (i*457)%3701-2101)}
        return
    for rate in [-32768, -16385, -16384, -1, 0, 1, 16383, 16384, 32767]:
        yield {0x650: (2, rate), 0x652: (2, -rate if rate != -32768 else 32767)}
    for angle in [-32768, -513, -512, -511, 0, 511, 512, 513, 32767]:
        yield {0x644: (2, angle), 0x646: (2, -angle if angle != -32768 else 32767)}
    for height in [-32769, -32768, -4097, -1379, -317, 0, 731, 1379, 4097, 32767]:
        yield {0x664: (4, height)}
    for ticks in [0, 1, 2, 14, 15, 16, 134, 135, 136, 149, 150, 151]:
        for height in [-1379, 4097]:
            yield {0x78E: (2, ticks), 0x664: (4, height)}
    def read(address, fmt='i'):
        return struct.unpack_from('<'+fmt, ram, address & 0x1FFFFF)[0]
    record = read(read(0x801C8568)+24)+read(BASE+0x6B2,'b')*40
    target = read(record+40)
    for ticks, offset in [(14, 0), (60, 1228)]:
        for gap in [-1, 0, 1]:
            yield {0x78E: (2, ticks), 0x664: (4, read(BASE+0x688)+offset+gap)}
    for delta in [-2049, -2048, -12, -11, -10, -1, 0, 1, 10, 11, 12, 2047, 2048]:
        yield {0x78E: (2, 60), 0x664: (4, -1379), 0x648: (2, target-delta), 0x654: (4, 0)}
    for mass in [0, -37, 317, 32767]:
        yield {0x2C: (2, mass)}
    for height in [-300, 100, 300]:
        for pitch, roll in [(-300, -200), (-300, 200), (300, -200), (300, 200)]:
            yield {0x664: (4, height), 0x644: (2, pitch), 0x646: (2, roll)}
    for speed in [-10001, -1, 0, 1, 10001]:
        # Inclined retained road axes exercise the velocity floor separately
        # from the newly integrated body basis and the late road refresh.
        yield {0x664: (4, -317), 0x628: (4, speed), 0x630: (4, -731),
               0x68C: (2, 3276), 0x68E: (2, 0), 0x690: (2, 2457),
               0x694: (2, 0), 0x696: (2, 4096), 0x698: (2, 0)}
    for force in [1, 317, 20000, 100000]:
        for rate in [-1, 1]:
            yield {0x650: (2, rate), 0x652: (2, rate),
                   0x140: (4, force), 0x14C: (4, force), 0x144: (4, 0), 0x150: (4, 0)}
    yield {0x786: (1, 7)}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--holdout', action='store_true')
    p.add_argument('--limit', type=int)
    args = p.parse_args()
    original = args.state.with_name('ram.bin').read_bytes()
    name = 'probe' if args.limit else 'holdout' if args.holdout else 'boundaries'
    probes = list(cases(original, args.holdout))
    if args.limit:
        probes = probes[:args.limit]
    jobs = [(str(i), 2, [], case) for i, case in enumerate(probes)]
    if not args.limit:
        jobs += [('natural', 120 if not args.holdout else 84,
                  [dict(frame=0, buttons=['CROSS', 'LEFT']), dict(frame=38, buttons=['SQUARE', 'RIGHT']),
                   dict(frame=64, buttons=['CROSS'])], {})]
    rows, records = [], []
    for sequence, (label, frames, replay, case) in enumerate(jobs):
        folder = Path('.psx/runs')/f'vertical-body-{name}-{label}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames), '--input', json.dumps(replay),
                   '--trace', '--trace-range', '80043AE0-800448C8', '--max-events', '250000',
                   '--watch', f'{BASE:08X}:1952', '--out', str(folder), '--summary']
        changes = dict(case)
        if case:
            changes[0x45D] = (1, 1)
        initial = bytearray(original)
        for off, (width, value) in changes.items():
            address = BASE+off
            command += ['--poke', f'{address:08X}={value}:{width}']
            pos = address & 0x1FFFFF
            initial[pos:pos+width] = (value % (1 << (width*8))).to_bytes(width, 'little')
        subprocess.run(command, check=True, capture_output=True, text=True)
        actual = export(folder, frames, initial)
        for row in actual:
            row.update(sequence=sequence, case=len(rows), controlled=int(bool(case)))
            rows.append(row)
        records.append(dict(command=command, report=str(folder/'report.json')))
    OUT.mkdir(parents=True, exist_ok=True)
    with (OUT/f'{name}.csv').open('w', newline='') as f:
        writer = csv.DictWriter(f, sorted(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows), records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(), ram_sha256=hashlib.sha256(original).hexdigest(),
        psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
        scope='Original body tilt, vertical impulse and suspension-stop correction; road and horizontal force boundary observed'), indent=2)+'\n')
    print(f'{len(rows)} original {name} body calls exported')


if __name__ == '__main__':
    main()
