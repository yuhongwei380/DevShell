#!/usr/bin/env python3
"""Stage the Windows distribution folder from a frozen binary.

Reuses the exact staging logic of package-release.py so CI, local builds and
the full website pipeline always produce the same layout: DevShell.exe,
DevShellUI.exe and data/docs/data/licenses only, never user data.
"""
from pathlib import Path
import argparse, importlib.util

ROOT = Path(__file__).resolve().parent.parent.parent
spec = importlib.util.spec_from_file_location('package_release', ROOT / 'scripts' / 'build' / 'package-release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True, help='frozen (prepared) Windows executable')
parser.add_argument('--native-ui', type=Path, required=True, help='compiled GPUI native frontend executable')
parser.add_argument('--stage', type=Path, required=True, help='empty staging directory to create')
args = parser.parse_args()

release.distribution(args.stage, args.binary.resolve(), True, release.VERSION, args.native_ui.resolve())
print(f'staged: {args.stage}')
