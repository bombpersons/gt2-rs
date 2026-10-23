#!/usr/bin/env python3
"""Build the current B-1 runtime assets from a user-supplied USA Rev 2 disc.

No assets, BIOS or emulator are downloaded. The psx CLI captures a local B-1
checkpoint for wheel profiles and car/camera tuning; the native game never
emulates the original. All outputs are private, ignored by Git. Python stdlib
only, plus cargo and psx >=0.3.2. See docs/assets.md.
"""
import argparse
import gzip
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BASE = 0xA96B4


class RonVariant(str):
    """Authored enum syntax; values still come from the supplied checkpoint."""


def run(command):
    print('+ ' + ' '.join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), cwd=ROOT, check=True)


def capture(disc, bios, work):
    previous = None
    hops = [
        ('list', 3000, json.loads((ROOT / 'docs/checkpoints/poweron-to-b1-list.json').read_text())),
        ('menu', 1800, [{'frame': 10, 'buttons': ['CROSS'], 'until': 16}]),
        ('transmission', 1500, [{'frame': 10, 'buttons': ['CROSS'], 'until': 16}]),
        ('driving', 1200, [{'frame': 10, 'buttons': ['CROSS'], 'until': 16},
                           {'frame': 400, 'buttons': ['CROSS']}]),
    ]
    for name, frames, replay in hops:
        folder = work / name
        command = ['psx', 'run', '--frames', frames, '--input', json.dumps(replay),
                   '--save-state', '--out', folder, '--summary']
        if bios:
            command += ['--bios', bios]
        command += ['--from-state', previous / 'state.bin'] if previous else [disc]
        run(command)
        report = json.loads((folder / 'report.json').read_text())
        if report['status'] != 'complete' or report['frames']['reached'] != frames:
            raise RuntimeError(f'Incomplete original run: {folder}')
        previous = folder
    return previous / 'ram.bin'


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('disc', type=Path, help='USA Simulation Mode Rev 2 .cue/.bin/.iso')
    parser.add_argument('--bios', type=Path, help='User-supplied NTSC-U BIOS; otherwise psx local discovery')
    parser.add_argument('--checkpoint', type=Path, help='Reuse a B-1 ram.bin instead of replaying the original')
    parser.add_argument('--assets', type=Path, default=ROOT / 'game/assets')
    parser.add_argument('--work', type=Path, default=ROOT / '.psx/asset-export')
    args = parser.parse_args()
    disc, assets, work = args.disc.resolve(strict=True), args.assets.resolve(), args.work.resolve()
    if not shutil.which('cargo') or not shutil.which('psx'):
        parser.error('cargo and psx must be installed locally; this script downloads neither')
    # Reject alternate paths that would bypass the source-only Git exclusions.
    for folder in [assets, work]:
        if folder.is_relative_to(ROOT) and not (folder.is_relative_to(ROOT / 'game/assets') or folder.is_relative_to(ROOT / '.psx')):
            parser.error('Project-local output must be under ignored game/assets or .psx')
    work.mkdir(parents=True, exist_ok=True)
    run(['cargo', 'build', '-p', 'import', '--all-features'])
    importer = ROOT / 'target/debug/import'
    ram = args.checkpoint.resolve(strict=True) if args.checkpoint else capture(disc, args.bios.resolve(strict=True) if args.bios else None, work)
    files = {}
    for name, source in [('course.tro', '/crsobj/TC_lisence.tro.gz'), ('course.trp', '/crsobj/TC_lisence.trp.gz'),
                         ('t2vzn.cdo', '/carobj/t2vzn.cdo.gz'), ('t2vzn.cdp', '/carobj/t2vzn.cdp.gz')]:
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
    manifest = dict(disc=str(disc), ram_sha256=hashlib.sha256(ram.read_bytes()).hexdigest(),
                    psx_version=subprocess.check_output(['psx', '--version'], text=True).strip(),
                    scope='Current B-1 runtime assets; unused text, audio and all-game content are not needed yet',
                    files=[dict(path=str(p.relative_to(assets)), sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                           for p in sorted(assets.rglob('*')) if p.is_file() and p.name != 'export-manifest.json'])
    (assets / 'export-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Export complete: {assets}. Generated data must remain private.', flush=True)


if __name__ == '__main__':
    main()
