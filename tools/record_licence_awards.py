#!/usr/bin/env python3
"""Execute licence award/attempt updates in their original menu context.

All RAM, traces and numerical fixtures remain private. No native rule supplies
expected values. --fresh records additional table rows after implementation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess

from export_assets import export_records, licence_definitions

PREFIX = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
CURRENT = 0x1D585C
RESULT = 0x1D5DE8


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, default=Path('.psx/awards/before-commit/state.bin'))
    parser.add_argument('--psx', default='psx')
    parser.add_argument('--fresh', action='store_true')
    parser.add_argument('--export-progress', type=Path, help='Verify native progress import from a completed original RAM dump')
    args = parser.parse_args()
    if args.export_progress:
        ram = args.export_progress.read_bytes()
        definitions = licence_definitions(ram)
        work = Path('.psx/awards/native-import')
        export_records(ram, work)
        output = Path('game/tests/data/original/licence_awards')
        output.mkdir(parents=True, exist_ok=True)
        for name in ['licence-progress.initial.ron', 'licence.ron']:
            shutil.copyfile(work/'race'/name, output/name)
        rows = [dict(id=key, grade=ram[0x1C98E0+0x1418+i*164+1],
                     assisted_attempts=ram[0x1C98E0+0x1418+i*164+2])
                for i,key in enumerate(definitions)]
        (output/'exported.json').write_text(json.dumps(dict(rows=rows,
            ram_sha256=hashlib.sha256(ram).hexdigest(), source=str(args.export_progress)),indent=2)+'\n')
        print(f'Native progress export: {len(rows)} saved original achievements')
        return
    initial = args.state.with_name('ram.bin').read_bytes()
    definitions = licence_definitions(initial)
    group, test = initial[CURRENT+11:CURRENT+13]
    group_base = 0x1C98E0 + 0x1418 + group * 1640
    record = group_base + test * 164
    assert group < 6 and test < 10
    selected = initial[CURRENT+16:initial.index(0, CURRENT+16)].decode('ascii')
    definition = definitions[selected]
    cases = []
    if args.fresh:
        for i, (key, definition) in enumerate(definitions.items()):
            boundary = definition['time_limits_ms'][i % 4]
            cases.append(dict(key=key, time_ms=boundary + (i % 3 - 1), grade=i % 5,
                              attempts=(definition['assisted_pass_attempts'] - 1) % 256,
                              others=4 if i % 2 else 0, group=i//10, test=i%10))
    else:
        required = definition['assisted_pass_attempts']
        for boundary in definition['time_limits_ms']:
            for delta in [-1, 0, 1]:
                cases.append(dict(time_ms=boundary+delta, attempts=max(0, required-1)))
        slow = (definition['time_limits_ms'][2] + definition['time_limits_ms'][3]) // 2
        for grade in range(5):
            for count in [0, max(0, required-2), max(0, required-1), required, 254, 255]:
                cases.append(dict(time_ms=slow, grade=grade, attempts=count))
        for grade in [0, 1, 3, 4]:
            cases.append(dict(time_ms=definition['time_limits_ms'][0]-1, grade=grade, others=4))
            cases.append(dict(time_ms=0, grade=grade, outcome=2))
    rows = []
    for index, case in enumerate(cases):
        name = f"{'fresh' if args.fresh else 'initial'}-{index}"
        folder = Path('.psx/awards/probes') / name
        key = case.get('key', selected)
        case_group, case_test = case.get('group', group), case.get('test', test)
        group_base = 0x1C98E0 + 0x1418 + case_group * 1640
        record = group_base + case_test * 164
        # The selected key chooses the actual GTDT thresholds, while the
        # supplied menu group/test owns the saved achievement being updated.
        pokes = [(RESULT+4, case.get('outcome', 1), 4), (RESULT+8, case['time_ms'], 4),
                 (record+1, case.get('grade', 0), 1), (record+2, case.get('attempts', 0), 1),
                 (CURRENT+11, case_group, 1), (CURRENT+12, case_test, 1)]
        for i, c in enumerate(key.encode('ascii') + b'\0'):
            pokes.append((CURRENT+16+i, c, 1))
        for i in range(10):
            if i != case_test:
                pokes.append((group_base+i*164+1, case.get('others', 0), 1))
        command = [args.psx, 'run', '--from-state', str(args.state), '--frames', '4',
                   '--trace', '--trace-range', '8004DCAC-8004DF04',
                   '--watch', f'{group_base+0x80000000:08X}:1640',
                   '--out', str(folder), '--summary']
        memory = bytearray(initial)
        for address, value, width in pokes:
            memory[address:address+width] = value.to_bytes(width, 'little')
            command += ['--poke', f'{address+0x80000000:08X}={value}:{width}']
        completed = subprocess.run(command, capture_output=True, text=True)
        if completed.returncode:
            raise RuntimeError(completed.stderr or completed.stdout)
        report = json.loads((folder/'report.json').read_text())
        assert report['status'] == 'complete' and report['watch']['complete']
        assert report['trace']['lines_dropped'] == 0
        writes = iter(json.loads(line) for line in (folder/'writes.jsonl').open())
        pending = next(writes, None)
        def view():
            return dict(grade=memory[record+1], assisted_attempts=memory[record+2],
                        group_grades=[memory[group_base+i*164+1] for i in range(10)])
        before = view()
        checks = []
        calls = 0
        for line in (folder/'trace.txt').open():
            match = PREFIX.match(line)
            if not match:
                continue
            cycle, pc = int(match[2]), int(match[3], 16)
            while pending and pending['cycle'] <= cycle:
                address, width = pending['address'] & 0x1FFFFF, pending['width']
                memory[address:address+width] = pending['value'].to_bytes(width, 'little')
                pending = next(writes, None)
            if pc == 0x8004DD80:
                calls += 1
            if pc in [0x8004DD10, 0x8004DD7C]:
                value = re.search(r'v0=([0-9a-f]{8})', line)
                assert value
                checks.append(dict(all_gold=pc == 0x8004DD7C, value=int(value[1], 16), state=view()))
        while pending:
            address, width = pending['address'] & 0x1FFFFF, pending['width']
            memory[address:address+width] = pending['value'].to_bytes(width, 'little')
            pending = next(writes, None)
        assert calls == (1 if case.get('outcome', 1) == 1 else 0)
        rows.append(dict(definition=definitions[key], time_ms=case['time_ms'] if calls else None,
                         before=before, after=view(), checks=checks, test_index=case_test,
                         command=command, report=str(folder/'report.json'), build=report['build']))
    output = Path('game/tests/data/original/licence_awards')
    output.mkdir(parents=True, exist_ok=True)
    (output/('fresh.json' if args.fresh else 'initial.json')).write_text(json.dumps(dict(
        rows=rows, state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest()), indent=2)+'\n')
    print(f'{len(rows)} original award/attempt cases; {sum(len(r["checks"]) for r in rows)} group checks')


if __name__ == '__main__':
    main()
