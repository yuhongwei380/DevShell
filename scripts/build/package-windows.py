#!/usr/bin/env python3
"""Build a per-user Windows installer from a clean portable distribution."""
from pathlib import Path
import argparse, hashlib, json, os, re, shutil, subprocess, tempfile

ROOT = Path(__file__).resolve().parent.parent.parent

def nsis_string(value):
    return str(value).replace('$', '$$').replace('"', '$\\"')

def build(stage, output):
    application_build = int(re.search(r"const ApplicationBuild uint64 = (\d+)", (ROOT / "internal/app/updates.go").read_text(encoding='utf-8')).group(1))
    source = (ROOT / "internal/app/updates.go").read_text(encoding='utf-8')
    version = re.search(r'const ApplicationVersion = "([^"]+)"', source).group(1)
    package_version = re.search(r'const ApplicationPackageVersion = "([^"]+)"', source).group(1)
    stage, output = stage.resolve(), output.resolve()
    if sorted(p.name for p in stage.iterdir()) != ['DevShell.exe', 'data']:
        raise SystemExit('Windows staging must contain only DevShell.exe and data.')
    if any(p.name not in ['docs', 'licenses'] for p in (stage / 'data').iterdir()):
        raise SystemExit('User data must never enter the installer.')
    tool = shutil.which('makensis')
    if not tool:
        raise SystemExit('Install NSIS (makensis) before packaging Windows.')
    output.parent.mkdir(parents=True, exist_ok=True)
    cache = Path.home() / '.cache/cloudshell-build'
    cache.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='devshell-nsis-', dir=cache) as temp:
        temp = Path(temp)
        # Ship the control helper inside the installer, not in the application
        # directory. It can safely replace a running/older DevShell executable.
        installer_control = temp / 'installer-control.exe'
        subprocess.run(['go', 'build', '-buildvcs=false', '-trimpath',
                        '-ldflags', '-s -w -H windowsgui', '-o', str(installer_control),
                        './cmd/installer-control'], cwd=ROOT, check=True,
                       env={**os.environ, 'GOOS': 'windows', 'GOARCH': 'amd64', 'CGO_ENABLED': '0'})
        files = sorted(p for p in stage.rglob('*') if p.is_file())
        uninstall = []
        for p in files:
            if p.relative_to(stage).as_posix() == 'DevShell.exe':
                continue  # Explicit checked deletion precedes registry/shortcut removal.
            relative = nsis_string(str(p.relative_to(stage)).replace('/', '\\'))
            uninstall.append(f'Delete "$INSTDIR\\{relative}"')
        for p in sorted((p for p in stage.rglob('*') if p.is_dir()), key=lambda p:len(p.parts), reverse=True):
            relative = nsis_string(str(p.relative_to(stage)).replace('/', '\\'))
            uninstall.append(f'RMDir "$INSTDIR\\{relative}"')
        (temp / 'uninstall-files.nsh').write_text('\n'.join(uninstall)+'\n', encoding='utf-8')
        # makensis uses libc tmpfile(), which ignores TMPDIR on Linux. A private
        # mount keeps its mmap scratch on disk without touching the system /tmp.
        prefix = []
        # Binding a private /tmp would hide a source or staging tree located
        # there; use the normal temporary directory for this release workspace.
        if shutil.which('bwrap') and not any(p.is_relative_to(Path('/tmp')) for p in (ROOT, stage, output)):
            scratch = temp / 'nsis-tmp'; scratch.mkdir()
            prefix = ['bwrap','--bind','/','/','--bind',str(scratch),'/tmp',
                      '--dev-bind','/dev','/dev','--die-with-parent','--']
            if subprocess.run(prefix+['/bin/true'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode:
                prefix = []
        subprocess.run(prefix + [tool, '-V2', '-INPUTCHARSET', 'UTF8',
                        '-DSTAGE='+str(stage), '-DFILESPEC='+str(stage / '*'), '-DOUTPUT='+str(output),
                        '-DICON='+str(ROOT/'build/devshell.ico'),
                        '-DLICENSE_FILE='+str(ROOT/'LICENSE'),
                        '-DUNINSTALL_FILES='+str(temp/'uninstall-files.nsh'),
                        '-DINSTALLER_CONTROL='+str(installer_control),
                        '-DESTIMATED_SIZE='+str(sum(p.stat().st_size for p in files)//1024),
                        '-DVERSION='+version, '-DPACKAGE_VERSION='+package_version, '-DBUILD_REVISION='+str(application_build % 1000),
                        str(ROOT/'scripts/build/windows-installer.nsi')], check=True,
                       env={**os.environ,'TMPDIR':str(cache)})
    with output.open('rb') as f:
        digest = hashlib.file_digest(f,'sha256').hexdigest()
    print(json.dumps({'file':str(output),'bytes':output.stat().st_size,'sha256':digest,'scope':'current-user','preservesConfigOnUninstall':True},indent=2))

if __name__ == '__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--stage',type=Path,required=True)
    parser.add_argument('--output',type=Path,default=ROOT/'build/windows/DevShell-Setup-x64.exe')
    args=parser.parse_args();build(args.stage,args.output)
