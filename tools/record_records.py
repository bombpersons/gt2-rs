#!/usr/bin/env python3
"""Capture actual B-1 records-panel GPU primitives. All outputs stay private."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
from export_assets import licence_definitions


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state',type=Path,default=Path('.psx/default-start-export/start/state.bin'))
    parser.add_argument('--fresh',action='store_true')
    parser.add_argument('--catalogue',action='store_true',help='Probe every original licence target row through the source HUD')
    args=parser.parse_args()
    assert not (args.fresh and args.catalogue)
    name='catalogue' if args.catalogue else 'fresh' if args.fresh else 'initial'
    natural=[14,126] if args.fresh else [2,50,158,800]
    probes=[(61000,7),(3599999,42),(3600000,1005),(35999999,10000),(36000000,30303),
            (359999999,65535),(360000000,110),(0xFFFFFFFE,9)] if args.fresh else [
            (0,0),(999,1),(1000,99),(59999,100),(60000,101),(38650,109),(599999,65535)]
    cases=[(f'natural-{n}',n,None) for n in natural]+[(f'probe-{i}',6,dict(time_ms=t,speed_readout=s)) for i,(t,s) in enumerate(probes)]
    initial=args.state.with_name('ram.bin').read_bytes()
    current=0x1D585C
    definitions=licence_definitions(initial)
    selected=initial[current+16:initial.index(0,current+16)].decode('ascii')
    if args.catalogue:cases=[(key,6,None) for key in definitions]
    best=0x1C98E0+0x1418+initial[current+11]*1640+initial[current+12]*164+4
    rows=[]
    for tag,frame,record in cases:
        folder=Path('.psx/runs')/f'records-{name}-{tag}'
        command=['psx','run','--from-state',str(args.state),'--frames',str(frame+2),'--gpu-frame',str(frame),
                 '--out',str(folder),'--summary']
        if record:
            command+=['--poke',f'{0x80000000+best:08X}={record["time_ms"]}',
                      '--poke',f'{0x80000000+best+16:08X}={record["speed_readout"]}:2']
        if args.catalogue:
            encoded=(tag+'\0').encode('ascii').ljust(8,b'\0');assert len(encoded)==8
            command+=['--poke',f'801D586C={int.from_bytes(encoded[:4],"little")}',
                      '--poke',f'801D5870={int.from_bytes(encoded[4:],"little")}']
        subprocess.run(command,check=True,capture_output=True,text=True)
        report=json.loads((folder/'report.json').read_text());assert report['status']=='complete' and report['gpu_frame']['status']=='captured'
        source=json.loads((folder/'gpu_frame.json').read_text());assert source['summary']['complete']
        ram=(folder/'ram.bin').read_bytes();vram=(folder/'gpu_frame_vram.bin').read_bytes()
        lookup={}
        for char in set('Record0123456789-:.mph'):
            a=struct.unpack_from('<I',ram,0x95AC8+ord(char)*8)[0]
            t=struct.unpack_from('<I',ram,0x95AC8+0x818+((a>>6)&255)*4)[0]
            key=((t&255)+(t>>29)*256,(t>>8)&255,2*((t>>16)&31),(t>>21)&63)
            assert key not in lookup;lookup[key]=char
        glyphs=[];quads=[];pixels=set()
        for c in source['commands']:
            if c['kind'] not in ('rect','poly4'):continue
            ox,oy=c['offset'];vertices=c['vertices'];x,y=vertices[0]['x']-ox,vertices[0]['y']-oy
            if not (220<=x<320 and 20<=y<100):continue
            if c.get('textured'):
                if c['kind']!='rect' or c.get('clut')!=[416,0]:continue
                _,_,w,h=c['area'];u,v=vertices[0]['uv'];u+=(c['texpage']['x']-384)*4
                key=(u,v,w,h)
                if key not in lookup:continue
                assert not c.get('semi')
                glyphs.append(dict(character=lookup[key],large=False,area=[x,y,w,h],rgb=vertices[0]['color'],additive=False))
                page=c['texpage'];u,v=vertices[0]['uv']
                for dy in range(h):
                    for dx in range(w):
                        q=u+dx;tex=struct.unpack_from('<H',vram,((page['y']+v+dy)*1024+page['x']+q//4)*2)[0]
                        idx=(tex>>((q%4)*4))&15;col=struct.unpack_from('<H',vram,(416+idx)*2)[0]
                        if col:pixels.add((x+dx,y+dy))
            elif c['kind']=='poly4' and c.get('gouraud') and vertices[0]['color']==[130,0,0] and vertices[1]['color']==[80,0,0]:
                w,h=vertices[3]['x']-vertices[0]['x'],vertices[3]['y']-vertices[0]['y']
                quads.append(dict(area=[x,y,w,h],rgb=[vertices[0]['color'],vertices[1]['color']]))
                pixels.update((x+dx,y+dy) for dy in range(h) for dx in range(w))
            elif c['kind']=='rect' and not c.get('semi') and c['area'][2:] in ([4,4],[6,6]) and 250<=x<260:
                _,_,w,h=c['area'];rgb=vertices[0]['color']
                quads.append(dict(area=[x,y,w,h],rgb=[rgb,rgb]))
                pixels.update((x+dx,y+dy) for dy in range(h) for dx in range(w))
        assert len(glyphs)>=38 and len(quads)==7,(tag,len(glyphs),len(quads))
        actual_time=struct.unpack_from('<I',ram,best)[0]
        actual_record=None if actual_time==0xFFFFFFFF else dict(time_ms=actual_time,speed_readout=struct.unpack_from('<H',ram,best+16)[0])
        assert record==actual_record
        rows.append(dict(name=tag,frame=frame,record=record,definition=definitions[tag if args.catalogue else selected],
                         layout=dict(glyphs=glyphs,quads=quads),pixels=sorted(pixels),
                         command=command,screen=str(folder/'gpu_frame_screen.png'),report=str(folder/'report.json')))
    target=Path('game/tests/data/original/records');target.mkdir(parents=True,exist_ok=True)
    (target/f'{name}.json').write_text(json.dumps(dict(rows=rows,state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
        psx_version=subprocess.check_output(['psx','--version'],text=True).strip()),indent=2)+'\n')
    print(f'{len(rows)} original records-panel frames; glyphs, quads and pixel masks come from source draws')


if __name__=='__main__':main()
