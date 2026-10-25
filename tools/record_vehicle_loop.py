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
import struct

BASE = 0x800A96B4
SIZE = 0x7A0
PHASES = {0x8003EC24: 'before', 0x8003EC2C: 'forces', 0x8003EC48: 'moved',
          0x8003EC68: 'road', 0x8003EC74: 'end'}
PREFIX = re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
SCENARIOS = {
    'right-start': [dict(frame=0, buttons=['CROSS','RIGHT'])],
    'gas': [dict(frame=0, buttons=['CROSS'])],
    'stop-holdout': [dict(frame=0, buttons=['CROSS','LEFT','TRIANGLE','R2']),
                     dict(frame=40, buttons=['CROSS','CIRCLE','RIGHT','L2']),
                     dict(frame=100, buttons=['CROSS','SQUARE']), dict(frame=160, buttons=['CROSS','LEFT'])],
    'brake': [dict(frame=0, buttons=['SQUARE'])],
    'coast': [dict(frame=0, buttons=[])],
    'holdout': [dict(frame=0, buttons=['CROSS','LEFT']), dict(frame=8, buttons=['CROSS','RIGHT']),
                dict(frame=16, buttons=['CROSS']), dict(frame=28, buttons=['SQUARE']),
                dict(frame=44, buttons=['CIRCLE']), dict(frame=54, buttons=[])],
    'brake-launch': [dict(frame=0, buttons=['SQUARE']), dict(frame=40, buttons=['CROSS'])],
    'reverse-forward': [dict(frame=0, buttons=['TRIANGLE']), dict(frame=50, buttons=['CROSS'])],
    'shifts': [dict(frame=0, buttons=['CROSS','R2']), dict(frame=20, buttons=['CROSS']),
               dict(frame=30, buttons=['CROSS','L2']), dict(frame=50, buttons=['CROSS','L2','R2']),
               dict(frame=70, buttons=['CROSS','R2']), dict(frame=90, buttons=['CROSS'])],
    'launch-holdout': [dict(frame=0,buttons=['SQUARE']),dict(frame=16,buttons=['CROSS','SQUARE']),
        dict(frame=32,buttons=['CROSS']),dict(frame=60,buttons=['CIRCLE']),
        dict(frame=76,buttons=['TRIANGLE']),dict(frame=100,buttons=['CROSS','LEFT']),
        dict(frame=124,buttons=['SQUARE']),dict(frame=144,buttons=['CROSS','RIGHT'])],
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
    initialized_controls = set()
    for line in (run/'trace.txt').open():
        m = PREFIX.match(line)
        if not m: continue
        absolute, cycle, pc = m.groups()
        frame, cycle, pc = int(absolute)-start, int(cycle), int(pc,16)
        if pc not in PHASES and pc not in (0x8003E2C8,0x8003EC88,0x8003E310): continue
        while pending and pending['cycle'] <= cycle:
            p, size = pending['address'] & 0x1FFFFF, pending['width']
            ram[p:p+size] = pending['value'].to_bytes(size,'little')
            for off in range(size):
                if 0x1FF750 <= p+off < 0x1FF75C:
                    initialized_controls.add(p+off-0x1FF750)
            pending = next(writes, None)
        tick = ticks.setdefault(frame,{})
        if pc==0x8003E310:
            tick['automatic_mode']=ram[(BASE&0x1FFFFF)+0x786]
            if tick['automatic_mode'] in (5,6,7):
                tick['controls']=dict(flags=0,steering=0,throttle=0,brake=0,handbrake=0,reverse=0,shift=0)
            continue
        if pc == 0x8003EC88:
            tick['session'] = dict(countdown_fields=struct.unpack_from('<H',ram,0xA9520)[0],
                                   clock=struct.unpack_from('<I',ram,0x46F64)[0])
            continue
        if pc == 0x8003E2C8:
            assert initialized_controls == set(range(12)), 'all effective control bytes must be observed'
            assert 'controls' not in tick, 'one player control boundary per tick required'
            flags, steering, brake, throttle, shift, reverse, handbrake, _ = struct.unpack_from('<HhhhbbBB',ram,0x1FF750)
            assert flags == 0, 'digital input boundary required'
            tick['controls'] = dict(flags=flags, steering=steering, throttle=throttle, brake=brake,
                                    handbrake=handbrake, reverse=reverse, shift=shift)
            initialized_controls.clear()
            continue
        phase = PHASES[pc]
        assert phase not in tick, 'single-car tick boundary required'
        data = ram[BASE & 0x1FFFFF:(BASE & 0x1FFFFF)+SIZE]
        name = f'{frame:04}-{phase}.bin'
        (target/name).write_bytes(data)
        tick[phase] = name
    assert sorted(ticks) == list(range(0,frames,2))
    for tick in ticks.values(): assert set(tick) == set(PHASES.values()) | {'controls','session','automatic_mode'}
    result = []
    for frame,tick in sorted(ticks.items()):
        assert tick['controls']['flags'] == 0
        result.append(dict(run_frame=frame,**tick))
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--psx', default='psx', help='Toolkit executable used for capture and provenance')
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--scenario',choices=SCENARIOS,action='append')
    p.add_argument('--frames',type=int,default=60)
    p.add_argument('--prefix',default='')
    p.add_argument('--session',action='store_true',help='Record the natural countdown/session clock too')
    args=p.parse_args()
    assert args.frames > 0 and args.frames % 2 == 0
    assert re.fullmatch('[a-z0-9_-]*',args.prefix)
    initial=args.state.with_name('ram.bin').read_bytes()
    assert len(initial)==2*1024*1024
    assert initial[0xAF231] == 1, 'single-car checkpoint required (8003EC1C)'
    if not args.session:
        assert initial[0xA9520:0xA9522] == b'\0\0', 'use --session for a countdown checkpoint'
    for scenario in args.scenario or ['gas','brake','coast','holdout']:
        name=args.prefix+scenario
        out=Path('game/tests/data/original/vehicle_loop')/name
        out.mkdir(parents=True,exist_ok=True)
        run=Path('.psx/runs')/f'vehicle-loop-{name}'
        command=[args.psx,'run','--from-state',str(args.state),'--frames',str(args.frames),
                 '--input',json.dumps(SCENARIOS[scenario]),'--trace','--trace-range','8003EC24-8003EC78',
                 '--trace-range','8003E2C8-8003E2CC','--trace-range','8003EC88-8003EC8C',
                 '--trace-range','8003E310-8003E314',
                 '--watch',f'{BASE:08X}:{SIZE}','--watch','801FF750:12','--watch-reads','801FF750:12',
                 '--watch','800A9520:2','--watch','80046F64:4',
                 '--max-events','2000000','--out',str(run),'--summary']
        completed = subprocess.run(command,capture_output=True,text=True)
        if completed.returncode:
            raise RuntimeError(completed.stderr or completed.stdout)
        ticks=export(run,out,args.frames,initial)
        if initial[(BASE&0x1FFFFF)+0x786] in (5,6,7):
            for tick in ticks:
                buttons = []
                for event in SCENARIOS[scenario]:
                    if event['frame'] <= tick['run_frame']:
                        buttons = event['buttons']
                # Native tests supply the physical requests too, proving the
                # automatic driver actually ignores conflicting player input.
                tick['controls'] = dict(flags=0,steering=int('LEFT' in buttons)-int('RIGHT' in buttons),
                    throttle=int('CROSS' in buttons),brake=int('SQUARE' in buttons),handbrake=int('CIRCLE' in buttons),
                    reverse=int('TRIANGLE' in buttons),shift=int('R2' in buttons)-int('L2' in buttons))
        metadata=dict(ticks=ticks,command=command,report=str(run/'report.json'),
                      initial_automatic_mode=initial[(BASE&0x1FFFFF)+0x786],
                      state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
                      ram_sha256=hashlib.sha256(initial).hexdigest(),
                      psx_version=subprocess.check_output([args.psx,'--version'],text=True).strip(),
                      alternate_locked_step=initial[0x1D5866]==3,
                      scope='Natural complete ticks; native tests may initialize state only once')
        if args.session:
            metadata['session'] = dict(countdown_fields=struct.unpack_from('<H',initial,0xA9520)[0],
                                       clock=struct.unpack_from('<I',initial,0x46F64)[0])
        (out/'recording.json').write_text(json.dumps(metadata,indent=2)+'\n')
        print(f'{scenario}: {len(ticks)} complete original ticks',flush=True)


if __name__=='__main__': main()
