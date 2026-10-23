#!/usr/bin/env python3
"""Capture README images from the native game and handmade example mod.

Build first: cargo build --workspace --all-features
Requires locally exported B-1 assets and a software GPU adapter. No emulator
is launched. PNGs come directly from game_screenshot, without image editing.
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import struct
import subprocess
import threading

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'docs/images'
WORK = ROOT / '.psx/readme-capture'


def capture(binary, asset_root, name, size, look, chase):
    folder = WORK / name
    folder.mkdir(parents=True, exist_ok=True)
    profile = folder / 'display.json'
    profile.write_text(json.dumps(dict(version=1, display=dict(
        screen=size, aspect='fixed', fullscreen=False,
        look=dict(vertex_snap=look, dither=look, quantize=look)))))
    env = os.environ.copy()
    env.update(BEVY_ASSET_ROOT=str(ROOT / asset_root), XDG_CONFIG_HOME=str(folder / 'config'))
    env.pop('DISPLAY', None)
    env.pop('WAYLAND_DISPLAY', None)
    command = [str(ROOT / 'target/debug' / binary), '--mcp', '--headless', '--software',
               '--display-settings', str(profile)]
    with (folder / 'game.log').open('w') as log:
        proc = subprocess.Popen(command, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=log, text=True)
        replies = queue.Queue()
        def read():
            for line in proc.stdout:
                replies.put(line)
            replies.put(None)
        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        request_id = 0
        def request(method, params):
            nonlocal request_id
            request_id += 1
            proc.stdin.write(json.dumps(dict(jsonrpc='2.0', id=request_id, method=method, params=params)) + '\n')
            proc.stdin.flush()
            while True:
                line = replies.get(timeout=120)
                if line is None:
                    raise RuntimeError(f'{binary} exited; inspect {log.name}')
                reply = json.loads(line)
                if reply.get('id') == request_id:
                    assert 'error' not in reply, reply
                    return reply['result']
        def call(tool, **args):
            result = request('tools/call', dict(name=tool, arguments=args))
            assert not result.get('isError'), result
            return result
        try:
            request('initialize', dict(protocolVersion='2025-06-18', capabilities={},
                                      clientInfo=dict(name='readme-capture', version='1')))
            proc.stdin.write(json.dumps(dict(jsonrpc='2.0', method='notifications/initialized')) + '\n')
            proc.stdin.flush()
            # Wait for the initial scene/pipelines before using the public camera controls.
            call('game_screenshot', target='screen')
            call('game_step', frames=1, buttons=['r1'] if chase else [])
            status = call('game_status')['structuredContent']
            assert status['paused'] and status['headless'], status
            shot = call('game_screenshot', target='screen')
            data = base64.b64decode(next(item['data'] for item in shot['content'] if item['type'] == 'image'))
            assert data.startswith(b'\x89PNG\r\n\x1a\n')
            assert struct.unpack('>II', data[16:24]) == tuple(size)
            path = OUTPUT / f'{name}.png'
            path.write_bytes(data)
            (folder / 'status.json').write_text(json.dumps(status, indent=2) + '\n')
            return dict(path=str(path.relative_to(ROOT)), sha256=hashlib.sha256(data).hexdigest(),
                        command=command, screen=size, ps1_look=look, camera='ChaseNear')
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    records = [capture('gran_turismo_2', 'game', 'b1-ps1', [320, 240], True, True),
               capture('gran_turismo_2', 'game', 'b1-widescreen', [640, 360], False, True),
               capture('example-mod', 'mods/example', 'example-mod', [320, 240], True, False)]
    (WORK / 'manifest.json').write_text(json.dumps(dict(
        commit=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        captures=records), indent=2) + '\n')
    print(json.dumps(records, indent=2))


if __name__ == '__main__':
    main()
