#!/usr/bin/env python3
"""Observe original B-1 stopping decisions and wheel-distance queries; private outputs."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

BASE = 0xA96B4
CASES = {
    'early': (2600, [dict(frame=0, buttons=['CROSS'], until=2200), dict(frame=2200, buttons=['SQUARE'])]),
    'gold': (2500, [dict(frame=0, buttons=['CROSS'], until=2210), dict(frame=2210, buttons=['SQUARE'])]),
    'overshoot': (2450, [dict(frame=0, buttons=['CROSS'])]),
    'offroad': (1550, [dict(frame=0, buttons=['CROSS', 'RIGHT'])]),
    # Capture this after the initial comparisons pass; it is a fresh holdout.
    'holdout': (2500, [dict(frame=0, buttons=['CROSS'], until=2220), dict(frame=2220, buttons=['SQUARE'])]),
}
LINE = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=Path, default=Path('.psx/default-start-export/start/state.bin'))
    p.add_argument('--case', choices=CASES, action='append')
    p.add_argument('--reuse', action='store_true', help='Re-export the identical completed captures without running them again')
    args = p.parse_args()
    initial = args.state.with_name('ram.bin').read_bytes()
    for name in args.case or ['early', 'gold', 'overshoot', 'offroad']:
        frames, inputs = CASES[name]
        run = Path('.psx/runs') / f'licence-stop-{name}'
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames), '--input', json.dumps(inputs),
                   '--watch', f'{BASE+0x80000000:08X}:1952', '--watch', '80046F64:4', '--watch', '801FF750:12', '--watch-changes-only',
                   '--trace', '--trace-range', '8003D610-8003D614', '--trace-range', '8003D57C-8003D580',
                   '--trace-range', '8003EC2C-8003EC30',
                   '--trace-range', '8003D7A0-8003D7A4', '--max-events', '2000000', '--out', str(run), '--summary']
        if args.reuse:
            previous=json.loads((Path('game/tests/data/original/licence_stop')/f'{name}.json').read_text())
            assert previous['command']==command and previous['state_sha256']==hashlib.sha256(args.state.read_bytes()).hexdigest()
        else:
            subprocess.run(command, check=True, capture_output=True, text=True)
        report = json.loads((run/'report.json').read_text())
        assert report['status'] == 'complete' and report['watch']['complete'] and report['trace']['lines_dropped'] == 0
        memory = bytearray(initial)
        writes = iter(json.loads(line) for line in (run/'writes.jsonl').open())
        pending = next(writes, None)
        rows = []
        row = None
        controls = None
        def read(off, fmt): return struct.unpack_from('<'+fmt, memory, BASE+off)[0]
        def outcome():
            state = read(0x751, 'B')
            return dict(state=state, reason=read(0x756, 'B'), time_ms=read(0x75C, 'I'))
        for line in (run/'trace.txt').open():
            m = LINE.match(line)
            if not m: continue
            absolute, cycle, pc = (int(m[1]), int(m[2]), int(m[3], 16))
            while pending and pending['cycle'] <= cycle:
                a, width = pending['address'] & 0x1FFFFF, pending['width']
                memory[a:a+width] = pending['value'].to_bytes(width, 'little')
                pending = next(writes, None)
            if pc == 0x8003EC2C:
                flags, steering, brake, throttle, shift, reverse, handbrake, _ = struct.unpack_from('<HhhhbbBB', memory, 0x1FF750)
                assert flags == 0
                controls = dict(steering=steering, brake=brake, throttle=throttle, shift=shift, reverse=reverse, handbrake=handbrake)
            elif pc == 0x8003D610:
                assert row is None
                assert controls is not None
                position = [read(0x65C+i*4, 'i') for i in range(3)]
                points = []
                for wheel in range(4):
                    v = [(position[i]+read(0x484+wheel*0x68+i*2, 'h'))/4096 for i in range(3)]
                    points.append([v[0], v[2], -v[1]])
                velocity = [read(0x628+i*4, 'i')/4096 for i in range(3)]
                row = dict(frame=absolute-report['frames']['start'], before=outcome(), points_m=points, controls=controls,
                           pose_m=[position[0]/4096,position[2]/4096,-position[1]/4096],
                           seed=read(0x600, 'I'), velocity_mps=[velocity[0], velocity[2], -velocity[1]],
                           surface=[read(0x474+i*0x68, 'B') for i in range(4)],
                           faults=read(0x6FA, 'B'), clock=struct.unpack_from('<I', memory, 0x46F64)[0],
                           time_origin_ms=read(0x780, 'I'), distances_m=[])
            elif pc == 0x8003D57C:
                assert row is not None
                value = int(re.search(r'v1=([0-9a-f]{8})', line)[1], 16)
                if value >= 2**31: value -= 2**32
                row['distances_m'].append(value/65536)
            elif pc == 0x8003D7A0:
                assert row is not None and len(row['distances_m']) in (0, 4)
                row['after'] = outcome()
                rows.append(row)
                row = None
        assert row is None and len(rows) == frames//2
        out = Path('game/tests/data/original/licence_stop')
        out.mkdir(parents=True, exist_ok=True)
        (out/f'{name}.json').write_text(json.dumps(dict(rows=rows, command=command, report=str(run/'report.json'),
            state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
            # Re-exporting captures after a toolkit update must retain the
            # build that executed them, rather than the currently installed CLI.
            psx_version=f"psx {report['build']['version']}+{report['build']['id']}",
            psx_build=report['build']), indent=2)+'\n')
        print(f'{name}: {len(rows)} original decisions, {sum(len(r["distances_m"]) for r in rows)} wheel projections', flush=True)


if __name__ == '__main__': main()
