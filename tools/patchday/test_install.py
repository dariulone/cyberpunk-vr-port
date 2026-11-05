"""A test install of the staged frameworks and the port for a new game build -- and its exact undo.

    py -3.13 -B tools/patchday/test_install.py install [--stage build/patchday/stage-2.5] [--port-dll <dir>]
    py -3.13 -B tools/patchday/test_install.py restore
    py -3.13 -B tools/patchday/test_install.py status
    py -3.13 -B tools/patchday/test_install.py deploy-port      # the port's own files for good, no frameworks

The installed frameworks are Vortex hardlinks: writing through one changes Vortex's own staging copy. So a game
file a staged one replaces is RENAMED to <name>.cvr25bak (the link and Vortex's copy untouched) and the staged file
is copied in as a new file; restore deletes the copy and renames the original back, and checks it is the same file
as before (size, sha256, link count). The port's own files (plugin DLL/PDB, the archive, the redscript and CET files
the 2.5 port changed) are not Vortex's: those are copied over, the previous ones kept under
build/patchday/test-install/<stamp>/ and put back by restore. Everything is recorded in
build/patchday/test-install/journal.json, so restore works from a fresh shell. The game must be closed.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
GAME = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'
WORK = os.path.join(REPO, 'build', 'patchday', 'test-install')
JOURNAL = os.path.join(WORK, 'journal.json')
BAK = '.cvr25bak'
# The port's files a 2.5 test needs, game-relative -> repo source. User state (vrcam.json, quickboot.json,
# db.sqlite3, ini files) is deliberately not in this list.
PORT_FILES = {
    # the port's archive and its manifest, mounted and applied by the plugin (Core/ArchiveExtensions.hpp)
    'red4ext/plugins/cyberpunkvrport/archive/cyberpunkvrport.archive': 'mods/archive/cyberpunkvrport.archive',
    'red4ext/plugins/cyberpunkvrport/config/archive.yaml': 'mods/archive/cyberpunkvrport.archive.xl',
}
# Every script mod of the port, as the repository has it: a fix in any of them reaches the game with the plugin.
PORT_REDS = sorted(n for n in os.listdir(os.path.join(REPO, 'mods', 'redscript'))
                   if os.path.isdir(os.path.join(REPO, 'mods', 'redscript', n)))
# Every Lua mod of the port (QuickBoot included: the launch tools resume saves with it), run by the plugin's host
# from its lua folder (src/Lua); the diagnostics-only WorldMapDiag stays out.
PORT_LUA = sorted(n for n in os.listdir(os.path.join(REPO, 'mods', 'cet'))
                  if n.startswith('CyberpunkVRPort_') and n != 'CyberpunkVRPort_WorldMapDiag'
                  and os.path.isdir(os.path.join(REPO, 'mods', 'cet', n)))
# Never deployed from a mod folder: a working copy's leftovers.
LUA_SKIP = ('db.sqlite3', '.log', '.bak', '.orig', '.rar', '.zip', '.md')
# The plugin's one folder (include/Core/Paths.hpp) and the folder an earlier release used.
PLUGIN = 'red4ext/plugins/cyberpunkvrport'
LEGACY_PLUGIN = 'red4ext/plugins/CyberpunkVR_Stereo'
# The DirectX shader compiler single-pass stereo loads beside the plugin (dxc\), as tools/vr_bench deploys it.
DXC_SOURCE = r'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64'
# The port's own shaders (src/Shaders) under the names the plugin loads them by, in its shaders folder.
OWN_SHADERS = {
    'CyberpunkVR_SightPs.dxil': 'sight_reflex_ps.dxil',
    'CyberpunkVR_SightVs.dxil': 'sight_reflex_vs.dxil',
    'CyberpunkVR_SightPsScope.dxil': 'sight_reflex_ps_scope.dxil',
    'CyberpunkVR_LensVelocityPs.dxil': 'lens_velocity_ps.dxil',
    'CyberpunkVR_LensMaskPs.dxil': 'lens_mask_ps.dxil',
    'CyberpunkVR_SightMaskPs.dxil': 'sight_mask_ps.dxil',
    'CyberpunkVR_LensSourceResolveCs.dxil': 'lens_source_resolve_cs.dxil',
    'CyberpunkVR_CameraSourceResolveCs.dxil': 'camera_source_resolve_cs.dxil',
    'CyberpunkVR_VisionCs.dxil': 'vision_highlight_cs.dxil',
}


def sha(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def links(p):
    return os.stat(p).st_nlink


def game_running():
    out = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq Cyberpunk2077.exe'], capture_output=True, text=True).stdout
    return 'Cyberpunk2077.exe' in out


def seed_only(rel):
    """A Lua mod's data file (its settings: json, cfg, ...) is the mod's to rewrite: deployed only where it is
    missing, never over the player's. Its code and its sounds are the port's."""
    return rel.startswith(PLUGIN + '/lua/') and not rel.endswith(('.lua', '.wav', '.ogg'))


def port_files(a):
    """Game-relative path -> repo-relative source of every port file the new build needs."""
    files = dict(PORT_FILES)
    # the port's Lua mods in the plugin's lua folder, run by its host (src/Lua)
    for mod in PORT_LUA:
        for d, _, ns in os.walk(os.path.join(REPO, 'mods', 'cet', mod)):
            for n in ns:
                if n == 'db.sqlite3' or n.endswith(LUA_SKIP):
                    continue
                p = os.path.join(d, n)
                files[PLUGIN + '/lua/' + os.path.relpath(p, os.path.join(REPO, 'mods', 'cet')).replace('\\', '/')] = \
                    os.path.relpath(p, REPO)
    # the port's scripts in the plugin's scripts folder, compiled with the game's (Core/ScriptCompiler.hpp)
    for mod in PORT_REDS:
        for d, _, ns in os.walk(os.path.join(REPO, 'mods', 'redscript', mod)):
            for n in ns:
                p = os.path.join(d, n)
                files[PLUGIN + '/scripts/' + os.path.relpath(p, os.path.join(REPO, 'mods', 'redscript')).replace('\\', '/')] = \
                    os.path.relpath(p, REPO)
    dll = a.port_dll
    for n in ('cyberpunkvrport.dll', 'cyberpunkvrport.pdb'):
        files[PLUGIN + '/' + n] = os.path.relpath(os.path.join(dll, n), REPO)
    # redscript's compiler, built with the plugin, for a game without redscript installed
    for n in ('scc_lib.dll', 'LICENSE.txt'):
        files[PLUGIN + '/redscript/' + n] = os.path.relpath(os.path.join(dll, 'redscript', n), REPO)
    # the TweakDB's record names, for the Lua host (src/Lua)
    for n in ('tweakdbstr.kark', 'usedhashes.kark', 'LICENSE.txt'):
        files[PLUGIN + '/config/tweakdb/' + n] = os.path.relpath(os.path.join(dll, 'config', 'tweakdb', n), REPO)
    for name, src in OWN_SHADERS.items():
        files[PLUGIN + '/shaders/' + name] = 'src/Shaders/' + src
    for variant in ('649567EB5A039A01', '372EEC30C414B67A'):
        files[PLUGIN + '/shaders/CyberpunkVR_ScopeGlass_%s.dxil' % variant] = 'src/Shaders/scope_glass_%s.dxil' % variant
    # single-pass stereo rewrites the engine's shaders in process with the DirectX shader compiler
    # (src/Render/ViewTwinCompiler.cpp): dxcompiler.dll + dxil.dll in the plugin's dxc folder, from the Windows SDK;
    # without them single pass stays off
    for n in ('dxcompiler.dll', 'dxil.dll'):
        if os.path.isfile(os.path.join(DXC_SOURCE, n)):
            files[PLUGIN + '/dxc/' + n] = os.path.relpath(os.path.join(DXC_SOURCE, n), REPO)
    # the shipped settings template (copied over the game's own on the first launch) and the recorded poses
    files[PLUGIN + '/config/UserSettings.json'] = 'mods/config/UserSettings.json'
    for n in sorted(os.listdir(os.path.join(REPO, 'mods', 'config'))):
        if n.startswith('CyberpunkVR_') and 'Grip_' in n and n.endswith('.ini'):
            files[PLUGIN + '/grips/' + n] = 'mods/config/' + n
    # the port's input bindings, merged into the game's input configuration by the plugin (Core/InputConfig.hpp)
    for n in sorted(os.listdir(os.path.join(REPO, 'mods', 'config', 'input'))):
        if n.endswith('.xml'):
            files[PLUGIN + '/config/input/' + n] = 'mods/config/input/' + n
    # the port's TweakDB records, applied by the plugin as the TweakDB loads (Core/TweakRecords.hpp)
    tweaks = os.path.join(REPO, 'mods', 'tweaks')
    for d, _, ns in os.walk(tweaks):
        for n in ns:
            if n.endswith('.yaml'):
                p = os.path.join(d, n)
                files[PLUGIN + '/config/tweaks/' + os.path.relpath(p, tweaks).replace('\\', '/')] = os.path.relpath(p, REPO)
    # the shader twins rebuilt for the new build (tools/patchday/regen_twins.py): shaders/<folder>/<fnv>.dxil, the
    # scope-glass ones under the plugin's own names
    if a.twins and os.path.isdir(a.twins):
        for folder in sorted(os.listdir(a.twins)):
            full = os.path.join(a.twins, folder)
            for n in sorted(os.listdir(full)) if os.path.isdir(full) else []:
                if not n.endswith('.dxil'):
                    continue
                name = 'CyberpunkVR_ScopeGlass_' + n[len('scope_glass_'):] if folder == 'scope_glass' else folder + '/' + n
                files[PLUGIN + '/shaders/' + name] = os.path.relpath(os.path.join(full, n), REPO)
    # the families no generator rebuilds per build come from the repo as they are (src/Shaders/<folder>; the fog
    # twins' folder is the plugin's asym\)
    regenerated = set(os.listdir(a.twins)) if a.twins and os.path.isdir(a.twins) else set()
    shaders = os.path.join(REPO, 'src', 'Shaders')
    for folder in sorted(os.listdir(shaders)):
        plugin_folder = {'asym_fog': 'asym'}.get(folder, folder)
        full = os.path.join(shaders, folder)
        if not os.path.isdir(full) or plugin_folder in regenerated:
            continue
        for n in sorted(os.listdir(full)):
            if n.endswith('.dxil'):
                files[PLUGIN + '/shaders/' + plugin_folder + '/' + n] = 'src/Shaders/%s/%s' % (folder, n)
    return files


def retire_legacy_plugin(keep):
    """An earlier release's plugin folder would load beside the new one (the plugin then stays off and says so):
    its calibration moves to the new config folder unless one is there, the rest goes under `keep`."""
    old = os.path.join(GAME, LEGACY_PLUGIN)
    if not os.path.isdir(old):
        return None
    calibration = os.path.join(old, 'vrik_calibration.ini')
    target = os.path.join(GAME, PLUGIN, 'config', 'vrik_calibration.ini')
    if os.path.exists(calibration) and not os.path.exists(target):
        os.makedirs(os.path.dirname(target), exist_ok=True)
        shutil.copy2(calibration, target)
    dst = os.path.join(keep, LEGACY_PLUGIN)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.move(old, dst)
    return os.path.relpath(dst, REPO)


def retire_legacy_scripts(keep):
    """The port's scripts an earlier build put in r6/scripts/CyberpunkVRPort_*: compiled beside the plugin's they would
    define everything twice. They go under `keep` (the plugin does the same at load for a player's install)."""
    root = os.path.join(GAME, 'r6', 'scripts')
    moved = []
    for n in sorted(os.listdir(root)) if os.path.isdir(root) else []:
        old = os.path.join(root, n)
        if not (n.startswith('CyberpunkVRPort_') and os.path.isdir(old)):
            continue
        for d, _, ns in os.walk(old):
            for f in ns:
                if links(os.path.join(d, f)) > 1:
                    sys.exit(f'{os.path.join(d, f)} is a hardlink, a mod manager install: remove that first')
        dst = os.path.join(keep, 'r6', 'scripts', n)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.move(old, dst)
        moved.append('r6/scripts/' + n)
    return moved


def deploy_port(a):
    """The port's own files for the new build, for good (the frameworks stay Vortex's: their zips are in the
    stage). What each file replaces, and the old build's twins (keyed by its shader hashes, so dead on the new
    one, but still counted as loaded), go to build/patchday/deploy/<stamp>/ with a manifest."""
    if os.path.exists(JOURNAL):
        sys.exit('a test install is active: restore first')
    if game_running():
        sys.exit('close the game first')
    stamp = time.strftime('%Y%m%d-%H%M%S')
    keep = os.path.join(REPO, 'build', 'patchday', 'deploy', stamp)
    files = port_files(a)
    rows, stale = [], []
    retired = retire_legacy_plugin(keep)
    old_scripts = retire_legacy_scripts(keep)
    shaders = PLUGIN + '/shaders/'
    root = os.path.join(GAME, *shaders.rstrip('/').split('/'))
    folders = {rel[len(shaders):].split('/')[0] for rel in files
               if rel.startswith(shaders) and rel.count('/') == shaders.count('/') + 1 and rel.endswith('.dxil')}
    for folder in sorted(folders):
        d = os.path.join(root, folder)
        wanted = {rel.rsplit('/', 1)[1] for rel in files if rel.startswith(shaders + folder + '/')}
        for n in sorted(os.listdir(d)) if os.path.isdir(d) else []:
            if n.endswith('.dxil') and n not in wanted:
                stale.append(shaders + folder + '/' + n)
    for n in sorted(os.listdir(root)) if os.path.isdir(root) else []:
        rel = shaders + n
        if n.startswith('CyberpunkVR_ScopeGlass_') and n.endswith('.dxil') and rel not in files:
            stale.append(rel)
    # the plugin's file name up to 0.1.9: RED4ext would load it beside cyberpunkvrport.dll
    for n in ('CyberpunkVR_Stereo.dll', 'CyberpunkVR_Stereo.pdb', 'CyberpunkVR_Stereo.dll.old'):
        if os.path.exists(os.path.join(GAME, PLUGIN, n)):
            stale.append(PLUGIN + '/' + n)
    for rel in stale:
        k = os.path.join(keep, rel)
        os.makedirs(os.path.dirname(k), exist_ok=True)
        shutil.move(os.path.join(GAME, rel), k)
    for rel, src_rel in files.items():
        src, dst = os.path.join(REPO, src_rel), os.path.join(GAME, rel)
        if os.path.exists(dst) and (seed_only(rel) or sha(dst) == sha(src)):
            continue
        row = dict(rel=rel, source=src_rel, new=sha(src), previous=None)
        if os.path.exists(dst):
            if links(dst) > 1:
                sys.exit(f'{dst} is a hardlink: not a port file, refusing to copy over it')
            k = os.path.join(keep, rel)
            os.makedirs(os.path.dirname(k), exist_ok=True)
            shutil.copy2(dst, k)
            row['previous'] = dict(sha256=sha(dst), kept=os.path.relpath(k, REPO))
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy2(src, dst)
        assert sha(dst) == row['new'], dst
        rows.append(row)
    os.makedirs(keep, exist_ok=True)
    json.dump(dict(stamp=stamp, port=rows, stale=stale, retired=retired, old_scripts=old_scripts),
              open(os.path.join(keep, 'deploy.json'), 'w', encoding='utf-8'), indent=1)
    print(f'deployed {len(rows)} port files ({len(files) - len(rows)} already current), moved {len(stale)} stale '
          f'files{", retired " + LEGACY_PLUGIN if retired else ""}'
          f'{", moved %d old script folders out of r6/scripts" % len(old_scripts) if old_scripts else ""}; '
          f'kept under {os.path.relpath(keep, REPO)}')


def install(a):
    if os.path.exists(JOURNAL):
        sys.exit('a test install is active (journal exists): restore first')
    if game_running():
        sys.exit('close the game first')
    stamp = time.strftime('%Y%m%d-%H%M%S')
    keep = os.path.join(WORK, stamp)
    os.makedirs(keep, exist_ok=True)
    journal = dict(stamp=stamp, frameworks=[], port=[])
    manifest = json.load(open(os.path.join(a.stage, 'manifest.json'), encoding='utf-8'))
    for name, m in manifest.items():
        for rel, info in m['files'].items():
            src = os.path.join(a.stage, name, rel)
            dst = os.path.join(GAME, rel)
            row = dict(framework=name, rel=rel, staged=info['sha256'], original=None)
            if os.path.exists(dst):
                if sha(dst) == info['sha256']:
                    continue        # already the staged file
                if os.path.exists(dst + BAK):
                    sys.exit(f'{dst + BAK} exists already: a previous test was not restored')
                row['original'] = dict(sha256=sha(dst), size=os.path.getsize(dst), links=links(dst))
                os.rename(dst, dst + BAK)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copyfile(src, dst)
            assert sha(dst) == info['sha256'], dst
            journal['frameworks'].append(row)
            json.dump(journal, open(JOURNAL, 'w', encoding='utf-8'), indent=1)
    files = port_files(a)
    for rel, src_rel in files.items():
        src, dst = os.path.join(REPO, src_rel), os.path.join(GAME, rel)
        if os.path.exists(dst) and (seed_only(rel) or sha(dst) == sha(src)):
            continue
        row = dict(rel=rel, source=src_rel, new=sha(src), previous=None)
        if os.path.exists(dst):
            if links(dst) > 1:
                sys.exit(f'{dst} is a hardlink: not a port file, refusing to copy over it')
            k = os.path.join(keep, rel)
            os.makedirs(os.path.dirname(k), exist_ok=True)
            shutil.copy2(dst, k)
            row['previous'] = dict(sha256=sha(dst), kept=os.path.relpath(k, REPO))
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy2(src, dst)
        journal['port'].append(row)
        json.dump(journal, open(JOURNAL, 'w', encoding='utf-8'), indent=1)
    print(f'installed: {len(journal["frameworks"])} framework files, {len(journal["port"])} port files; '
          f'journal {JOURNAL}')


def restore(a):
    if not os.path.exists(JOURNAL):
        sys.exit('no test install recorded')
    if game_running():
        sys.exit('close the game first')
    journal = json.load(open(JOURNAL, encoding='utf-8'))
    problems = 0
    for row in reversed(journal['port']):
        dst = os.path.join(GAME, row['rel'])
        if row['previous']:
            shutil.copy2(os.path.join(REPO, row['previous']['kept']), dst)
            if sha(dst) != row['previous']['sha256']:
                print('MISMATCH after restore', dst)
                problems += 1
        elif os.path.exists(dst):
            os.remove(dst)
    for row in reversed(journal['frameworks']):
        dst = os.path.join(GAME, row['rel'])
        if os.path.exists(dst):
            if sha(dst) != row['staged']:
                print('changed during the test (kept as .cvr25test):', dst)
                os.replace(dst, dst + '.cvr25test')
            else:
                os.remove(dst)
        o = row['original']
        if o:
            os.rename(dst + BAK, dst)
            if sha(dst) != o['sha256'] or os.path.getsize(dst) != o['size'] or links(dst) != o['links']:
                print('ORIGINAL DIFFERS after restore', dst)
                problems += 1
    os.replace(JOURNAL, os.path.join(WORK, journal['stamp'], 'journal.json'))
    print('restored', len(journal['frameworks']), 'framework files and', len(journal['port']), 'port files;',
          'problems:', problems)


def status(a):
    if not os.path.exists(JOURNAL):
        print('no test install active')
        return
    j = json.load(open(JOURNAL, encoding='utf-8'))
    print('active since', j['stamp'], len(j['frameworks']), 'framework files,', len(j['port']), 'port files')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('cmd', choices=['install', 'restore', 'status', 'deploy-port'])
    ap.add_argument('--stage', default=os.path.join(REPO, 'build', 'patchday', 'stage-2.5'))
    ap.add_argument('--port-dll', default=os.path.join(REPO, 'build', 'patchday', 'sdk25-build', 'bin', 'red4ext',
                                                        'plugins', 'cyberpunkvrport', 'Release'))
    ap.add_argument('--twins', default=os.path.join(REPO, 'build', 'patchday', 'shaders-2.5', 'twins'),
                    help='shader twins rebuilt for the new build (regen_twins.py); "" for none')
    a = ap.parse_args()
    {'install': install, 'restore': restore, 'status': status, 'deploy-port': deploy_port}[a.cmd](a)


if __name__ == '__main__':
    main()
