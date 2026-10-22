#!/usr/bin/env python3
"""Record original engagement, engine feedback and gear choice, with coupling.

Numerical fixtures and all emulator outputs remain private. Controlled input
requests are explicit scratchpad pokes; natural requests are observed at their
producers even when a cooldown prevents the selector from reading them.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import struct
import subprocess

from record_drivetrain import export as drivetrain_rows
from record_forward_dynamics import RANGES
from record_tyre_forces import BASE, EFFECT
from record_wheel_rotation import PREFIX

OUT = Path('game/tests/data/original/transmission')
PHASES = {0x8003E168: 'timer', 0x8003A060: 'initial', 0x8003A198: 'prepared',
          0x8003AC24: 'rotation', 0x8003991C: 'selection', 0x8003AE94: 'final'}


def export(folder, frames, initial, requests=None):
    report = json.loads((folder/'report.json').read_text())
    assert report['status'] == 'complete' and report['trace']['lines_dropped'] == 0
    assert report['watch']['complete']
    start = report['frames']['start']
    ram, scratch = bytearray(initial), bytearray(1024)
    initialized = [requests is not None]*2
    if requests is not None:
        scratch[0x364:0x366] = bytes(v %256 for v in requests)
    writes = iter(sorted((json.loads(line) for line in (folder/'writes.jsonl').open()), key=lambda e: e['cycle']))
    pending = next(writes, None)

    def snapshot(cycle, timer_only=False):
        nonlocal pending
        while pending and pending['cycle'] <= cycle:
            address, size = pending['address'] &0x1FFFFF, pending['width']
            ram[address:address+size] = pending['value'].to_bytes(size, 'little')
            pending = next(writes, None)
        def car(offset, fmt='h'):
            return struct.unpack_from('<'+fmt, ram, (BASE&0x1FFFFF)+offset)[0]
        if timer_only:
            return dict(first_counter=car(0x78E, 'H'))
        values = {name: car(offset, fmt) for name, offset, fmt in [
            ('gear', 0x618, 'B'), ('engagement', 0x619, 'B'), ('cooldown', 0x61C, 'B'),
            ('requested', 0x60A, 'h'), ('engine_speed', 0x624, 'i'), ('rpm', 0x6AC, 'h'),
            ('cut', 0x61D, 'B'), ('throttle', 0x708, 'h'), ('accelerator', 0x610, 'h'),
            ('brake', 0x612, 'h'), ('forward', 0x6A4, 'i'), ('mode', 0x642, 'B'),
            ('force_auto', 0x786, 'B'), ('force_first', 0x78E, 'H'),
            ('source', 0x372, 'B'), ('layout', 0x370, 'B'), ('induction', 0x128, 'h'),
        ]}
        assert all(initialized), 'unobserved request bytes must not default to zero'
        values['reverse_request'], values['shift_request'] = struct.unpack_from('<bb', scratch, 0x364)
        values['clutch_engine'], values['clutch_shaft'] = struct.unpack_from('<ii', scratch, 0x7C)
        for i in range(4):
            for name, offset, fmt in [('load', 0x468, 'i'), ('slip', 0x4A4, 'h'), ('response', 0x48A, 'h')]:
                values[f'{name}{i}'] = car(offset+i*0x68, fmt)
        return values

    cars = {}
    for line in (folder/'trace.txt').open():
        prefix = PREFIX.match(line)
        if not prefix:
            continue
        absolute, cycle, pc = prefix.groups()
        frame, pc = int(absolute)-start, int(pc, 16)
        effect = EFFECT.match(line.strip())
        if effect and effect[4] == '=':
            address = int(effect[3], 16)
            if 0x1F800000 <= address < 0x1F800400:
                data = bytes.fromhex(effect[5])[::-1]
                p = address-0x1F800000
                scratch[p:p+len(data)] = data
                for i in range(2):
                    if p <= 0x364+i < p+len(data):
                        initialized[i] = True
        if pc in PHASES:
            car = cars.setdefault(frame, {})
            phase = PHASES[pc]
            assert phase not in car
            car[phase] = snapshot(int(cycle), phase == 'timer')
    assert sorted(cars) == list(range(0, frames, 2))
    for car in cars.values():
        assert set(car) == set(PHASES.values()), 'missing ordinary transmission phase'
    result = drivetrain_rows(folder, frames, initial)
    for row in result:
        for phase, values in cars[row['run_frame']].items():
            row.update({f'tr_{phase}_{key}': value for key, value in values.items()})
    return result


def shift_cases(ram):
    def read(offset, fmt='H'):
        return struct.unpack_from('<'+fmt, ram, (BASE&0x1FFFFF)+offset)[0]
    def isolated(**changes):
        case=dict(mode=0,gear=3,engagement=1,requested=1,rpm=3000,speed=200001,
                  forward=4096,throttle=3072,cooldown=0,shift=0,reverse=0,
                  isolated=True,target_rpm=3000,layout=1)
        case.update(changes)
        return case
    for gear in range(1,5):
        for adjacent in [-1,0,1]:
            yield isolated(gear=gear,target_rpm=read(0x396)+adjacent)
    for gear in range(2,6):
        for adjacent in [-1,0,1]:
            yield isolated(gear=gear,target_rpm=read(0x398+(gear-2)*2)+adjacent)
    for throttle in [3072,3073]:
        threshold=read(0x39A) if throttle==3072 else max(read(0x39A)-500,read(0x394)+500)
        for adjacent in [-1,0,1]:
            yield isolated(throttle=throttle,target_rpm=threshold+adjacent)
    for layout in range(7):
        for up in [False,True]:
            for wheel in range(4):
                threshold=read((0x1FC if up else 0x1FE)+(wheel//2)*0xD8,'h')
                for adjacent in [-1,0,1,None]:
                    yield isolated(layout=layout,gear=1 if up else 3,
                        target_rpm=read(0x396)+1 if up else read(0x39A)-1,
                        probe_wheel=wheel,probe_slip=threshold+adjacent if adjacent is not None else None,
                        unloaded=adjacent is None)
    for gear in range(1,5):
        # Manual recovery uses the ordinary upshift RPM; automatic safety
        # uses redline. These are separate source predicates.
        for manual, rpm in [(True,read(0x396)+500),(False,read(0x108)+500)]:
            coefficient=read(0x3C4+gear*4,'i')
            engine_raw=int(rpm*4096/60)
            limit=engine_raw*4096//coefficient
            for adjacent in [-1,0,1]:
                yield isolated(mode=1 if manual else 0,gear=0 if manual else gear,
                    forward=limit+adjacent,all_unloaded=not manual)
    for speed in [-1,0,1]:
        yield isolated(mode=1,gear=0,forward=speed)
    for gear in [0,1,3]:
        for speed in [-1139,-1138,-1137,1137,1138,1139]:
            for throttle in [0,1]:
                yield isolated(gear=gear,forward=speed,throttle=throttle)
    for engagement in [0,4]:
        limit=int(((read(0x108)+10) if engagement else read(0x10A))*4096/60)
        for speed in [-4097,-1,0,1,limit-1,limit,limit+1]:
            yield isolated(mode=2,engagement=engagement,requested=0 if engagement==0 else 1,
                           speed=speed,target_rpm=None)
    yield isolated(mode=2,engagement=0,requested=0,speed=int(read(0x108)*4096/60)+1000,target_rpm=None)
    idle_raw=int(read(0x10A)*4096/60)
    for adjacent in [-1,0,1]:
        yield isolated(mode=2,gear=5,engagement=3,speed=100000,
                       target_rpm=None,target_clutch=idle_raw+adjacent)
    braking_idle_raw=int((read(0x10A)+500)*4096/60)
    for brake in [4095,4096]:
        for adjacent in [-1,0,1]:
            yield isolated(mode=2,engagement=2,speed=braking_idle_raw+adjacent,
                           target_rpm=None,brake=brake)
    for mode in [0,1,2]:
        for engagement in [2,3]:
            for brake in [0,4096]:
                yield isolated(mode=mode,gear=1,engagement=engagement,speed=0,
                               target_rpm=None,brake=brake)


def cases(holdout, final_holdout=False):
    if final_holdout:
        for mode in [0,1,2]:
            for gear in [1,4]:
                for sign in [-1,1]:
                    yield dict(mode=mode,gear=gear,engagement=3,requested=1,rpm=6301,
                        speed=411779,forward=sign*71,throttle=2579,cooldown=0 if mode==1 else 2,shift=sign,reverse=0)
        return
    if holdout:
        # Fresh signed states and unequal histories, chosen after the rules.
        for mode in [0, 1, 2]:
            for gear in [0, 1, 2, 5]:
                for request in [-1, 1]:
                    yield dict(mode=mode, gear=gear, engagement=2, requested=1,
                               rpm=3811, speed=173911, forward=-1139, throttle=2779,
                               cooldown=1, shift=request, reverse=0)
        return
    for engagement in range(5):
        for requested in [0, 1]:
            for mode in [0, 1, 2]:
                yield dict(mode=mode, gear=3, engagement=engagement, requested=requested,
                           rpm=3000, speed=200001, forward=102605, throttle=4096,
                           cooldown=0, shift=1, reverse=0)
    # Preparation's strict speed/RPM/throttle branches.
    for gear in [0, 3]:
        for forward in [-11381, -11380, -11379, 11379, 11380, 11381]:
            for throttle in [1024, 1025]:
                yield dict(mode=2, gear=gear, engagement=0, requested=1, rpm=2500,
                           speed=200001, forward=forward, throttle=throttle,
                           cooldown=0, shift=0, reverse=0)
    for rpm in [2499, 2500, 2501]:
        yield dict(mode=2, gear=3, engagement=0, requested=1, rpm=rpm,
                   speed=200001, forward=0, throttle=0, cooldown=0, shift=0, reverse=0)
    for force_auto, force_first in [(1, 0), (0, 1), (1, 1)]:
        for mode in [0, 1, 2]:
            for gear in [0, 1, 5]:
                yield dict(mode=mode, gear=gear, engagement=4, requested=1,
                           rpm=3000, speed=200001, forward=102605, throttle=4096,
                           cooldown=6, shift=-1, reverse=1, force_auto=force_auto, force_first=force_first*2)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=Path, default=Path('.psx/runs/b1-state1200/state.bin'))
    p.add_argument('--holdout', action='store_true')
    p.add_argument('--shift-holdout', action='store_true')
    p.add_argument('--final-holdout', action='store_true')
    args = p.parse_args()
    original = args.state.with_name('ram.bin').read_bytes()
    assert sum([args.holdout,args.shift_holdout,args.final_holdout])<=1
    name = 'final_holdout' if args.final_holdout else 'shift_holdout' if args.shift_holdout else 'holdout' if args.holdout else 'boundaries'
    probes=shift_cases(original) if args.shift_holdout else cases(args.holdout,args.final_holdout)
    jobs = [(f'{i:03}', 2, [], case) for i, case in enumerate(probes)]
    scenarios = [('alternating',[dict(frame=0,buttons=['CROSS']),dict(frame=10,buttons=['CROSS','SQUARE','RIGHT']),
        dict(frame=26,buttons=[]),dict(frame=36,buttons=['SQUARE','LEFT']),dict(frame=54,buttons=['CROSS','CIRCLE'])])] if args.final_holdout else [] if args.shift_holdout else [('mixed', ['CROSS', 'SQUARE', 'RIGHT'])] if args.holdout else [
        ('gas', ['CROSS']), ('brake', ['SQUARE']), ('turn', ['CROSS', 'LEFT'])]
    jobs += [(label, 80, buttons, None) for label, buttons in scenarios]
    if not args.holdout and not args.shift_holdout and not args.final_holdout:
        jobs.append(('first-gear-countdown',20,[],dict(mode=1,gear=3,engagement=4,requested=1,
            rpm=3000,speed=200001,forward=102605,throttle=4096,cooldown=0,shift=0,reverse=1,force_first=4)))
        for request in [-1, 1]:
            jobs.append((f'manual-held-{request}', 40, [], dict(mode=1, gear=3, engagement=4,
                requested=1, rpm=3000, speed=200001, forward=102605, throttle=4096,
                cooldown=0, shift=request, reverse=0)))
    rows, records = [], []
    inverse_tau=(struct.unpack_from('<I',original,0x39400)[0]&0xFFFF)/4096
    for sequence, (label, frames, buttons, case) in enumerate(jobs):
        folder = Path('.psx/runs')/f'transmission-{name}-{label}'
        initial = bytearray(original)
        replay=buttons if buttons and isinstance(buttons[0],dict) else [dict(frame=0,buttons=buttons)]
        command = ['psx', 'run', '--from-state', str(args.state), '--frames', str(frames),
                   '--input', json.dumps(replay), '--trace', '--max-events', '1000000',
                   '--watch', f'{BASE:08X}:1952', '--out', str(folder), '--summary']
        for region in RANGES + ['8002FFE0-80030178', '8003E168-8003E17C', '8003A000-8003A280', '8003E4F0-8003E5E8',
                '80045688-80046B58', '8003AC24-8003AEA4', '8003941C-8003946C',
                '8003991C-80039994', '800448C8-80044EA4']:
            command += ['--trace-range', region]
        requests = None
        target=None
        angular=0
        if case:
            changes = [(0x45D, 1, 1), (0x642, 1, case['mode']), (0x618, 1, case['gear']),
                (0x619, 1, case['engagement']), (0x60A, 2, case['requested']), (0x6AC, 2, case['rpm']),
                (0x624, 4, case['speed']), (0x6A4, 4, case['forward']), (0x708, 2, case['throttle']),
                (0x610, 2, case['throttle']), (0x61C, 1, case['cooldown']),
                (0x786, 1, case.get('force_auto', 0)), (0x78E, 2, case.get('force_first', 0))]
            if case.get('isolated'):
                changes += [(0x370,1,case.get('layout',1)),(0x10C,4,0),(0x3F4,2,0),(0x3F6,2,0),
                            (0x71A,2,0),(0x612,2,case.get('brake',0)),(0x60C,2,0),
                            (0x36C,2,0),(0x36E,2,0),(0x64,2,4096),(0x66,2,4096)]
                count, _, _, ys=struct.unpack_from('<HHII',original,(BASE&0x1FFFFF)+0x7C)
                changes += [((ys&0x1FFFFF)-(BASE&0x1FFFFF)+i*4,4,0) for i in range(count)]
                for i in range(4):
                    load=0 if case.get('all_unloaded') or (case.get('unloaded') and case['probe_wheel']==i) else 1
                    slip=case.get('probe_slip') if case.get('probe_wheel')==i else None
                    # Exact slip inputs using a power-of-two reference speed,
                    # as in the tyre recorder. Expected gear results still
                    # come solely from the original execution.
                    tread=0 if slip is None else 524288-(slip*128 if slip>0 else 0)
                    forward=0 if slip is None else 524288+(slip*128 if slip<0 else 0)
                    changes += [(0x468+i*0x68,4,load),(0x494+i*0x68,4,0),
                        (0x478+i*0x68,4,tread),(0x48C+i*0x68,4,forward),(0x490+i*0x68,4,0),
                        (0x48A+i*0x68,2,4096),(0x476+i*0x68,2,0),(0x4C0+i*0x68,2,0)]
                changes += [(0x37A,1,2),(0x37B,1,2)]
                target=case.get('target_rpm')
                clutch_target=case.get('target_clutch')
                if target is not None or clutch_target is not None:
                    ratio=struct.unpack_from('<i',original,(BASE&0x1FFFFF)+0x3A4+case['gear']*4)[0]/4096
                    desired_rev=(target+0.5)/60 if target is not None else (clutch_target+0.5)/4096
                    angular=round(desired_rev/(ratio*inverse_tau)*4096)
                    if case['gear']==0:angular=-angular
            for off, width, value in changes:
                command += ['--poke', f'{BASE+off:08X}={value}:{width}']
                pos = (BASE&0x1FFFFF)+off
                initial[pos:pos+width] = (value %(1<<(width*8))).to_bytes(width, 'little')
            requests = (case['reverse'], case['shift'])
            for i, value in enumerate(requests):
                command += ['--poke', f'{0x1F800364+i:08X}={value}:1']
        attempts=[]
        for attempt in range(8):
            actual_command=command.copy();actual_initial=bytearray(initial)
            actual_folder=folder if attempt==0 else folder.with_name(folder.name+f'-calibration-{attempt}')
            actual_command[actual_command.index('--out')+1]=str(actual_folder)
            if case and case.get('isolated'):
                for i in range(2):
                    actual_command += ['--poke',f'{BASE+0x634+i*4:08X}={angular}:4']
                    struct.pack_into('<i',actual_initial,(BASE&0x1FFFFF)+0x634+i*4,angular)
            subprocess.run(actual_command, check=True, capture_output=True, text=True)
            actual = export(actual_folder, frames, actual_initial, requests)
            attempts.append(dict(command=actual_command,report=str(actual_folder/'report.json')))
            if not case or not case.get('isolated') or target is None and case.get('target_clutch') is None:break
            observed=actual[0]['tr_selection_rpm'] if target is not None else actual[0]['tr_prepared_clutch_shaft']
            wanted=target if target is not None else case['target_clutch']
            if observed==wanted:break
            # Calibration changes input shaft speed until the ORIGINAL
            # consumer reaches the requested boundary; no expected output
            # is calculated by a replacement model.
            slope=60*ratio*inverse_tau/4096 if target is not None else ratio*inverse_tau
            angular += (1 if case['gear'] else -1)*round((wanted-observed)/slope)
        else:raise RuntimeError('original boundary calibration did not converge')
        if case:
            r = actual[0]
            assert [r['tr_initial_'+key] for key in ['gear', 'engagement', 'rpm', 'engine_speed', 'forward']] == [case[k] for k in ['gear', 'engagement', 'rpm', 'speed', 'forward']]
            assert (r['tr_selection_reverse_request'], r['tr_selection_shift_request']) == requests
        for row in actual:
            row['tr_isolated']=int(bool(case and case.get('isolated')))
            row['tr_target_rpm']=case.get('target_rpm',-1) if case and case.get('target_rpm') is not None else -1
            row['tr_target_clutch']=case.get('target_clutch',-1) if case else -1
            row['sequence'] = sequence
            row['case'] = len(rows)//4
            rows.append(row)
        if case and case.get('probe_slip') is not None:
            assert actual[0]['tr_selection_slip'+str(case['probe_wheel'])]==case['probe_slip']
        records.append(dict(attempts=attempts))
    OUT.mkdir(parents=True, exist_ok=True)
    with (OUT/f'{name}.csv').open('w', newline='') as f:
        writer = csv.DictWriter(f, sorted(rows[0])); writer.writeheader(); writer.writerows(rows)
    (OUT/f'{name}.json').write_text(json.dumps(dict(car_executions=len(rows)//4, records=records,
        state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
        ram_sha256=hashlib.sha256(original).hexdigest(),
        psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
        scope='Ordinary preparation, coupling, engine feedback, RPM refresh and gear selection; contact/body/control request inputs observed'
    ), indent=2)+'\n')
    print(f'{len(rows)//4} original {name} transmission calls exported')


if __name__ == '__main__':
    main()
