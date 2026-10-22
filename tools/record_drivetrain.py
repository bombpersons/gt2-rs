#!/usr/bin/env python3
"""Record the original engine/gear/centre/axle coupling with outer wheel phases.

Only the original executes the expected rules. All snapshots, traces, tuning
and numerical fixtures remain in ignored local directories.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

from record_forward_dynamics import RANGES
from record_tyre_forces import BASE, EFFECT
from record_wheel_rotation import export as wheel_rows, PREFIX

OUT = Path('game/tests/data/original/drivetrain')
REGISTER = re.compile(r'^\d+ \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \w+=([0-9a-f]{8})$')


def export(folder, frames, initial):
    report = json.loads((folder/'report.json').read_text())
    assert report['status'] == 'complete' and report['trace']['lines_dropped'] == 0
    assert report['watch']['complete']
    start = report['frames']['start']
    ram, scratch = bytearray(initial), bytearray(1024)
    writes = iter(sorted((json.loads(line) for line in (folder/'writes.jsonl').open()), key=lambda e: e['cycle']))
    pending = next(writes, None)
    external_speed = [False, False]

    def snapshot(cycle):
        nonlocal pending
        while pending and pending['cycle'] <= cycle:
            address, size = pending['address'], pending['width']
            data = pending['value'].to_bytes(size, 'little')
            if 0x1F800000 <= address < 0x1F800400:
                p = address-0x1F800000
                scratch[p:p+size] = data
            else:
                p = address & 0x1FFFFF
                ram[p:p+size] = data
                for axle in range(2):
                    if address == BASE+0x634+axle*4 and not 0x80045AE8 <= pending['pc'] < 0x800465E0:
                        assert pending['pc'] == 0x80030830, 'new upstream shaft writer requires investigation'
                        external_speed[axle] = True
            pending = next(writes, None)
        def car(offset, fmt='i'):
            return struct.unpack_from('<'+fmt, ram, (BASE & 0x1FFFFF)+offset)[0]
        result = {}
        for name, offset, fmt in [
            ('layout', 0x370, 'B'), ('source', 0x372, 'B'), ('reference', 0x373, 'B'),
            ('viscosity', 0x374, 'h'), ('lock_torque', 0x376, 'h'), ('engine_inertia', 0x378, 'h'),
            ('gear', 0x618, 'B'), ('engagement', 0x619, 'B'), ('locked', 0x63C, 'B'),
            ('engine_speed', 0x624, 'i'), ('rpm', 0x6AC, 'h'), ('cut', 0x61D, 'B'),
            ('throttle', 0x708, 'h'), ('angle', 0x64A, 'h'), ('brake', 0x612, 'h'),
            ('fixed_clutch', 0x3F4, 'h'), ('coast_clutch', 0x3F6, 'h'),
            ('engine_response', 0x40C, 'i'), ('centre_response', 0x408, 'i'),
            ('full_torque', 0x710, 'i'), ('drag', 0x714, 'i'), ('induction', 0x128, 'h'),
        ]:
            result[name] = car(offset, fmt)
        for i in range(8):
            result[f'gear_ratio{i}'] = car(0x3A4+i*4)
        for axle in range(2):
            result[f'external_speed{axle}'] = external_speed[axle]
            result[f'wheel_kind{axle}'] = car(0x37A+axle, 'B')
            result[f'axle_inertia{axle}'] = car(0x400+axle*4)
            result[f'speed{axle}'] = car(0x634+axle*4)
            result[f'delta{axle}'] = car(0x4DE+axle*0xD0, 'h')
            result[f'clutch_speed{axle}'] = struct.unpack_from('<i', scratch, 0x7C+axle*4)[0]
            result[f'torque{axle}'] = struct.unpack_from('<i', scratch, 0x74+axle*4)[0]
        for i in range(4):
            result[f'flags{i}'] = car(0x4C3+i*0x68, 'B')
        result['inertia_fraction'] = struct.unpack_from('<i', scratch, 0x8C)[0]
        result['alternate_locked_step'] = ram[0x1D5866] == 3
        return result

    cars = {}
    for line in (folder/'trace.txt').open():
        prefix = PREFIX.match(line)
        if not prefix:
            continue
        absolute, cycle, pc = prefix.groups()
        frame, pc = int(absolute)-start, int(pc, 16)
        branch = {0x800460CC: 'stop', 0x800461D8: 'stop', 0x800464C4: 'stop',
                  0x80046264: 'centre_clamp', 0x80046580: 'centre_acquire',
                  0x80045ED4: 'centre_release', 0x80046060: 'locked_direct'}.get(pc)
        if branch:
            cars[frame][branch] = cars[frame].get(branch, 0)+1
        # Scratchpad accesses are present in the instruction trace but are
        # not emitted by the toolkit's RAM write watch. Retain real stores
        # here, including the upstream clutch-speed preparation.
        effect = EFFECT.match(line.strip())
        if effect and effect[4] == '=':
            address = int(effect[3], 16)
            if 0x1F800000 <= address < 0x1F800400:
                data = bytes.fromhex(effect[5])[::-1]
                p = address-0x1F800000
                scratch[p:p+len(data)] = data
        if pc in [0x80045AE8, 0x800465E0]:
            key = 'before' if pc == 0x80045AE8 else 'after'
            car = cars.setdefault(frame, {})
            assert key not in car
            car[key] = snapshot(int(cycle))
            if key == 'after':
                external_speed[:] = [False, False]
        register = REGISTER.match(line.strip())
        if register and pc in [0x80045C40, 0x80045C70, 0x80045C80, 0x80045D84]:
            raw = int(register[2], 16)
            value = raw if raw < 0x80000000 else raw-0x100000000
            key = 'gearbox' if pc == 0x80045D84 else 'source_torque'
            assert key not in cars[frame]
            cars[frame][key] = value
    assert sorted(cars) == list(range(0, frames, 2))
    for car in cars.values():
        assert 'before' in car and 'after' in car and 'gearbox' in car
        if car['before']['engagement']:
            assert 'source_torque' in car
        car.setdefault('source_torque', 0)
    rows = wheel_rows(folder, frames, initial)
    for row in rows:
        car = cars[row['run_frame']]
        for phase in ['before', 'after']:
            row.update({f'dt_{phase}_{key}': int(value) for key, value in car[phase].items()})
        row['dt_gearbox'] = car['gearbox']
        row['dt_source_torque'] = car['source_torque']
        for branch in ['stop', 'centre_clamp', 'centre_acquire', 'centre_release', 'locked_direct']:
            row['dt_'+branch] = car.get(branch, 0)
        for axle in range(2):
            assert car['after'][f'delta{axle}'] == row[f'rot_before_delta{axle*2+1}']
    return rows


def cases(holdout, coupling_holdout=False):
    if coupling_holdout:
        for angle in [0, 1024, 1025, 4095, 4096, 4097, -1025, -4097]:
            for brake in [3071, 3072, 3073]:
                yield (5, 3, 1, 1, [8192, 0], [137, -193], angle,
                       {0x612: (2, brake), 0x373: (1, 0), 0x1D5866-(BASE&0x1FFFFF): (1, 3)})
        for layout in range(7):
            for gear in [1, 2, 5]:
                yield layout, 1, 0, 1, [13011, 713], [-137, 193], -193, {0x618: (1, gear), 0x373: (1, 1-layout%2)}
        for layout in [0, 1, 3, 4, 5, 6]:
            for speeds in [[0, 0], [1, -1], [-1, 1], [4000001, 4000193], [-4000001, -4000193]]:
                brakes = {0x4C0+i*0x68: (2, 4096) for i in range(4)}
                yield layout, 1, 0, 1, speeds, [17, -19], 137, brakes
        return
    if holdout:
        for layout in range(7):
            for engagement in [1, 2, 4]:
                yield layout, engagement, layout % 2, -1, [-317, 791], [32767, -32768], 137
        return
    for layout in range(7):
        for engagement in range(5):
            for locked in [0, 1]:
                for sign in [-1, 1]:
                    yield layout, engagement, locked, sign, [sign*81921, sign*49153], [17, -19], 0
    for layout in [4, 5]:
        # Centre-slip crossing with opposite integration history, to enter
        # the persistent locked state rather than only test its release.
        yield layout, 3, 0, 1, [8192, 0], [-32768, 32767], 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    parser.add_argument('--holdout', action='store_true')
    parser.add_argument('--coupling-holdout', action='store_true')
    args = parser.parse_args()
    original = args.state.with_name('ram.bin').read_bytes()
    assert not (args.holdout and args.coupling_holdout)
    name = 'coupling_holdout' if args.coupling_holdout else 'holdout' if args.holdout else 'boundaries'
    rows, records = [], []
    jobs = [(f'{i:03}', 2, [], case) for i, case in enumerate(cases(args.holdout, args.coupling_holdout))]
    natural = [] if args.coupling_holdout else [('mixed', ['SQUARE', 'RIGHT', 'CIRCLE'])] if args.holdout else [
        ('gas', ['CROSS']), ('brake', ['SQUARE']), ('turn', ['CROSS', 'LEFT'])]
    jobs += [(label, 40, buttons, None) for label, buttons in natural]
    if not args.holdout and not args.coupling_holdout:
        jobs.append(('centre-sequence', 40, [], (4, 3, 0, 1, [8192, 0], [-32768, 32767], 0)))
    for sequence, (label, frames, buttons, case) in enumerate(jobs):
        folder = Path('.psx/runs')/f'drivetrain-{name}-{label}'
        initial = bytearray(original)
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames),
                   '--input', json.dumps([dict(frame=0, buttons=buttons)]), '--trace', '--max-events', '500000',
                   '--watch', f'{BASE:08X}:1952', '--watch', '1F800000:148', '--out', str(folder), '--summary']
        for region in RANGES + ['8003A000-8003A280', '8003E4F0-8003E5E8', '80045688-800465E4']:
            command += ['--trace-range', region]
        # Wheel recorder takes its final snapshot on the return delay slot.
        command += ['--trace-range', '800465E4-80046B58']
        if case:
            layout, engagement, locked, sign, speeds, previous, angle = case[:7]
            changes = [(0x45D, 1, 1), (0x370, 1, layout), (0x372, 1, 5), (0x373, 1, layout % 2),
                       (0x374, 2, 2048 if layout in [4, 6] else 137), (0x376, 2, 1024),
                       (0x618, 1, 0 if sign < 0 else 3), (0x619, 1, engagement), (0x63C, 1, locked),
                       (0x60A, 2, 0 if engagement == 0 else 1),
                       (0x624, 4, 200001), (0x6AC, 2, 3000), (0x61D, 1, 0), (0x708, 2, 4096 if sign > 0 else 0),
                       (0x3F4, 2, 512), (0x3F6, 2, 537), (0x64A, 2, angle),
                       (0x64, 2, 4096), (0x66, 2, 4096), (0x36C, 2, 0), (0x36E, 2, 173), (0x60C, 2, 173)]
            for axle in range(2):
                changes += [(0x37A+axle, 1, 2), (0x634+axle*4, 4, speeds[axle]),
                            (0x4DE+axle*0xD0, 2, previous[axle])]
            if len(case) == 8:
                changes += [(offset, width, value) for offset, (width, value) in case[7].items()]
            for offset, width, value in changes:
                command += ['--poke', f'{BASE+offset:08X}={value}:{width}']
                p = (BASE & 0x1FFFFF)+offset
                initial[p:p+width] = (value % (1 << (width*8))).to_bytes(width, 'little')
        subprocess.run(command, check=True, capture_output=True, text=True)
        actual = export(folder, frames, initial)
        if case:
            first = actual[0]
            assert (first['dt_before_layout'], first['dt_before_engagement']) == (layout, engagement)
            assert [first[f'dt_before_speed{i}'] for i in range(2)] == speeds
        for row in actual:
            row['sequence'] = sequence
            row['case'] = len(rows)//4
            rows.append(row)
        records.append(dict(command=command, report=str(folder/'report.json')))
    OUT.mkdir(parents=True, exist_ok=True)
    with (OUT/f'{name}.csv').open('w', newline='') as f:
        writer = csv.DictWriter(f, sorted(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(
        car_executions=len(rows)//4, records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
        ram_sha256=hashlib.sha256(original).hexdigest(),
        psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
        scope='Original engine engagement, gearing, centre/axle integration and outer tyre/wheel phases; upstream contact and torque-controller inputs observed'
    ), indent=2)+'\n')
    print(f'{len(rows)//4} original {name} drivetrain executions exported')


if __name__ == '__main__':
    main()
