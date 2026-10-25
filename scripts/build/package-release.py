#!/usr/bin/env python3
"""Stage a Windows + source release from frozen binaries; never include user data.

Release policy (2026-10-20): only the Windows installer, the frozen DevShell.exe
and the source archive are published; the portable ZIP and all Linux packages
were retired with their packaging scripts. Outputs are written to --output and
the prepared --website directory. Deploy only after validating this complete
set. This script never overwrites share, private configuration, or the desktop
source checkout.
"""
from pathlib import Path
import argparse, hashlib, json, re, shutil, subprocess, tarfile, zipfile
ROOT = Path(__file__).resolve().parent.parent.parent
BUILD = int(re.search(r'const ApplicationBuild uint64 = (\d+)', (ROOT/'internal/app/updates.go').read_text(encoding='utf-8')).group(1))
VERSION = re.search(r'const ApplicationVersion = "([^"]+)"', (ROOT/'internal/app/updates.go').read_text(encoding='utf-8')).group(1)
PACKAGE_VERSION = re.search(r'const ApplicationPackageVersion = "([^"]+)"', (ROOT/'internal/app/updates.go').read_text(encoding='utf-8')).group(1)
# Old pacman updaters bind pkgrel to the last three build digits.
# Public release naming is independent of this internal package revision.
LABEL = f'{VERSION}-{BUILD}'

def digest(p):
    with p.open('rb') as f: return hashlib.file_digest(f, 'sha256').hexdigest()

def copy(src, dst):
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)

def runtime_credential_file(p):
    name = p.name.lower()
    return (name in {'config.json', 'id_rsa', 'id_ed25519', '.env'}
            or name.startswith('.env.') and name != '.env.example'
            or re.search(r'\.(?:enc|key|pem|keystore|jks)(?:[.~]|$)', name) is not None
            or name in {'dengshell.config.json', 'cloudshell.config.json'})

