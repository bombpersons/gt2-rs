#!/usr/bin/env python3
"""Record original surface resistance and forward integration before drivetrain.

All measured data and original traces remain in ignored local directories.
The tyre recorder supplies observations of both slip stages and tyre forces.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
from record_tyre_forces import BASE, EFFECT, export as tyre_rows

OUT = Path('game/tests/data/original/forward_dynamics')
RANGES = ['80039EE0-80039F04', '80039490-80039778', '8003E470-8003E4C0',
          '800397D0-800398F0', '8003D940-8003DAA8', '8003A280-8003A938']
REGISTER = re.compile(r'^(\d+) \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \w+=([0-9a-f]{8})$')


def export(folder, frames):
    report = json.loads((folder/'report.json').read_text())
    assert report['status'] == 'complete' and report['trace']['lines_dropped'] == 0
    start = report['frames']['start']
    cars = {}
    current_wheel = None
    for line in (folder/'trace.txt').open():
        register = REGISTER.match(line.strip())
        if register:
            frame, pc, raw = register.groups()
            field = {0x8003A8B4: 'second_force', 0x8003A8B8: 'second_acceleration',
                     0x8003A8D0: 'second_candidate'}.get(int(pc, 16))
            if field:
                value = int(raw, 16)
                cars.setdefault(int(frame)-start, {})[field] = value if value < 0x80000000 else value-0x100000000
        m = EFFECT.match(line.strip())
        if not m:
            continue
        frame, pc, address, op, raw = m.groups()
        frame, pc, address = int(frame)-start, int(pc, 16), int(address, 16)
        value = int(raw, 16)
        if value >= 1 << (len(raw)*4-1):
            value -= 1 << (len(raw)*4)
        row = cars.setdefault(frame, {})
        field = None
        if pc in [0x8003A2D0, 0x8003A2D4, 0x8003A754, 0x8003A7A4, 0x8003A87C]:
            field, offset = {0x8003A2D0: ('load', 0x468), 0x8003A2D4: ('surface', 0x474),
                             0x8003A754: ('consumed_wheel_force', 0x4B8), 0x8003A7A4: ('flags', 0x4C3),
                             0x8003A87C: ('consumed_ground_force', 0x4BC)}[pc]
            wheel = (address-BASE-offset)//0x68
            assert 0 <= wheel < 4
            if pc == 0x8003A7A4:
                current_wheel = wheel
                assert value in [0, 1, 2, 3], (folder, value)
            field += str(wheel)
        elif pc in [0x8003A760, 0x8003A894]:
            offset = 0x1E if pc == 0x8003A760 else 0x1C
            wheel = (address-0x1F800000-offset)//0x1C
            assert 0 <= wheel < 4
            field = ('cosine' if pc == 0x8003A760 else 'sine')+str(wheel)
        elif pc == 0x8003A7C0:
            assert current_wheel is not None
            assert address == BASE+0x410+2*(current_wheel//2)
        elif pc == 0x8003A898:
            # The original uses the front-left lateral force in ALL four
            # projections here. Observe its address rather than correcting it.
            assert address == BASE+0x4B4
            field = 'consumed_lateral_force'
        elif pc in [0x8003A32C, 0x8003A350, 0x8003A354]:
            field = 'resistance'
        elif pc in [0x8003A804, 0x8003A810]:
            field = 'inverse_mass'
        elif pc in [0x8003A794, 0x8003A8F4]:
            field = 'force'
        elif pc in [0x8003A83C, 0x8003A910]:
            field = 'acceleration'
        elif pc in [0x8003A85C, 0x8003A914]:
            field = 'speed'
            if pc == 0x8003A85C:
                row['first_speed'] = value
        elif pc == 0x8003A738:
            field = 'duration'
        elif pc == 0x8003A820:
            field = 'aerodynamic_force'
        elif pc == 0x8003A8F8:
            row['stopped'] = 1
        elif pc in [0x8003A81C, 0x8003A778, 0x8003A830]:
            assert value == 0, (pc, value)
        if field:
            if field in row and field not in ['force', 'acceleration', 'speed']:
                assert row[field] == value, (folder, field, row[field], value)
            row[field] = value
    assert sorted(cars) == list(range(0, frames, 2))
    for car in cars.values():
        for key in ['resistance', 'inverse_mass', 'force', 'acceleration', 'speed', 'duration', 'aerodynamic_force']:
            assert key in car, (folder, key)
        car.setdefault('stopped', 0)
        if car['first_speed'] == 0:
            assert not any(key in car for key in ['second_force', 'second_acceleration', 'second_candidate'])
            car.update(second_force=0, second_acceleration=0, second_candidate=0)
        else:
            assert all(key in car for key in ['second_force', 'second_acceleration', 'second_candidate'])
            assert all('sine'+str(i) in car and 'consumed_ground_force'+str(i) in car for i in range(4))
            assert 'consumed_lateral_force' in car
        for i in range(4):
            for key in ['load', 'surface', 'flags', 'cosine', 'consumed_wheel_force']:
                assert key+str(i) in car, (folder, key, i)
            # The second phase is skipped when the first update reaches zero.
            car.setdefault('sine'+str(i), 0)
            car.setdefault('consumed_ground_force'+str(i), 0)
        car.setdefault('consumed_lateral_force', 0)
    result = tyre_rows(folder, frames)
    for row in result:
        car = cars[row['run_frame']]
        assert car['consumed_wheel_force'+str(row['wheel'])] == row['wheel_force']
        if car['first_speed'] != 0:
            assert car['consumed_ground_force'+str(row['wheel'])] == row['ground_force']
        row.update({'fd_'+key: value for key, value in car.items()})
    return result


def probes(ram, holdout):
    if holdout:
        for angle, speed in [(137, 197), (-719, -193), (1025, 32769), (-2047, -65537)]:
            yield angle, speed, [3741, -3741, 431, -431], [17391, -17391, 317, -317], [2779, -2779, 2291, -2291], [1, 0, 7319, 15239], [3, 1, 6, 0]
        # The original boundary runs observe a 699-unit second-phase speed
        # change here. Probe both exact zero arrivals and adjacent values.
        for speed in [-700, -699, -698, 698, 699, 700]:
            yield 0, speed, [0]*4, [65536 if speed > 0 else -65536]*4, [0]*4, [8192]*4, [0]*4
        return
    # Every global resistance knot and its neighbours, both directions.
    descriptor = 0x1C8730
    count, xs = struct.unpack_from('<H2xI', ram, descriptor)
    speeds = {0}
    knots = struct.unpack_from('<'+'i'*count, ram, xs&0x1FFFFF)
    for k in knots:
        speeds.update(v for v in [k-1, k, k+1] if v >= 0)
    speeds.update((a+b)//2 for a, b in zip(knots, knots[1:]))
    for speed in sorted(speeds):
        for sign in [-1, 1] if speed else [1]:
            yield 0, sign*speed, [sign*65536]*4, [sign*65536]*4, [0]*4, [8193, 12289, 16385, 4097], [0, 1, 2, 3]
    for surface in range(8):
        yield 0, 197633, [262144]*4, [245760]*4, [0]*4, [8193, 12289, 16385, 4097], [surface]*4
    # Every combination of wheel contact; brake flags must not exclude mass.
    for mask in range(16):
        yield 173, 102605, [131072, -131072, 147456, -147456], [114688, -147456, 131072, -131072], [8192, -4096, -12288, 16384], [8193 if mask & 1<<i else 0 for i in range(4)], [0, 1, 4, 5]
    # Tiny forward/reverse speeds and strong opposite ground-reference forces
    # reach the separate stopping branch without supplying native outputs.
    for speed in [-400, -200, -2, -1, 0, 1, 2, 200, 400]:
        yield 0, speed, [0]*4, [65536 if speed >= 0 else -65536]*4, [0]*4, [8192]*4, [0]*4
    yield 0, 0, [0]*4, [0]*4, [0]*4, [0]*4, [0]*4


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--holdout', action='store_true')
    p.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    args = p.parse_args()
    ram = args.state.with_name('ram.bin').read_bytes()
    name = 'holdout' if args.holdout else 'boundaries'
    rows, records = [], []
    cases = list(probes(ram, args.holdout))
    scenarios = [('mixed', ['SQUARE', 'RIGHT', 'CIRCLE'])] if args.holdout else [('gas', ['CROSS']), ('brake', ['SQUARE']), ('turn', ['CROSS', 'LEFT'])]
    jobs = [(f'{i:03}', 2, [], case) for i, case in enumerate(cases)]
    jobs += [(scenario, 40, buttons, None) for scenario, buttons in scenarios]
    for sequence, (label, frames, buttons, case) in enumerate(jobs):
        folder = Path('.psx/runs')/f'forward-dynamics-{name}-{label}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames), '--input', json.dumps([dict(frame=0, buttons=buttons)]),
                   '--trace', '--max-events', '100000', '--out', str(folder), '--summary']
        for region in RANGES:
            command += ['--trace-range', region]
        if case:
            angle, body, treads, forward, lateral, loads, surfaces = case
            for off, width, value in [(0x45D, 1, 1), (0x60C, 2, angle), (0x64, 2, 4096), (0x66, 2, 4096), (0x36C, 2, 0), (0x36E, 2, angle), (0x6A4, 4, body)]:
                command += ['--poke', f'{BASE+off:08X}={value}:{width}']
            for i in range(4):
                for off, width, value in [(0x8, 4, loads[i]), (0x14, 1, surfaces[i]), (0x18, 4, treads[i]), (0x2C, 4, forward[i]), (0x30, 4, lateral[i]), (0x2A, 2, 4096)]:
                    command += ['--poke', f'{BASE+0x460+i*0x68+off:08X}={value}:{width}']
        subprocess.run(command, check=True, capture_output=True, text=True)
        actual = export(folder, frames)
        if case:
            for i, row in enumerate(actual):
                assert (row['body_forward'], row['tread'], row['forward'], row['lateral'], row['fd_load'+str(i)], row['fd_surface'+str(i)]) == (body, treads[i], forward[i], lateral[i], loads[i], surfaces[i])
        for row in actual:
            row['sequence'] = sequence
            row['case'] = len(rows)//4
            rows.append(row)
        records.append(dict(command=command, report=str(folder/'report.json')))
    OUT.mkdir(parents=True, exist_ok=True)
    with (OUT/f'{name}.csv').open('w', newline='') as f:
        w = csv.DictWriter(f, sorted(rows[0])); w.writeheader(); w.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows)//4, records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(), ram_sha256=hashlib.sha256(ram).hexdigest(),
        psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
        scope='Original forward phase before drivetrain; slip and tyre forces also observed; contact load/grip and aerodynamic input remain measured'), indent=2)+'\n')
    print(f'{len(rows)//4} original {name} forward-dynamics executions exported')


if __name__ == '__main__':
    main()
