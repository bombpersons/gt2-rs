#!/usr/bin/env python3
"""Record original suspension travel, spring/damper load and body-force preparation.

Expected values come only from original execution. Road/orientation inputs
are observed at the entry boundary; no reference-model outputs are recorded.
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
OUT = Path('game/tests/data/original/suspension')
PREFIX = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
EFFECT = re.compile(r'; \[([0-9a-f]{8})\]=([0-9a-f]+)$')
PHASES = {0x80039AB0: 'grip_before', 0x80039D9C: 'grip_after',
          0x80043AE0: 'before', 0x80043F18: 'after'}
AXLE_FIELDS = [('bump_start', 0, 'h'), ('minimum', 2, 'h'), ('maximum', 4, 'h'),
               ('height_offset', 8, 'h'), ('spring', 12, 'i'),
               ('rebound_threshold', 16, 'i'), ('rebound_base', 20, 'i'), ('rebound_gain', 24, 'i'),
               ('compression_threshold', 28, 'i'), ('compression_base', 32, 'i'), ('compression_gain', 36, 'i'),
               ('antiroll', 40, 'i'), ('bump_gain', 44, 'i'), ('preload', 50, 'h')]


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
        def car(off, fmt='i'):
            return struct.unpack_from('<'+fmt, ram, (BASE & 0x1FFFFF)+off)[0]
        result = {k: car(o, f) for k, o, f in [
            ('height', 0x664, 'i'), ('normal_up', 0x6A0, 'h'), ('body_up', 0x67C, 'h'),
            ('inverse_duration', 0x704, 'i'), ('mode', 0x786, 'B'), ('contact_mask', 0x719, 'B'),
            ('lateral_lever', 0x1E, 'h'), ('forward_lever', 0x20, 'h'),
            ('front_force', 0x71C, 'i'), ('rear_force', 0x720, 'i'),
            ('lateral_force', 0x728, 'i'), ('vertical_force', 0x72C, 'i'), ('forward_force', 0x734, 'i'),
            ('roll', 0x646, 'h'), ('contact_roll', 0x6F6, 'h')]}
        for i in range(4):
            for k, o, f in [('road_height', 0, 'i'), ('load', 8, 'i'), ('travel', 16, 'h'), ('rate', 18, 'h'),
                            ('surface', 20, 'B'), ('attachment', 40, 'h'), ('road_offset', 66, 'h'),
                            ('spring_force', 72, 'i'), ('damper_force', 76, 'i'), ('capacity', 52, 'i')]:
                result[f'{k}{i}'] = car(0x460+i*0x68+o, f)
        for i in range(2):
            result[f'radius{i}'] = car(0x3E4+i*2, 'h')
            for k, o, f in AXLE_FIELDS:
                result[f'{k}{i}'] = car(0x12C+i*0x34+o, f)
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
            assert phase not in row, 'unexpected multiple cars'
            row[phase] = snapshot(cycle)
        effect = EFFECT.search(line.strip())
        if effect:
            address, value = [int(v, 16) for v in effect.groups()]
            value = value if value < 0x80000000 else value-0x100000000
            if pc == 0x80043C04:
                wheel = (address-0x1F800008)//4
                assert 0 <= wheel < 4
                row[f'requested_travel{wheel}'] = value
            if pc in [0x80043E64, 0x80043E8C, 0x80043EB4, 0x80043EDC]:
                wheel = (address-0x1F800018)//4
                assert 0 <= wheel < 4
                row[f'body_force{wheel}'] = value
    assert sorted(cars) == list(range(0, frames, 2))
    results = []
    for frame, car in sorted(cars.items()):
        assert set(PHASES.values()) <= set(car), (folder, frame, car.keys())
        row = dict(run_frame=frame)
        for phase in PHASES.values():
            row.update({f'{phase}_{k}': v for k, v in car[phase].items()})
        for k, v in car.items():
            if k not in PHASES.values():
                row[k] = v
        assert all(f'{k}{i}' in row for k in ['requested_travel', 'body_force'] for i in range(4))
        results.append(row)
    return results


def cases(ram, holdout):
    def car(off):
        return struct.unpack_from('<h', ram, (BASE & 0x1FFFFF)+off)[0]
    front, rear = car(0x470), car(0x540)
    bump, minimum = car(0x12C), car(0x12E)
    if holdout:
        for rate in [-32768, -4095, -1703, -1, 1, 1779, 4095, 32767]:
            yield dict(old_rate=rate, old_travel=[-719, -397, -577, -173])
        for height in [-317, 197, 731, 1379]:
            yield dict(height=height, limits=[-997, -1791, -317], inverse_duration=118784)
        for threshold in [-137, 0, 197]:
            yield dict(damper_threshold=threshold, old_rate=317)
        return
    for rate in [-32768, -8194, -8193, -8192, -8191, -1, 0, 1, 8191, 8192, 8193, 8194, 32767]:
        # One-second inverse duration and deliberately different histories
        # expose average/sign/clamp boundaries on every wheel.
        yield dict(old_rate=rate, inverse_duration=4096)
    for minimum in [minimum, front-1, front, front+1, 0, 317]:
        yield dict(limits=[bump, minimum, 731])
    for maximum in [front-1, front, front+1, rear-1, rear, rear+1]:
        yield dict(limits=[bump, car(0x12E), maximum])
    for bump in [car(0x12E), front-1, front, front+1, 0, front+32768, front+32769]:
        yield dict(limits=[bump, car(0x12E), 731])
    for height in [-1379, -317, 0, 731, 1379]:
        yield dict(height=height)
    for history in [[0]*4, [1, -1, 1, -1], [-1, 1, -1, 1], [-719, -397, -577, -173]]:
        yield dict(old_travel=history)
    for preload in [-32768, -1, 0, 1, 32767]:
        yield dict(preload=preload)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--holdout', action='store_true')
    p.add_argument('--limit', type=int)
    args = p.parse_args()
    original = args.state.with_name('ram.bin').read_bytes()
    name = 'probe' if args.limit is not None else 'holdout' if args.holdout else 'boundaries'
    steady_history = [struct.unpack_from('<h', original, (BASE & 0x1FFFFF)+0x470+i*0x68)[0] for i in range(4)]
    probes = list(cases(original, args.holdout))
    if args.limit is not None:
        probes = probes[:args.limit]
    jobs = [(f'{i:03}', 2, [], case) for i, case in enumerate(probes)]
    if args.limit is None:
        scenarios = [('alternating', [dict(frame=0, buttons=['CROSS', 'RIGHT']), dict(frame=14, buttons=['SQUARE', 'CIRCLE']),
                                      dict(frame=38, buttons=['CROSS', 'LEFT']), dict(frame=64, buttons=[])])] if args.holdout else [
            ('gas', [dict(frame=0, buttons=['CROSS'])]), ('brake', [dict(frame=0, buttons=['SQUARE'])]),
            ('turn', [dict(frame=0, buttons=['CROSS', 'LEFT'])])]
        jobs += [(label, 80, replay, None) for label, replay in scenarios]
    rows, records = [], []
    for sequence, (label, frames, replay, case) in enumerate(jobs):
        folder = Path('.psx/runs')/f'suspension-{name}-{label}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames), '--input', json.dumps(replay),
                   '--trace', '--trace-range', '800438F0-80043F1C', '--trace-range', '80039A4C-80039DE8',
                   '--max-events', '200000', '--watch', f'{BASE:08X}:1952', '--out', str(folder), '--summary']
        changes = []
        if case:
            changes += [(BASE+0x45D, 1, 1)]  # Freeze earlier contact preprocessing, observe actual inputs.
            if 'height' in case:
                changes += [(BASE+0x664, 4, case['height'])]
            if 'inverse_duration' in case:
                changes += [(0x801C8570, 4, case['inverse_duration']//4096)]
            for i in range(4):
                b = BASE+0x460+i*0x68
                changes += [(b+0x10, 2, case.get('old_travel', steady_history)[i]),
                            (b+0x12, 2, case.get('old_rate', 0))]
            for i in range(2):
                b = BASE+0x12C+i*0x34
                if 'limits' in case:
                    changes += [(b+o, 2, v) for o, v in zip([0, 2, 4], case['limits'])]
                if 'preload' in case:
                    changes += [(b+50, 2, case['preload'])]
                if 'damper_threshold' in case:
                    changes += [(b+16, 4, case['damper_threshold']), (b+28, 4, case['damper_threshold'])]
        initial = bytearray(original)
        for address, width, value in changes:
            command += ['--poke', f'{address:08X}={value}:{width}']
            pos = address & 0x1FFFFF
            initial[pos:pos+width] = (value % (1 << (width*8))).to_bytes(width, 'little')
        subprocess.run(command, check=True, capture_output=True, text=True)
        actual = export(folder, frames, initial)
        if case:
            r = actual[0]
            assert [r[f'before_travel{i}'] for i in range(4)] == case.get('old_travel', steady_history)
            assert all(r[f'before_rate{i}'] == case.get('old_rate', 0) for i in range(4))
            if 'inverse_duration' in case:
                assert r['before_inverse_duration'] == case['inverse_duration']
            for i in range(2):
                if 'limits' in case:
                    assert [r[f'before_{k}{i}'] for k in ['bump_start', 'minimum', 'maximum']] == case['limits']
                if 'damper_threshold' in case:
                    assert r[f'before_compression_threshold{i}'] == case['damper_threshold']
        for r in actual:
            r.update(sequence=sequence, case=len(rows), controlled=int(case is not None))
            rows.append(r)
        records.append(dict(command=command, report=str(folder/'report.json')))
    OUT.mkdir(parents=True, exist_ok=True)
    with (OUT/f'{name}.csv').open('w', newline='') as f:
        writer = csv.DictWriter(f, sorted(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows), records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(), ram_sha256=hashlib.sha256(original).hexdigest(),
        psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
        scope='Original suspension load and body-force preparation; road/orientation and intervening collision observed'), indent=2)+'\n')
    print(f'{len(rows)} original {name} suspension calls exported')


if __name__ == '__main__':
    main()
