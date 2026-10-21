#!/usr/bin/env python3
"""Record the original longitudinal wheel-slip rule, including signed boundaries.

Inputs and outputs come from the original consuming instructions and stores.
All generated fixtures stay private. Record --holdout after implementation.
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
OUT = Path('game/tests/data/original/longitudinal_slip')
MEMORY = re.compile(r'^(\d+) \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \[([0-9a-f]+)\](=>|=)([0-9a-f]+)$')
REGISTER = re.compile(r'^(\d+) \d+ (80039524|80039730) [0-9a-f]{8} .*; (s0|v0)=([0-9a-f]+)$')


def vectors(holdout):
    # (tread speed, forward contact velocity, lateral velocity, speed response).
    # Speeds are /4096 m/s; response is /4096. No expected outputs here.
    if holdout:
        return [(r, g, l, gain) for r, g, l, gain in [
            (3911, 7001, -131, 2789), (-3911, -7001, 131, 2789),
            (7001, 3911, 137, 3913), (-7001, -3911, -137, 3913),
            (69, -19345, 211, 3011), (-69, 19345, -211, 3011),
            (71, -19345, 211, 3011), (-71, 19345, -211, 3011),
            (22758, 19997, -411, 3887), (22761, 19997, -411, 3887),
            (-22758, -19997, 411, 3887), (-22761, -19997, 411, 3887),
            (193471, 173691, -2789, 3997), (-173691, -193471, 2789, 3997),
            (101, -239173, 991, 4096), (-101, 239173, -991, 4096)]]
    v = [(0, 0, 0, 4096)]
    for tread in [-71, -70, -69, 0, 69, 70, 71]:
        for ground in [tread - 1, tread, tread + 1, -40960, 40960]:
            v.append((tread, ground, 0, 3889))
    for tread, ground in [(40960, 20480), (20480, 40960), (-40960, -20480), (-20480, -40960),
                           (40960, -20480), (-40960, 20480), (22759, 20000), (22760, 20000),
                           (20000, 22759), (20000, 22760), (-22759, -20000), (-22760, -20000)]:
        v.append((tread, ground, 0, 3889))
    for gain in [0, 1, 2048, 4096, 32767, -4096]:
        v.append((8192, 4096, 187, gain))
        v.append((4096, 8192, -187, gain))
    assert len(v) % 4 == 0
    return v


def export(run, frames):
    report = json.loads((run / 'report.json').read_text())
    if report['status'] != 'complete' or report['trace']['lines_dropped'] != 0:
        raise RuntimeError(f'Incomplete original run: {run}')
    start = report['frames']['start']
    rows = {}
    wheel = {}
    for line in (run / 'trace.txt').open():
        m = MEMORY.match(line.strip())
        if m:
            absolute, pc, address, op, raw = m.groups()
            frame, pc, address = int(absolute) - start, int(pc, 16), int(address, 16)
            value = int(raw, 16)
            if value >= 1 << (len(raw) * 4 - 1): value -= 1 << (len(raw) * 4)
            if pc == 0x800394FC:
                wheel[frame] = (address - 0x1F80001E) // 0x1C
            if frame not in wheel: continue
            i = wheel[frame]
            row = rows.setdefault((frame, i), dict(run_frame=frame, wheel=i))
            def put(key, v=value):
                if key in row and row[key] != v: raise RuntimeError(f'Conflicting {key}: {row}')
                row[key] = v
            if pc == 0x800394FC: put('cosine')
            elif pc == 0x80039510: put('sine')
            elif pc == 0x80039504: put('forward')
            elif pc == 0x80039514: put('lateral')
            elif pc == 0x80039520: put('tread')
            elif pc in (0x8003957C, 0x800395D0, 0x80039618, 0x8003966C): put('speed_response')
            elif pc == 0x80039744: put('slip')
            elif op == '=' and 0x80039534 <= pc <= 0x8003972C:
                field = {0x1F800014+i*0x1C: 'regime', 0x1F800016+i*0x1C: 'direction'}.get(address)
                if field: row[field] = value
        m = REGISTER.match(line.strip())
        if m:
            absolute, pc, register, raw = m.groups()
            frame = int(absolute) - start
            if frame not in wheel: continue
            value = int(raw, 16)
            if value >= 1 << 31: value -= 1 << 32
            if pc == '80039524': rows[(frame, wheel[frame])]['projected'] = value
    if sorted(rows) != [(f, i) for f in range(0, frames, 2) for i in range(4)]:
        raise RuntimeError(f'Missing calls: {run}')
    for row in rows.values():
        row['response_consumed'] = int('speed_response' in row)
        row.setdefault('speed_response', 0)
        required = {'run_frame', 'wheel', 'cosine', 'sine', 'forward', 'lateral', 'tread',
                    'speed_response', 'response_consumed', 'slip', 'regime', 'direction', 'projected'}
        if set(row) != required: raise RuntimeError(f'Incomplete consumer: {row}')
    return list(rows.values()), report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--holdout', action='store_true')
    parser.add_argument('--state', type=Path, default=STATE)
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    all_rows, records = [], []
    name = 'holdout' if args.holdout else 'boundaries'
    probes = vectors(args.holdout)
    for batch in range(0, len(probes), 4):
        run = Path('.psx/runs') / f'longitudinal-{name}-{batch//4:02}'
        # Mode 1 skips live control updates. Wheel-angle construction still
        # executes; controlled equal steering ratios give two chosen fronts,
        # and rear toe gives opposite rear angles. The trace reads actual bases.
        angle = ([137, -719, 1025, -2047] if args.holdout else [0, 512, -512, 1024, -1024, 2048])[batch//4 % (4 if args.holdout else 6)]
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', '2', '--input', '[]',
                   '--trace', '--trace-range', '80039490-80039778', '--trace-range', '8003E470-8003E4C0',
                   '--max-events', '10000', '--out', str(run), '--summary', '--poke', f'{BASE+0x45D:08X}=1:1',
                   '--poke', f'{BASE+0x60C:08X}={angle}:2', '--poke', f'{BASE+0x64:08X}=4096:2',
                   '--poke', f'{BASE+0x66:08X}=4096:2', '--poke', f'{BASE+0x36C:08X}=0:2',
                   '--poke', f'{BASE+0x36E:08X}={angle}:2']
        for i, (tread, forward, lateral, gain) in enumerate(probes[batch:batch+4]):
            base = BASE + 0x460 + i*0x68
            for offset, width, value in [(0x18, 4, tread), (0x2C, 4, forward), (0x30, 4, lateral), (0x2A, 2, gain)]:
                command.extend(['--poke', f'{base+offset:08X}={value}:{width}'])
        subprocess.run(command, check=True, capture_output=True, text=True)
        rows, report = export(run, 2)
        for row, (tread, forward, lateral, gain) in zip(rows, probes[batch:batch+4]):
            assert (row['tread'], row['forward'], row['lateral']) == (tread, forward, lateral)
            if row['response_consumed']: assert row['speed_response'] == gain
            row['case'] = len(all_rows)
            row['sequence'] = 0
            # Fronts both receive angle; rear toe subtracts on left, adds on right.
            row['steering'] = angle if row['wheel'] != 2 else -angle
            all_rows.append(row)
        records.append(dict(command=command, report=str(run/'report.json')))
    for scenario, buttons in [('gas', ['CROSS']), ('brake', ['SQUARE']), ('turn', ['SQUARE', 'LEFT'])] if not args.holdout else [('mixed', ['CROSS', 'RIGHT', 'SQUARE'])]:
        run = Path('.psx/runs') / f'longitudinal-{scenario}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', '24', '--input', json.dumps([dict(frame=0, buttons=buttons)]),
                   '--trace', '--trace-range', '80039490-80039778', '--trace-range', '8003E470-8003E4C0',
                   '--trace-range', '8003DBE8-8003DE68', '--trace-range', '8003E740-8003E74C',
                   '--trace-range', '8003E2D0-8003E2F8', '--trace-range', '8003A19C-8003A278',
                   '--max-events', '30000', '--out', str(run), '--summary']
        subprocess.run(command, check=True, capture_output=True, text=True)
        rows, report = export(run, 24)
        # Capture actual wheel angle from the original trig-table consumer.
        angles={}
        for line in (run/'trace.txt').open():
            m=MEMORY.match(line.strip())
            if m and int(m[2],16)==0x8003E47C:
                frame=int(m[1])-report['frames']['start'];i=(int(m[3],16)-BASE-0x46C)//0x68
                angles[(frame,i)]=int(m[5],16)
        for row in rows:
            row['steering']=angles[(row['run_frame'],row['wheel'])];row['case']=len(all_rows);all_rows.append(row)
            row['sequence']={'gas':1,'brake':2,'turn':3,'mixed':4}[scenario]
        from record_brake_assist import export as export_assist
        assisted = export_assist(run,24,report['frames']['start'])
        with (OUT/f'{name}_{scenario}_assistance.csv').open('w',newline='') as f:
            w=csv.DictWriter(f,sorted(assisted[0]));w.writeheader();w.writerows(assisted)
        records.append(dict(command=command, report=str(run/'report.json')))
    with (OUT/f'{name}.csv').open('w',newline='') as f:
        w=csv.DictWriter(f,sorted(all_rows[0]));w.writeheader();w.writerows(all_rows)
    ram=args.state.with_name('ram.bin').read_bytes()
    # Independent exhaustive table evidence for the native phase quantizer.
    if not args.holdout:
        with (OUT/'steering_basis.csv').open('w',newline='') as f:
            w=csv.writer(f);w.writerow(['phase','sine','cosine'])
            for i in range(4096):w.writerow([i,struct.unpack_from('<h',ram,0x93150+2*i)[0],struct.unpack_from('<h',ram,0x93950+2*i)[0]])
    (OUT/f'{name}.json').write_text(json.dumps(dict(
        records=records, holdout=args.holdout, cases=len(all_rows),
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),ram_sha256=hashlib.sha256(ram).hexdigest(),
        basis_sha256=hashlib.sha256(ram[0x93150:0x95950]).hexdigest(),
        psx_version=subprocess.check_output(['psx','--version'],text=True).strip(),
        scope='Original longitudinal-slip consumer: steering, wheel surface speed, contact velocity and speed response; not tyre/body dynamics'),indent=2)+'\n')
    print(f'{len(all_rows)} original {name} wheel executions exported')


if __name__=='__main__':main()
