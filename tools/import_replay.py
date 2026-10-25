#!/usr/bin/env python3
"""Convert an original finalized driver-input recording to native RON runs.

Only the established digital control boundary is imported. Analog/special
requests are rejected, never silently replaced. Runtime code reads only RON.
"""
import argparse
from pathlib import Path
import struct
from export_assets import ROOT, ron


def decode(ram, offset=None):
    if len(ram)!=2*1024*1024:raise ValueError('Expected complete original RAM')
    if offset is None:offset=struct.unpack_from('<I',ram,0xA9688+0x1C)[0]&0x1FFFFF
    if offset<0 or offset+25>len(ram):raise ValueError('Replay header is outside RAM')
    ticks=struct.unpack_from('<I',ram,offset)[0]
    finished=struct.unpack_from('<H',ram,offset+12)[0]
    length,capacity=struct.unpack_from('<HH',ram,offset+16)
    if finished!=1 or length>capacity or offset+25+length>len(ram):raise ValueError('Expected a finalized, bounded recording')
    packet=ram[offset+25:offset+25+length];at=0;state=[0]*4;runs=[];total=0
    def byte():
        nonlocal at
        if at==len(packet):raise ValueError('Truncated replay packet')
        value=packet[at];at+=1;return value
    while at<len(packet):
        command=byte()
        for channel in range(4):
            if command&(8<<channel):state[channel]=byte()
        repeat=0
        if command&128:
            repeat=byte()
            for flag,mask in [(128,127),(16384,16383),(2097152,2097151)]:
                if not repeat&flag:break
                repeat=((repeat&mask)<<8)|byte()
        repeat=(repeat<<3)|(command&7)
        selectors,buttons,steering,analogs=state
        if selectors:raise ValueError(f'Unported analog/special replay selectors {selectors:#x}')
        controls=dict(throttle=bool(buttons&4),brake=bool(buttons&8),handbrake=bool(buttons&16),
                      steering=int(bool(buttons&1))-int(bool(buttons&2)),reverse=bool(buttons&32),
                      shift=int(bool(buttons&64))-int(bool(buttons&128)))
        count=repeat+1;total+=count
        if total>ticks:raise ValueError('Replay packets exceed their recorded duration')
        # Keep original packet boundaries; native recording may coalesce them.
        runs.append(dict(ticks=count,controls=controls))
    if total!=ticks:raise ValueError(f'Replay duration {total} differs from header {ticks}')
    return dict(runs=runs)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ram',type=Path,required=True)
    parser.add_argument('--offset',type=lambda n:int(n,0))
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args();out=args.out.resolve()
    if out.is_relative_to(ROOT) and not any(out.is_relative_to(ROOT/p) for p in ['game/assets','game/tests/data/original','.psx']):
        parser.error('Project-local output must remain in ignored asset/reference/analysis directories')
    clip=decode(args.ram.read_bytes(),args.offset);out.parent.mkdir(parents=True,exist_ok=True)
    out.write_text('// Generated locally from original recorded controls; do not commit.\n'+ron(clip)+'\n')
    print(f'{len(clip["runs"])} native input runs, {sum(r["ticks"]for r in clip["runs"])} recorded ticks')


if __name__=='__main__':main()
