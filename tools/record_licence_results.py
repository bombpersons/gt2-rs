#!/usr/bin/env python3
"""Record the original result timer at function boundaries, not vblank samples.

Expected ages, consumed button edges and completion flags come solely from
the executing original. Captures and fixtures remain in ignored directories.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

from export_assets import export_result_timing

LINE = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
TIMER = 0xAF226
PAD = 0xA9590


def record(psx, name, state, frames, inputs, age=None):
    folder = Path('.psx/results/timing') / name
    initial = bytearray(state.with_name('ram.bin').read_bytes())
    command = [psx, 'run', '--from-state', str(state), '--frames', str(frames),
               '--input', json.dumps(inputs), '--trace',
               '--trace-range', '8002A700-8002A704',
               '--trace-range', '8002A740-8002A744',
               '--trace-range', '8002AAC8-8002AAD8',
               '--watch', '800AF226:2', '--watch', '800A9590:8',
               '--out', str(folder), '--summary']
    if age is not None:
        struct.pack_into('<h', initial, TIMER, age)
        command += ['--poke', f'800AF226={age}:2']
    subprocess.run(command, check=True, capture_output=True, text=True)
    report = json.loads((folder/'report.json').read_text())
    assert report['status'] == 'complete' and report['watch']['complete']
    assert report['trace']['lines_dropped'] == 0
    pending = iter(json.loads(s) for s in (folder/'writes.jsonl').read_text().splitlines())
    write = next(pending, None)
    rows, row, result = [], None, None
    for line in (folder/'trace.txt').open():
        match = LINE.match(line)
        if not match:
            continue
        frame, cycle, pc = int(match[1]), int(match[2]), int(match[3], 16)
        while write and write['cycle'] <= cycle:
            address = write['address'] & 0x1FFFFF
            initial[address:address+write['width']] = write['value'].to_bytes(write['width'], 'little')
            write = next(pending, None)
        if pc == 0x8002A700:
            assert row is None
            held, pressed = struct.unpack_from('<II', initial, PAD)
            row = dict(frame=frame-report['frames']['start'],
                       before=struct.unpack_from('<h', initial, TIMER)[0],
                       held=bool(held & 0xA00), pressed=bool(pressed & 0xA00))
            result = None
        value = re.search(r'v0=([0-9a-f]{8})', line)
        if value:
            result = int(value[1], 16)
        if pc == 0x8002AAD4:
            assert row is not None and result in (0, 1)
            row.update(after=struct.unpack_from('<h', initial, TIMER)[0], running=bool(result))
            rows.append(row)
            row = None
    assert row is None and rows
    return dict(name=name, passed=struct.unpack_from('<I', initial, 0x1D5DEC)[0] == 1,
                rows=rows, command=command, report=str(folder/'report.json'), build=report['build'],
                state_sha256=hashlib.sha256(state.read_bytes()).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--psx', default='psx')
    parser.add_argument('--fresh', action='store_true')
    args = parser.parse_args()
    success = Path('.psx/awards/gold-session/state.bin')
    failure = Path('.psx/new-core/failure-checkpoint/state.bin')
    cases = []
    if args.fresh:
        # Different held/edge schedules, recorded only after implementation.
        cases.append(record(args.psx, 'fresh-success', success, 180,
                            [dict(frame=0, buttons=['CIRCLE'], until=100),
                             dict(frame=120, buttons=['CROSS'], until=130)], 73))
        cases.append(record(args.psx, 'fresh-failure', failure, 100,
                            [dict(frame=0, buttons=['SQUARE'], until=70),
                             dict(frame=72, buttons=['CIRCLE'], until=80)], 7))
    else:
        for name, state, frames, inputs in [
            ('success', success, 340, [dict(frame=10, buttons=['CROSS'], until=18),
                                     dict(frame=270, buttons=['CIRCLE'], until=278)]),
            ('failure', failure, 120, [dict(frame=10, buttons=['CROSS'], until=18),
                                     dict(frame=90, buttons=['CIRCLE'], until=98)]),
        ]:
            cases.append(record(args.psx, name, state, frames, inputs))
        for passed, state, ages in [(True, success, [-1, 0, 118, 119, 120, 144, 145, 146, 147]),
                                    (False, failure, [-1, 0, 28, 29, 30])]:
            for age in ages:
                for buttons in [[], ['CROSS'], ['CIRCLE'], ['SQUARE'], ['START']]:
                    cases.append(record(args.psx, f'probe-{passed}-{age}-{buttons}', state, 6,
                                        [dict(frame=0, buttons=buttons)], age))
    output = Path('game/tests/data/original/licence_results')
    output.mkdir(parents=True, exist_ok=True)
    export_result_timing(success.with_name('ram.bin').read_bytes(), output)
    (output/('fresh.json' if args.fresh else 'initial.json')).write_text(json.dumps(cases, indent=2)+'\n')
    print(f'{len(cases)} original recordings, {sum(len(c["rows"]) for c in cases)} result ticks')


if __name__ == '__main__':
    main()
