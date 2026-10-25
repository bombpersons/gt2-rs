#!/usr/bin/env python3
"""Build the current B-1 runtime assets from a user-supplied USA Rev 2 disc.

No assets, BIOS or emulator are downloaded. The psx CLI captures local B-1
moving and countdown checkpoints for wheel profiles, tuning and startup.
The native game never emulates the original. All outputs are private, ignored by Git. Python stdlib
only, plus cargo and psx >=0.3.2. See docs/assets.md.
"""
import argparse
import csv
import gzip
import hashlib
import json
import math
from pathlib import Path
import re
import shutil
import struct
import subprocess
import zlib

ROOT = Path(__file__).resolve().parents[1]
BASE = 0xA96B4


class RonVariant(str):
    """Authored enum syntax; values still come from the supplied checkpoint."""


def run(command):
    print('+ ' + ' '.join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), cwd=ROOT, check=True)


def capture(disc, bios, work, psx='psx'):
    version = subprocess.check_output([psx, '--version'], text=True)
    match = re.search(r'psx (\d+)\.(\d+)\.(\d+)', version)
    if not match:
        raise RuntimeError(f'Unrecognized toolkit version: {version}')
    if tuple(map(int, match.groups())) >= (0, 4, 0):
        return capture_current(disc, bios, work, psx)
    hops = [
        ('list', 3000, json.loads((ROOT / 'docs/checkpoints/poweron-to-b1-list.json').read_text()), None),
        ('menu', 1800, [{'frame': 10, 'buttons': ['CROSS'], 'until': 16}], 'list'),
        ('transmission', 1500, [{'frame': 10, 'buttons': ['CROSS'], 'until': 16}], 'menu'),
        ('start', 250, [{'frame': 10, 'buttons': ['CROSS'], 'until': 16}], 'transmission'),
        ('driving', 1200, [{'frame': 10, 'buttons': ['CROSS'], 'until': 16},
                           {'frame': 400, 'buttons': ['CROSS']}], 'transmission'),
    ]
    for name, frames, replay, parent in hops:
        folder = work / name
        command = [psx, 'run', '--frames', frames, '--input', json.dumps(replay),
                   '--save-state', '--out', folder, '--summary']
        if bios:
            command += ['--bios', bios]
        command += ['--from-state', work / parent / 'state.bin'] if parent else [disc]
        run(command)
        report = json.loads((folder / 'report.json').read_text())
        if report['status'] != 'complete' or report['frames']['reached'] != frames:
            raise RuntimeError(f'Incomplete original run: {folder}')
    return work / 'driving/ram.bin', work / 'start/ram.bin'


def capture_current(disc, bios, work, psx):
    """The 0.4 core boots/loads at different field boundaries than 0.3.8.

    Observe the initialized car and countdown rather than capturing black
    loading RAM. Preserve the moving checkpoint's input offsets from the
    first ready countdown field, independent of the loading duration.
    """
    def hop(name, frames, replay, parent=None, samples=()):
        folder = work / name
        command = [psx, 'run', '--frames', frames, '--input', json.dumps(replay),
                   '--save-state', '--out', folder, '--summary']
        if bios:
            command += ['--bios', bios]
        command += ['--from-state', work / parent / 'state.bin'] if parent else [disc]
        for sample in samples:
            command += ['--sample', sample]
        run(command)
        report = json.loads((folder / 'report.json').read_text())
        if report['status'] != 'complete' or report['frames']['reached'] != frames:
            raise RuntimeError(f'Incomplete original run: {folder}')
        return folder

    replay = json.loads((ROOT / 'docs/checkpoints/poweron-to-b1-list-current.json').read_text())
    hop('list', 4800, replay)
    confirm = [dict(frame=10, buttons=['CROSS'], until=22)]
    hop('menu', 1800, confirm, 'list')
    hop('transmission', 1500, confirm, 'menu')
    probe = hop('start-probe', 900, confirm, 'transmission', (
        '800A9520:u16:countdown', '800A9A26:u8:engine_source', '800A9DB2:i16:duration'))
    rows = list(csv.DictReader((probe / 'samples.csv').open()))
    first_running = next((i for i, row in enumerate(rows) if 0 < int(row['countdown']) < 120), None)
    if first_running is None or first_running == 0:
        raise RuntimeError('B-1 never reached its live countdown; inspect start-probe/screen.png')
    ready = rows[first_running - 1]
    if int(ready['countdown']) != 120 or int(ready['engine_source']) != 5 or int(ready['duration']) <= 0:
        raise RuntimeError(f'B-1 car not initialized before countdown: {ready}')
    start_frame = int(ready['run_frame'])
    hop('start', start_frame, confirm, 'transmission')
    hop('driving', start_frame + 950,
        confirm + [dict(frame=start_frame + 150, buttons=['CROSS'])], 'transmission')
    return work / 'driving/ram.bin', work / 'start/ram.bin'


def ron(value):
    if isinstance(value, RonVariant):
        return str(value)
    if isinstance(value, bool):
        return 'true' if value else 'false'
    if isinstance(value, dict):
        return '(' + ', '.join(k + ': ' + ron(v) for k, v in value.items()) + ')'
    if isinstance(value, tuple):
        return '(' + ', '.join(map(ron, value)) + ')'
    if isinstance(value, list):
        return '[' + ', '.join(map(ron, value)) + ']'
    if isinstance(value, str):
        return json.dumps(value)
    return repr(value)


def quaternion(matrix):
    # Row-major original matrix, the same transpose used by camera tests.
    m = [[matrix[y * 3 + x] / 4096 for x in range(3)] for y in range(3)]
    trace = sum(m[i][i] for i in range(3))
    if trace > 0:
        s = math.sqrt(trace + 1) * 2
        q = [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s,
             (m[1][0] - m[0][1]) / s, s / 4]
    else:
        i = max(range(3), key=lambda k: m[k][k])
        j, k = (i + 1) % 3, (i + 2) % 3
        s = math.sqrt(1 + m[i][i] - m[j][j] - m[k][k]) * 2
        q = [0.0] * 4
        q[i], q[j], q[k], q[3] = s / 4, (m[j][i] + m[i][j]) / s, (m[k][i] + m[i][k]) / s, (m[k][j] - m[j][k]) / s
    length = math.sqrt(sum(v * v for v in q))
    return tuple(v / length for v in q)


