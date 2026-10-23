#!/usr/bin/env python3
"""Original polygon queries, smoothed road contact and late orientation frames.

The native port never supplies expected values. Geometry interventions are
authored height/metadata changes to original producers and recorded explicitly.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

BASE=0x800A96B4
OUT=Path('game/tests/data/original/road_contact')
PREFIX=re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
EFFECT=re.compile(r'; \[([0-9a-f]{8})\](=>|=)([0-9a-f]+)$')
REGISTER=re.compile(r'; a0=([0-9a-f]{8})$')
PHASES={0x8003DAA8:'aero_before',0x80043388:'before',0x80043578:'queried',0x800438C0:'after',0x8003E7EC:'frame_before',0x8003E8CC:'frame_after',0x80043AE0:'suspension_before',0x80043F18:'suspension_after'}


def export(folder,frames,initial):
    report=json.loads((folder/'report.json').read_text())
    assert report['status']=='complete' and report['watch']['complete'] and report['trace']['lines_dropped']==0
    start=report['frames']['start'];ram=bytearray(initial);cars={};query=None
    def read(p,fmt='I'):return struct.unpack_from('<'+fmt,initial,p&0x1FFFFF)[0]
    section=read(0x800B4A44);node_ids={read(section+12+i*4):i for i in range(read(section+4,'H'))}
    polygon_ids={}
    for node in node_ids:
        grid=read(node+0x9C);ordered={}
        for cell in range(16):
            pointer=read(grid+40+cell*4)
            for j in range(read(grid+8+cell*2,'H')):
                polygon=read(pointer+j*4)
                if polygon not in ordered:ordered[polygon]=len(ordered)
        polygon_ids[node]=ordered
    writes=iter(sorted((json.loads(l) for l in (folder/'writes.jsonl').open()),key=lambda w:w['cycle']))
    pending=next(writes,None)
    def snapshot(cycle):
        nonlocal pending
        while pending and pending['cycle']<=cycle:
            p,size=pending['address']&0x1FFFFF,pending['width']
            ram[p:p+size]=pending['value'].to_bytes(size,'little');pending=next(writes,None)
        def car(off,fmt='i'):return struct.unpack_from('<'+fmt,ram,(BASE&0x1FFFFF)+off)[0]
        result={k:car(o,f) for k,o,f in [('x',0x65C,'i'),('y',0x660,'i'),('z',0x664,'i'),
            ('pitch',0x644,'h'),('roll',0x646,'h'),('heading',0x648,'h'),('node',0x600,'H'),
            ('pitch_rate',0x650,'h'),('roll_rate',0x652,'h'),('vx',0x628,'i'),('vy',0x62C,'i'),('vz',0x630,'i'),
            ('forward_speed',0x6A4,'i'),('aero_growth',0x774,'h'),('aero_reduction',0x776,'h'),
            ('duration',0x6FE,'h'),('reset_ticks',0x78E,'H'),('airborne',0x6B1,'b'),('mode',0x786,'B'),
            ('condition_flags',0x78D,'B'),('road_pitch',0x6F4,'h'),('road_roll',0x6F6,'h'),('road_height',0x688,'i'),('inverse_duration',0x704,'i'),('compressed_mask',0x719,'B'),
            ('front_force',0x71C,'i'),('rear_force',0x720,'i'),('lateral_force',0x728,'i'),('vertical_force',0x72C,'i'),('forward_force',0x734,'i')]}
        for key,off in [('body_forward',0x668),('body_lateral',0x670),('body_up',0x678),
                        ('road_forward',0x68C),('road_lateral',0x694),('road_up',0x69C)]:
            for i in range(3):result[f'{key}{i}']=car(off+2*i,'h')
        for i in range(4):
            for k,o,f in [('height',0,'i'),('raw_height',4,'i'),('surface',0x14,'B'),('variant',0x15,'B'),
                ('travel',0x10,'h'),('rate',0x12,'h'),('spring_force',0x48,'i'),('damper_force',0x4C,'i'),('load',8,'i'),
                ('forward',0x2C,'i'),('lateral',0x30,'i'),('bump_response',0x40,'h'),('offset',0x42,'h')]:
                result[f'{k}{i}']=car(0x460+0x68*i+o,f)
            for axis in range(3):result[f'attachment{i}_{axis}']=car(0x484+0x68*i+2*axis,'h')
        return result
    for line in (folder/'trace.txt').open():
        m=PREFIX.match(line)
        if not m:continue
        absolute,cycle,pc=m.groups();frame=int(absolute)-start;pc=int(pc,16)
        row=cars.setdefault(frame,{'queries':[]})
        if pc in PHASES:
            phase=PHASES[pc];assert phase not in row,(folder,frame,phase)
            row[phase]=snapshot(int(cycle))
        if pc==0x80028830:
            query={'polygon_node':-1,'polygon':-1};row['queries'].append(query)
        register=REGISTER.search(line.strip())
        if pc==0x800284BC and register:
            selected_node=int(register[1],16);query['polygon_node']=node_ids[selected_node]
        effect=EFFECT.search(line.strip())
        if not effect:continue
        address,operation,raw=effect.groups();value=int(raw,16)
        value=value if value<1<<(len(raw)*4-1) else value-(1<<(len(raw)*4))
        if pc==0x800284C0:
            query['polygon']=polygon_ids[selected_node][int(address,16)-4]
        fields={0x80028848:'seed',0x8002884C:'x',0x80028850:'height',0x80028864:'y',
                0x800288CC:'tags',0x800288D0:'node',0x800288E8:'result_height'}
        if pc in fields:
            key=fields[pc];query[key]=value&65535 if key in ['seed','node'] else value
    assert sorted(cars)==list(range(0,frames,2))
    result=[]
    for frame,car in sorted(cars.items()):
        assert set(PHASES.values())<=set(car) and len(car['queries'])==4,(folder,frame,car.keys())
        row={'run_frame':frame}
        for phase in PHASES.values():row.update({f'{phase}_{k}':v for k,v in car[phase].items()})
        for i,q in enumerate(car['queries']):
            assert set(q)=={'seed','x','height','y','tags','node','result_height','polygon_node','polygon'},q
            row.update({f'query{i}_{k}':v for k,v in q.items()})
        result.append(row)
    return result


def cases(holdout):
    if holdout:
        for lateral in [-79173,-32769,-1,1,32769,79173]:yield dict(lateral=lateral,warp=True)
        for surface in [2,5,6,7]:yield dict(surface=surface,flags=3,variant=2,old_heights=[-7319,17391,-317,719])
        for angles in [[173,-317,719],[-197,397,-1371]]:yield dict(angles=angles,warp=True)
        yield dict(near_node=-397,empty_grid=True)
        return
    for forward in [-409601,0,409601]:
        for lateral in [-819201,-65537,-49153,-32769,-1,0,1,32769,49153,65537,819201]:
            yield dict(forward=forward,lateral=lateral)
    for near in [-512,-256,0]:yield dict(near_node=near,empty_grid=True)
    for surface in range(8):
        for flags in [0,1,2,3]:yield dict(surface=surface,flags=flags,variant=surface%4)
    for history in [[0]*4,[-65537,-1,1,65537],[65537,1,-1,-65537]]:yield dict(old_heights=history,warp=True)
    for angles in [[0,0,0],[0,0,1024],[0,0,2048],[0,0,-1024],[173,317,731],[-173,-317,-731]]:
        yield dict(angles=angles)
    for velocity in [-262145,-4097,-1,0,1,4097,262145]:yield dict(velocity=velocity,surface=5)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--holdout',action='store_true');p.add_argument('--limit',type=int)
    args=p.parse_args();original=args.state.with_name('ram.bin').read_bytes()
    def read(address,fmt='I'):return struct.unpack_from('<'+fmt,original,address&0x1FFFFF)[0]
    section=read(0x800B4A44);seed=read(BASE+0x600,'H')
    nodes=[read(section+12+i*4) for i in range(read(section+4,'H'))]
    jobs=[(f'{i:03}',2,[],c) for i,c in enumerate(list(cases(args.holdout))[:args.limit])]
    name='probe' if args.limit is not None else 'holdout' if args.holdout else 'boundaries'
    if args.limit is None:
        scenarios=[('mixed',[dict(frame=0,buttons=['CROSS','RIGHT']),dict(frame=14,buttons=['SQUARE','CIRCLE']),
            dict(frame=38,buttons=['CROSS','LEFT']),dict(frame=64,buttons=[])])] if args.holdout else [
            ('gas',[dict(frame=0,buttons=['CROSS'])]),('brake',[dict(frame=0,buttons=['SQUARE'])]),
            ('turn',[dict(frame=0,buttons=['CROSS','LEFT'])])]
        jobs += [(label,80,replay,None) for label,replay in scenarios]
    rows,records=[],[]
    for sequence,(label,frames,replay,case) in enumerate(jobs):
        folder=Path('.psx/runs')/f'road-contact-{name}-{label}'
        command=['psx','run','--from-state',str(args.state),'--frames',str(frames),'--input',json.dumps(replay),
            '--trace','--max-events','200000','--watch',f'{BASE:08X}:1952','--out',str(folder),'--summary']
        for region in ['8003DAA8-8003DAAC','800431A0-800438F0','80028830-80028900','80028470-80028588','8003E7EC-8003E8E4','80043AE0-80043AE4','80043F18-80043F1C']:command+=['--trace-range',region]
        changes=[]
        if case:
            changes=[(BASE+0x45D,1,1),(BASE+0x65C,4,read(BASE+0x65C,'i')+case.get('forward',0)),
                     (BASE+0x660,4,read(BASE+0x660,'i')+case.get('lateral',0))]
            if 'near_node' in case:
                # Front contacts straddle an actual mesh overlap at the node
                # seam. Removing this node's cells exercises neighbor fallback.
                point=read(nodes[seed]+0x18,'i')//16+read(BASE+0x0C,'h')+case['near_node']
                changes +=[(BASE+0x65C,4,point),(BASE+0x628,4,0),(BASE+0x62C,4,0),(BASE+0x6A4,4,0),(BASE+0x6A8,4,0),(BASE+0x6B1,1,1)]
            if case.get('empty_grid'):
                grid=read(nodes[seed]+0x9C)
                changes +=[(grid+8+i*2,2,0) for i in range(16)]
            if 'angles' in case:changes +=[(BASE+o,2,v) for o,v in zip([0x644,0x646,0x648],case['angles'])]
            for i in range(4):
                if 'old_heights' in case:changes.append((BASE+0x464+i*0x68,4,case['old_heights'][i]))
                if 'velocity' in case:
                    changes +=[(BASE+0x48C+i*0x68,4,case['velocity']),(BASE+0x490+i*0x68,4,case['velocity']//3)]
            # Intervene at the polygon/vertex producer, rather than poking
            # wheel surfaces/heights that the sampler immediately rewrites.
            if 'surface' in case:
                for node in nodes:
                    grid=read(node+0x9C);polygons=set()
                    for cell in range(16):
                        count=read(grid+8+cell*2,'H');pointer=read(grid+40+cell*4)
                        polygons.update(read(pointer+j*4) for j in range(count))
                    for polygon in sorted(polygons):
                        word=read(polygon+4)
                        word=(word&0x087FFFFF)|(case['surface']<<28)|(case.get('flags',0)<<25)|(case.get('variant',0)<<23)
                        changes.append((polygon+4,4,word))
            if case.get('warp'):
                node=nodes[seed];vertices=read(node+0xA4)
                for i in range(read(node+0xD0)):
                    x,z=read(vertices+i*8,'h'),read(vertices+i*8+2,'h')
                    changes.append((vertices+i*8+4,2,((x^z)&127)-63))
        initial=bytearray(original)
        for address,width,value in changes:
            command+=['--poke',f'{address:08X}={value}:{width}'];pos=address&0x1FFFFF
            initial[pos:pos+width]=(value%(1<<(width*8))).to_bytes(width,'little')
        subprocess.run(command,check=True,capture_output=True,text=True)
        actual=export(folder,frames,initial)
        for row in actual:
            row.update(sequence=sequence,case=len(rows),surface_override=case.get('surface',-1) if case else -1,
                flags_override=case.get('flags',0) if case else 0,variant_override=case.get('variant',0) if case else 0,
                warp_node=seed if case and case.get('warp') else -1,empty_node=seed if case and case.get('empty_grid') else -1)
            rows.append(row)
        records.append(dict(command=command,report=str(folder/'report.json')))
    OUT.mkdir(parents=True,exist_ok=True)
    with (OUT/f'{name}.csv').open('w',newline='') as f:
        w=csv.DictWriter(f,sorted(rows[0]));w.writeheader();w.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows),records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),ram_sha256=hashlib.sha256(original).hexdigest(),
        psx_version=subprocess.check_output(['psx','--version'],text=True).strip(),
        scope='Original decoded-course polygon queries, road contact preparation and late road frames; corrected pose/body angles external'),indent=2)+'\n')
    print(f'{len(rows)} original {name} road-contact calls exported')


if __name__=='__main__':main()
