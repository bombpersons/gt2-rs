#!/usr/bin/env python3
"""Observe original grip preparation and post-collision contact motion.

No replacement model supplies expected values. Original RAM/traces and all
numerical CSV/JSON remain ignored. Grip executes before controls; contact
motion refresh executes after integration/collision and feeds the next tick.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
from record_tyre_forces import BASE, EFFECT, export as tyre_rows
from record_forward_dynamics import RANGES
from record_wheel_rotation import PREFIX

OUT=Path('game/tests/data/original/contact_inputs')
PHASES={0x80039AB0:'grip_before',0x80039D9C:'grip_after',0x800304DC:'motion_before',
        0x80030698:'projected',0x8003090C:'motion_after'}
REGISTER=re.compile(r'^\d+ \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \w+=([0-9a-f]{8})$')


def export(folder,frames,initial):
    report=json.loads((folder/'report.json').read_text())
    assert report['status']=='complete' and report['watch']['complete'] and report['trace']['lines_dropped']==0
    start=report['frames']['start'];ram=bytearray(initial);cars={};wheel=None
    writes=iter(sorted((json.loads(line) for line in (folder/'writes.jsonl').open()),key=lambda q:q['cycle']))
    pending=next(writes,None)
    def snapshot(cycle):
        nonlocal pending
        while pending and pending['cycle']<=cycle:
            p,size=pending['address']&0x1FFFFF,pending['width']
            ram[p:p+size]=pending['value'].to_bytes(size,'little');pending=next(writes,None)
        def read(p,fmt='i'):return struct.unpack_from('<'+fmt,ram,p&0x1FFFFF)[0]
        def car(off,fmt='i'):return read(BASE+off,fmt)
        v={k:car(o,f) for k,o,f in [('vx',0x628,'i'),('vy',0x62C,'i'),('vz',0x630,'i'),
            ('yaw',0x64C,'i'),('body_forward',0x6A4,'i'),('body_lateral',0x6A8,'i'),('roll',0x646,'h'),
            ('contact_roll',0x6F6,'h'),('condition_flags',0x78D,'B'),('duration',0x6FE,'h'),
            ('speed',0x6AE,'H'),('maximum_speed',0x6F8,'H')]}
        for key,off in [('condition_maximum',0x46F48),('condition_max_loss',0x46F4C),('condition_cold',0x46F54),
                        ('condition_cold_loss',0x46F58),('condition_warm',0x46F5C),('condition_warm_loss',0x46F60)]:v[key]=read(off)
        for axis,off in [('forward_axis',0x68C),('lateral_axis',0x694)]:
            for i in range(3):v[f'{axis}{i}']=car(off+i*2,'h')
        for i in range(4):
            for k,o,f in [('load',0x8,'i'),('surface',0x14,'B'),('tread',0x18,'i'),('heading',0xC,'h'),
                ('spin',0x20,'H'),('forward',0x2C,'i'),('lateral',0x30,'i'),('response',0x2A,'h'),
                ('capacity',0x34,'i'),('condition_scale',0x38,'h'),('indicator',0x3F,'b'),
                ('flags',0x63,'B'),('accumulator',0x64,'i')]:v[f'{k}{i}']=car(0x460+i*0x68+o,f)
        for i in range(2):
            v[f'shaft{i}']=car(0x634+i*4)
            v[f'independent{i}']=int(car(0x37A+i,'B')==0)
            v[f'camber{i}']=car(0x368+i*2,'h')
        return v
    for line in (folder/'trace.txt').open():
        m=PREFIX.match(line)
        if not m:continue
        absolute,cycle,pc=m.groups();frame=int(absolute)-start;pc=int(pc,16)
        car=cars.setdefault(frame,{})
        if pc in PHASES:
            phase=PHASES[pc];assert phase not in car
            car[phase]=snapshot(int(cycle))
        effect=EFFECT.match(line.strip())
        if effect and pc==0x80039CD8:
            wheel=(int(effect[3],16)-BASE-0x468)//0x68;assert 0<=wheel<4
        register=REGISTER.match(line.strip())
        if register and pc in [0x80039D1C,0x80039D38,0x80039D5C]:
            value=int(register[2],16);value=value if value<0x80000000 else value-0x100000000
            assert wheel in range(4)
            key={0x80039D1C:'effective_load',0x80039D38:'load_capacity',0x80039D5C:'camber_response'}[pc]
            car[f'{key}{wheel}']=value
    assert sorted(cars)==list(range(0,frames,2))
    for car in cars.values():
        assert set(PHASES.values())<=set(car)
        for i in range(4):assert all(k+str(i) in car for k in ['effective_load','load_capacity','camber_response'])
    result=tyre_rows(folder,frames)
    for row in result:
        car=cars[row['run_frame']]
        assert row['grip']==car['grip_after'][f'capacity{row["wheel"]}']
        for phase in PHASES.values():row.update({f'ci_{phase}_{k}':v for k,v in car[phase].items()})
        for k,v in car.items():
            if k not in PHASES.values():row['ci_'+k]=v
    return result


def cases(ram,holdout):
    def car(off,fmt='h'):return struct.unpack_from('<'+fmt,ram,(BASE&0x1FFFFF)+off)[0]
    if holdout:
        for sign in [-1,1]:
            for roll in [173,719,3191]:
                yield dict(kind=0,loads=[17391,5197,31917,7319],surfaces=[3,6,1,7],roll=sign*roll,contact_roll=-sign*317)
        for speed in [113797,113798,113799,113800,-113797,-113798,-113799,-113800]:
            yield dict(kind=1,velocity=[speed,-317,197],yaw=17391,flags=[0,1,2,8],independent=[1,0])
        return
    loads={-1,0,1}
    cambers={0}
    for off in [0x224,0x2FC]:
        n,_,xs,_=struct.unpack_from('<HHII',ram,(BASE&0x1FFFFF)+off)
        knots=struct.unpack_from('<'+'i'*n,ram,xs&0x1FFFFF)
        for value in knots:loads.update([value-1,value,value+1])
        loads.update((a+b)//2 for a,b in zip(knots,knots[1:]))
    for off in [0x250,0x328]:
        n,_,xs,_=struct.unpack_from('<HHII',ram,(BASE&0x1FFFFF)+off)
        for value in struct.unpack_from('<'+'h'*n,ram,xs&0x1FFFFF):cambers.update([value-1,value,value+1])
    for load in sorted(loads):yield dict(kind=0,loads=[load]*4)
    for camber in sorted(cambers):
        for sign in [-1,1]:yield dict(kind=0,roll=sign*camber,camber=[17,-31])
    for surface in range(16):yield dict(kind=0,surfaces=[surface]*4)
    for roll,contact in [(32767,-1),(32767,0),(-32768,0),(-32768,1),(32767,-32768)]:
        yield dict(kind=0,roll=roll,contact_roll=contact,camber=[1,-1])
    for enabled in [False,True]:
        for bypass in [False,True]:
            for value in [-13792,-13791,-13790,-1,0,1,13501,27000,27001,27002,63502,100002,100003,100004]:
                yield dict(kind=0,condition=enabled,bypass=bypass,accumulator=[value,value+1,value-1,-value],
                           scales=[1701,0,-317,4095])
    # Drive flags are rewritten at 8003A264 and 80045BA4..C10. Change their
    # load/brake/layout producers to exercise each free-axle wheel gate.
    for layout in [0,1]:
        free=[0,1] if layout==0 else [2,3]
        for left in [0,1,2]:
            for right in [0,1,2]:
                flags=[8]*4;flags[free[0]]=left;flags[free[1]]=right
                for independent in [[0,0],[1,0],[0,1],[1,1]]:
                    yield dict(kind=1,layout=layout,flags=flags,independent=independent,
                               velocity=[173917,-7319,3191],yaw=-17391)
    for speed in [-113800,-113799,-113798,-4097,-1,0,1,4097,113798,113799,113800]:
        yield dict(kind=1,velocity=[0,speed,0],yaw=0,flags=[1]*4)
    for yaw in [-524288,-17391,-1,0,1,17391,524288]:
        yield dict(kind=1,velocity=[0,0,0],yaw=yaw,flags=[0,1,2,0],spin=[0,1,4095,2048])


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--holdout',action='store_true')
    p.add_argument('--limit',type=int,help='Debug prefix; uses separate probe output')
    args=p.parse_args();original=args.state.with_name('ram.bin').read_bytes()
    name='probe' if args.limit is not None else 'holdout' if args.holdout else 'boundaries'
    probes=list(cases(original,args.holdout))
    if args.limit is not None:probes=probes[:args.limit]
    jobs=[(f'{i:03}',2,[],c) for i,c in enumerate(probes)]
    if args.limit is None:
        scenarios=[('alternating',[dict(frame=0,buttons=['CROSS','RIGHT']),dict(frame=14,buttons=['SQUARE','CIRCLE']),
            dict(frame=38,buttons=['CROSS','LEFT']),dict(frame=64,buttons=[])])] if args.holdout else [
            ('gas',[dict(frame=0,buttons=['CROSS'])]),('brake',[dict(frame=0,buttons=['SQUARE'])]),('turn',[dict(frame=0,buttons=['CROSS','LEFT'])])]
        jobs +=[(label,80,replay,None) for label,replay in scenarios]
    rows,records=[],[]
    for sequence,(label,frames,replay,case) in enumerate(jobs):
        folder=Path('.psx/runs')/f'contact-inputs-{name}-{label}';initial=bytearray(original)
        command=['psx','run','--from-state',str(args.state),'--frames',str(frames),'--input',json.dumps(replay),
            '--trace','--max-events','1000000','--watch',f'{BASE:08X}:1952','--watch','80046F48:28','--out',str(folder),'--summary']
        for region in RANGES+['800304DC-80030920','80039A4C-80039DE8']:command+=['--trace-range',region]
        changes=[]
        if case:
            motion=case['kind']==1
            changes=[(BASE+0x45D,1,1),(BASE+0x646,2,case.get('roll',0)),(BASE+0x6F6,2,case.get('contact_roll',0)),
                (BASE+0x368,2,case.get('camber',[0,0])[0]),(BASE+0x36A,2,case.get('camber',[0,0])[1]),
                (BASE+0x78D,1,16 if case.get('bypass') else 0),(0x80046F48,4,100003 if case.get('condition') else 0),
                (0x80046F4C,4,1025),(0x80046F54,4,-13791),(0x80046F58,4,733),(0x80046F5C,4,27001),(0x80046F60,4,257)]
            for i in range(4):
                b=BASE+0x460+i*0x68
                for off,width,value in [(8,4,(0 if case['flags'][i]&1 else 8192) if motion else case.get('loads',[8193,12289,16385,4097])[i]),
                    (0x14,1,case.get('surfaces',[0,1,4,7])[i]),(0x64,4,case.get('accumulator',[0]*4)[i]),
                    (0x38,2,case.get('scales',[4096]*4)[i]),(0x3F,1,[-17,31,-63,127][i])]:changes.append((b+off,width,value))
            if motion:
                # Keep the earlier force phase quiescent; motion's actual
                # post-collision inputs are still captured, never assumed.
                changes +=[(BASE+0x6A4,4,0),(BASE+0x6A8,4,0),(BASE+0x64C,4,case.get('yaw',0)),
                    (BASE+0x644,2,0),(BASE+0x646,2,0),(BASE+0x648,2,0),(BASE+0x650,2,0),(BASE+0x652,2,0),
                    (BASE+0x71A,2,0),(BASE+0x6B1,1,1),(BASE+0x370,1,case.get('layout',0)),
                    (BASE+0x619,1,0),(BASE+0x60A,2,0),(BASE+0x642,1,2),
                    (BASE+0x634,4,0),(BASE+0x638,4,0),(BASE+0x614,2,0)]
                changes +=[(BASE+0x628+i*4,4,v) for i,v in enumerate(case['velocity'])]
                for i in range(2):changes.append((BASE+0x37A+i,1,0 if case.get('independent',[0,0])[i] else 2))
                for i in range(4):
                    for off,width,value in [(0x18,4,0),(0x2C,4,0),(0x30,4,0),(0x60,2,1024 if case['flags'][i]&2 else 0),
                        (0x20,2,case.get('spin',[0,1,4095,2048])[i])]:changes.append((BASE+0x460+i*0x68+off,width,value))
        for address,width,value in changes:
            command+=['--poke',f'{address:08X}={value}:{width}'];pos=address&0x1FFFFF
            initial[pos:pos+width]=(value%(1<<(8*width))).to_bytes(width,'little')
        subprocess.run(command,check=True,capture_output=True,text=True)
        actual=export(folder,frames,initial)
        if case and case['kind']==0:
            r=actual[0]
            assert [r[f'ci_grip_before_load{i}'] for i in range(4)]==case.get('loads',[8193,12289,16385,4097])
            assert [r[f'ci_grip_before_surface{i}'] for i in range(4)]==case.get('surfaces',[0,1,4,7])
            assert r['ci_grip_before_roll']==case.get('roll',0) and r['ci_grip_before_contact_roll']==case.get('contact_roll',0)
            assert bool(r['ci_grip_before_condition_maximum'])==bool(case.get('condition'))
            assert bool(r['ci_grip_before_condition_flags']&16)==bool(case.get('bypass'))
        if case and case['kind']==1:
            r=actual[0]
            assert [r[f'ci_motion_before_independent{i}'] for i in range(2)]==case.get('independent',[0,0])
            if 'layout' in case:
                free=[0,1] if case['layout']==0 else [2,3]
                for i in free:assert r[f'ci_motion_before_flags{i}']==case['flags'][i],(label,i,r[f'ci_motion_before_flags{i}'],case)
        for row in actual:
            row['sequence']=sequence;row['case']=len(rows)//4;row['input_kind']=case['kind'] if case else 2;rows.append(row)
        records.append(dict(command=command,report=str(folder/'report.json')))
    OUT.mkdir(parents=True,exist_ok=True)
    with (OUT/f'{name}.csv').open('w',newline='') as f:
        w=csv.DictWriter(f,sorted(rows[0]));w.writeheader();w.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows)//4,records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),ram_sha256=hashlib.sha256(original).hexdigest(),
        psx_version=subprocess.check_output(['psx','--version'],text=True).strip(),
        scope='Original pre-control grip, tyre force consumers and post-collision contact motion; load/orientation/collision producers external'),indent=2)+'\n')
    print(f'{len(rows)//4} original {name} contact-input calls exported')


if __name__=='__main__':main()
