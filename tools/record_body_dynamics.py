#!/usr/bin/env python3
"""Record original lateral/yaw accumulation and world-velocity feedback.

Only original execution supplies expected outputs. All tuning, traces,
calibration/recording metadata and numerical fixtures remain private.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

from record_forward_dynamics import RANGES, export as forward_rows
from record_tyre_forces import BASE, EFFECT
from record_wheel_rotation import PREFIX

OUT = Path('game/tests/data/original/body_dynamics')
PHASES = {0x8003AEDC: 'before', 0x8003B0A4: 'lateral', 0x8003B0E4: 'world', 0x8003B304: 'after'}
REGISTER = re.compile(r'^\d+ \d+ ([0-9a-f]{8}) [0-9a-f]{8} .*; \w+=([0-9a-f]{8})$')


def export(folder, frames, initial):
    report = json.loads((folder/'report.json').read_text())
    assert report['status'] == 'complete' and report['trace']['lines_dropped'] == 0
    assert report['watch']['complete']
    start = report['frames']['start']
    ram = bytearray(initial)
    writes = iter(sorted((json.loads(line) for line in (folder/'writes.jsonl').open()), key=lambda e: e['cycle']))
    pending = next(writes, None)
    def snapshot(cycle):
        nonlocal pending
        while pending and pending['cycle'] <= cycle:
            p, size = pending['address'] &0x1FFFFF, pending['width']
            ram[p:p+size] = pending['value'].to_bytes(size, 'little')
            pending = next(writes, None)
        def car(offset, fmt='i'):
            return struct.unpack_from('<'+fmt, ram, (BASE&0x1FFFFF)+offset)[0]
        values = {name: car(off, fmt) for name, off, fmt in [
            ('yaw',0x64C,'i'),('vx',0x628,'i'),('vy',0x62C,'i'),('vz',0x630,'i'),
            ('secondary0',0x650,'h'),('secondary1',0x652,'h'),('feedback',0x64A,'h'),
            ('forward_acceleration',0x724,'i'),('lateral_acceleration',0x728,'i'),
            ('force',0x734,'i'),('forward_speed',0x6A4,'i'),('lateral_speed',0x6A8,'i'),
            ('bypass',0x6B1,'b'),('reset',0x718,'B'),('first',0x78E,'H'),('forced_mode',0x786,'B'),
            ('lateral_resistance',0x730,'i'),('feedback_response',0x34,'h'),('external0',0x778,'i'),('external1',0x77C,'i'),
            ('forward_axis0',0x668,'h'),('forward_axis1',0x66A,'h'),
            ('lateral_axis0',0x670,'h'),('lateral_axis1',0x672,'h'),('duration',0x6FE,'h')
        ]}
        for i in range(4):
            values[f'flags{i}'] = car(0x4C3+i*0x68,'B')
            values[f'activation{i}'] = car(0x4B2+i*0x68,'h')
        return values
    cars = {}
    consumed = {}
    for line in (folder/'trace.txt').open():
        prefix = PREFIX.match(line)
        if not prefix:
            continue
        absolute, cycle, pc = prefix.groups()
        frame, cycle, pc = int(absolute)-start, int(cycle), int(pc,16)
        car = cars.setdefault(frame,{})
        if pc in PHASES:
            phase = PHASES[pc]
            assert phase not in car, 'unexpected second car at single-car checkpoint'
            car[phase] = snapshot(cycle)
        register = REGISTER.match(line.strip())
        if register and pc == 0x8003B030:
            value = int(register[2],16)
            car['yaw_torque'] = value if value < 0x80000000 else value-0x100000000
        effect = EFFECT.match(line.strip())
        if not effect:
            continue
        _, _, address, operation, raw = effect.groups()
        value, address = int(raw,16), int(address,16)
        if value >= 1 << (len(raw)*4-1):
            value -= 1 << (len(raw)*4)
        if pc == 0x8003AFD8:
            car['lateral_force'] = value
        if pc in [0x8003AF94,0x8003AF98,0x8003AF9C]:
            name, offset = {0x8003AF94:('wheel',0x4B8),0x8003AF98:('lateral',0x4B4),0x8003AF9C:('ground',0x4BC)}[pc]
            wheel = (address-BASE-offset)//0x68
            assert 0 <= wheel < 4
            consumed[frame,wheel,name] = value
    assert sorted(cars) == list(range(0,frames,2))
    for car in cars.values():
        assert set(PHASES.values()) <= set(car), 'missing body phase'
        if car['before']['bypass']:
            assert 'lateral_force' not in car and 'yaw_torque' not in car
            car.update(lateral_force=0,yaw_torque=0)
        else:
            assert 'lateral_force' in car and 'yaw_torque' in car
    result = forward_rows(folder,frames)
    for row in result:
        car = cars[row['run_frame']]
        if not car['before']['bypass']:
            for name, field in [('wheel','wheel_force'),('lateral','lateral_force'),('ground','ground_force')]:
                assert consumed[row['run_frame'],row['wheel'],name] == row[field]
        for phase in PHASES.values():
            row.update({f'bd_{phase}_{key}': value for key,value in car[phase].items()})
        for key in ['lateral_force','yaw_torque']:
            row['bd_total_'+key] = car[key]
    return result


def cases(holdout):
    if holdout:
        for angle in [-1789,-319,733,1541]:
            for sign in [-1,1]:
                yield dict(angle=angle,yaw=sign*523997,external=[sign*17391,-sign*3971],
                           body=sign*17391)
        # Fresh exact clamp and feedback-narrowing cases after implementation.
        for yaw in [-524289,-524288,-524287,524287,524288,524289]:
            yield dict(yaw=yaw,body=1138,zero_forces=True,brake_wheel=4,activation_wheel=-1)
        for angle in [-997,-31,197,1193]:
            yield dict(angle=angle,external=[-317,1791])
        for response in [2048,4096]:
            for sign in [-1,1]:
                yield dict(angle=0,feedback_response=response,treads=[131072]*4,
                           wheel_forward=[131072]*4,wheel_lateral=[sign*131072]*4)
        for speed in [-1137,1137]:
            yield dict(body=speed,zero_forces=True,brake_wheel=3,activation_wheel=-1,external=[17391,-7319])
        return
    for angle in [-2048,-1025,-1024,-1,0,1,173,1023,1024,2047]:
        for yaw in [-524289,-524288,-524287,0,524287,524288,524289]:
            yield dict(angle=angle,yaw=yaw)
    # Every reset/first-gear/bypass combination, with nonzero external forces.
    for flags in range(16):
        yield dict(reset=flags&1,forced_mode=7 if flags&2 else 0,first=2 if flags&4 else 0,
                   bypass=-1 if flags&8 else 0,external=[17391,-7319])
    # Freeze all force sources at zero so consumed speed is the exact bound.
    # Each wheel's nonzero brake demand must trigger; activation on any wheel
    # must block. Inputs are observed at the original consumer afterwards.
    for speed in [-1138,-1137,-1,0,1,1137,1138]:
        for brake_wheel in range(5):
            for activation_wheel in [-1,0,1,2,3]:
                yield dict(body=speed,zero_forces=True,brake_wheel=brake_wheel,
                           activation_wheel=activation_wheel)
    for axis in [[4096,0,0,4096],[0,4096,-4096,0],[2896,2895,-2897,2894],[-3011,1779,1371,3197]]:
        for external in [[0,0],[1,-1],[-1,1],[17391,-7319]]:
            yield dict(axis=axis,external=external,angle=173)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state',type=Path,default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--holdout',action='store_true')
    p.add_argument('--limit',type=int,help='Debug prefix; writes a separate probe fixture')
    args = p.parse_args()
    original = args.state.with_name('ram.bin').read_bytes()
    name = 'probe' if args.limit is not None else 'holdout' if args.holdout else 'boundaries'
    probes = list(cases(args.holdout))
    if args.limit is not None:
        probes = probes[:args.limit]
    jobs = [(f'{i:03}',2,[],case) for i,case in enumerate(probes)]
    if args.limit is None:
        scenarios = [('alternating',[dict(frame=0,buttons=['CROSS','RIGHT']),dict(frame=12,buttons=['SQUARE']),
            dict(frame=32,buttons=['CROSS','CIRCLE','LEFT']),dict(frame=58,buttons=[])])] if args.holdout else [
            ('gas',[dict(frame=0,buttons=['CROSS'])]),('brake',[dict(frame=0,buttons=['SQUARE'])]),
            ('turn',[dict(frame=0,buttons=['CROSS','LEFT'])])]
        jobs += [(label,80,replay,None) for label,replay in scenarios]
    rows, records = [], []
    for sequence,(label,frames,replay,case) in enumerate(jobs):
        folder = Path('.psx/runs')/f'body-dynamics-{name}-{label}'
        command = ['psx','run','--from-state',str(args.state),'--frames',str(frames),'--input',json.dumps(replay),
                   '--trace','--max-events','1000000','--watch',f'{BASE:08X}:1952','--out',str(folder),'--summary']
        for region in RANGES+['8003AEDC-8003B310']:
            command += ['--trace-range',region]
        initial = bytearray(original)
        if case:
            angle = case.get('angle',-319)
            changes = [(0x45D,1,1),(0x60C,2,angle),(0x64,2,4096),(0x66,2,4096),(0x36C,2,0),(0x36E,2,angle),
                (0x6A4,4,case.get('body',102605)),(0x6A8,4,-7319),(0x64C,4,case.get('yaw',17391)),
                (0x628,4,17391),(0x62C,4,-7319),(0x630,4,3191),(0x650,2,137),(0x652,2,-719),
                (0x64A,2,319),(0x6B1,1,case.get('bypass',0)),(0x718,1,case.get('reset',0)),
                (0x786,1,case.get('forced_mode',0)),(0x78E,2,case.get('first',0)),
                (0x778,4,case.get('external',[0,0])[0]),
                (0x77C,4,case.get('external',[0,0])[1]),(0x71A,2,0),(0x642,1,2)]
            # 8003E120 overwrites +718 from (car slot < global 800A9520).
            # Intervene at that producer, not at the subsequently overwritten byte.
            # The outer loop decrements that halfword first (80015CFC).
            changes += [(0x800A9520-BASE,2,1000 if case.get('reset') else 0)]
            if 'axis' in case:
                changes += [(off,2,v) for off,v in zip([0x668,0x66A,0x670,0x672],case['axis'])]
            if 'feedback_response' in case:
                changes += [(0x34,2,case['feedback_response'])]
            for i in range(4):
                zero = case.get('zero_forces',False)
                for off,width,v in [(0x8,4,0 if zero else 8193+i*731),
                    (0x34,4,0 if zero else 17391),
                    (0x18,4,0 if zero else case.get('treads',[137791,-73917,31917,-171931])[i]),
                    (0x2C,4,(4096 if case.get('activation_wheel')==i else 0) if zero else case.get('wheel_forward',[173917,73197,-17391,-173191])[i]),
                    (0x30,4,0 if zero else case.get('wheel_lateral',[7391,-17391,31917,-7319])[i]),(0x2A,2,4096),
                    (0x60,2,1 if case.get('brake_wheel')==i else 0),(0x14,1,0)]:
                    changes.append((0x460+i*0x68+off,width,v))
            if case.get('zero_forces'):
                # A zero steering axis prevents the separate low-speed
                # lateral correction from producing force in gate probes.
                changes += [(0x60C,2,0),(0x36E,2,0),(0x612,2,0),(0x614,2,0)]
            for off,width,v in changes:
                command += ['--poke',f'{BASE+off:08X}={v}:{width}']
                pos = (BASE&0x1FFFFF)+off
                initial[pos:pos+width] = (v%(1<<(width*8))).to_bytes(width,'little')
        subprocess.run(command,check=True,capture_output=True,text=True)
        actual = export(folder,frames,initial)
        if case:
            r = actual[0]
            for key in ['bypass','reset','forced_mode']:
                assert r['bd_before_'+key] == case.get(key,0),(label,key,r['bd_before_'+key],case)
            assert r['bd_before_first'] == max(case.get('first',0)-1,0)
            if 'feedback_response' in case:
                assert r['bd_before_feedback_response']==case['feedback_response']
            assert r['bd_before_lateral_resistance'] == 0, '8003A2C8 clears the reserved opposing-force accumulator'
            for key,default in [('yaw',17391),('external0',0),('external1',0)]:
                expected=case.get('yaw',default) if key=='yaw' else case.get('external',[0,0])[int(key[-1])]
                assert r['bd_before_'+key] == expected,(label,key,r['bd_before_'+key],expected)
            if 'axis' in case:
                assert [r['bd_before_'+k+str(i)] for k in ['forward_axis','lateral_axis'] for i in range(2)] == case['axis']
            if case.get('zero_forces'):
                assert r['bd_before_forward_speed'] == case['body']
                for i in range(4):
                    assert bool(r[f'bd_before_flags{i}']&2) == (case['brake_wheel']==i)
                    assert (r[f'bd_before_activation{i}']!=0) == (case['activation_wheel']==i)
        for row in actual:
            row['sequence'] = sequence
            row['case'] = len(rows)//4
            rows.append(row)
        records.append(dict(command=command,report=str(folder/'report.json')))
    OUT.mkdir(parents=True,exist_ok=True)
    with (OUT/f'{name}.csv').open('w',newline='') as f:
        writer = csv.DictWriter(f,sorted(rows[0]));writer.writeheader();writer.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows)//4,records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),ram_sha256=hashlib.sha256(original).hexdigest(),
        psx_version=subprocess.check_output(['psx','--version'],text=True).strip(),
        scope='Original lateral/yaw/world velocity stages; native slip/tyres/forward phase also observed; contact, orientation and collision external'),indent=2)+'\n')
    print(f'{len(rows)//4} original {name} body-dynamics calls exported')


if __name__ == '__main__':
    main()
