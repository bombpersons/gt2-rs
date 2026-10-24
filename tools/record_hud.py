#!/usr/bin/env python3
"""Record the original B-1 HUD's actual glyph draws; all output stays private."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state',type=Path,default=Path('.psx/default-start-export/start/state.bin'))
    parser.add_argument('--font',type=Path,required=True,help='Extracted /font/racefont.dat from the asset export')
    parser.add_argument('--fresh',action='store_true')
    args=parser.parse_args()
    frames=[14,74,126,254,266,274,800,1346] if args.fresh else [2,50,100,118,158,238,240,252,268,270,280]
    name='fresh' if args.fresh else 'initial'
    target=Path('game/tests/data/original/hud');target.mkdir(parents=True,exist_ok=True)
    rows=[]
    for frame in frames:
        run=Path('.psx/runs')/f'hud-{name}-{frame}'
        command=['psx','run','--from-state',str(args.state),'--frames',str(frame+2),'--gpu-frame',str(frame),
                 '--out',str(run),'--summary']
        subprocess.run(command,check=True,capture_output=True,text=True)
        report=json.loads((run/'report.json').read_text());assert report['status']=='complete'
        assert report['gpu_frame']['status']=='captured'
        ram=(run/'ram.bin').read_bytes()
        def word(off):return struct.unpack_from('<I',ram,off)[0]
        lookup={}
        for large,table,chars in [(False,0x95AC8,'LapTime0123456789:.'),(True,0x9B9B4,'123START')]:
            for c in set(chars):
                a=word(table+ord(c)*8);index=(a>>6)&255;assert index
                t=word(table+0x818+index*4)
                key=(large,(t&255)+(t>>29)*256,(t>>8)&255,2*((t>>16)&31),(t>>21)&63)
                assert key not in lookup;lookup[key]=c
        draws=[];opaque_pixels=set()
        vram=(run/'gpu_frame_vram.bin').read_bytes()
        captured=json.loads((run/'gpu_frame.json').read_text());assert captured['summary']['complete']
        for c in captured['commands']:
            if c['kind']!='rect' or not c.get('textured') or c.get('clut')!=[416,0]:continue
            x,y,w,h=c['area'];x-=c['offset'][0];y-=c['offset'][1]
            large=80<=x<=240 and 60<=y<=110
            small=x<90 and 20<=y<50
            if not (large or small):continue
            u,v=c['vertices'][0]['uv'];u+=(c['texpage']['x']-384)*4
            key=(large,u,v,w,h);assert key in lookup,(c,key)
            assert not c.get('semi') or c['texpage']['blend']==1,'announcement fade must use additive blending'
            draws.append(dict(character=lookup[key],large=large,area=[x,y,w,h],rgb=c['vertices'][0]['color'],
                              additive=bool(c.get('semi')) and c['texpage']['blend']==1))
            if not c.get('semi'):
                # Derive the comparison mask from the actual source GPU
                # texture/palette, independently of the native font export.
                source_u,source_v=c['vertices'][0]['uv'];page=c['texpage']
                assert page['depth']==4
                for dy in range(h):
                    for dx in range(w):
                        q=source_u+dx
                        tex=struct.unpack_from('<H',vram,((page['y']+source_v+dy)*1024+page['x']+q//4)*2)[0]
                        index=(tex>>((q%4)*4))&15
                        color=struct.unpack_from('<H',vram,(416+index)*2)[0]
                        if color:opaque_pixels.add((x+dx,y+dy))
        assert len(draws)>=15,'label and clock required'
        # This is the source atlas upload, not a synthesized expected bitmap.
        atlas=b''.join(vram[(y*1024+384)*2:(y*1024+512)*2] for y in range(256))
        assert atlas==args.font.read_bytes()
        rows.append(dict(frame=frame,draws=draws,opaque_pixels=sorted(opaque_pixels),command=command,report=str(run/'report.json'),screen=str(run/'gpu_frame_screen.png')))
    result=dict(rows=rows,state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),font_sha256=hashlib.sha256(args.font.read_bytes()).hexdigest(),
                psx_version=subprocess.check_output(['psx','--version'],text=True).strip())
    (target/f'{name}.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'{len(rows)} original HUD frames; expected glyphs are actual GPU primitives')


if __name__=='__main__':main()
