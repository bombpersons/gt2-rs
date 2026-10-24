#!/usr/bin/env python3
"""Record source gauge inputs and actual GPU draws; all outputs remain private."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state',type=Path,default=Path('.psx/default-start-export/start/state.bin'))
    parser.add_argument('--pack',type=Path,required=True,help='Extracted game_status_files[.gz]')
    parser.add_argument('--fresh',action='store_true')
    args=parser.parse_args()
    pack=args.pack.read_bytes()
    if pack.startswith(b'\x1f\x8b'):pack=gzip.decompress(pack)
    name='fresh' if args.fresh else 'initial'
    # Natural, unmodified driving captures remain independent from the probes.
    cases=[dict(name=f'idle-{n}',frame=n,buttons=[]) for n in ([14,74,126,266] if args.fresh else [2,50,118,158])]
    cases += [dict(name=f'gas-{n}',frame=n,buttons=['CROSS']) for n in ([194,326,446] if args.fresh else [140,200,260,360])]
    if not args.fresh:
        # Only the HUD's load instructions are changed. The original drawing
        # routine then exercises otherwise inaccessible digit/gear/angle edges.
        cases += [dict(name=f'probe-{i}',frame=6,buttons=[],probe=dict(rpm=r,speed_readout=s,gear=g,coupled=c,visible=True))
                  for i,(r,s,g,c) in enumerate([(0,0,0,False),(999,99,1,True),(1000,100,2,False),
                    (2999,999,3,True),(4000,1000,4,False),(6300,9999,5,True),(7000,10000,6,True),
                    (7800,65535,0,True),(-600,100,2,True)])]
    rows=[]
    for case in cases:
        frame=case['frame'];folder=Path('.psx/runs')/f'gauges-{name}-{case["name"]}'
        command=['psx','run','--from-state',str(args.state),'--frames',str(frame+2),'--gpu-frame',str(frame),
                 '--input',json.dumps([dict(frame=0,buttons=case['buttons'])]),'--out',str(folder),'--summary',
                 '--watch-reads','800A9D60-800A9D64','--watch-reads','800A9CCC-800A9CCE','--max-events','200000']
        probe=case.get('probe')
        if probe:
            for addr,reg,value in [(0x8002C1FC,2,probe['rpm']),(0x8002C42C,2,probe['speed_readout']),
                                   (0x8002C27C,3,int(probe['coupled'])),(0x8002C2A8,3,probe['gear']),
                                   (0x8002C304,2,probe['gear']),(0x8002C334,2,probe['gear'])]:
                # ADDIU for signed RPM, ORI for the unsigned speed halfword.
                opcode=13 if addr==0x8002C42C else 9
                instruction=(opcode<<26)|(reg<<16)|(value&65535)
                command+=['--poke',f'{addr:08x}=0x{instruction:08x}']
        subprocess.run(command,check=True,capture_output=True,text=True)
        report=json.loads((folder/'report.json').read_text())
        assert report['status']=='complete' and report['read_watch']['complete']
        assert report['gpu_frame']['status']=='captured'
        source=json.loads((folder/'gpu_frame.json').read_text());assert source['summary']['complete']
        ram=(folder/'ram.bin').read_bytes();vram=(folder/'gpu_frame_vram.bin').read_bytes()
        def word(off):return struct.unpack_from('<I',ram,off)[0]
        def half(off):return struct.unpack_from('<H',ram,off)[0]
        maximum=half(0xA9688+0x134);thousands=(maximum+999)//1000
        limits=struct.unpack_from('<11b',ram,0x2F868)
        dial=next(i for i,n in enumerate(limits) if n<0 or n>=thousands)+1
        offsets=struct.unpack_from('<11I',pack,4)
        expected=bytearray(pack[offsets[0]+20:offsets[0]+20+29952])
        at=offsets[dial]+8;at+=struct.unpack_from('<I',pack,at)[0]
        for y in range(80):expected[y*128:y*128+40]=pack[at+12+y*40:at+12+(y+1)*40]
        actual=b''.join(vram[(y*1024+512)*2:(y*1024+576)*2] for y in range(234))
        assert actual==expected,'source pack must reproduce the entire gauge upload'
        mapping=struct.unpack_from('<7b',ram,0x2F874)
        descriptors=[0x2F630]+[0x2F678+i*12 for i in range(10)]+[0x2F6FC]+[0x2F708+i*12 for i in mapping]
        lookup={}
        for slot,off in enumerate(descriptors):
            value,w,h,page=struct.unpack_from('<I3H',ram,off)
            key=(value&255,(value>>8)&255,w,h,((value>>16)&63)*16,value>>22)
            assert key not in lookup;lookup[key]=slot
        view=probe or dict(visible=True)
        if not probe:
            pcs={0x8002C1FC:'rpm',0x8002C42C:'speed_readout',0x8002C27C:'coupled',0x8002C2A8:'gear'}
            reads=[json.loads(line) for line in (folder/'reads.jsonl').open()]
            built=max(e['run_frame'] for e in reads if e['run_frame']<frame and e['pc'] in pcs)
            for e in reads:
                if e['run_frame']==built and e['pc'] in pcs:
                    key=pcs[e['pc']];value=e['value']
                    if key=='rpm':value=struct.unpack('<h',struct.pack('<H',value))[0]
                    if key=='coupled':value=value==1
                    assert key not in view;view[key]=value
            assert len(view)==5,view
        draws=[];opaque_pixels=set()
        primitives=[c for c in source['commands'] if c['kind'] in ('rect','poly4')]
        backing=next(i for i,c in enumerate(primitives) if c.get('textured') and c.get('clut')==[512,228]
                     and c['vertices'][0].get('uv')==[0,0])
        sections=thousands*4
        end=backing;needles=0
        while needles<2:
            end+=1
            if primitives[end].get('gouraud'):needles+=1
        for c in primitives[backing-sections:end+1]:
            vertices=c['vertices'];ox,oy=c['offset']
            assert all(v['x']-ox>=230 and 130<=v['y']-oy<=230 for v in vertices)
            blend='Opaque' if not c.get('semi') else ['Half','Add','Subtract','AddQuarter'][c.get('texpage',{}).get('blend',0)]
            slot=None
            if c.get('textured'):
                assert c['texpage']['x']==512 and c['texpage']['depth']==4
                u,v=vertices[0]['uv'];_,_,w,h=c['area'];cx,cy=c['clut']
                slot=lookup[(u,v,w,h,cx,cy)]
                if blend=='Opaque':
                    for dy in range(h):
                        for dx in range(w):
                            q=u+dx;t=struct.unpack_from('<H',vram,((v+dy)*1024+512+q//4)*2)[0]
                            index=(t>>((q%4)*4))&15;color=struct.unpack_from('<H',vram,(cy*1024+cx+index)*2)[0]
                            if color:opaque_pixels.add((vertices[0]['x']-ox+dx,vertices[0]['y']-oy+dy))
            if c['kind']=='rect':
                x,y,w,h=c['area'];x-=ox;y-=oy
                points=[[x,y],[x+w,y],[x,y+h],[x+w,y+h]];rgb=[vertices[0]['color']]*4
            else:
                points=[[v['x']-ox,v['y']-oy] for v in vertices];rgb=[v['color'] for v in vertices]
            draws.append(dict(sprite=slot,vertices=points,rgb=rgb,blend=blend))
        assert len(draws)>=35,len(draws)
        rows.append(dict(name=case['name'],frame=frame,buttons=case['buttons'],view=view,draws=draws,
                         opaque_pixels=sorted(opaque_pixels),screen=str(folder/'gpu_frame_screen.png'),command=command))
    destination=Path('game/tests/data/original/gauges');destination.mkdir(parents=True,exist_ok=True)
    result=dict(rows=rows,state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),pack_sha256=hashlib.sha256(pack).hexdigest(),
                psx_version=subprocess.check_output(['psx','--version'],text=True).strip())
    (destination/f'{name}.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'{len(rows)} source gauge frames; actual GPU primitives and source-load inputs')


if __name__=='__main__':main()
