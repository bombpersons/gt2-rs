#!/usr/bin/env python3
"""Record wheel reaction/differential rotation from original runtime only.

The middle axle/engine solver remains an observed input boundary. All raw
traces, tuning and numerical reference fixtures stay in ignored folders.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
from record_tyre_forces import BASE, EFFECT
from record_forward_dynamics import export as forward_rows, RANGES

OUT=Path('game/tests/data/original/wheel_rotation')
PREFIX=re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')


def export(folder,frames,initial):
    report=json.loads((folder/'report.json').read_text())
    assert report['status']=='complete' and report['trace']['lines_dropped']==0
    assert report['watch']['complete']
    start=report['frames']['start'];cars={};snapshots={};ram=bytearray(initial)
    writes=iter(sorted((json.loads(line) for line in (folder/'writes.jsonl').open()),key=lambda e:e['cycle']))
    pending=next(writes,None)
    current_axle=None
    def snapshot(cycle):
        nonlocal pending
        while pending and pending['cycle']<=cycle:
            address=pending['address']&0x1FFFFF;size=pending['width']
            ram[address:address+size]=pending['value'].to_bytes(size,'little')
            pending=next(writes,None)
        values={}
        for i in range(4):
            b=(BASE&0x1FFFFF)+0x460+i*0x68
            values[f'delta{i}']=struct.unpack_from('<h',ram,b+0x16)[0]
            values[f'tread{i}']=struct.unpack_from('<i',ram,b+0x18)[0]
        for axle in range(2):values[f'latch{axle}']=ram[(BASE&0x1FFFFF)+0x61A+axle]
        return values
    for line in (folder/'trace.txt').open():
        prefix=PREFIX.match(line)
        if not prefix:continue
        frame,cycle,pc=prefix.groups();frame=int(frame)-start;cycle=int(cycle);pc=int(pc,16)
        car=cars.setdefault(frame,{})
        if pc==0x80046A40:
            assert current_axle in [0,1]
            car[f'word_clamp{current_axle}']=1
        if pc in [0x800465E0,0x80046B54]:
            key='before' if pc==0x800465E0 else 'after'
            assert (frame,key) not in snapshots
            snapshots[frame,key]=snapshot(cycle)
        m=EFFECT.match(line.strip())
        if not m:continue
        _,_,address,op,raw=m.groups();address=int(address,16);value=int(raw,16)
        if value>=1<<(len(raw)*4-1):value-=1<<(len(raw)*4)
        field=None
        if pc in [0x8003E4FC,0x8003E5E4]:
            field='source_yaw_correction'
        elif pc==0x8003A1B8:
            wheel=(address-BASE-0x4C0)//0x68;assert 0<=wheel<4
            field='brake_input'+str(wheel)
        elif pc==0x8003A1E8:field='handbrake'
        elif pc in [0x8003A1E4,0x8003A22C]:
            wheel=(address-0x1F800004)//0x1C;assert 0<=wheel<4
            field='brake'+str(wheel)
        elif pc in [0x8004570C,0x80045710,0x80045744,0x80045750]:
            label,offset={0x8004570C:('response_force',8),0x80045710:('auxiliary_force',12),0x80045744:('brake',4),0x80045750:('brake',4)}[pc]
            wheel=(address-0x1F800000-offset)//0x1C;assert 0<=wheel<4
            field=label+str(wheel)
        elif pc in [0x80045728,0x8004575C,0x80045914,0x8004593C,0x8004594C,0x800467F0,0x80046814,0x80046868,0x8004688C,0x8004689C]:
            wheel=(address-0x1F800010)//0x1C;assert 0<=wheel<4
            field=('prepared_torque' if pc<0x800465E0 else 'final_torque')+str(wheel)
        elif pc==0x8004572C:
            wheel=(address-BASE-0x478)//0x68;assert 0<=wheel<4
            field='consumed_tread'+str(wheel)
        elif pc==0x80045860:field='yaw_correction'
        elif pc==0x800458A4:field='steering'
        elif pc in [0x800469B4,0x80046A68]:
            axle=(address-BASE-0x634)//4;assert 0<=axle<2
            field='shaft_speed'+str(axle)
        elif pc==0x80046738:
            axle=(address-0x1F800074)//4;assert 0<=axle<2
            field='drive_torque'+str(axle)
        elif pc==0x80046790:
            axle=address-BASE-0x61A;assert 0<=axle<2
            field='consumed_latch'+str(axle)
        elif pc==0x800466EC:
            current_axle=address-BASE-0x37A
            assert current_axle in [0,1]
            assert value==initial[(BASE&0x1FFFFF)+0x37A+current_axle]
        if field:
            if field in car and not field.startswith(('prepared_torque','final_torque')) and pc not in [0x8003A22C,0x8003E5E4]:assert car[field]==value,(folder,field,car[field],value)
            car[field]=value
    assert sorted(cars)==list(range(0,frames,2))
    result=forward_rows(folder,frames)
    for frame,car in cars.items():
        before=snapshots[frame,'before'];after=snapshots[frame,'after']
        for key,value in before.items():car['before_'+key]=value
        for key,value in after.items():car['after_'+key]=value
        for i in range(4):
            for label in ['response_force','auxiliary_force','prepared_torque','consumed_tread','brake_input']:assert label+str(i) in car,(folder,label,i)
            assert 'brake'+str(i) in car
            car.setdefault('final_torque'+str(i),car['prepared_torque'+str(i)])
        for axle in range(2):
            mode=initial[(BASE&0x1FFFFF)+0x37A+axle]
            car[f'mode{axle}']=mode
            for label,offset,fmt in [('preload',0x37C+axle*4,'i'),('drive_response',0x384+axle*4,'i'),('coast_response',0x38C+axle*4,'i'),('radius',0x3E4+axle*2,'h'),('rotation_response',0x3F8+axle*4,'i')]:
                car[label+str(axle)]=struct.unpack_from('<'+fmt,initial,(BASE&0x1FFFFF)+offset)[0]
            car.setdefault('shaft_speed'+str(axle),0)
            car.setdefault('drive_torque'+str(axle),0)
            car.setdefault('consumed_latch'+str(axle),before['latch'+str(axle)])
            car.setdefault('word_clamp'+str(axle),0)
            assert car['consumed_latch'+str(axle)]==before['latch'+str(axle)]
        car.setdefault('yaw_correction',0);car.setdefault('steering',0)
        assert 'source_yaw_correction' in car
        if car['yaw_correction']:
            assert car['yaw_correction']==car['source_yaw_correction']
    for row in result:
        car=cars[row['run_frame']];wheel=row['wheel']
        assert car['response_force'+str(wheel)]==row['response_force']
        assert car['consumed_tread'+str(wheel)]==row['tread']
        assert car['before_tread'+str(wheel)]==row['tread']
        row.update({'rot_'+key:value for key,value in car.items()})
    return result


def cases(holdout, final_holdout=False, gate_holdout=False):
    if gate_holdout:
        for mode in [6, 7]:
            for body in [-11381, 0, 11379, 11380, 11381]:
                yield mode, 0, 0, [8193, 4097, 16385, 12289], [12289]*4, [17, -19, 23, -29], 173, body
        return
    if final_holdout:
        # Fresh mixed front/rear kinds, unequal signs, tiny tread speeds and
        # nonzero history. These are interventions in the original, never
        # outputs calculated by a reference implementation.
        for mode in range(8):
            for sign in [-1, 1]:
                yield (mode, 7-mode), mode % 2, sign*317, [sign*71, -sign*69, sign*19713, -sign*23917], [613, -791, 21891, -15917], [-32768, 32767, -137, 193], sign*173, 11381
        return
    if holdout:
        for mode in range(8):
            for latch in [0,1]:yield mode,latch,-173,[-701,217,3791,-4111],[917,-317,2013,-1771], [32767,-32768,137,-193],-239, 11381
        return
    for mode in range(8):
        for latch in [0,1]:
            for sign in [-1,0,1]:
                yield mode,latch,sign*193,[sign*131073,sign*81921,-sign*73729,sign*49153],[sign*122881]*4,[17,-19,23,-29],sign*100,102605
    for mode in [0,1,2,3,4,5,6,7]:
        for body in [-11380,0,11379,11380,11381]:
            yield mode,1,173,[body]*4,[body]*4,[32767,-32768,32766,-32767],100,body


def main():
    p=argparse.ArgumentParser(description=__doc__)
    suites=p.add_mutually_exclusive_group()
    suites.add_argument('--holdout',action='store_true')
    suites.add_argument('--final-holdout',action='store_true')
    suites.add_argument('--gate-holdout',action='store_true')
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-state1200/state.bin'))
    args=p.parse_args()
    original=args.state.with_name('ram.bin').read_bytes()
    name='gate_holdout' if args.gate_holdout else 'final_holdout' if args.final_holdout else 'holdout' if args.holdout else 'boundaries'
    rows=[];records=[]
    jobs=[(f'{i:03}',2,[],case) for i,case in enumerate(cases(args.holdout, args.final_holdout, args.gate_holdout))]
    natural=[] if args.final_holdout or args.gate_holdout else [('mixed',['SQUARE','RIGHT','CIRCLE'])] if args.holdout else [('gas',['CROSS']),('brake',['SQUARE']),('turn',['CROSS','LEFT'])]
    jobs += [(label,40,buttons,None) for label,buttons in natural]
    if not args.holdout and not args.final_holdout and not args.gate_holdout:
        jobs.append(('clutch-sequence',40,['CROSS'],(3,0,137,[131072,98304,81920,65536],[122880]*4,[0]*4,100,102605)))
    for sequence,(label,frames,buttons,case) in enumerate(jobs):
        folder=Path('.psx/runs')/f'wheel-rotation-{name}-{label}';initial=bytearray(original)
        command=['psx','run','--from-state',str(args.state),'--frames',str(frames),'--input',json.dumps([dict(frame=0,buttons=buttons)]),'--trace','--max-events','200000','--out',str(folder),'--summary']
        for region in RANGES+['8003A19C-8003A278','8003E4F0-8003E5E8','80045688-80045980','800465E0-80046B58']:command+=['--trace-range',region]
        for i in range(4):command+=['--watch',f'{BASE+0x476+i*0x68:08X}:6']
        command+=['--watch',f'{BASE+0x61A:08X}:2']
        if case:
            mode,latch,angle,treads,forward,previous,yaw,body=case
            modes=mode if isinstance(mode,tuple) else (mode,mode)
            changes=[(0x45D,1,1),(0x60C,2,angle),(0x64,2,4096),(0x66,2,4096),(0x36C,2,0),(0x36E,2,angle),(0x6A4,4,body),(0x458,2,100),(0x45A,2,-512),(0x64C,4,yaw),(0x60A,2,1),(0x619,1,1),(0x708,2,4096 if yaw>=0 else 0)]
            for axle in range(2):
                mode=modes[axle]
                changes += [(0x37A+axle,1,mode),(0x61A+axle,1,latch),(0x37C+axle*4,4,65536 if mode==5 else 4096 if mode>=6 else 512),(0x384+axle*4,4,128 if mode==5 else 1024),(0x38C+axle*4,4,3072)]
            for i in range(4):
                changes += [(0x478+i*0x68,4,treads[i]),(0x48C+i*0x68,4,forward[i]),(0x490+i*0x68,4,0),(0x48A+i*0x68,2,4096),(0x476+i*0x68,2,previous[i]),(0x4C0+i*0x68,2,4096 if yaw<0 else 0)]
            if args.gate_holdout:
                # Remove forward forces upstream, so the consumer reaches
                # exactly the supplied body speed instead of a nearby value.
                # Keep wheel bases unsteered but request body steering for
                # nonzero original yaw-shortfall production at 8003E5E4.
                changes += [(0x71A,2,0),(0x60C,2,173)]
                for i in range(4):
                    changes += [(0x468+i*0x68,4,0),(0x494+i*0x68,4,0)]
            for off,size,value in changes:
                command+=['--poke',f'{BASE+off:08X}={value}:{size}']
                pos=(BASE&0x1FFFFF)+off;initial[pos:pos+size]=(value%(1<<(size*8))).to_bytes(size,'little')
        subprocess.run(command,check=True,capture_output=True,text=True)
        actual=export(folder,frames,initial)
        if case:
            for row in actual[:4]:
                wheel=row['wheel']
                assert (row['body_forward'],row['tread'],row['forward']) == (body,treads[wheel],forward[wheel]), (folder,wheel,'intervention not consumed')
                if args.gate_holdout:
                    assert row['fd_speed']==body, (folder,'gate speed not consumed')
        for row in actual:row['sequence']=sequence;row['case']=len(rows)//4;rows.append(row)
        records.append(dict(command=command,report=str(folder/'report.json')))
    OUT.mkdir(parents=True,exist_ok=True)
    with (OUT/f'{name}.csv').open('w',newline='') as f:
        writer=csv.DictWriter(f,sorted(rows[0]));writer.writeheader();writer.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows)//4,records=records,state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),ram_sha256=hashlib.sha256(original).hexdigest(),psx_version=subprocess.check_output(['psx','--version'],text=True).strip(),scope='Wheel torque preparation and differential tread integration; intervening shaft speed/torque measured from original solver'),indent=2)+'\n')
    print(f'{len(rows)//4} original {name} wheel-rotation executions exported')


if __name__=='__main__':main()
