#!/usr/bin/env python3
"""Capture the original result overlay, keeping all expected draws private.

Controlled probes disable only the result timer update; the original drawing
routines, text layout, qualification, textures and GPU run unchanged. Three
consecutive fields retain one complete back-buffer packet, including text
that the GPU finishes in the next field. Expectations come from GPU commands.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
from export_assets import licence_definitions


def capture(name, passed, age, time_ms, grade=0, attempts=0, natural=False):
    state = Path('.psx/awards/gold-session/state.bin' if passed else '.psx/new-core/failure-checkpoint/state.bin')
    initial = state.with_name('ram.bin').read_bytes()
    selected=0x1D585C
    key=initial[selected+16:initial.index(0,selected+16)].decode('ascii')
    definition=licence_definitions(initial)[key]
    saved=0x1C98E0+0x1418+initial[selected+11]*1640+initial[selected+12]*164
    glyphs=[];logo=None;shade=None;pixels=set();screens=[];commands=[]
    for frame in ([234,235,236] if natural else [4,5,6]):
        folder=Path('.psx/results/presentation')/f'{name}-{frame}'
        command=['psx','run','--from-state',str(state),'--frames','240' if natural else '10',
                 '--poke','8002A700=0x03E00008','--poke','8002A704=0x24020001',
                 '--poke',f'800AF226={age}:2','--poke',f'801D5DF0={time_ms}',
                 '--poke',f'{0x80000000+saved+1:08X}={grade}:1',
                 '--poke',f'{0x80000000+saved+2:08X}={attempts}:1',
                 '--gpu-frame',str(frame),'--out',str(folder),'--summary']
        if natural:
            command=command[:command.index('--poke')]+command[command.index('--gpu-frame'):]
            grade,attempts=initial[saved+1:saved+3]
        subprocess.run(command,check=True,capture_output=True,text=True)
        commands.append(command)
        report=json.loads((folder/'report.json').read_text())
        assert report['status']=='complete' and report['gpu_frame']['status']=='captured'
        data=json.loads((folder/'gpu_frame.json').read_text());assert data['summary']['complete']
        ram=(folder/'ram.bin').read_bytes()
        assert struct.unpack_from('<h',ram,0xAF226)[0]==age
        vram=(folder/'gpu_frame_vram.bin').read_bytes()
        lookup={}
        for time_font,table,chars in [(False,0x9B9B4,'FinishFAILKDS PRZECQU!BONGVT'),(True,0x9825C,'0123456789:.')]:
            for c in set(chars):
                value=struct.unpack_from('<I',ram,table+ord(c)*8)[0]
                index=(value>>6)&255
                if not index:continue
                t=struct.unpack_from('<I',ram,table+0x818+index*4)[0]
                key=((t&255)+(t>>29)*256,(t>>8)&255,2*((t>>16)&31),(t>>21)&63)
                assert key not in lookup,(c,lookup.get(key));lookup[key]=(time_font,c)
        for c in data['commands']:
            if c.get('offset') != ([0,0] if natural else [0,240]):continue
            if c['kind']=='rect' and c.get('textured') and c.get('clut')==[416,0]:
                x,y,w,h=c['area'];ox,oy=c['offset'];x-=ox;y-=oy
                if y<100:continue
                u,v=c['vertices'][0]['uv'];u+=(c['texpage']['x']-384)*4
                key=(u,v,w,h)
                assert key in lookup,(key,c)
                time_font,character=lookup[key]
                additive=bool(c.get('semi'));assert not additive or c['texpage']['blend']==1
                glyphs.append(dict(character=character,time_font=time_font,area=[x,y,w,h],rgb=c['vertices'][0]['color'],additive=additive))
                if not additive:
                    page=c['texpage'];source_u,source_v=c['vertices'][0]['uv']
                    for dy in range(h):
                        for dx in range(w):
                            q=source_u+dx;t=struct.unpack_from('<H',vram,((page['y']+source_v+dy)*1024+page['x']+q//4)*2)[0]
                            index=(t>>((q%4)*4))&15
                            color=struct.unpack_from('<H',vram,(416+index)*2)[0]
                            if color:pixels.add((x+dx,y+dy))
            elif c['kind']=='poly4' and c.get('clut') in ([384,425],[496,424],[480,424],[464,424]):
                assert logo is None
                index=[[384,425],[496,424],[480,424],[464,424]].index(c['clut'])
                ox,oy=c['offset'];verts=c['vertices']
                logo=dict(logo=index,vertices=[[v['x']-ox,v['y']-oy]for v in verts],rgb=verts[0]['color'])
            elif c['kind']=='rect' and c.get('semi') and c['area'][2:]==[320,240]:
                assert shade is None and c['texpage']['blend']==2
                rgb=c['vertices'][0]['color'];assert len(set(rgb))==1
                shade=rgb[0]
        screens.append(str(folder/'gpu_frame_screen.png'))
    assert glyphs or age<0
    return dict(name=name,passed=passed,age=age,time_ms=time_ms,grade=grade,attempts=attempts,definition=definition,
                layout=dict(glyphs=glyphs,logo=logo,shade=shade),pixels=sorted(pixels),screens=screens,
                commands=commands,state_sha256=hashlib.sha256(state.read_bytes()).hexdigest())


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fresh',action='store_true')
    args=parser.parse_args()
    cases=[]
    if args.fresh:
        for age,time in [(81,38649),(95,38900),(103,38950),(122,38910),(127,38850),(131,38649),(147,38910)]:
            cases.append(capture(f'fresh-{age}-{time}',True,age,time))
        cases.append(capture('native-gold',True,146,38641))
        cases.append(capture('natural-wait',True,146,38675,natural=True))
        cases.append(capture('fresh-fail',False,30,0))
        cases.append(capture('fresh-qualified',True,135,39999,attempts=9))
        cases.append(capture('fresh-already-passed',True,139,39999,grade=2,attempts=9))
    else:
        for age in [0,79,80,86,91,92,98,104,120,121,126,132,146]:
            cases.append(capture(f'age-{age}',True,age,38675))
        for time in [38649,38650,38900,38910,40999,41000]:
            cases.append(capture(f'time-{time}',True,146,time))
        for attempts in [8,9,10,255]:
            cases.append(capture(f'assisted-{attempts}',True,146,39999,attempts=attempts))
        cases.append(capture('failure',False,29,0))
        cases.append(capture('inactive',True,-1,38675))
    target=Path('game/tests/data/original/result_presentation');target.mkdir(parents=True,exist_ok=True)
    (target/('fresh.json'if args.fresh else 'initial.json')).write_text(json.dumps(dict(rows=cases,
        psx_version=subprocess.check_output(['psx','--version'],text=True).strip()),indent=2)+'\n')
    print(f'{len(cases)} original overlay cases, {sum(len(c["layout"]["glyphs"]) for c in cases)} glyph draws')


if __name__=='__main__':main()
