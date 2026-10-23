#!/usr/bin/env python3
"""Record original drag/downforce, reduction history and its native consumers."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

from record_vertical_body import BASE, PREFIX, export as body_rows

OUT = Path('game/tests/data/original/aerodynamics')
PHASES = {0x8003DAA8: 'aero_before', 0x8003DBE0: 'aero_after',
          0x8003A738: 'forward_before', 0x8003A93C: 'forward_after'}
REGISTER = re.compile(r'; \w+=([0-9a-f]{8})$')


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
        result = {k: car(o, f) for k, o, f in [
            ('speed', 0x6A4, 'i'), ('growth', 0x774, 'h'), ('reduction', 0x776, 'h'),
            ('duration', 0x6FE, 'h'), ('drag', 0x71A, 'h'), ('axle0', 0x71C, 'i'), ('axle1', 0x720, 'i'),
            ('drag_coefficient', 0x43C, 'i'), ('front_coefficient', 0x440, 'i'), ('rear_coefficient', 0x444, 'i'),
            ('force', 0x734, 'i'), ('acceleration', 0x724, 'i')]}
        result['minimum_response'] = read(0x80046EF4)
        for i in range(4):
            for k, o, f in [('load', 8, 'i'), ('surface', 20, 'B'), ('steering', 12, 'h'),
                            ('wheel_force', 88, 'i'), ('ground_force', 92, 'i'), ('lateral_force', 84, 'i')]:
                result[f'{k}{i}'] = car(0x460+i*0x68+o, f)
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
        key = {0x8003DAD4: 'square', 0x8003DB74: 'response', 0x8003DB84: 'effective'}.get(pc)
        register = REGISTER.search(line.strip())
        if key:
            assert register, line
            v = int(register[1], 16)
            row[key] = v if v < 0x80000000 else v-0x100000000
    result = body_rows(folder, frames, initial)
    for row in result:
        car = cars[row['run_frame']]
        assert set(PHASES.values()) <= set(car)
        for phase in PHASES.values():
            row.update({f'{phase}_{k}': v for k, v in car[phase].items()})
        row.update({f'aero_{k}': car[k] for k in ['square', 'response', 'effective']})
    return result


def cases(holdout, ram):
    if holdout:
        for i in range(16):
            yield {0x6A4: (4, (i*197123)%1179173-589317), 0x774: (2, (i*1739)%14003-7001),
                   0x776: (2, (i*719)%8193-2049)}
        return
    for speed in [-8388608, -1482912, -32769, -32768, -32767, -4097, -4096, -4095, -33, -32, -31,
                  -1, 0, 1, 31, 32, 33, 4095, 4096, 4097, 32767, 32768, 32769, 1482912, 8388608]:
        yield {0x6A4: (4, speed)}
    recovery=struct.unpack_from("<h",ram,(BASE&0x1FFFFF)+0x6FE)[0]//64
    for growth in [-32768, -4097, -1, 0, 1, 4095, 4096, 32767]:
        for reduction in [-32768, -1, 0, recovery-1, recovery, recovery+1, 4095, 4096, 4097, 32767]:
            yield {0x774: (2, growth), 0x776: (2, reduction)}
    for minimum in [-317, 0, 1, 4095, 4096, 4097, 8191]:
        yield {0x80046EF4: (4, minimum), 0x774: (2, 317), 0x776: (2, 1979)}
    for rate in [16, 25, 45, 60]:
        yield {0x801C8570: (4, rate), 0x774: (2, 317), 0x776: (2, 1979)}
    for coefficient in [-32769, -1, 0, 1, 32767, 65537]:
        yield {0x43C: (4, coefficient), 0x440: (4, coefficient-1), 0x444: (4, coefficient+1)}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--holdout', action='store_true')
    p.add_argument('--limit', type=int)
    args = p.parse_args()
    original = args.state.with_name('ram.bin').read_bytes()
    name = 'probe' if args.limit else 'holdout' if args.holdout else 'boundaries'
    probes = list(cases(args.holdout, original))
    if args.limit:
        probes = probes[:args.limit]
    jobs = [(str(i), 2, [], case) for i, case in enumerate(probes)]
    if not args.limit:
        jobs += [('natural', 100 if not args.holdout else 84,
                  [dict(frame=0, buttons=['CROSS', 'LEFT']), dict(frame=30, buttons=['SQUARE', 'RIGHT']),
                   dict(frame=60, buttons=['CROSS'])], {})]
    if not args.limit:
        jobs += [('recovery',32 if args.holdout else 40,[],
                  {0x776:(2,3197 if args.holdout else 4096),0x774:(2,317 if args.holdout else 4096)})]
    rows, records = [], []
    for sequence, (label, frames, replay, case) in enumerate(jobs):
        folder = Path('.psx/runs')/f'aerodynamics-{name}-{label}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames), '--input', json.dumps(replay),
                   '--trace', '--trace-range', '8003DAA8-8003DBE8', '--trace-range', '8003A710-8003A940',
                   '--trace-range', '80043AE0-800448C8', '--max-events', '250000',
                   '--watch', f'{BASE:08X}:1952', '--out', str(folder), '--summary']
        changes = {off if off>=0x80000000 else BASE+off: v for off, v in case.items()}
        if case:
            changes[BASE+0x45D] = (1, 1)
        initial = bytearray(original)
        for address, (width, value) in changes.items():
            command += ['--poke', f'{address:08X}={value}:{width}']
            pos = address & 0x1FFFFF
            initial[pos:pos+width] = (value % (1 << (width*8))).to_bytes(width, 'little')
        subprocess.run(command, check=True, capture_output=True, text=True)
        actual = export(folder, frames, initial)
        if case:
            assert actual[0]['aero_before_growth'] == case.get(0x774, (2, 0))[1]
            if 0x776 in case:
                assert actual[0]['aero_before_reduction'] == case[0x776][1]
            if 0x6A4 in case:
                assert actual[0]['aero_before_speed'] == case[0x6A4][1]
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
        scope='Original aerodynamic force/reduction, forward phase, suspension and vertical body; traffic growth and road/tyre/other-force boundaries observed'), indent=2)+'\n')
    print(f'{len(rows)} original {name} aerodynamic calls exported')


if __name__ == '__main__':
    main()
