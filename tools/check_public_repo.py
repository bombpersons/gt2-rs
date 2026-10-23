#!/usr/bin/env python3
"""Audit public Git content; optionally export a fresh source-only worktree.

Uses a positive path/type list, rejects links, unexpected binary content,
large encoded blobs and all generated asset/reference/analysis paths. The
history check audits every reachable commit, not just the current tree.
This is a guard alongside provenance review, not a copyright classifier.
"""
import argparse
import hashlib
from pathlib import Path, PurePosixPath
import re
import shutil
import struct
import subprocess

ROOT_FILES = {'AGENTS.md', 'Cargo.toml', 'Cargo.lock', 'PORT_PLAN.md', 'README.md',
              '.gitignore', '.psx-template.toml', 'rustfmt.toml', 'tools_bin2iso.py'}
AUTHORED_IMAGES = {'engine/assets/sandbox/panel-large.png', 'engine/assets/sandbox/panel-small.png'}
# Explicitly reviewed documentation captures, requested for the README.
# These are native-rendered presentation images, never runtime game assets.
README_SCREENSHOTS = {'docs/images/b1-ps1.png': (320, 240),
                      'docs/images/b1-widescreen.png': (640, 360),
                      'docs/images/example-mod.png': (320, 240)}
PRIVATE = ('game/assets/', 'game/tests/data/', 'docs/overlays/', '.psx/', '.private/',
           'extracted/', 'bios/', 'media/', 'external/', 'ghidra-projects/', 'target/')
# Complete tuning arrays previously copied into an investigation note. Store
# only digests, so the guard itself does not publish the source table data.
# This catches their historical copies as well as whitespace-only variants;
# provenance review is still required for other embedded data.
PRIVATE_TABLE_DIGESTS = {
    '6f6aa1814ac93edac914af4e61d3a3392af690a640f95f9b44046b8a68d5a765',
    'e56acfe9e863970beec342c9c8f81ba1222c46268a2010b84532c54a8bb2c667',
    '76866826c4d3acadaa9120f900b800c9d62d75fddd425e0d52c61aca693391d1',
}


def allowed(name):
    p = PurePosixPath(name)
    if p.is_absolute() or '..' in p.parts or any(name.startswith(prefix) for prefix in PRIVATE):
        return False
    if name in ROOT_FILES or name in AUTHORED_IMAGES or name in README_SCREENSHOTS:
        return True
    if name.startswith('.github/workflows/') and p.suffix in {'.yml', '.yaml'}:
        return True
    if name.startswith('docs/'):
        return (len(p.parts) == 2 and p.suffix == '.md') or (name.startswith('docs/checkpoints/') and p.suffix in {'.md', '.json'})
    if name.startswith('game/'):
        return name in {'game/AGENTS.md', 'game/Cargo.toml', 'game/build.rs'} or (
            (name.startswith('game/src/') or name.startswith('game/tests/')) and p.suffix == '.rs')
    if name.startswith(('engine/', 'psx-bevy/', 'mods/example/', 'tools/')):
        if '/assets/' in name:
            return name.startswith(('engine/assets/sandbox/', 'mods/example/assets/')) and p.suffix == '.ron'
        return p.suffix in {'.rs', '.md', '.toml', '.py', '.wesl', '.wgsl'}
    return False


def check_blob(name, data):
    if not allowed(name):
        raise ValueError(f'Not on the public source/authored-content list: {name}')
    if len(data) > 512 * 1024:
        raise ValueError(f'Oversized public file requires review: {name}')
    if name in AUTHORED_IMAGES or name in README_SCREENSHOTS:
        if not data.startswith(b'\x89PNG\r\n\x1a\n'):
            raise ValueError(f'Expected approved PNG: {name}')
        if name in README_SCREENSHOTS:
            if len(data) < 33 or data[12:16] != b'IHDR' or struct.unpack('>II', data[16:24]) != README_SCREENSHOTS[name]:
                raise ValueError(f'Unexpected documentation screenshot dimensions: {name}')
        return
    try:
        text = data.decode('utf8')
    except UnicodeDecodeError as error:
        raise ValueError(f'Unexpected binary file: {name}') from error
    if '\0' in text:
        raise ValueError(f'Binary content in text file: {name}')
    if re.search(r'[A-Za-z0-9+/]{512,}={0,2}', text):
        raise ValueError(f'Long encoded payload requires review: {name}')
    for sequence in re.finditer(r'(?<![\w])[-+]?\d+(?:\s*,\s*[-+]?\d+){7,}(?![\w])', text):
        canonical = ','.join(re.findall(r'[-+]?\d+', sequence.group()))
        if hashlib.sha256(canonical.encode()).hexdigest() in PRIVATE_TABLE_DIGESTS:
            raise ValueError(f'Copied private source tuning table: {name}')


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args])


def candidates(root):
    return sorted(set(git(root, 'ls-files', '-z', '--cached', '--others', '--exclude-standard').decode().split('\0')) - {''})


def export(root, target):
    if target.exists() and any(target.iterdir()):
        raise ValueError('Export destination must be empty')
    target.mkdir(parents=True, exist_ok=True)
    manifest = []
    excluded = 0
    for name in candidates(root):
        source = root / name
        if not source.exists():
            continue  # deleted tracked file
        if not allowed(name):
            excluded += 1
            continue
        if source.is_symlink():
            raise ValueError(f'Symlink requires review: {name}')
        data = source.read_bytes()
        check_blob(name, data)
        destination = target / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
        manifest.append((name, hashlib.sha256(data).hexdigest()))
    print(f'Exported {len(manifest)} source/authored files; excluded {excluded} private/unapproved files.')
    return manifest


def audit(root, history):
    count = 0
    if history:
        seen = set()
        commits = git(root, 'rev-list', '--all').decode().splitlines()
        if not commits:
            raise ValueError('No commits to audit')
        for commit in commits:
            for row in git(root, 'ls-tree', '-rz', commit).split(b'\0'):
                if not row:
                    continue
                metadata, path = row.split(b'\t', 1)
                mode, kind, oid = metadata.decode().split()
                name = path.decode()
                if mode not in {'100644', '100755'} or kind != 'blob':
                    raise ValueError(f'Non-regular content: {name}')
                if (name, oid) not in seen:
                    check_blob(name, git(root, 'cat-file', 'blob', oid))
                    seen.add((name, oid))
                    count += 1
        print(f'PASS: {len(commits)} reachable commits, {count} source/authored blobs; no generated data paths or unexpected binaries.')
    else:
        for name in candidates(root):
            path = root / name
            if not path.exists():
                continue
            if path.is_symlink():
                raise ValueError(f'Symlink requires review: {name}')
            check_blob(name, path.read_bytes())
            count += 1
        print(f'PASS: {count} indexed/unignored files meet the public content policy.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--history', action='store_true')
    parser.add_argument('--export', type=Path, help='Copy only approved files to an empty fresh-repository directory')
    args = parser.parse_args()
    try:
        if args.export:
            export(args.root.resolve(), args.export.resolve())
        else:
            audit(args.root.resolve(), args.history)
    except (ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'Public content audit failed: {error}\n')


if __name__ == '__main__':
    main()