def export_tuning(ram_path, assets):
    ram = ram_path.read_bytes()
    if len(ram) != 2 * 1024 * 1024:
        raise ValueError('Expected a complete 2 MiB B-1 RAM dump')
    def read(address, fmt):
        return struct.unpack_from('<' + fmt, ram, address & 0x1FFFFF)[0]
    def car(offset, fmt='h'):
        return read(BASE + offset, fmt)
    def values(address, count, fmt, stride):
        return [read(address + i * stride, fmt) for i in range(count)]
    def immediate(address, opcode):
        word = read(address, 'I')
        if word >> 16 != opcode:
            raise ValueError(f'Unsupported executable instruction at {address:08x}')
        return word & 65535
    def write(name, data):
        path = assets / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('// Generated locally from user-supplied disc; do not commit.\n' + ron(data) + '\n')
    def curve(offset, xname, yname, xscale, fmt='i'):
        count = car(offset, 'H')
        if count < 2 or count > 128:
            raise ValueError(f'Invalid curve at car+{offset:x}')
        xs, ys = car(offset + 4, 'I'), car(offset + 8, 'I')
        stride = struct.calcsize(fmt)
        points = [{xname: read(xs + i * stride, fmt) / xscale, yname: read(ys + i * stride, fmt) / (4096 if yname == 'angle_turns' else 1)}
                for i in range(count)]
        if any(a[xname] >= b[xname] for a, b in zip(points, points[1:])):
            raise ValueError(f'Non-increasing curve coordinates at car+{offset:x}')
        return points
    write('vehicle/pedals.ron', {name: read(0x46DB0 + i * 4, 'h') / 4096 for i, name in enumerate(
        ['throttle_press', 'throttle_release', 'brake_press', 'brake_release', 'handbrake_press', 'handbrake_release'])})
    write('vehicle/steering.ron', dict(
        stiffness=read(0x46F3C, 'i') / 4096, damping=read(0x46F40, 'i') / 4096,
        centering_stiffness=read(0x46F44, 'i') / 4096,
        rate_limit_turns_per_second=car(0x62) / 4096, extension_threshold_turns=car(0x60) / 4096,
        extension_min_speed_mps=immediate(0x2FBE8, 0x3402) / 4096,
        extended_limit_turns=car(0x3A) / 4096, inner_wheel_ratio=car(0x64) / 4096,
        outer_wheel_ratio=car(0x66) / 4096, toe_turns=(car(0x36C) / 4096, car(0x36E) / 4096),
        speed_curve=curve(0x3C, 'speed_kph', 'angle_turns', 128, 'h')))
    write('vehicle/automatic_stop.ron', dict(
        steering_rate_turns_per_second=car(0x62)/4096,
        settled_linear_mps=immediate(0x3775C,0x2A10)/4096,
        settled_angular_turns_per_second=immediate(0x37768,0x2843)/4096,
        reengage_above_rpm=float(car(0x396,'H')*3//4),disengage_below_rpm=float(car(0x10A,'H'))))
    write('vehicle/engine.ron', dict(
        drag_per_rev_s=car(0x10C, 'i'), redline_rpm=car(0x108, 'H'), idle_rpm=car(0x10A, 'H'),
        fuel_cut_hysteresis_rpm=65536 - immediate(0x3538C, 0x2442),
        torque_curve=curve(0x7C, 'speed_rev_s', 'torque', 4096)))
    layout_kind = car(0x370, 'B')
    if layout_kind in [0, 1, 2, 3, 5]:
        layout = {0: 'Rear', 1: 'Front', 2: 'Viscous', 3: 'Open', 5: 'AdaptiveClutch'}[layout_kind]
    elif layout_kind in [4, 6]:
        layout = ('Clutch' if layout_kind == 4 else 'Split') + ron(dict(front_fraction=car(0x374) / 4096))
    else:
        raise ValueError('Unsupported original drive layout')
    reference = car(0x373, 'B')
    if reference not in [0, 1]:
        raise ValueError('Unsupported original reference axle')
    write('vehicle/drivetrain.ron', dict(
        layout=RonVariant(layout), reference_axle=RonVariant(['Front', 'Rear'][reference]),
        gear_ratios=[car(0x3A4 + i * 4, 'i') / 4096 for i in range(8)],
        axle_inertia=tuple(car(0x400 + i * 4, 'i') / 16 for i in range(2)),
        engine_inertia=car(0x378) / 16, centre_rotation_response=car(0x408, 'i') / 256,
        centre_viscosity=car(0x374), adaptive_lock=car(0x376) != 0,
        centre_lock_torque=car(0x376), engine_rotation_response=car(0x40C, 'i') / 4096,
        revolutions_per_radian=immediate(0x39400, 0x2404) / 4096,
        fixed_clutch_torque=car(0x3F4), coast_clutch_torque=car(0x3F6),
        clutch_free_difference_rev_s=(65536-immediate(0x392EC, 0x3442)) / 4096,
        clutch_gain_per_rev_s=immediate(0x39304, 0x2405) / 4096,
        max_clutch_gain=immediate(0x39318, 0x2404) / 4096,
        adaptive_full_angle_turns=(immediate(0x459C8, 0x2882)-1) / 4096,
        adaptive_min_angle_turns=immediate(0x459DC, 0x2882) / 4096,
        adaptive_min_response=immediate(0x459EC, 0x2411) / 4096,
        adaptive_taper_intercept=immediate(0x459E4, 0x2402) / 4096,
        lock_brake_travel=(immediate(0x45A40, 0x2842)-1) / 4096))
    write('vehicle/braking.ron', dict(front_capacity=car(0x414, 'i'), rear_capacity=car(0x418, 'i'), handbrake_capacity=car(0x41C, 'i')))
    write('vehicle/body_dynamics.ron', dict(
        wheel_levers_m=((-car(0x18)/4096, car(0x0C)/4096), (car(0x18)/4096, car(0x0C)/4096),
                        (-car(0x1A)/4096, -car(0x0E)/4096), (car(0x1A)/4096, -car(0x0E)/4096)),
        yaw_response_turns_per_torque_second2=car(0x68, 'i') /4096 /524288,
        inverse_mass=car(0x2A)/4096, lateral_feedback_response=car(0x34)/4096,
        maximum_yaw_turns_per_second=immediate(0x3B048, 0x3C04)*65536 /524288,
        brake_stop_speed_mps=immediate(0x3B160, 0x2442) /4096))
    suspension_axles = []
    for axle in range(2):
        offset = 0x12C + axle*0x34
        def damper(start):
            return dict(threshold_mps=car(offset+start, 'i')/4096,
                        base_force=car(offset+start+4, 'i'), square_root_response=car(offset+start+8, 'i'))
        suspension_axles.append(dict(bump_start_m=car(offset)/4096,
            minimum_travel_m=car(offset+2)/4096, maximum_travel_m=car(offset+4)/4096,
            travel_offset_m=(car(offset+8)-car(0x3E4+axle*2))/4096,
            spring_force_per_m=car(offset+12, 'i'), bump_force_per_m2=car(offset+44, 'i'),
            antiroll_force_per_m2=car(offset+40, 'i'), preload_force=car(offset+50),
            rebound=damper(16), compression=damper(28)))
    write('vehicle/suspension.ron',dict(axles=tuple(suspension_axles),
        lateral_lever_m=car(0x1E)/4096, forward_lever_m=car(0x20)/4096))
    write('vehicle/contact_motion.ron',dict(
        wheel_levers_m=((-car(0x18)/4096,car(0x0C)/4096),(car(0x18)/4096,car(0x0C)/4096),
                        (-car(0x1A)/4096,-car(0x0E)/4096),(car(0x1A)/4096,-car(0x0E)/4096)),
        inverse_radius_per_m=(car(0x3E8)/4096,car(0x3EA)/4096),
        radians_per_turn=immediate(0x3059C,0x2405)/4096,
        turns_per_radian=immediate(0x30890,0x2405)/4096,
        speed_readout_units_per_mps=(immediate(0x30570,0x3C05)*65536+immediate(0x30574,0x34A5))/4096,
        full_response_speed_mps=(immediate(0x305A8,0x3C15)*65536+immediate(0x305AC,0x36B5))/4096,
        response_gain_per_mps=immediate(0x30670,0x2405)/4096,
        base_speed_response=immediate(0x3067C,0x2445)/4096))
    grip_axles=[]
    for axle in range(2):
        offset=axle*0xD8
        camber_curve=curve(0x250+offset,'angle_turns','response',4096,'h')
        for point in camber_curve:point['response']/=4096
        grip_axles.append(dict(load_curve=curve(0x224+offset,'load','capacity',1),
                               camber_curve=camber_curve,camber_turns=car(0x368+axle*2)/4096))
    condition=None
    if read(0x46F48,'i')!=0:
        condition=RonVariant('Some('+ron(dict(maximum=read(0x46F48,'i'),warm_start=read(0x46F5C,'i'),
            cold_limit=read(0x46F54,'i'),maximum_load_loss=read(0x46F4C,'i')/4096,
            warm_load_loss=read(0x46F60,'i')/4096,cold_load_loss=read(0x46F58,'i')/4096))+')')
    write('vehicle/tyre_grip.ron',dict(axles=tuple(grip_axles),
        surface_response=[car(0x348+i*2)/256 for i in range(16)],condition=condition if condition else RonVariant('None')))
    gears = car(0x372, 'B')
    if not 1 <= gears <= 7:
        raise ValueError('Unsupported original forward gear count')
    write('vehicle/transmission.ron', dict(
        forward_gears=gears, minimum_shift_rpm=car(0x394, 'H'), upshift_rpm=car(0x396, 'H'),
        downshift_rpm=[car(0x398+i*2, 'H') for i in range(gears-1)],
        engine_rev_s_per_mps=[car(0x3C4+i*4, 'i') /4096 for i in range(8)],
        downshift_slip_limit=tuple(car(0x1FE+i*0xD8) /4096 for i in range(2)),
        upshift_slip_limit=tuple(car(0x1FC+i*0xD8) /4096 for i in range(2)),
        launch_speed_mps=(immediate(0x3A098, 0x2862)-1) /4096,
        launch_throttle=(immediate(0x3A0D4, 0x2842)-1) /4096,
        forward_select_speed_mps=immediate(0x44AB0, 0x2842) /4096,
        reverse_select_speed_mps=(65536-immediate(0x44AD4, 0x2842)) /4096,
        forward_return_speed_mps=immediate(0x44A84, 0x2842) /4096,
        full_throttle_threshold=(immediate(0x44BD4, 0x2863)-1) /4096,
        throttle_downshift_margin_rpm=65536-immediate(0x44BE4, 0x2484),
        redline_speed_margin_rpm=immediate(0x44B14, 0x2463),
        first_downshift_rpm=immediate(0x44B98, 0x2404)-65536,
        top_upshift_rpm=immediate(0x44BB8, 0x2405),
        feedback_redline_margin_rpm=immediate(0x3AE50, 0x2442),
        braking_idle_margin_rpm=immediate(0x3ADFC, 0x2442),
        reengagement_delay_ticks=int(read(0x1C8570, 'i') /5) %256))
    # Global resistance curve uses the same word-coordinate descriptor as
    # engine torque, but is not relative to this car's physics base.
    resistance_curve = curve(0x1C8730 - BASE, 'speed_mps', 'response', 4096)
    for knot in resistance_curve:
        knot['response'] /= 4096
    write('vehicle/forward_dynamics.ron', dict(
        body_mass=car(0x28) / 4096, body_inverse_mass=car(0x2A) / 4096,
        wheel_rotational_mass=(car(0x410, 'H') / 4096, car(0x412, 'H') / 4096),
        surface_resistance=[v / 4096 for v in values(0x46E00, 8, 'i', 4)],
        resistance_curve=resistance_curve))
    differentials = []
    for axle in range(2):
        mode = car(0x37A + axle, 'B')
        preload = car(0x37C + axle * 4, 'i')
        drive = car(0x384 + axle * 4, 'i')
        coast = car(0x38C + axle * 4, 'i')
        if mode in [0, 1, 2]:
            variant = ['Independent', 'Locked', 'Open'][mode]
        elif mode in [3, 4]:
            variant = 'Clutch' + ron(dict(preload_torque=preload, drive_response=drive / 4096, coast_response=coast / 4096))
        elif mode == 5:
            variant = 'Viscous' + ron(dict(slip_squared_limit_mps2=preload / 4096, torque_per_squared_mps=drive))
        elif mode in [6, 7]:
            variant = 'YawBiased' + ron(dict(response=preload))
        else:
            raise ValueError('Unsupported original differential kind')
        differentials.append(RonVariant(variant))
    write('vehicle/wheel_rotation.ron', dict(
        radius_m=tuple(car(0x3E4 + axle * 2) / 4096 for axle in range(2)),
        rotation_response=tuple(car(0x3F8 + axle * 4, 'i') / 512 for axle in range(2)),
        differentials=tuple(differentials), yaw_bias_min_forward_mps=immediate(0x45854, 0x2842) / 4096))
    # Wheel steering uses a biased signed sine quantizer. Verify the native
    # formula against every consumed sine/cosine table entry for this overlay.
    for phase in range(5120):
        expected = math.trunc(math.sin((phase % 4096) * math.tau / 4096) * 4096 + 0.5)
        if read(0x93150 + phase * 2, 'h') != expected:
            raise ValueError('Unsupported wheel steering direction curve')
    write('vehicle/longitudinal_slip.ron', dict(
        stationary_wheel_speed_mps=immediate(0x39528, 0x2642) / 4096,
        full_response_speed_mps=immediate(0x396F8, 0x28C2) / 4096,
        # 80039708..3971C constructs speed * ((3*8-1)*32+1).
        response_per_mps=((3 * 8 - 1) * 32 + 1) / 4096))
    def tyre_curve(offset):
        count = car(offset, 'H')
        xs, ys = car(offset + 4, 'I'), car(offset + 8, 'I')
        if not 2 <= count <= 128:
            raise ValueError('Invalid tyre curve point count')
        x = values(xs, count, 'h', 2)
        y = values(ys, count, 'h', 2)
        if any(a >= b for a, b in zip(x, x[1:])):
            raise ValueError('Unordered tyre curve coordinates')
        return x, y
    axles = []
    for offset in [0x194, 0x26C]:
        angles, lateral = tyre_curve(offset)
        slips, longitudinal = tyre_curve(offset + 0x2C)
        limits_x, limits = tyre_curve(offset + 0x6C)
        if limits_x != slips or any(v < 0 for v in limits):
            raise ValueError('Unsupported paired tyre curve or negative cornering limit')
        axles.append(dict(
            slip_curve=[dict(slip=x / 4096, longitudinal=y / 4096, cornering_limit=z / 4096)
                        for x, y, z in zip(slips, longitudinal, limits)],
            cornering_curve=[dict(angle_turns=x / 4096, response=y / 4096) for x, y in zip(angles, lateral)]))
    write('vehicle/tyres.ron', dict(axles=tuple(axles), low_speed=dict(
        tread_limit_mps=immediate(0x3A690, 0x2402) / 4096,
        body_tread_difference_limit_mps=(immediate(0x3A5F0, 0x2862) - 1) / 4096,
        saturation_speed_mps=immediate(0x3A650, 0x28A2) / 4096,
        force_per_lateral_mps=immediate(0x3A684, 0x2404),
        max_force=immediate(0x3A660, 0x2405), fade_per_mps=(((3 * 8 - 1) * 32 + 1) * 4) / 4096)))
    write('vehicle/brake_assist.ron', dict(
        slip_target=(car(0x1FE) / 4096, car(0x2D6) / 4096),
        slip_feedback=(car(0x424, 'i') / 4096, car(0x428, 'i') / 4096),
        grip_feedback=(car(0x42C) / 4096, car(0x42E) / 4096), yaw_target_strength=read(0x46EE8, 'B') / 100))
    table = values(0xA4AC8, 4097, 'h', 2)
    if table[0] != 0 or table[-1] != 512 or any(a > b for a, b in zip(table, table[1:])):
        raise ValueError('Unsupported original direction curve')
    thresholds = [next(i for i, v in enumerate(table) if v >= a) for a in range(1, 513)]
    import bisect
    if any(bisect.bisect_right(thresholds, i) != v for i, v in enumerate(table)):
        raise ValueError('Direction-curve compression changed a source entry')
    write('vehicle/slip.ron', dict(
        stationary_speed_mps=(65536 - immediate(0x39EC0, 0x2462)) / 4096,
        full_speed_mps=immediate(0x39E50, 0x2622) / 4096,
        # 80039EC4..EDC expands x * 13 * 63 * 2 with shifts/adds.
        activation_per_mps=(13 * 63 * 2) / 4096,
        angle_threshold_ratios=[v / 4096 for v in thresholds]))
    write('vehicle/road_contact.ron',dict(
        wheel_levers_m=((-car(0x18)/4096,car(0x0C)/4096),(car(0x18)/4096,car(0x0C)/4096),
                        (-car(0x1A)/4096,-car(0x0E)/4096),(car(0x1A)/4096,-car(0x0E)/4096)),
        wheelbase_m=car(0x10)/4096,track_m=car(0x12)/4096,rear_height_fraction=car(0x14)/4096,
        bump_response_per_mps=immediate(0x43428,0x2405)/4096,
        bumps=[dict(amplitude_m=read(0x46F88+i*2,'h')/4096,cycles_per_m=read(0x46F98+i*2,'h')/256,
                    scale_with_speed=read(0x46FA8+i,'B')!=0) for i in range(8)],
        angle_threshold_ratios=[v/4096 for v in thresholds]))
    write('vehicle/driver_assist.ron', dict(
        wheelbase_m=car(0x10)/4096, radians_per_turn=25736/4096,
        angle_threshold_ratios=[v/4096 for v in thresholds],
        traction_strength=car(0x450,'i')/4096, feedback_fade_per_turn=car(0x454)/4096,
        steering_response_suppression=car(0x456)/4096,
        traction_slip_target=tuple(car(0x1FC+i*0xD8)/4096 for i in range(2)),
        brake_correction_gain=car(0x458)/4096, brake_correction_threshold=car(0x45A)/4096))
    write('vehicle/aerodynamics.ron', dict(
        drag_force_per_squared_mps=car(0x43C,'i')/1024,
        axle_force_per_squared_mps=tuple(car(0x440+i*4,'i')/1024 for i in range(2)),
        minimum_response=read(0x46EF4,'i')/4096,recovery_per_second=0.25))
    write('vehicle/vertical_body.ron', dict(
        pitch_response=tuple(car(0x6C+i*4,'i')/4096/65536 for i in range(2)),
        roll_response=tuple(car(0x74+i*4,'i')/4096/65536 for i in range(2)),
        inverse_mass=car(0x2C)/4096,
        minimum_tilt_rate=-16384/65536, maximum_tilt_rate=16383/65536,
        maximum_tilt_turns=immediate(0x4418C,0x2402)/4096,
        gravity_mps2=40141/4096,
        minimum_travel_m=tuple(car(0x12E+i*0x34)/4096 for i in range(2)),
        travel_offset_m=tuple((car(0x134+i*0x34)-car(0x3E4+i*2))/4096 for i in range(2)),
        wheel_levers_m=((-car(0x18)/4096,car(0x0C)/4096),(car(0x18)/4096,car(0x0C)/4096),
                        (-car(0x1A)/4096,-car(0x0E)/4096),(car(0x1A)/4096,-car(0x0E)/4096)),
        wheelbase_m=car(0x10)/4096, track_m=car(0x12)/4096, rear_height_fraction=car(0x14)/4096,
        angle_threshold_ratios=[v/4096 for v in thresholds],
        reset_height_m=immediate(0x35870,0x2402)/4096, reset_seconds=5.0,
        reset_ramp_seconds=0.5, reset_heading_turns_per_second=341/4096))
    offsets = [v / 65536 for v in values(0x2F350, 8, 'i', 4)]
    focal = read(0x2F370 + 2 * read(0x1C9992, 'B'), 'h')
    outer = BASE - 0x2C
    body = outer + 0x81C
    position = tuple(v / 65536 for v in values(body + 20, 3, 'i', 4))
    reference = dict(position_m=position, rotation_xyzw=quaternion(values(body, 9, 'h', 2)), stabilization=dict(
        yaw_correction_radians=read(outer + 0x766, 'h') * math.tau / 4096,
        pitch_correction_radians=read(outer + 0x768, 'h') * math.tau / 4096))
    camera = dict(bumper_height_m=immediate(0x10430, 0x3402) / 65536,
                  chase_offsets_m=((offsets[0], offsets[2]), (offsets[1], offsets[3])),
                  split_chase_offsets_m=((offsets[4], offsets[6]), (offsets[5], offsets[7])),
                  split_pitch_radians=32 * math.tau / 4096,
                  lens=dict(reference_size=(320.0, 240.0), focal_px=(float(focal), focal * math.floor(240 / 264 * 4096) / 4096),
                            centre_px=(160.0, 120.0), near_m=0.1, far_m=1000.0))
    # RON enums/options are syntax, not strings.
    path = assets / 'race/camera.ron'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('// Generated locally; do not commit.\n' + ron(camera)[:-1] + ', initial_view: Bumper, reference_subject: Some(' + ron(reference) + '))\n')
    export_initial_vehicle(ram, assets)
    tracking = read(0x1C9990, 'B')
    if tracking not in (0, 1):
        raise ValueError('Unsupported chase tracking option')
    write('vehicle/visual.ron', dict(
        body_offset_m=(0.0, 0.0, read(outer + 0x5A, 'h') / 4096),
        chase_heading=RonVariant(['Body', 'Velocity'][tracking]),
        velocity_heading_min_mps=immediate(0x3E9C8, 0x2842) / 4096,
        yaw_stiffness=read(0x46C94 + tracking * 4, 'i') / 4096,
        yaw_damping=read(0x46C9C + tracking * 4, 'i') / 4096,
        pitch_stiffness=((immediate(0x3EAA0, 0x3C05) << 16) | immediate(0x3EAA8, 0x34A5)) / 4096,
        pitch_damping=immediate(0x3EAAC, 0x3405) / 4096,
    ))


def export_initial_vehicle(ram, assets, filename='initial.ron'):
    """Convert one supported checkpoint to authorable native simulation state."""
    if len(ram) != 2 * 1024 * 1024:
        raise ValueError('Expected a complete 2 MiB B-1 RAM dump')
    def raw(off, fmt='h'):
        return struct.unpack_from('<' + fmt, ram, BASE + off)[0]
    def q(off): return raw(off, 'i') / 4096
    def a(off): return raw(off) / 4096
    def b(off): return raw(off, 'B')
    def wheels(off, fn): return tuple(fn(0x460 + i * 0x68 + off) for i in range(4))
    def vector(off): return (q(off), q(off + 8), -q(off + 4))
    def axis(off): return (a(off), a(off + 4), -a(off + 2))
    def basis(off): return dict(forward=axis(off), lateral=axis(off + 8), up=axis(off + 16))
    body, road = basis(0x668), basis(0x68C)
    independent = tuple(b(0x37A + i) == 0 for i in range(2))
    if b(0x372) != 5 or raw(0x128) != 0:
        raise ValueError('Initial state currently supports the ordinary non-induction engine only')
    try:
        engagement = {0: 'Disconnected', 1: 'Coupled', 2: 'Synchronizing', 3: 'FixedClutch', 4: 'Slipping'}[b(0x619)]
        shift_mode = {0: 'Automatic', 1: 'Manual', 2: 'Disabled'}[b(0x642)]
    except KeyError as error:
        raise ValueError('Unsupported checkpoint engine engagement or transmission mode') from error
    state = dict(
        pose=dict(position_m=vector(0x65C), heading_turns=a(0x648)),
        orientation=dict(pitch_turns=a(0x644), roll_turns=a(0x646)),
        velocity=dict(linear_mps=vector(0x628), yaw_turns_per_second=raw(0x64C,'i')/524288),
        step_duration=RonVariant('(' + repr(raw(0x6FE)/65536) + ')'),
        pedals=dict(throttle=a(0x610), brake=a(0x612), handbrake=a(0x614)),
        steering=dict(angle_turns=a(0x60C), rate_turns_per_second=a(0x60E)),
        steering_kinematics=dict(forward_velocity_mps=q(0x6A4), front_slip_turns=(a(0x4B0),a(0x518))),
        local_motion=dict(forward_mps=wheels(0x2C,q), lateral_mps=wheels(0x30,q)),
        wheel_motion=dict(tread_speed_mps=wheels(0x18,q), speed_response=wheels(0x2A,a)),
        tyre_contact=dict(body_forward_mps=q(0x6A4), grip_capacity=wheels(0x34,lambda o:float(raw(o,'i')))),
        grip_input=dict(load=wheels(8,lambda o:float(raw(o,'i'))), surface=wheels(0x14,b),
                        roll_difference_turns=a(0x646)-a(0x6F6), bypass_condition=bool(b(0x78D)&16)),
        tyre_condition=dict(accumulator=wheels(0x64,lambda o:float(raw(o,'i'))), load_scale=wheels(0x38,a),
                            indicator=wheels(0x3F,lambda o:raw(o,'b'))),
        engine=dict(speed_rev_s=q(0x624),integer_rpm=float(raw(0x6AC)),effective_throttle=a(0x708)),
        fuel_cut=dict(active=b(0x61D)!=0),
        transmission=dict(mode=RonVariant(shift_mode),force_automatic=b(0x786)!=0),
        transmission_state=dict(first_gear_ticks=raw(0x78E,'H'),engagement_requested=raw(0x60A)!=0,cooldown_ticks=b(0x61C)),
        drivetrain=dict(gear=b(0x618),engagement=RonVariant(engagement),adaptive_angle_turns=a(0x64A),
                        brake_travel=a(0x612),alternate_locked_step=ram[0x1D5866]==3,independent_wheels=independent),
        drivetrain_state=dict(previous_angular_delta=(a(0x4DE),a(0x5AE)),centre_locked=b(0x63C)!=0),
        axles=dict(radians_per_second=(q(0x634),q(0x638))),
        wheel_state=dict(previous_delta_mps=wheels(0x16,a),clutch_locked=(b(0x61A)!=0,b(0x61B)!=0)),
        body_input=dict(bypass_tyre_acceleration=b(0x6B1)!=0,reset_motion=b(0x718)!=0 or b(0x786)==7,
                        first_gear_override=raw(0x78E)!=0,braked_wheels=wheels(0x63,lambda o:bool(b(o)&2)),
                        forward_axis=(body['forward'][0],body['forward'][2]), lateral_axis=(body['lateral'][0],body['lateral'][2]),
                        lateral_resistance=float(raw(0x730,'i')),external_world_force=(float(raw(0x778,'i')),-float(raw(0x77C,'i')))),
        body_dynamics=dict(local_lateral_mps=q(0x6A8),lateral_acceleration_mps2=q(0x728),lateral_feedback_turns=a(0x64A),
                           tilt_turns_per_second=(raw(0x650)/65536,raw(0x652)/65536)),
        aerodynamics_input=dict(forward_mps=q(0x6A4),reduction_growth_per_second=a(0x774)),
        aerodynamics=dict(reduction=a(0x776)), road_input=dict(),
        road=dict(node=raw(0x600,'H'),raw_height_m=wheels(4,lambda o:raw(o,'i')/65536),height_m=wheels(0,q),
                  surface=wheels(0x14,b),variant=wheels(0x15,b),attachment_m=tuple(axis(0x484+i*0x68) for i in range(4)),
                  road_offset_m=wheels(0x42,a),bump_response=wheels(0x40,a),body_basis=body,road_basis=road,
                  road_height_m=q(0x688),road_pitch_turns=a(0x6F4),road_roll_turns=a(0x6F6),bypass_condition=bool(b(0x78D)&16)),
        suspension_input=dict(body_height_m=q(0x664),road_height_m=wheels(0,q),attachment_height_m=wheels(0x28,a),
                              road_offset_m=wheels(0x42,a),road_normal_up=road['up'][1],body_up=body['up'][1],
                              inverse_step_seconds=q(0x704),axle_force=(float(raw(0x71C,'i')),float(raw(0x720,'i'))),
                              lateral_acceleration_mps2=q(0x728),vertical_force=float(raw(0x72C,'i')),
                              forward_force=float(raw(0x734,'i')),disabled=b(0x786)==7),
        suspension=dict(travel_m=wheels(0x10,a),travel_rate_mps=wheels(0x12,a),tyre_load=wheels(8,lambda o:float(raw(o,'i')))),
        vertical_input=dict(),vertical=dict(airborne=b(0x6B1)!=0),
        contact_input=dict(forward_axis=road['forward'],lateral_axis=road['lateral'],
                           synchronize_wheels=wheels(0x63,lambda o:b(o)==0),independent_axles=independent,
                           wheel_heading_turns=wheels(0xC,a)),
        contact_motion=dict(body_forward_mps=q(0x6A4),body_lateral_mps=q(0x6A8),speed_readout=raw(0x6AE,'H'),
                            maximum_speed_readout=raw(0x6F8,'H'),spin_turns=wheels(0x20,lambda o:raw(o,'H')/4096)),
        chase_stabilization=RonVariant('Some(' + ron(dict(yaw_turns=a(0x748),yaw_rate_turns_per_second=a(0x74A),
            pitch_turns=a(0x74C),pitch_rate_turns_per_second=a(0x74E),previous_yaw_acceleration=a(0x752))) + ')'),
    )
    path = assets / 'vehicle' / filename
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text('// Generated locally from the supplied checkpoint; do not commit.\n' + ron(state) + '\n')


def export_scene_setup(start_ram, assets):
    """Select the actual B-1 start, or explicit free driving for a reused checkpoint."""
    setup = dict(initial_state=RonVariant('Some("vehicle/initial.ron")'), session=RonVariant('None'))
    if start_ram is not None:
        if len(start_ram) != 2 * 1024 * 1024:
            raise ValueError('Expected a complete countdown RAM dump')
        fields = struct.unpack_from('<H', start_ram, 0xA9520)[0]
        clock = struct.unpack_from('<I', start_ram, 0x46F64)[0]
        if start_ram[0x1D5866] != 3 or start_ram[0xAF231] != 1 or fields == 0 or clock % 100:
            raise ValueError('Expected an ordinary single-car B-1 countdown at a complete physics tick')
        export_initial_vehicle(start_ram, assets, 'start.ron')
        session = dict(phase=RonVariant(f'Countdown(fields_left: {fields})'), elapsed_ticks=clock // 100)
        setup = dict(initial_state=RonVariant('Some("vehicle/start.ron")'), session=RonVariant('Some(' + ron(session) + ')'))
    path = assets / 'race/vehicle.ron'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('// Generated locally from the supplied checkpoints; do not commit.\n' + ron(setup) + '\n')


def write_rgba_png(path, width, height, rgba):
    def chunk(kind, data):
        return struct.pack('>I', len(data))+kind+data+struct.pack('>I', zlib.crc32(kind+data))
    rows=b''.join(b'\0'+rgba[i*width*4:(i+1)*width*4] for i in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,6,0,0,0))
                     +chunk(b'IDAT',zlib.compress(rows))+chunk(b'IEND',b''))


def export_gauges(ram, pack, assets):
    """8002BD84/8002E390 select a TIM dial; export ordinary sprites and design values."""
    def u32(off):return struct.unpack_from('<I',ram,off)[0]
    def u16(off):return struct.unpack_from('<H',ram,off)[0]
    def imm(off):
        assert u32(off)>>26 in (9,10,13,15)
        return struct.unpack('<h',struct.pack('<H',u32(off)&65535))[0]
    def rgb(hi,lo):
        n=((u32(hi)&65535)<<16)|(u32(lo)&65535)
        return (n&255,(n>>8)&255,(n>>16)&255)
    def color(n):return (n&255,(n>>8)&255,(n>>16)&255)
    assert struct.unpack_from('<I',pack)[0]==11
    offsets=struct.unpack_from('<11I',pack,4)
    assert offsets[0]==48 and sorted(offsets)==list(offsets)
    def bitmap(index):
        begin=offsets[index];end=offsets[index+1] if index<10 else len(pack)
        magic,flags=struct.unpack_from('<II',pack,begin);assert magic==16 and flags in (0,8)
        at=begin+8
        if flags&8:at+=struct.unpack_from('<I',pack,at)[0]
        length,x,y,w,h=struct.unpack_from('<I4H',pack,at)
        assert at+length==end and length==12+2*w*h
        return w,h,pack[at+12:end]
    w,h,base=bitmap(0);assert (w,h)==(64,234)
    texels=bytearray(base)
    maximum=u16(BASE-0x2C+0x134);redline=u16(BASE-0x2C+0x3C2)
    thousands=(maximum+999)//1000
    thresholds=struct.unpack_from('<11b',ram,0x2F868)
    selected=next(i for i,n in enumerate(thresholds) if n<0 or n>=thousands)+1
    w,h,dial=bitmap(selected);assert (w,h)==(20,80)
    for y in range(h):texels[y*128:y*128+40]=dial[y*40:(y+1)*40]
    gears=struct.unpack_from('<7b',ram,0x2F874)
    descriptors=[0x2F630]+[0x2F678+i*12 for i in range(10)]+[0x2F6FC]+[0x2F708+i*12 for i in gears]
    atlas_width,atlas_height=480,384;rgba=bytearray(atlas_width*atlas_height*4);sprites=[]
    for slot,off in enumerate(descriptors):
        uv_clut,sw,sh,page=struct.unpack_from('<I3H',ram,off)
        assert page==8 and sw<=96 and sh<=96
        u,v=uv_clut&255,(uv_clut>>8)&255;clut=uv_clut>>16
        cx,cy=(clut&63)*16,(clut>>6)
        assert 512<=cx<576 and cy<234 and u+sw<=256 and v+sh<=234
        x,y=(slot%5)*96,(slot//5)*96;sprites.append((x,y,sw,sh))
        for dy in range(sh):
            for dx in range(sw):
                q=u+dx;idx=(texels[(v+dy)*128+q//2]>>(4*(q%2)))&15
                col=struct.unpack_from('<H',texels,cy*128+(cx-512+idx)*2)[0]
                expand=lambda n:((n&31)<<3)|((n&31)>>2)
                pixel=bytes([expand(col),expand(col>>5),expand(col>>10),0 if not col else 128 if col&0x8000 else 255])
                at=((y+dy)*atlas_width+x+dx)*4;rgba[at:at+4]=pixel
    needles=[]
    for off in [0x2F898,0x2F880]: # actual draw order is the reverse of packet construction
        a,b,x0,x1,w0,w1=struct.unpack_from('<II4i',ram,off)
        needles.append(dict(distance_px=(x0/16,x1/16),half_width_px=(w0/16,w1/16),edge_rgb=(color(a),color(b))))
    settings=dict(image='race/gauges.png',sprites=sprites,center_px=(float(imm(0x2E754)),float(imm(0x2E75C))),
                  maximum_rpm=float(thousands*1000),redline_rpm=float(redline),sections_per_1000_rpm=float(4),
                  sweep_turns=2412/4096,zero_turns=imm(0x2C24C)/4096,
                  outer_radius_px=imm(0x2BE9C)/16,inner_radius_px=imm(0x2BE48)/16,needles=needles,
                  band_rgb=rgb(0x2C0CC,0x2C0E4),redline_rgb=rgb(0x2C130,0x2C134),
                  dial_rgb=rgb(0x2C030,0x2C034),text_rgb=rgb(0x2C3B4,0x2C3BC),
                  coupled_gear_rgb=rgb(0x2C278,0x2C288),free_gear_rgb=rgb(0x2C28C,0x2C290),
                  gear_offset_px=(23.0,2.0),units_offset_px=(18.0,40.0),speed_offset_px=(2.0,24.0),
                  digit_step_px=float(imm(0x2C52C)),gear_shade_offset_px=(float(imm(0x2C38C)),float(imm(0x2C394))),
                  gear_shade_size_px=(float(imm(0x2C39C)),float(imm(0x2C3A4))),gear_shade_rgb=rgb(0x2C378,0x2C380))
    folder=assets/'race';folder.mkdir(parents=True,exist_ok=True)
    write_rgba_png(folder/'gauges.png',atlas_width,atlas_height,rgba)
    (folder/'gauges.ron').write_text('// Generated locally from disc TIMs and gauge routines; do not commit.\n'+ron(settings)+'\n')


def licence_definitions(ram):
    """Decode the GTDT licence table and its local length-prefixed key strings."""
    def u32(off):return struct.unpack_from('<I',ram,off)[0]
    root=u32(0x92E6C)&0x1FFFFF
    assert ram[root:root+4]==b'GTDT'
    tables=struct.unpack_from('<H',ram,root+6)[0]
    assert tables==64
    pointer,stride,count=struct.unpack_from('<IHH',ram,root+0xF8)
    pointer&=0x1FFFFF
    assert stride==156 and count==60
    strings=u32(root+0xF8+(tables//2)*8)&0x1FFFFF
    n=struct.unpack_from('<H',ram,strings)[0];strings+=2;keys=[]
    for _ in range(n):
        length=ram[strings];keys.append(ram[strings+1:strings+1+length].decode('ascii'))
        assert ram[strings+1+length]==0
        strings+=length+2
    definitions={}
    for i in range(count):
        row=pointer+i*stride;key=keys[struct.unpack_from('<H',ram,row)[0]]
        limits=[]
        for tier in range(4):
            seconds,centis=ram[row+0x6C+tier*2:row+0x6E+tier*2]
            minutes,seconds=divmod(seconds,100)
            assert seconds<60 and centis<100
            limits.append(minutes*60000+seconds*1000+centis*10)
        assert sorted(limits)==limits and key not in definitions
        definitions[key]=dict(id=key,time_limits_ms=tuple(limits),assisted_pass_attempts=ram[row+0x74])
    return definitions


def export_records(ram,assets):
    def u32(off):return struct.unpack_from('<I',ram,off)[0]
    def imm(off):
        assert u32(off)>>26 in (9,10,13,15)
        return struct.unpack('<h',struct.pack('<H',u32(off)&65535))[0]
    def string(off):return ram[off:ram.index(0,off)].decode('ascii')
    def rgb(hi,lo):
        n=((u32(hi)&65535)<<16)|(u32(lo)&65535)
        return (n&255,(n>>8)&255,(n>>16)&255)
    current=0x1D585C
    assert ram[current+10]==3,'licence records panel required'
    key=string(current+16)
    definitions=licence_definitions(ram)
    keys=list(definitions)
    assert keys[ram[current+11]*10+ram[current+12]]==key,'GTDT and saved licence order disagree'
    definition=dict(definitions[key])
    definition['assisted_pass_attempts']=RonVariant(f"Some({definition['assisted_pass_attempts']})")
    achievements={}
    grades=['Unpassed','AssistedPass','Bronze','Silver','Gold']
    for i, licence_id in enumerate(keys):
        offset=0x1C98E0+0x1418+i*164
        grade,attempts=ram[offset+1:offset+3]
        assert grade<len(grades),'unsupported saved licence grade'
        if grade or attempts:
            achievements[licence_id]=dict(grade=RonVariant(grades[grade]),assisted_attempts=attempts)
    progress='{'+', '.join(json.dumps(k)+': '+ron(v) for k,v in achievements.items())+'}'
    best_offset=0x1C98E0+0x1418+ram[current+11]*1640+ram[current+12]*164+4
    time=u32(best_offset)
    best='{}' if time==0xFFFFFFFF else '{'+json.dumps(key)+': '+ron(dict(time_ms=time,speed_readout=struct.unpack_from('<H',ram,best_offset+16)[0]))+'}'
    descriptor=0x2F8E0
    gradient=tuple(tuple(ram[descriptor+8+i*4:descriptor+11+i*4]) for i in range(2))
    settings=dict(font='race/fonts/small.ron',label=string(0x1C6C72),missing_time=string(0x91AAC),units=string(0x1C6C87),
        origin_px=(float(imm(0x2E740)),float(imm(0x2E748))),label_offset_px=(0.0,float(imm(0x2D550))),
        time_offset_px=(float(imm(0x2D094)),float(imm(0x2D430))),speed_offset_px=(0.0,float(imm(0x2D430)+imm(0x2D0F4))),
        target_offsets_px=tuple((0.0,float(imm(off))) for off in [0x2D4C4,0x2D4F8,0x2D52C]),text_rgb=rgb(0x2D390,0x2D394),
        underline_offset_px=(float(imm(0x2D574)),float(imm(0x2D584)-imm(0x2D550))),
        underline_size_extra_px=(float(imm(0x2D57C)),float(struct.unpack_from('<h',ram,descriptor+6)[0])),underline_rgb=gradient,
        marker_rgb=tuple(rgb(hi,lo) for hi,lo in [(0x2D4AC,0x2D4B0),(0x2D4E0,0x2D4E4),(0x2D514,0x2D518)]),
        marker_offset_px=(float(imm(0x2D194)),float(imm(0x2D1B0))),marker_inner_px=float(imm(0x2D1B8)),marker_border_px=float(-imm(0x2D1D0)),
        cells=dict(digit_px=float(imm(0x2D09C)),punctuation_px=float(imm(0x2D0A4)),decimal_inset_px=float(imm(0x2D0AC)),origin_inset_px=ram[0x93124+8]/2),
        metadata_digit_px=float(ram[0x93124+8]+imm(0x2D100)),metadata_digit_inset_px=float(imm(0x2D0F8)-ram[0x93124+8]/2))
    folder=assets/'race';folder.mkdir(parents=True,exist_ok=True)
    for name,value in [('licence.ron',definition),('records-panel.ron',settings),('records.initial.ron',dict(best=RonVariant(best))),
                       ('licence-progress.initial.ron',dict(achievements=RonVariant(progress)))]:
        (folder/name).write_text('// Generated locally from supplied checkpoint data; do not commit.\n'+ron(value)+'\n')
    assert ram[0x1C98A2]==2,'stopping licence setup required'
    stop=dict(start_m=float(ram[0x1C98C4]*10),end_m=float(ram[0x1C98C4]*10+ram[0x1C98C5]),
              stop_speed_mps=float(imm(0x3D6C8))/4096)
    (folder/'licence-stop.ron').write_text('// Generated locally from selected licence setup; do not commit.\n'+ron(stop)+'\n')


def export_result_timing(ram, assets):
    """8002A780..87C: timings stay in ticks because the waits are exact."""
    def immediate(off):
        instruction=struct.unpack_from('<I',ram,off)[0]
        assert instruction>>26==9 and (instruction>>21)&31==0
        return instruction&65535
    timing=dict(success_wait_ticks=immediate(0x2A7A0),failure_wait_ticks=immediate(0x2A78C),
                held_updates_per_tick=immediate(0x2A790),prize_after_ticks=immediate(0x2A798))
    assets.mkdir(parents=True,exist_ok=True)
    (assets/'licence-results.ron').write_text('// Generated locally from original result timing; do not commit.\n'+ron(timing)+'\n')


def export_result_presentation(ram, tim, assets):
    """8002AB60/8002B170: decoded fonts, logo sprites and animation tuning."""
    def word(off):return struct.unpack_from('<I',ram,off)[0]
    def imm(off):return word(off)&65535
    def text(off):return ram[off:ram.index(0,off)].decode('ascii')
    def rgb(value):return tuple((value>>shift)&255 for shift in (0,8,16))
    assert struct.unpack_from('<II',tim)==(16,0)
    block,x,y,w,h=struct.unpack_from('<I4H',tim,8)
    assert (block,w,h)==(12+128*170*2,128,170) and len(tim)==8+block
    texels=tim[20:]
    # The original uploads this complete TIM to (384,256); runtime output
    # contains four independent RGBA images, with no VRAM coordinates.
    (assets/'race/results').mkdir(parents=True,exist_ok=True)
    logos=[]
    for slot,descriptor in enumerate((0x5B1B0,0x5B1A4,0x5B198,0x5B18C)):
        value,sw,sh,page=struct.unpack_from('<I3H',ram,descriptor)
        u=(value&255)+((page&15)*64-384)*4;v=((page>>4)&1)*256+((value>>8)&255)-256
        cx=((value>>16)&63)*16-384;cy=(value>>22)-256
        assert 0<=u and u+sw<=w*4 and 0<=v and v+sh<=168 and 0<=cx and cx+16<=w and 168<=cy<h
        rgba=bytearray()
        for dy in range(sh):
            for dx in range(sw):
                q=u+dx;index=(texels[(v+dy)*w*2+q//2]>>(4*(q%2)))&15
                color=struct.unpack_from('<H',texels,(cy*w+cx+index)*2)[0]
                expand=lambda n:((n&31)<<3)|((n&31)>>2)
                rgba+=bytes((expand(color),expand(color>>5),expand(color>>10),0 if color==0 else 128 if color&0x8000 else 255))
        image=f'race/results/prize-{slot}.png'
        write_rgba_png(assets/image,sw,sh,rgba)
        logos.append(dict(image=image,size_px=(float(sw),float(sh))))
    prizes=[]
    for i,(label,color) in enumerate(((0x1C708B,0x2F62C),(0x1C8488,0x2F628),(0x1C703C,0x2F628),(0x1C7051,0x2F624),(0x1C7065,0x2F620))):
        prizes.append(dict(text=text(label),rgb=rgb(word(color)),logo=None if i==0 else RonVariant(f'Some({i-1})')))
    settings=dict(large_font='race/fonts/large.ron',time_font='race/fonts/medium.ron',
        finish_text=text(0x1C6C99),failure_text=text(0x1C708B),
        finish_baseline=(float(imm(0x2ACB0)),float(imm(0x2ACB4))),
        failure_baseline=(float(imm(0x2B63C)),float(imm(0x2B63C))),failure_rgb=rgb(imm(0x2B648)),
        finish_rgb=rgb(word(0x2F608)),finish_peak=ram[0x2F60C],
        finish_fade_after_ticks=65536-imm(0x2ABC8),finish_fade_ticks=imm(0x2ABFC),
        finish_echo_px=8.0,finish_until_ticks=imm(0x2AB74),
        shade_after_ticks=65536-imm(0x2B610),shade_ticks=imm(0x2B624),shade_peak=64,
        prize_spacing_px=float(struct.unpack_from('<h',ram,0x2B42C)[0]),prize_fade_ticks=imm(0x2B3F8),prize_initial_rgb=rgb(word(0x2F61C)),
        prize_baseline=(float(imm(0x2B420)),float(imm(0x2B424))),
        time_baseline=(float(imm(0x2B460)),float(imm(0x2B464))),
        time_cells=dict(digit_px=float(imm(0x2B468)),punctuation_px=float(imm(0x2B470)),decimal_inset_px=0.0,origin_inset_px=ram[0x93130+8]/2),
        logo_center_px=(float(imm(0x2B544)),float(imm(0x2B568))),logo_curve=list(ram[0x2F5E8:0x2F5F9]),
        logo_initial_rgb=rgb(word(0x2F604)),logo_final_rgb=rgb(word(0x2F5FC)),logo_settled_rgb=rgb(word(0x2F600)),
        prizes=tuple(prizes),logos=logos)
    (assets/'race/result-presentation.ron').write_text('// Generated locally from result routines and license_tim.tim; do not commit.\n'+ron(settings)+'\n')


def export_hud_fonts(ram, font, assets):
    """Decode ASCII glyphs/kerning (8007DC78/8007DD3C) and the raw font bitmap."""
    if len(font) != 65536 or len(ram) != 2*1024*1024:
        raise ValueError('Expected the 512x256 4-bit race font and complete RAM')
    def word(off): return struct.unpack_from('<I',ram,off)[0]
    def char(c): return "'"+c.replace('\\','\\\\').replace("'","\\'")+"'"
    folder=assets/'race/fonts';folder.mkdir(parents=True,exist_ok=True)
    for name,descriptor in [('small',0x93124),('medium',0x93130),('large',0x9313C)]:
        table,kern=word(descriptor)&0x1FFFFF,word(descriptor+4)&0x1FFFFF
        spacing=ram[descriptor+9]
        header=word(kern);stride=header&255;bits=(header>>16)&255
        assert bits in (4,5,6) and stride>0
        def reduction(c,n):
            a=word(table+ord(c)*8);b=word(table+ord(n)*8+4)
            if a&0x80000000:return (a>>23)&63
            if b&0x80000000:return (b>>23)&63
            row,col=(a>>23)&255,(b>>23)&255
            offset=kern+4+row*stride+col*bits//8
            return (word(offset)>>(col*bits%8))&((1<<bits)-1)
        glyphs=[];pairs=[];width,height=512,192;rgba=bytearray(width*height*4)
        for slot,c in enumerate(map(chr,range(32,127))):
            a,b=word(table+ord(c)*8),word(table+ord(c)*8+4);index=(a>>6)&255
            x,y=(slot%16)*32,(slot//16)*32;w=h=0
            assert (a>>14)&7==0,'decorated glyphs require their separate layer'
            if index:
                t=word(table+0x818+index*4);w=2*((t>>16)&31);h=(t>>21)&63
                assert w<=32 and h<=32
                u=(t&255)+(t>>29)*256;v=(t>>8)&255;palette=((t>>27)&3)*16
                assert u+w<=512 and v+h<=256
                for dy in range(h):
                    for dx in range(w):
                        q=u+dx;packed=font[(v+dy)*256+q//2];idx=(packed>>(4*(q%2)))&15
                        color=struct.unpack_from('<H',font,(palette+idx)*2)[0]
                        expand=lambda n:((n&31)<<3)|((n&31)>>2)
                        alpha=0 if not color else 128 if color&0x8000 else 255
                        pixel=bytes([expand(color),expand(color>>5),expand(color>>10),alpha])
                        off=((y+dy)*width+x+dx)*4;rgba[off:off+4]=pixel
            glyph=dict(rect=(x,y,w,h),bearing_px=(float(b&63),-float((b>>6)&63)),
                       advance_px=float((a&63)+spacing-reduction(c,'\0')),cell_inset_px=float((a>>17)&63))
            glyphs.append(char(c)+': '+ron(glyph))
            for n in map(chr,range(32,127)):
                adjustment=reduction(c,'\0')-reduction(c,n)
                if adjustment:pairs.append(dict(pair=RonVariant('('+char(c)+','+char(n)+')'),adjustment_px=float(adjustment)))
        write_rgba_png(folder/f'{name}.png',width,height,rgba)
        native=dict(image=f'race/fonts/{name}.png',glyphs=RonVariant('{'+','.join(glyphs)+'}'),kerning=pairs)
        (folder/f'{name}.ron').write_text('// Generated locally; ASCII font metrics and bitmap from the supplied disc.\n'+ron(native)+'\n')
    def string(off):return ram[off:ram.index(0,off)].decode('ascii')
    def immediate(off):
        assert word(off)>>26 in (9,10,13,15),f'expected an immediate instruction at {off:08x}'
        return word(off)&65535
    def rgb(hi,lo):
        assert word(hi)>>26==15 and word(lo)>>26==13
        n=(immediate(hi)<<16)|immediate(lo)
        return (n&255,(n>>8)&255,(n>>16)&255)
    # 8002C76C's ordinary B-1 clock/label; 8002A19C's announcement.
    origin=(immediate(0x2E7EC),immediate(0x2E7F4))
    settings=dict(small_font='race/fonts/small.ron',large_font='race/fonts/large.ron',label=string(0x1C6C64),start_text=string(0x1C6C90),
                  label_baseline=(float(origin[0]+immediate(0x2CB44)),float(origin[1]+immediate(0x2CB34))),
                  clock_baseline=(float(origin[0]+immediate(0x2CD74)),float(origin[1]+immediate(0x2CB34)+immediate(0x2CB7C))),
                  announcement_baseline=(float(immediate(0x2A368)),float(immediate(0x2A36C))),
                  text_rgb=rgb(0x2C928,0x2C92C),digit_rgb=rgb(0x2A260,0x2A264),start_rgb=rgb(0x2A294,0x2A298),
                  announcement_after_release_fields=65536-immediate(0x2A23C),full_banner_until_fields=immediate(0x2A290),fade_fields=immediate(0x2A2A8),
                  digit_cell_px=float(immediate(0x2CD7C)),punctuation_cell_px=float(immediate(0x2CD84)),decimal_inset_px=float(immediate(0x2CD8C)),
                  number_origin_inset_px=ram[0x93124+8]/2,fade_peak=immediate(0x2A340))
    (assets/'race/hud.ron').write_text('// Generated locally from the B-1 HUD routines; do not commit.\n'+ron(settings)+'\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('disc', type=Path, help='USA Simulation Mode Rev 2 .cue/.bin/.iso')
    parser.add_argument('--bios', type=Path, help='User-supplied NTSC-U BIOS; otherwise psx local discovery')
    parser.add_argument('--psx', default='psx', help='Toolkit executable used for capture and provenance')
    parser.add_argument('--checkpoint', type=Path, help='Reuse a B-1 ram.bin instead of replaying the original')
    parser.add_argument('--start-checkpoint', type=Path, help='Use this countdown ram.bin for scene startup when reusing --checkpoint')
    parser.add_argument('--assets', type=Path, default=ROOT / 'game/assets')
    parser.add_argument('--work', type=Path, default=ROOT / '.psx/asset-export')
    args = parser.parse_args()
    if args.start_checkpoint and not args.checkpoint:
        parser.error('--start-checkpoint requires --checkpoint; a full replay captures both states automatically')
    disc, assets, work = args.disc.resolve(strict=True), args.assets.resolve(), args.work.resolve()
    if not shutil.which('cargo') or not shutil.which(args.psx):
        parser.error('cargo and psx must be installed locally; this script downloads neither')
    # Reject alternate paths that would bypass the source-only Git exclusions.
    for folder in [assets, work]:
        if folder.is_relative_to(ROOT) and not (folder.is_relative_to(ROOT / 'game/assets') or folder.is_relative_to(ROOT / '.psx')):
            parser.error('Project-local output must be under ignored game/assets or .psx')
    work.mkdir(parents=True, exist_ok=True)
    run(['cargo', 'build', '-p', 'import', '--all-features'])
    importer = ROOT / 'target/debug/import'
    if args.checkpoint:
        ram = args.checkpoint.resolve(strict=True)
        start_ram = args.start_checkpoint.resolve(strict=True) if args.start_checkpoint else None
    else:
        ram, start_ram = capture(disc, args.bios.resolve(strict=True) if args.bios else None, work, args.psx)
    files = {}
    for name, source in [('course.tro', '/crsobj/TC_lisence.tro.gz'), ('course.trp', '/crsobj/TC_lisence.trp.gz'),
                         ('t2vzn.cdo', '/carobj/t2vzn.cdo.gz'), ('t2vzn.cdp', '/carobj/t2vzn.cdp.gz'),
                         ('racefont.dat', '/font/racefont.dat'), ('license_tim.tim', '/arcade/license_tim.tim'), ('game_status_files', '/arcade/game_status_files.gz')]:
        path = work / name
        run([importer, 'extract', disc, '--path', source, '--out', path])
        raw = path.read_bytes()
        if raw.startswith(b'\x1f\x8b'):
            path.write_bytes(gzip.decompress(raw))
        files[name] = path
    run([importer, 'tro', files['course.tro'], '--out', assets / 'gt2/course/TC_lisence.ron', '--name', '/crsobj/TC_lisence.tro.gz'])
    run([importer, 'tro-scene', files['course.tro'], '--textures', files['course.trp'], '--out', assets / 'gt2/course/TC_lisence/scene.ron', '--texture-prefix', 'gt2/course/TC_lisence'])
    run([importer, 'car', files['t2vzn.cdo'], '--textures', files['t2vzn.cdp'], '--paint', 5, '--lod', 0, '--out', assets / 'gt2/vehicle/t2vzn.ron', '--texture-prefix', 'gt2/vehicle'])
    run([importer, 'wheels', '--ram', ram, '--textures', files['t2vzn.cdp'], '--paint', 5, '--out', assets / 'gt2/vehicle/wheels', '--texture-prefix', 'gt2/vehicle/wheels'])
    export_tuning(ram, assets)
    export_scene_setup(start_ram.read_bytes() if start_ram else None, assets)
    export_hud_fonts(ram.read_bytes(),files['racefont.dat'].read_bytes(),assets)
    export_gauges(ram.read_bytes(),files['game_status_files'].read_bytes(),assets)
    export_records(ram.read_bytes(),assets)
    export_result_timing(ram.read_bytes(),assets/'race')
    export_result_presentation(ram.read_bytes(),files['license_tim.tim'].read_bytes(),assets)
    manifest = dict(disc=str(disc), ram_sha256=hashlib.sha256(ram.read_bytes()).hexdigest(),
                    race_font_sha256=hashlib.sha256(files['racefont.dat'].read_bytes()).hexdigest(),
                    gauge_pack_sha256=hashlib.sha256(files['game_status_files'].read_bytes()).hexdigest(),
                    start_ram_sha256=hashlib.sha256(start_ram.read_bytes()).hexdigest() if start_ram else None,
                    startup='countdown' if start_ram else 'free-driving checkpoint',
                    psx_version=subprocess.check_output([args.psx, '--version'], text=True).strip(),
                    scope='Current B-1 runtime assets; unused text, audio and all-game content are not needed yet',
                    files=[dict(path=str(p.relative_to(assets)), sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                           for p in sorted(assets.rglob('*')) if p.is_file() and p.name != 'export-manifest.json'])
    (assets / 'export-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Export complete: {assets}. Generated data must remain private.', flush=True)


if __name__ == '__main__':
    main()
