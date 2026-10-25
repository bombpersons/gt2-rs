#!/usr/bin/env python3
"""Observe the original recording writer/finalizer. All expected state is execution.

Small capacities and long pending holds are controlled probes of the unchanged
writer. Natural cases retain the whole input-to-result recording boundary.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
from export_assets import export_replay_recording, ron
from import_replay import decode
from record_replay_inputs import record as playback_probe

STREAM=0x1D5F84
LINE=re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')


def requests(memory):
    selectors,buttons,_,_,_=memory[0x1FF6B0:0x1FF6B5]
    assert selectors==0,'digital recording corpus required'
    return dict(throttle=bool(buttons&4),brake=bool(buttons&8),handbrake=bool(buttons&16),
        left=bool(buttons&1),right=bool(buttons&2),reverse=bool(buttons&32),
        shift_up=bool(buttons&64),shift_down=bool(buttons&128))


def record(name, inputs, frames, capacity=None, seed_ticks=0, stop_at=None, discard=False, natural=False):
    state=Path('.psx/current-core-export/start/state.bin')
    memory=bytearray(state.with_name('ram.bin').read_bytes())
    folder=Path('.psx/replay/recording')/name
    command=['psx','run','--from-state',str(state),'--frames',str(frames),'--input',json.dumps(inputs),
        '--trace','--trace-range','800166CC-800166D0','--trace-range','800167AC-800167B0',
        '--trace-range','800167D0-800167D4','--trace-range','8001680C-80016810',
        '--watch','801D5F84:96','--watch','801FF6B0:5','--watch','800A9522:2',
        '--watch','800A9E05:1','--max-events','200000','--out',str(folder),'--summary']
    if frames>10000:
        command+=['--timeout','360']
        # The whole real buffer needs more write observations than the
        # short probes; reject incomplete evidence below rather than fitting it.
        command[command.index('--max-events')+1]='1000000'
        folder.parent.mkdir(parents=True,exist_ok=True)
        replay=folder.with_suffix('.input.json');replay.write_text(json.dumps(inputs)+'\n')
        index=command.index('--input');command[index:index+2]=['--replay',str(replay)]
    if capacity is not None:
        struct.pack_into('<H',memory,STREAM+18,capacity);command+=['--poke',f'801D5F96={capacity}:2']
    if seed_ticks:
        # A pre-existing first digital run, with no flushed packets. Seeding
        # counters avoids simulating millions of identical held requests.
        struct.pack_into('<I',memory,STREAM,seed_ticks)
        struct.pack_into('<i',memory,STREAM+8,seed_ticks-1)
        memory[STREAM+20:STREAM+25]=bytes([120,0,0,0,0])
        command+=['--poke',f'801D5F84={seed_ticks}','--poke',f'801D5F8C={seed_ticks-1}',
                  '--poke','801D5F98=120:1','--poke','801D5F99=0:4']
    if stop_at is not None:command+=['--poke',f'800A9522=60:2@{stop_at}']
    if discard:command+=['--poke','80013D9C=0x24050001']
    completed=subprocess.run(command,capture_output=True,text=True)
    if completed.returncode:raise RuntimeError(completed.stderr or completed.stdout)
    report=json.loads((folder/'report.json').read_text())
    assert report['status']=='complete' and report['watch']['complete'] and report['trace']['lines_dropped']==0
    writes=iter(json.loads(s)for s in (folder/'writes.jsonl').read_text().splitlines());pending=next(writes,None)
    row=None;rows=[]
    for line in (folder/'trace.txt').open():
        m=LINE.match(line)
        if not m:continue
        frame,cycle,pc=int(m[1]),int(m[2]),int(m[3],16)
        while pending and pending['cycle']<=cycle:
            at=pending['address']&0x1FFFFF
            memory[at:at+pending['width']]=pending['value'].to_bytes(pending['width'],'little');pending=next(writes,None)
        if pc in [0x800166CC,0x800167D0]:
            assert row is None
            row=dict(action='record' if pc==0x800166CC else 'discard' if discard else 'finish',
                requests=requests(memory),
                completed=memory[0xA96B4+0x751]!=0,elapsed_fields=struct.unpack_from('<H',memory,0xA9522)[0])
        elif pc in [0x800167AC,0x8001680C]:
            assert row is not None
            row.update(recorded_ticks=struct.unpack_from('<I',memory,STREAM)[0],
                storage_bytes=struct.unpack_from('<H',memory,STREAM+16)[0],closed=bool(struct.unpack_from('<H',memory,STREAM+12)[0]),
                pending_ticks=struct.unpack_from('<i',memory,STREAM+8)[0]+1)
            rows.append(row);row=None
    assert row is None and rows
    final=(folder/'ram.bin').read_bytes()
    return dict(name=name,seed_ticks=seed_ticks,capacity=capacity or 17408,natural=natural,discard=discard,
        rows=rows,command=command,report=str(folder/'report.json'),build=report['build'],
        state_sha256=hashlib.sha256(state.read_bytes()).hexdigest(),final_ram=str(folder/'ram.bin'))


def export_clips(cases,target):
    playback=[]
    for case in cases:
        if not case['discard'] and case['rows'][-1]['closed'] and case['rows'][-1]['recorded_ticks']:
            ram=Path(case['final_ram']).read_bytes()
            (target/(case['name']+'.ron')).write_text(ron(decode(ram))+'\n')
            if case['name'] in ['cap-22','cap-23','cap-40']:
                probe=playback_probe('recorded-'+case['name'],ram,case['rows'][-1]['recorded_ticks'])
                playback.append(probe)
                (target/(probe['name']+'.ron')).write_text(ron(probe['clip'])+'\n')
    if playback:
        (target/'capacity-playback.json').write_text(json.dumps(playback,indent=2)+'\n')
        print(f'{len(playback)} capacity round trips, {sum(len(p["rows"])for p in playback)} source decoder calls')


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--fresh',action='store_true');parser.add_argument('--reexport',action='store_true');parser.add_argument('--full-capacity',action='store_true');args=parser.parse_args()
    assert not(args.fresh and args.full_capacity)
    target=Path('game/tests/data/original/replay_recording')
    if args.reexport:
        cases=json.loads((target/('fresh.json'if args.fresh else 'initial.json')).read_text())
        digest=hashlib.sha256(Path('.psx/current-core-export/start/state.bin').read_bytes()).hexdigest()
        assert all(c['state_sha256']==digest for c in cases)
        export_clips(cases,target);return
    cases=[]
    if args.full_capacity:
        inputs=[dict(frame=f,buttons=['SQUARE','LEFT'] if (f//2)%2 else ['SQUARE','RIGHT'])for f in range(0,17600,2)]
        case=record('full-capacity',inputs,17600)
        assert case['rows'][-1]['closed'], 'full-size storage must be filled'
        target.mkdir(parents=True,exist_ok=True)
        (target/'full-capacity.json').write_text(json.dumps([case],indent=2)+'\n')
        export_clips([case],target)
        print(f'{len(case["rows"])} full-capacity original writer calls');return
    if args.fresh:
        cases.append(record('fresh-natural', [dict(frame=0,buttons=['CROSS'],until=2220),dict(frame=2220,buttons=['SQUARE'])],2600,natural=True))
        for i,cap in enumerate([21,26,29,38]):
            inputs=[dict(frame=f,buttons=['LEFT','RIGHT','L2','R2'] if (f//4)%2 else ['TRIANGLE','CIRCLE','CROSS'])for f in range(0,60,4)]
            cases.append(record(f'fresh-cap-{cap}',inputs,70,capacity=cap,stop_at=60))
        cases.append(record('fresh-drop',[dict(frame=0,buttons=['SQUARE','LEFT'])],34,stop_at=24,discard=True))
    else:
        cases.append(record('natural-gold',[dict(frame=0,buttons=['CROSS'],until=2210),dict(frame=2210,buttons=['SQUARE'])],2500,natural=True))
        cases.append(record('natural-failure',[dict(frame=0,buttons=['CROSS'])],2450,natural=True))
        for cap in [17,22,23,24,25,32,40]:
            inputs=[dict(frame=f,buttons=['CROSS','LEFT','R2'] if (f//2)%2 else ['SQUARE','RIGHT','L2'])for f in range(0,40,2)]
            cases.append(record(f'cap-{cap}',inputs,50,capacity=cap,stop_at=40))
        for n in [7,8,1023,1024,131071,131072,16777215,16777216]:
            cases.append(record(f'hold-{n}',[dict(frame=0,buttons=['CROSS'])],10,seed_ticks=n,stop_at=6))
        cases.append(record('discard',[dict(frame=0,buttons=['CROSS'])],30,stop_at=20,discard=True))
        cases.append(record('empty',[],10,stop_at=0))
    target=Path('game/tests/data/original/replay_recording');target.mkdir(parents=True,exist_ok=True)
    export_replay_recording(Path('.psx/awards/gold-session/ram.bin').read_bytes(),target)
    (target/('fresh.json'if args.fresh else 'initial.json')).write_text(json.dumps(cases,indent=2)+'\n')
    export_clips(cases,target)
    print(f'{len(cases)} original recorder cases, {sum(len(c["rows"])for c in cases)} calls')


if __name__=='__main__':main()
