#!/usr/bin/env python3
"""Produce up.exe and an unsigned up.exe.json for the GitHub Releases update flow.

This branch anchors update trust in the GitHub repository and HTTPS transport,
so descriptors are published without a detached signature while keeping the
SHA-256, size, version and anti-rollback build checks.
"""
from pathlib import Path
import argparse, hashlib, json, re, shutil

ROOT = Path(__file__).resolve().parent.parent.parent
source = (ROOT / 'internal/app/updates.go').read_text(encoding='utf-8')
BUILD = int(re.search(r'const ApplicationBuild uint64 = (\d+)', source).group(1))
VERSION = re.search(r'const ApplicationVersion = "([^"]+)"', source).group(1)
DISPLAY_VERSION = re.search(r'const ApplicationDisplayVersion = "([^"]+)"', source).group(1)

def digest(p):
    with p.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True, help='prepared Windows executable to publish as up.exe')
parser.add_argument('--output-dir', type=Path, required=True)
parser.add_argument('--notes-file', type=Path, help='UTF-8 release notes shown by clients')
args = parser.parse_args()

out = args.output_dir
out.mkdir(parents=True, exist_ok=True)
notes_file = args.notes_file or (ROOT / 'docs' / 'release' / f'RELEASE-{VERSION}.md')
shutil.copy2(args.binary.resolve(), out / 'up.exe')
notes = notes_file.read_text(encoding='utf-8').strip() if notes_file.is_file() else f'{VERSION} 更新：详情见 GitHub Releases。'
info = {'schemaVersion': 2, 'build': BUILD, 'product': 'DevShell', 'platform': 'windows-amd64',
        'version': VERSION, 'displayVersion': DISPLAY_VERSION, 'notes': notes, 'sha256': digest(out / 'up.exe'),
        'size': (out / 'up.exe').stat().st_size, 'executableSHA256': digest(args.binary.resolve())}
(out / 'up.exe.json').write_text(json.dumps(info, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(json.dumps({'build': BUILD, 'version': VERSION, 'sha256': info['sha256'], 'size': info['size']}, ensure_ascii=False))
