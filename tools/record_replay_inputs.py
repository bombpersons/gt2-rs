#!/usr/bin/env python3
"""Observe the actual original replay decoder, including its terminal call.

The natural case re-enters playback with the original's own recording. Other
cases change only recording bytes/state; all expected outputs come from MIPS
execution. Original packet data and results remain private and ignored.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
from export_assets import ron
from import_replay import decode

PREFIX=re.compile(r'^(\d+) (\d+) ([0-9a-f]{8}) ')
STREAM=0x1D5F84
OUTPUT=0x1FF6B0


def record(name, ram, ticks, packet=None):
    state=Path('.psx/results/replay-start/state.bin')
    initial=bytearray(state.with_name('ram.bin').read_bytes())
    if packet is None:
        length=struct.unpack_from('<H',ram,STREAM+16)[0]
        clip=ram[STREAM:STREAM+25+length]
    else:
        clip=bytearray(25)+packet
        struct.pack_into('<I',clip,0,ticks)
        struct.pack_into('<4H',clip,12,1,0,len(packet),17408)
    original=bytearray(initial);original[STREAM:STREAM+len(clip)]=clip
    imported=decode(original,STREAM)
    playback=bytearray(clip)
    struct.pack_into('<IiHH',playback,4,struct.unpack_from('<I',clip)[0],-1,0,0)
    playback[20:25]=bytes(5)
    folder=Path('.psx/replay/inputs')/name
    # Natural data and controlled packet cases all execute the real decoder;
    # the original session's live driver remains in replay mode.
    command=['psx','run','--from-state',str(state),'--frames',str((ticks+2 if name=='natural-recording' else min(ticks+2,18))*2),
             '--trace','--trace-range','80016428-80016430','--trace-range','80016590-80016594','--trace-range','80016454-8001645C','--trace-range','80013EE8-80013EF0',
             '--watch',f'{0x80000000+STREAM:08X}:25','--watch',f'{0x80000000+OUTPUT:08X}:5','--watch','801FF750:12',
             '--out',str(folder),'--summary']
    for at in range(0,len(playback),4):
        chunk=playback[at:at+4]
        for j,value in enumerate(chunk):command+=['--poke',f'{0x80000000+STREAM+at+j:08X}={value}:1']
    subprocess.run(command,check=True,capture_output=True,text=True)
    report=json.loads((folder/'report.json').read_text())
    assert report['status']=='complete' and report['watch']['complete'] and report['trace']['lines_dropped']==0
    # Apply pokes before the first recorded instruction, then replay all stores
    # into a shadow of observed memory. Native formulas never make expected rows.
    initial[STREAM:STREAM+len(playback)]=playback
    writes=iter(json.loads(s)for s in (folder/'writes.jsonl').read_text().splitlines());pending=next(writes,None)
    rows=[];entered=False
    for line in (folder/'trace.txt').open():
        match=PREFIX.match(line)
        if not match:continue
        frame,cycle,pc=int(match[1]),int(match[2]),int(match[3],16)
        while pending and pending['cycle']<=cycle:
            at=pending['address']&0x1FFFFF
            initial[at:at+pending['width']]=pending['value'].to_bytes(pending['width'],'little');pending=next(writes,None)
        if pc==0x80016428:assert not entered;entered=True
        if pc==0x80013EE8:
            assert entered;entered=False
            flags,steering,brake,throttle,shift,reverse,handbrake,_=struct.unpack_from('<HhhhbbBB',initial,0x1FF750)
            assert flags==0,'digital corpus required'
            # These are the ORIGINAL normalizer's effective driver outputs,
            # not a second implementation of the importer button mapping.
            rows.append(dict(controls=dict(throttle=bool(throttle),brake=bool(brake),handbrake=bool(handbrake),
                steering=int(steering>0)-int(steering<0),reverse=bool(reverse),shift=shift),
                finished=bool(struct.unpack_from('<H',initial,STREAM+12)[0]),
                remaining=struct.unpack_from('<I',initial,STREAM+4)[0],
                repeat=struct.unpack_from('<i',initial,STREAM+8)[0]))
    assert rows and not entered
    case=dict(name=name,clip=imported,rows=rows,command=command,report=str(folder/'report.json'),
        build=report['build'],state_sha256=hashlib.sha256(state.read_bytes()).hexdigest())
    if name=='natural-recording':
        x,z,y=struct.unpack_from('<3i',ram,0xA96B4+0x65C)
        case['source_outcome']=dict(time_ms=struct.unpack_from('<I',ram,0x1D5DF0)[0],position_m=[x/4096,y/4096,-z/4096],native_ticks=json.loads(Path('.psx/awards/gold-session/report.json').read_text())['frames']['requested']//2,
            ram_sha256=hashlib.sha256(ram).hexdigest(),report='.psx/awards/gold-session/report.json')
    return case


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--fresh',action='store_true');args=parser.parse_args()
    ram=Path('.psx/awards/gold-session/ram.bin').read_bytes()
    cases=[]
    if args.fresh:
        for i,packet in enumerate([bytes([0x79,0,0x25,0,0,0x11,0xC8]),bytes([0x78,0,0x52,0,0,0x13,0x89,0x96,0x00,0x00])]):
            # Raw authored packets exercise retained channels and simultaneous
            # logical requests; the executing decoder supplies every output.
            # Duration is taken from independently executed packet hold values
            # below; these fixtures are not synthesized expected controls.
            total=[4,12][i]
            cases.append(record(f'fresh-{i}',ram,total,packet))
    else:
        cases.append(record('natural-recording',ram,1247))
        for i,(repeat,extension) in enumerate([(0,b''),(7,b''),(8,b'\x01'),(1023,b'\x7f'),(1024,b'\x80\x80'),(131071,b'\xbf\xff'),(131072,b'\xc0\x40\x00'),(16777216,b'\xe0\x20\x00\x00')]):
            command=0x78|(repeat&7)|(0x80 if extension else 0)
            packet=bytes([command,0,0xE7,0,0])+extension
            cases.append(record(f'hold-{i}',ram,repeat+1,packet))
        cases.append(record('all-buttons',ram,4,bytes([0x78,0,0xFF,0,0,0x10,0,0x11,0])))
    target=Path('game/tests/data/original/replay_inputs');target.mkdir(parents=True,exist_ok=True)
    for c in cases:(target/(c['name']+'.ron')).write_text(ron(c['clip'])+'\n')
    (target/('fresh.json'if args.fresh else 'initial.json')).write_text(json.dumps(cases,indent=2)+'\n')
    print(f'{len(cases)} source decoder cases, {sum(len(c["rows"])for c in cases)} calls')


if __name__=='__main__':main()
