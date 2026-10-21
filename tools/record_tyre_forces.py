#!/usr/bin/env python3
"""Record original tyre response/force consumers; all output data stays private."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

BASE = 0x800A96B4
OUT = Path('game/tests/data/original/tyre_forces')
EFFECT = re.compile(r'^(\d+) \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \[([0-9a-f]+)\](=>|=)([0-9a-f]+)$')


def probes(ram, holdout):
    if holdout:
        for angle in [137, -719, 1025, -2047]:
            yield angle, 3741, [(3741, 17391, -2779), (-3741, -17391, 2779),
                                (5688, 6619, 2274), (-5688, -6619, -2274)], None
        for tread, difference in [(5690, 1138), (-5690, -1138), (431, 1137), (-431, -1137)]:
            yield -193, tread+difference, [(tread, 317, n) for n in [-2291, -17, 19, 2291]], None
        return
    knots=set()
    for offset in [0x1C0,0x298]:
        count,x=struct.unpack_from('<H2xI',ram,(BASE&0x1FFFFF)+offset)
        xs=struct.unpack_from('<'+str(count)+'h',ram,x&0x1FFFFF)
        for k in xs:
            knots.update(v for v in [k-1,k,k+1] if -4096<=v<=4096)
        knots.update((a+b)//2 for a,b in zip(xs,xs[1:]))
    for ratio in sorted(knots):
        # Exact desired slip before original force sampling, from speeds
        # well above the low-speed fade. The trace verifies actual inputs.
        tread,forward=(524288,524288+ratio*128) if ratio<=0 else (524288-ratio*128,524288)
        yield 0,524288,[(tread,forward,0)]*4, ratio
    for angle in [-2048,-1025,-1024,-1023,-535,-173,-128,-72,-48,-1,0,1,48,72,128,173,535,1023,1024,1025,2047]:
        yield angle,262144,[(262144,245760,0),(-262144,-245760,0),(245760,262144,0),(-245760,-262144,0)], None
    # Fractional activation keeps the cornering curve below its limit, so
    # its original knot/interpolation response is observable without clipping.
    angles=set()
    for offset in [0x194,0x26C]:
        count,x=struct.unpack_from('<H2xI',ram,(BASE&0x1FFFFF)+offset)
        for knot in struct.unpack_from('<'+str(count)+'h',ram,x&0x1FFFFF):
            for adjacent in [knot-1,knot,knot+1]:
                angles.update([adjacent,-adjacent])
    for angle in sorted(angles):
        yield angle,262144,[(262144,4096,0),(-262144,4096,0),(262144,4096,0),(-262144,4096,0)], None
    for tread in [-5690,-5689,0,5689,5690]:
        for difference in [-1138,-1137,0,1137,1138]:
            yield 0,tread+difference,[(tread,0,n) for n in [-2276,-2275,2275,2276]], None
    yield 0,0,[(0,0,0),(0,0,1),(0,0,-1),(0,0,1138)], None


def export(run, frames):
    report=json.loads((run/'report.json').read_text())
    assert report['status']=='complete' and report['trace']['lines_dropped']==0,run
    start=report['frames']['start'];rows={}
    for line in (run/'trace.txt').open():
        m=EFFECT.match(line.strip())
        if not m:continue
        absolute,pc,address,op,raw=m.groups();frame=int(absolute)-start;pc=int(pc,16);address=int(address,16)
        value=int(raw,16)
        if value>=1<<(len(raw)*4-1):value-=1<<(len(raw)*4)
        mapping={
            0x8003E47C:('steering',0x46C),0x80039504:('forward',0x48C),0x80039514:('lateral',0x490),
            0x80039520:('tread',0x478),0x80039F00:('geometric_angle',0x4B0),0x80039EE8:('activation',0x4B2),
            0x80039744:('slip',0x4A4),0x8003A3BC:('grip',0x494),0x8003A3E0:('speed_response',0x48A),
            0x8003A430:('wheel_force',0x4B8),0x8003A438:('ground_force',0x4BC),
            0x8003A4A0:('lateral_force',0x4B4),0x8003A540:('lateral_force',0x4B4),
            0x8003A5C8:('lateral_force',0x4B4),0x8003A6D0:('lateral_force',0x4B4),
            0x8003A4A8:('corner_response',0x49C),0x8003A538:('corner_response',0x49C),0x8003A5C0:('corner_response',0x49C),
        }
        if pc in mapping:
            field,offset=mapping[pc];wheel=(address-BASE-offset)//0x68
        elif pc in [0x8003A3CC,0x8003A3EC,0x8003A420]:
            field,offset={0x8003A3CC:('direction',0x16),0x8003A3EC:('reference',0x14),0x8003A420:('response_force',8)}[pc]
            wheel=(address-0x1F800000-offset)//0x1C
        elif pc in [0x8003D980,0x8003D9BC,0x8003DA68]:
            field='longitudinal_response';wheel=(address-0x1F80001A)//0x1C
        elif pc in [0x8003D990,0x8003DAA4]:
            field='corner_limit';wheel=(address-BASE-0x4A6)//0x68
        elif pc==0x8003A5D0:
            for wheel in range(4):
                row=rows.setdefault((frame,wheel),dict(run_frame=frame,wheel=wheel))
                if 'body_forward' in row:assert row['body_forward']==value
                row['body_forward']=value
            continue
        else:continue
        assert 0<=wheel<4,(run,pc,address,wheel)
        row=rows.setdefault((frame,wheel),dict(run_frame=frame,wheel=wheel))
        if field in row and field!='lateral_force':assert row[field]==value,(run,row,field,value)
        row[field]=value
    expected={'run_frame','wheel','steering','forward','lateral','tread','geometric_angle','activation','slip','grip','speed_response',
              'wheel_force','ground_force','lateral_force','corner_response','direction','reference','response_force','longitudinal_response','corner_limit','body_forward'}
    assert sorted(rows)==[(f,i) for f in range(0,frames,2) for i in range(4)],run
    for row in rows.values():assert set(row)==expected,(run,set(row)^expected,row)
    return [rows[k] for k in sorted(rows)]


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--holdout',action='store_true')
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-state1200/state.bin'));args=p.parse_args()
    ram=args.state.with_name('ram.bin').read_bytes();name='holdout' if args.holdout else 'boundaries';OUT.mkdir(parents=True,exist_ok=True)
    rows=[];records=[]
    ranges=['80039EE0-80039F04','80039490-80039778','8003E470-8003E4C0','800397D0-800398F0','8003D940-8003DAA8','8003A380-8003A6F4']
    cases=list(probes(ram,args.holdout))
    for index,(angle,body,wheels,expected_slip) in enumerate(cases):
        folder=Path('.psx/runs')/f'tyre-forces-{name}-{index:03}'
        command=['psx','run','--from-state',str(args.state),'--frames','2','--input','[]','--trace','--max-events','20000','--out',str(folder),'--summary']
        for region in ranges:command+=['--trace-range',region]
        for off,width,value in [(0x45D,1,1),(0x60C,2,angle),(0x64,2,4096),(0x66,2,4096),(0x36C,2,0),(0x36E,2,angle),(0x6A4,4,body)]:
            command+=['--poke',f'{BASE+off:08X}={value}:{width}']
        for i,(tread,forward,lateral) in enumerate(wheels):
            b=BASE+0x460+i*0x68
            for off,width,value in [(0x18,4,tread),(0x2C,4,forward),(0x30,4,lateral),(0x2A,2,4096)]:
                command+=['--poke',f'{b+off:08X}={value}:{width}']
        subprocess.run(command,check=True,capture_output=True,text=True)
        actual=export(folder,2)
        for row,w in zip(actual,wheels):
            assert (row['tread'],row['forward'],row['lateral'])==w
            assert row['body_forward']==body and row['speed_response']==4096
            if expected_slip is not None: assert row['slip']==expected_slip,(row,expected_slip)
            row['sequence']=0;row['case']=len(rows);rows.append(row)
        records.append(dict(command=command,report=str(folder/'report.json')))
    scenarios=[('gas',['CROSS']),('brake',['SQUARE']),('turn',['CROSS','LEFT'])] if not args.holdout else [('mixed',['SQUARE','RIGHT','CIRCLE'])]
    for sequence,(scenario,buttons) in enumerate(scenarios,1):
        folder=Path('.psx/runs')/f'tyre-forces-{name}-{scenario}'
        command=['psx','run','--from-state',str(args.state),'--frames','40','--input',json.dumps([dict(frame=0,buttons=buttons)]),'--trace','--max-events','50000','--out',str(folder),'--summary']
        for region in ranges:command+=['--trace-range',region]
        subprocess.run(command,check=True,capture_output=True,text=True)
        for row in export(folder,40):row['sequence']=sequence;row['case']=len(rows);rows.append(row)
        records.append(dict(command=command,report=str(folder/'report.json')))
    with (OUT/f'{name}.csv').open('w',newline='') as f:
        w=csv.DictWriter(f,sorted(rows[0]));w.writeheader();w.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(cases=len(rows),holdout=args.holdout,records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),ram_sha256=hashlib.sha256(ram).hexdigest(),
        psx_version=subprocess.check_output(['psx','--version'],text=True).strip(),
        scope='Original paired tyre curves and per-wheel forces, including lateral low-speed correction; contact/grip and body inputs measured'),indent=2)+'\n')
    print(f'{len(rows)} original {name} wheel-force executions exported')


if __name__=='__main__':main()
