#!/usr/bin/env python3
"""Record whole B-1 physics ticks without intervening in vehicle state.

The reference fixture contains private car snapshots and consumed logical
controls. Only original execution supplies expected state. No snapshot beyond
the first is to be used to reseed the native trajectory.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

BASE = 0x800A96B4
SIZE = 0x7A0
PHASES = {0x8003EC24: 'before', 0x8003EC2C: 'forces', 0x8003EC48: 'moved',
          0x8003EC68: 'road', 0x8003EC74: 'end'}
PREFIX = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
INPUTS = {0x8002FCB0: 'flags', 0x8002FCC4: 'steering', 0x8002FFF4: 'throttle',
          0x800300A8: 'brake', 0x80030178: 'handbrake'}
SCENARIOS = {
    'gas': [dict(frame=0, buttons=['CROSS'])],
    'brake': [dict(frame=0, buttons=['SQUARE'])],
    'coast': [dict(frame=0, buttons=[])],
    'holdout': [dict(frame=0, buttons=['CROSS','LEFT']), dict(frame=8, buttons=['CROSS','RIGHT']),
                dict(frame=16, buttons=['CROSS']), dict(frame=28, buttons=['SQUARE']),
                dict(frame=44, buttons=['CIRCLE']), dict(frame=54, buttons=[])],
}


def export(run, target, frames, initial):
    report = json.loads((run/'report.json').read_text())
    assert report['status'] == 'complete' and report['watch']['complete'] and report['read_watch']['complete']
    assert report['trace']['lines_dropped'] == 0
    start = report['frames']['start']
    ram = bytearray(initial)
    writes = iter(sorted((json.loads(line) for line in (run/'writes.jsonl').open()), key=lambda e:e['cycle']))
    pending = next(writes, None)
    ticks = {}
    for line in (run/'trace.txt').open():
        m = PREFIX.match(line)
        if not m: continue
        absolute, cycle, pc = m.groups()
        frame, cycle, pc = int(absolute)-start, int(cycle), int(pc,16)
        if pc not in PHASES: continue
        while pending and pending['cycle'] <= cycle:
            p, size = pending['address'] & 0x1FFFFF, pending['width']
            ram[p:p+size] = pending['value'].to_bytes(size,'little')
            pending = next(writes, None)
        tick = ticks.setdefault(frame,{})
        phase = PHASES[pc]
        assert phase not in tick, 'single-car tick boundary required'
        data = ram[BASE & 0x1FFFFF:(BASE & 0x1FFFFF)+SIZE]
        name = f'{frame:04}-{phase}.bin'
        (target/name).write_bytes(data)
        tick[phase] = name
    assert sorted(ticks) == list(range(0,frames,2))
    for tick in ticks.values(): assert set(tick) == set(PHASES.values())
    for line in (run/'reads.jsonl').open():
        e = json.loads(line)
        key = INPUTS.get(e['pc'])
        if key is None: continue
        bits = e['width']*8
        v = e['value']
        if v >= 1 << (bits-1): v -= 1 << bits
        controls = ticks[e['run_frame']].setdefault('controls',{})
        assert key not in controls
        controls[key] = v
    result = []
    for frame,tick in sorted(ticks.items()):
        assert set(tick['controls']) == set(INPUTS.values()) and tick['controls']['flags'] == 0
        result.append(dict(run_frame=frame,**tick))
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--scenario',choices=SCENARIOS,action='append')
    p.add_argument('--frames',type=int,default=60)
    args=p.parse_args()
    assert args.frames > 0 and args.frames % 2 == 0
    initial=args.state.with_name('ram.bin').read_bytes()
    assert len(initial)==2*1024*1024
    assert initial[0xAF231] == 1, 'single-car checkpoint required (8003EC1C)'
    assert initial[0xA9520:0xA9522] == b'\0\0', 'unpaused physics required' 
    for scenario in args.scenario or SCENARIOS:
        out=Path('game/tests/data/original/vehicle_loop')/scenario
        out.mkdir(parents=True,exist_ok=True)
        run=Path('.psx/runs')/f'vehicle-loop-{scenario}'
        command=['psx','run','--from-state',str(args.state),'--frames',str(args.frames),
                 '--input',json.dumps(SCENARIOS[scenario]),'--trace','--trace-range','8003EC24-8003EC78',
                 '--watch',f'{BASE:08X}:{SIZE}','--watch-reads','801FF750:12',
                 '--max-events','2000000','--out',str(run),'--summary']
        subprocess.run(command,check=True,capture_output=True,text=True)
        ticks=export(run,out,args.frames,initial)
        metadata=dict(ticks=ticks,command=command,report=str(run/'report.json'),
                      state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
                      ram_sha256=hashlib.sha256(initial).hexdigest(),
                      psx_version=subprocess.check_output(['psx','--version'],text=True).strip(),
                      alternate_locked_step=initial[0x1D5866]==3,
                      scope='Natural complete ticks; native tests may initialize state only once')
        (out/'recording.json').write_text(json.dumps(metadata,indent=2)+'\n')
        print(f'{scenario}: {len(ticks)} complete original ticks',flush=True)


if __name__=='__main__': main()
