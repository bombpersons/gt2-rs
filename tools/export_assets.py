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
    write('vehicle/braking.ron', dict(front_capacity=car(0x414, 'i'), rear_capacity=car(0x418, 'i'), handbrake_capacity=car(0x41C, 'i')))
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
