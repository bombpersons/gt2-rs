#!/usr/bin/env python3
"""Exercise the engine-only handmade scene through its developer MCP.

Build first: cargo build -p engine --example sandbox --all-features
Requires a software GPU adapter and Pillow for image assertions. No disc data.
"""
import base64
import json
import math
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
output = Path('/tmp/gt2-template-sandbox')
output.mkdir(exist_ok=True)
env = os.environ.copy()
env['BEVY_ASSET_ROOT'] = str(root / 'engine')
env['XDG_CONFIG_HOME'] = str(output / 'config')
env.pop('DISPLAY', None)
env.pop('WAYLAND_DISPLAY', None)
with (output / 'sandbox.log').open('w') as log:
    proc = subprocess.Popen([str(root / 'target/debug/examples/sandbox'), '--mcp', '--headless', '--software'],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, env=env, text=True)
    request_id = 0

    def request(method, params):
        global request_id
        request_id += 1
        proc.stdin.write(json.dumps(dict(jsonrpc='2.0', id=request_id, method=method, params=params)) + '\n')
        proc.stdin.flush()
        while True:
            line = proc.stdout.readline()
            if not line:
                raise RuntimeError(f'sandbox exited: {proc.poll()}, inspect {log.name}')
            reply = json.loads(line)
            if reply.get('id') == request_id:
                assert 'error' not in reply, reply
                return reply['result']

    def call(name, args):
        result = request('tools/call', dict(name=name, arguments=args))
        assert not result.get('isError'), result
        return result

    try:
        request('initialize', dict(protocolVersion='2025-06-18', capabilities={},
                                   clientInfo=dict(name='sandbox-check', version='1')))
        proc.stdin.write(json.dumps(dict(jsonrpc='2.0', method='notifications/initialized')) + '\n')
        proc.stdin.flush()
        initial = call('game_query', dict(name='Walker', components=['Transform']))
        step = call('game_step', dict(frames=30, buttons=['right']))
        final = call('game_query', dict(name='Walker', components=['Transform']))
        def x(reply):
            return reply['structuredContent']['entities'][0]['components']['Transform']['translation'][0]
        assert x(final) > x(initial) + 0.5, (initial, final)
        shot = call('game_screenshot', dict(target='screen'))
        encoded = next(block['data'] for block in shot['content'] if block['type'] == 'image')
        (output / 'sandbox.png').write_bytes(base64.b64decode(encoded))
        call('game_set_component', dict(name='Sandbox camera', component='Transform', fields=dict(translation=[0.0, 6.0, 30.0], rotation=[math.sin(-math.atan2(4.5,33.0)/2),0.0,0.0,math.cos(-math.atan2(4.5,33.0)/2)])))
        far_shot = call('game_screenshot', dict(target='screen'))
        far_encoded = next(block['data'] for block in far_shot['content'] if block['type'] == 'image')
        (output / 'small-detail.png').write_bytes(base64.b64decode(far_encoded))
        from PIL import Image
        near_pixels = list(Image.open(output / 'sandbox.png').get_flattened_data())
        far_pixels = list(Image.open(output / 'small-detail.png').get_flattened_data())
        blue = lambda p: p[2] > p[0] * 1.3 and p[2] > p[1] * 1.15
        orange = lambda p: p[0] > p[1] * 1.2 and p[1] > p[2] * 1.2
        assert sum(map(blue, near_pixels)) > 500
        assert sum(map(orange, far_pixels)) > 100
        (output / 'evidence.json').write_text(json.dumps(dict(initial=initial, step=step, final=final), indent=2))
        print(json.dumps(dict(screenshots=[str(output/'sandbox.png'), str(output/'small-detail.png')], start_x=x(initial), end_x=x(final))))
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
