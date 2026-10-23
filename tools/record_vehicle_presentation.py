#!/usr/bin/env python3
"""Record original body, wheel and chase-stabilization presentation boundaries.

All expected poses come from original stores or consumed matrices. Outputs
are private reference data, never runtime assets or native-generated answers.
"""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

from record_vehicle_loop import SCENARIOS

OUTER = 0x800A9688
BASE = OUTER + 0x2C
PHASES = {0x8003E978: 'before', 0x8003EB20: 'stabilized', 0x80013720: 'presented'}
PREFIX = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')


def export(run, initial, frames):
    report = json.loads((run / 'report.json').read_text())
    assert report['status'] == 'complete' and report['frames']['reached'] == frames
    assert report['watch']['complete'] and report['read_watch']['complete']
    assert report['trace']['lines_dropped'] == 0
    ram = bytearray(initial)
    writes = iter(sorted((json.loads(l) for l in (run / 'writes.jsonl').open()), key=lambda e: e['cycle']))
    pending = next(writes, None)
    rows = defaultdict(dict)
    wheel_calls = []
    wheel_start = None
    for line in (run / 'trace.txt').open():
        m = PREFIX.match(line)
        if not m:
            continue
        absolute, cycle, pc = int(m[1]), int(m[2]), int(m[3], 16)
        if pc == 0x8006724C:
            assert wheel_start is None
            wheel_start = cycle
        elif pc == 0x80067254:
            assert wheel_start is not None
            wheel_calls.append((wheel_start, cycle))
            wheel_start = None
        if pc not in PHASES:
            continue
        while pending and pending['cycle'] <= cycle:
            p, size = pending['address'] & 0x1FFFFF, pending['width']
            ram[p:p+size] = pending['value'].to_bytes(size, 'little')
            pending = next(writes, None)
        def read(address, fmt='i'):
            return struct.unpack_from('<' + fmt, ram, address & 0x1FFFFF)[0]
        def car(off, fmt='i'):
            return read(BASE + off, fmt)
        def wheels(off, fmt='h'):
            return [car(0x460 + i * 0x68 + off, fmt) for i in range(4)]
        state = dict(position=[car(0x65C + i * 4) for i in range(3)],
                     angles=[car(0x644 + i * 2, 'h') for i in range(3)],
                     velocity=[car(0x628 + i * 4) for i in range(3)], duration=car(0x6FE, 'h'),
                     tracking=[car(o, 'h') for o in [0x748, 0x74A, 0x74C, 0x74E, 0x752]],
                     correction=[car(o, 'h') for o in [0x73A, 0x73C]],
                     body_basis=[car(0x668 + i * 2, 'h') for i in range(12)],
                     wheel_travel=wheels(0x10), wheel_heading=wheels(0xC), wheel_spin=wheels(0x20, 'H'),
                     camber=[car(0x368 + i * 2, 'h') for i in range(2)],
                     body_matrix=[read(OUTER + 0x81C + i * 2, 'h') for i in range(9)],
                     body_position=[read(OUTER + 0x830 + i * 4) for i in range(3)],
                     wheel_positions=[[read(OUTER + 0x7C4 + i * 16 + j * 2, 'h') for j in range(3)] for i in range(4)])
        frame = absolute - report['frames']['start']
        assert PHASES[pc] not in rows[frame], 'one car/update required'
        rows[frame][PHASES[pc]] = state
    assert sorted(rows) == list(range(0, frames, 2))
    assert all(set(r) == set(PHASES.values()) for r in rows.values())
    assert wheel_start is None
    matrices = defaultdict(list)
    cambers = defaultdict(dict)
    current = {}
    pcs = {0x8007B9C0: 0, 0x8007B9C4: 4, 0x8007B9CC: 12, 0x8007B9E4: 8, 0x8007BA14: 16}
    for line in (run / 'reads.jsonl').open():
        e = json.loads(line)
        if e['pc'] == 0x80014578:
            axle = (e['address'] - (OUTER + 0x394)) // 2
            assert axle in (0, 1)
            value = struct.unpack('<h',e['value'].to_bytes(2,'little'))[0]
            assert cambers[e['run_frame']].get(axle, value) == value
            cambers[e['run_frame']][axle] = value
        if not any(start < e['cycle'] < end for start, end in wheel_calls):
            continue
        off = pcs.get(e['pc'])
        if off is None or e['address'] != 0x1F80009C + off:
            continue
        if off == 0:
            assert not current
        assert off not in current
        current[off] = e['value']
        if off == 16:
            assert len(current) == 5
            data = b''.join(current[i].to_bytes(4, 'little') for i in range(0, 20, 4))
            matrices[e['run_frame']].append(struct.unpack_from('<9h', data))
            current = {}
    assert not current
    for frame, row in rows.items():
        row['wheel_matrices'] = matrices[frame]
        assert len(matrices[frame]) in (0, 4), (frame, len(matrices[frame]))
        if matrices[frame]:
            assert set(cambers[frame]) == {0, 1}
            row['presented']['camber'] = [cambers[frame][i] for i in range(2)]
    return [dict(run_frame=frame, **row) for frame, row in sorted(rows.items())]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--scenario', choices=SCENARIOS, action='append')
    p.add_argument('--frames', type=int, default=60)
    p.add_argument('--velocity-tracking', action='store_true')
    p.add_argument('--camber-probe', action='store_true')
    args = p.parse_args()
    initial = args.state.with_name('ram.bin').read_bytes()
    assert len(initial) == 2 * 1024 * 1024 and args.frames > 0 and args.frames % 2 == 0
    for scenario in args.scenario or ['gas', 'coast', 'brake']:
        inputs = [dict(e) for e in SCENARIOS[scenario]]
        inputs[0] = dict(inputs[0], buttons=inputs[0]['buttons'] + ['R1'])
        suffix = ('-velocity' if args.velocity_tracking else '') + ('-camber' if args.camber_probe else '')
        run = Path('.psx/runs') / f'vehicle-presentation-{scenario}{suffix}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(args.frames),
                   '--input', json.dumps(inputs), '--watch', f'{OUTER:08X}:2880',
                   '--watch-reads', '1F80009C-1F8000B0', '--watch-reads', '800A9A1C:4', '--trace',
                   '--trace-range', '8003E978-8003EB24', '--trace-range', '800133F0-80013724',
                   '--trace-range', '800671AC-80067258',
                   '--max-events', '1000000', '--out', str(run), '--summary']
        if args.velocity_tracking:
            command += ['--poke', '801C9990=1:1']
        if args.camber_probe:
            command += ['--poke', '800A9A1C=64:2', '--poke', '800A9A1E=-96:2']
        subprocess.run(command, check=True, capture_output=True, text=True)
        rows = export(run, initial, args.frames)
        target = Path('game/tests/data/original/vehicle_presentation') / f'{scenario}{suffix}.json'
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(dict(rows=rows, command=command,
            state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
            ram_sha256=hashlib.sha256(initial).hexdigest(), velocity_tracking=args.velocity_tracking,
            yaw_coefficients=[struct.unpack_from('<i',initial,a + (4 if args.velocity_tracking else 0))[0] for a in [0x46C94,0x46C9C]],
            psx_version=subprocess.check_output(['psx', '--version'], text=True).strip()), indent=2) + '\n')
        print(f'{scenario}: {len(rows)} original body/stabilization poses, '
              f'{sum(len(r["wheel_matrices"]) for r in rows)} consumed wheel matrices', flush=True)


if __name__ == '__main__':
    main()