def zip_tree(folder, target, prefix=Path()):
    with zipfile.ZipFile(target, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for p in sorted(folder.rglob('*')):
            if p.is_file(): z.write(p, prefix/p.relative_to(folder))

def distribution(folder, binary, windows, source_ref=VERSION):
    folder.mkdir(parents=True, exist_ok=False)
    copy(binary, folder/('DevShell.exe' if windows else 'devshell'))
    for sub, pattern, dest in [('web/assets/fonts/licenses','*.txt','fonts'), ('web/vendor','*LICENSE',''), ('build/go-licenses','*','go'), ('build/installer-licenses','*','installer')]:
        for p in (ROOT/sub).glob(pattern):
            if p.is_file(): copy(p, folder/'data/licenses'/dest/p.name)
    for name in ['LICENSE-GRAPHICS.txt','ATTRIBUTION.txt']: copy(ROOT/'web/assets/group-emoji'/name,folder/'data/licenses'/('Twemoji-'+name))
    for p in list((ROOT/'web/assets/ui-fonts').glob('*-OFL.txt'))+list((ROOT/'web/assets/ui-fonts').glob('*-LICENSE.txt')): copy(p,folder/'data/licenses/ui-fonts'/p.name)
    for p in (ROOT/'web/assets/system-logos').glob('*.txt'): copy(p,folder/'data/licenses/system-logos'/p.name)
    copy(ROOT/'LICENSE',folder/'data/licenses/DevShell-MIT.txt')
    if windows:
        for p in (ROOT/'native-rdp').glob('*'):
            if p.is_file() and ('LICENSE' in p.name or p.name in ('sources.json','NOTICE.txt')): copy(p,folder/'data/licenses/rdp'/p.name)
    for name in ['CONNECTION-CONFIG.md','COMMON-APPS.md','ONLINE-UPDATE-DESIGN.md','UPDATER-CONTRACT.md','SIGNED-UPDATES.md','COMMANDS-AND-EDITORS.md','FUNCTIONAL-AUDIT-2026-10-03.md',f'FUNCTIONAL-AUDIT-{VERSION}.md',f'RELEASE-{VERSION}.md']:
        if name.startswith('FUNCTIONAL-AUDIT-') and not (ROOT/'docs/release'/name).is_file():
            continue  # Per-version functional audits are optional; the 2026-10-03 baseline always ships.
        copy(ROOT/'docs/release'/name,folder/'data/docs'/name)
    copy(ROOT/'build/config.example.json',folder/'data/docs/config.example.json')
    readme = (ROOT/'README.md').read_text(encoding='utf-8')
    readme = re.sub(r'(?<=[(])((?:build|docs)/[^)]+)(?=[)])', lambda m: f'https://github.com/yuhongwei380/DevShell/blob/{source_ref}/' + m.group(1), readme)
    (folder/'data/docs/README.md').write_text(readme, encoding='utf-8')
    copy(ROOT/'CHANGELOG.md',folder/'data/docs/CHANGELOG.md')
    copy(ROOT/('build/windows/README.txt' if windows else 'build/linux/README.txt'),folder/'data/docs/使用说明.txt')
    return folder

def package(args):
    if not args.signing_key.is_file() or not args.signer.is_file():
        raise SystemExit('A private signing key outside the release and a compiled cmd/sign-update utility are required')
    key = args.signing_key.resolve()
    if any(key.is_relative_to(folder.resolve()) for folder in [ROOT, args.output, args.work_dir, args.website]):
        raise SystemExit('Private signing keys must remain outside source and publication directories')
    out, work, site = args.output.resolve(), args.work_dir.resolve(), args.website.resolve()
    for p in [out,work,site/'downloads']: p.mkdir(parents=True,exist_ok=True)
    winbinary = args.windows_binary.resolve()
    reference = (args.windows_reference or args.windows_binary).resolve()
    # Validate the actual packed EXE with the same debug/pe reader used by old
    # Windows updaters. Normalize UPX's empty COFF pointer before any hashes or
    # signatures are made, keeping the caller's frozen input unchanged.
    prepared_windows = work/'DevShell-validated.exe'
    subprocess.run(['go', 'run', './cmd/prepare-windows-image',
                    '-in', str(winbinary), '-out', str(prepared_windows)], cwd=ROOT, check=True)
    winbinary = prepared_windows
    binaries = [reference.read_bytes()]
    assets = {}
    for folder in [ROOT/'web',ROOT/'internal/app/shell_integration']:
        for p in sorted(folder.rglob('*')):
            if not p.is_file() or any(n.startswith(('.','_')) for n in p.relative_to(folder).parts):continue
            if runtime_credential_file(p):
                raise SystemExit(f'Runtime credential file must not be embedded: {p.relative_to(ROOT)}')
            assert all(p.read_bytes() in b for b in binaries), f'Embedded asset differs: {p}'
            assets[str(p.relative_to(ROOT))]=digest(p)
    win = distribution(work/'DevShell-windows-x64',winbinary,True,args.source_ref)
    subprocess.run(['python3',str(ROOT/'scripts/build/package-windows.py'),'--stage',str(win),'--output',str(out/'DevShell-Setup-x64.exe')],check=True)
    copy(winbinary,out/'DevShell.exe')
    copy(ROOT/'build/windows/README.txt',out/'DevShell-Windows-说明.txt')
    source = work/'source';source.mkdir()
    sourcefiles=[]
    for pattern in ['.gitattributes','*.go','go.mod','go.sum','package.json','package-lock.json','*.sh','*.ps1','*.syso','README.md','CHANGELOG.md','LICENSE','.gitignore']:
        sourcefiles += list(ROOT.glob(pattern))
    for folder in ['web','internal','scripts','cmd','native-rdp','LICENSES']:
        if not (ROOT/folder).is_dir(): continue
        sourcefiles += [p for p in (ROOT/folder).rglob('*') if p.is_file() and not any(part in ('__pycache__', '.cache', 'node_modules') for part in p.relative_to(ROOT/folder).parts)]
    sourcefiles += [p for p in (ROOT/'docs').rglob('*') if p.is_file()]
    for name in ['devshell.png','devshell.svg','devshell.ico','config.example.json','BACKGROUND-PROMPTS-v0.01.json','windows/app.manifest','windows/README.txt']:
        sourcefiles.append(ROOT/'build'/name)
    for folder in ['go-licenses','font-licenses','font-library-site','installer-licenses']:
        sourcefiles += [p for p in (ROOT/'build'/folder).glob('*') if p.is_file()]
    for p in sorted(set(sourcefiles)):
        if not p.is_file(): continue
        if not runtime_credential_file(p): copy(p,source/p.relative_to(ROOT))
    zip_tree(source,out/'DevShell-source.zip',Path('DevShell'))
    notes=(args.update_notes.read_text().strip() if args.update_notes else f'{VERSION} 更新：详情见官网更新日志。')
    for binary,artifact,platform,name in [(winbinary,winbinary,'windows-amd64','up.exe')]:
        descriptor = site/(name+'.json')
        release_notes = notes
        if args.preserve_update_notes:
            if not descriptor.is_file():
                raise SystemExit(f'Cannot preserve missing update notes: {descriptor}')
            release_notes = json.loads(descriptor.read_text())['notes']
        copy(artifact,site/name)
        info={'schemaVersion':2,'build':BUILD,'product':'DevShell','platform':platform,'version':VERSION,'notes':release_notes,'sha256':digest(artifact),'size':artifact.stat().st_size,'executableSHA256':digest(binary)}
        (site/(name+'.json')).write_text(json.dumps(info,ensure_ascii=False,indent=2)+'\n', encoding='utf-8')
        subprocess.run([str(args.signer.resolve()), '-key', str(args.signing_key.resolve()),
                        '-in', str(site/(name+'.json')), '-out', str(site/(name+'.json'))], check=True)
    for name in ['DevShell-Setup-x64.exe','DevShell.exe','DevShell-source.zip']:
        copy(out/name,site/'downloads'/name)
    files={p.name:{'bytes':p.stat().st_size,'sha256':digest(p)} for p in sorted(out.iterdir()) if p.suffix in ['.exe','.zip']}
    manifest={'version':VERSION,'build':LABEL,'applicationBuild':BUILD,'artifacts':files,'embeddedAssets':assets,'executableHashes':{'windows-amd64':digest(winbinary)}}
    (out/'release.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n', encoding='utf-8')
    (out/'SHA256SUMS.txt').write_text(''.join(f"{v['sha256']}  {n}\n" for n,v in files.items()), encoding='utf-8')
    (site/'downloads/SHA256SUMS.txt').write_text(''.join(f'{digest(p)}  {p.relative_to(site)}\n' for p in sorted(site.rglob('*')) if p.is_file() and p.suffix in ['.zip','.exe']), encoding='utf-8')
    print(json.dumps({'output':str(out),'website':str(site),'build':LABEL,'artifacts':files},ensure_ascii=False,indent=2))

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--preserve-update-notes',action='store_true',help='Keep existing signed descriptor notes while updating the build and hashes')
    parser.add_argument('--update-notes',type=Path,help='UTF-8 notes shown by existing clients for this signed update')
    parser.add_argument('--windows-binary',type=Path,required=True)
    parser.add_argument('--windows-reference',type=Path)
    parser.add_argument('--source-ref',default=VERSION,help='Published source tag used by packaged documentation links')
    parser.add_argument('--work-dir',type=Path,required=True)
    parser.add_argument('--website',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--signing-key',type=Path,required=True,help='Private seed outside source, output, work and website directories; never packaged')
    parser.add_argument('--signer',type=Path,required=True,help='Compiled cmd/sign-update publisher utility')
    package(parser.parse_args())
