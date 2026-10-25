#!/usr/bin/env python3
"""Probe original automatic stopping inputs/outputs; all captures remain private."""
import json
from pathlib import Path
import re
import struct
import subprocess
import hashlib
import argparse

BASE=0xA96B4
STATE=Path('.psx/runs/b1-result-overshoot-checkpoint/state.bin')
LINE=re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state',type=Path,default=STATE)
    parser.add_argument('--psx',default='psx')
    args=parser.parse_args()
    initial=args.state.with_name('ram.bin').read_bytes()
    idle=struct.unpack_from('<H',initial,BASE+0x10A)[0]
    reengage=struct.unpack_from('<H',initial,BASE+0x396)[0]*3//4
    # Linear axes use /4096 m/s; angular values here are /4096 turn/s
    # plus deliberate fractional raw values to test signed narrowing.
    cases=[]
    for linear in [[283,0,0],[284,0,0],[-283,0,0],[-284,0,0],[260,47,48],[260,48,48]]:
        cases.append(dict(linear=linear,angular=[0,0,0]))
    for angular in [[33*16,0,0],[34*16,0,0],[-33*16-15,0,0],[-34*16,0,0],
                    [0,33*128+127,0],[0,-33*128-127,0],[30*16,7*128,8*16],[30*16,8*128,8*16]]:
        cases.append(dict(linear=[0,0,0],angular=angular))
    for angle in [-35,-34,34,35]:cases.append(dict(angle=angle))
    for engaged,rpm in [(1,idle-1),(1,idle),(1,idle+1),(0,reengage-1),(0,reengage),(0,reengage+1)]:
        cases.append(dict(linear=[10000,0,0],engaged=engaged,rpm=rpm))
    cases += [dict(mode=7,linear=[10000,0,0],angle=-35),dict(linear=[0,0,0],engaged=1,rpm=4000)]
    rows=[]
    for index,case in enumerate(cases):
        pokes={0x786:(case.get('mode',6),'B'),0x60C:(case.get('angle',123),'h'),0x60E:(321,'h'),
               0x619:(case.get('engaged',1),'B'),0x6AC:(case.get('rpm',4000),'h')}
        for o,v in zip([0x628,0x630,0x62C],case.get('linear',[4096,0,0])):pokes[o]=(v,'i')
        for o,v,fmt in zip([0x650,0x64C,0x652],case.get('angular',[0,0,0]),['h','i','h']):pokes[o]=(v,fmt)
        run=Path('.psx/runs')/f'automatic-stop-probe-{index}'
        cmd=[args.psx,'run','--from-state',str(args.state),'--frames','2','--trace',
             '--trace-range','8003E304-8003E308','--trace-range','8003E310-8003E314',
             '--watch','800A96B4:1952','--out',str(run),'--summary']
        memory=bytearray(initial)
        for off,(value,fmt) in pokes.items():
            encoded=struct.pack('<'+fmt,value);memory[BASE+off:BASE+off+len(encoded)]=encoded
            cmd+=['--poke',f'{BASE+0x80000000+off:08X}={value}:{len(encoded)}']
        completed=subprocess.run(cmd,capture_output=True,text=True)
        if completed.returncode:
            raise RuntimeError(completed.stderr or completed.stdout)
        report=json.loads((run/'report.json').read_text())
        assert report['status']=='complete' and report['watch']['complete'] and report['trace']['lines_dropped']==0
        writes=iter(json.loads(line) for line in (run/'writes.jsonl').open());pending=next(writes,None)
        def read(o,fmt):return struct.unpack_from('<'+fmt,memory,BASE+o)[0]
        def view():return dict(mode=read(0x786,'B'),angle=read(0x60C,'h'),rate=read(0x60E,'h'),
            pedals=[read(o,'h') for o in [0x610,0x612,0x614]],brakes=[read(0x4C0+i*0x68,'h') for i in range(4)],
            coupling=read(0x60A,'h')!=0)
        row=dict(command=cmd,report=str(run/'report.json'))
        for line in (run/'trace.txt').open():
            m=LINE.match(line)
            if not m:continue
            cycle,pc=int(m[2]),int(m[3],16)
            while pending and pending['cycle']<=cycle:
                a,w=pending['address']&0x1fffff,pending['width'];memory[a:a+w]=pending['value'].to_bytes(w,'little')
                pending=next(writes,None)
            if pc==0x8003E304:
                row.update(before=view(),linear=[read(0x628,'i')/4096,read(0x630,'i')/4096,-read(0x62C,'i')/4096],
                    angular=[read(0x650,'h')/65536,read(0x64C,'i')/524288,read(0x652,'h')/65536],
                    duration=read(0x6FE,'h')/65536,rpm=read(0x6AC,'h'),engaged=read(0x619,'B'))
            elif pc==0x8003E310:row['after']=view()
        assert 'before' in row and 'after' in row
        rows.append(row)
    target=Path('game/tests/data/original/automatic_stop');target.mkdir(parents=True,exist_ok=True)
    (target/'boundaries.json').write_text(json.dumps(dict(rows=rows,state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
        psx_version=subprocess.check_output([args.psx,'--version'],text=True).strip()),indent=2)+'\n')
    print(f'{len(rows)} original automatic stopping boundary cases')

if __name__=='__main__':main()
