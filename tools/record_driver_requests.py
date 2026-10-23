#!/usr/bin/env python3
"""Observe original ordinary engagement-request gates with controlled inputs.

Only the original CPU store supplies expected requests. Pedal ramp rates are
temporarily zeroed so chosen fractions reach the producer unchanged; reads
inside that producer verify consumed inputs. All outputs remain private.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

from record_vehicle_loop import BASE, SIZE, PREFIX

FIELDS = {'reset':(0x718,'B'), 'counter':(0x78E,'H'), 'mode':(0x786,'B'),
          'rear_reference':(0x373,'B'), 'handbrake':(0x614,'h'), 'brake':(0x612,'h'),
          'engagement':(0x619,'B'), 'rpm':(0x6AC,'h'), 'idle':(0x10A,'H')}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-stationary-checkpoint/state.bin'))
    args=p.parse_args()
    initial=args.state.with_name('ram.bin').read_bytes()
    assert len(initial)==2*1024*1024 and initial[(BASE&0x1FFFFF)+0x372]==5
    idle=struct.unpack_from('<H',initial,(BASE&0x1FFFFF)+0x10A)[0]
    cases=[dict(name='disconnected-release',engagement=0),
           dict(name='disconnected-almost-braked',engagement=0,brake=4095),
           dict(name='disconnected-full-brake',engagement=0,brake=4096),
           dict(name='below-idle',rpm=idle-1),dict(name='at-idle',rpm=idle),
           *[dict(name=f'engagement-{i}',engagement=i,rpm=idle+1) for i in [2,3,4]],
           dict(name='rear-full-handbrake',rear_reference=1,handbrake=4096),
           dict(name='rear-almost-handbraked',rear_reference=1,handbrake=4095),
           dict(name='front-full-handbrake',rear_reference=0,handbrake=4096),
           dict(name='countdown-active',counter=2),dict(name='countdown-expires',counter=1),
           dict(name='reset',reset=1)]
    results=[]
    for case in cases:
        values=dict(engagement=1,rpm=idle,brake=0,handbrake=0,rear_reference=0,counter=0,reset=0)
        values.update({k:v for k,v in case.items() if k!='name'})
        pokes=[]
        for key in ['engagement','rpm','brake','handbrake','rear_reference','counter']:
            offset,fmt=FIELDS[key]
            pokes.append((BASE+offset,struct.calcsize(fmt),values[key]))
        # 8003E120 produces the reset byte from this real parent gate.
        pokes.append((0x800A9520,2,values['reset']))
        if values['reset']:
            # The licence loop overwrites the global gate. Override only the
            # parent's reset-input assignment (li s3,1); the observed
            # 8002F92C consumer is unmodified and must read reset=1.
            pokes.append((0x8003E10C,4,0x24130001))
        pokes.extend((a,2,0) for a in [0x80046DB0,0x80046DB4,0x80046DB8,0x80046DBC,0x80046DC0,0x80046DC4])
        ram=bytearray(initial)
        command=['psx','run','--from-state',str(args.state),'--frames','2','--input','[]',
                 '--watch',f'{BASE:08X}:{SIZE}','--watch-reads',f'{BASE:08X}:{SIZE}',
                 '--trace','--trace-range','8002F92C-8002F9CC','--max-events','200000']
        for address,size,value in pokes:
            command+=['--poke',f'{address:08X}={value}:{size}']
            off=address&0x1FFFFF
            ram[off:off+size]=(value&((1<<(size*8))-1)).to_bytes(size,'little')
        run=Path('.psx/runs')/f'driver-request-{case["name"]}'
        command+=['--out',str(run),'--summary']
        subprocess.run(command,check=True,capture_output=True,text=True)
        report=json.loads((run/'report.json').read_text())
        assert report['status']=='complete' and report['frames']['reached']==2
        assert report['watch']['complete'] and report['read_watch']['complete'] and report['trace']['lines_dropped']==0
        writes=[json.loads(l) for l in (run/'writes.jsonl').open()]
        entries=[m for l in (run/'trace.txt').open() if (m:=PREFIX.match(l)) and int(m[3],16)==0x8002F92C]
        assert len(entries)==1,'one original engagement producer required'
        cycle=int(entries[0][2])
        for e in sorted(writes,key=lambda e:e['cycle']):
            if e['cycle']>cycle:break
            off=e['address']&0x1FFFFF
            ram[off:off+e['width']]=e['value'].to_bytes(e['width'],'little')
        before={k:struct.unpack_from('<'+fmt,ram,(BASE&0x1FFFFF)+off)[0] for k,(off,fmt) in FIELDS.items()}
        for key in values:
            expected=max(values[key]-1,0) if key=='counter' else values[key]
            assert before[key]==expected,(case['name'],key,before[key],expected)
        stores=[e for e in writes if 0x8002F92C<=e['pc']<0x8002F9CC and e['address']==BASE+0x60A]
        assert len(stores) in (1,2),'expected request must come from original producer stores'
        stores.sort(key=lambda e:e['cycle'])
        consumed=set()
        for line in (run/'reads.jsonl').open():
            e=json.loads(line)
            if 0x8002F92C<=e['pc']<0x8002F9CC:
                key=next((k for k,(off,_) in FIELDS.items() if BASE+off==e['address']),None)
                assert key is not None,(hex(e['pc']),hex(e['address']))
                assert e['value']==(before[key]&((1<<(8*e['width']))-1)),key
                consumed.add(key)
        assert consumed
        results.append(dict(name=case['name'],before=before,requested=stores[-1]['value'],
            starting_counter=values['counter'],consumed=sorted(consumed),command=command,report=str(run/'report.json')))
    target=Path('game/tests/data/original/driver_requests/probes.json')
    target.parent.mkdir(parents=True,exist_ok=True)
    target.write_text(json.dumps(dict(cases=results,state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
        ram_sha256=hashlib.sha256(initial).hexdigest(),psx_version=subprocess.check_output(['psx','--version'],text=True).strip()),indent=2)+'\n')
    print(f'{len(results)} original producer probes; expected requests are original CPU stores')


if __name__=='__main__':main()
