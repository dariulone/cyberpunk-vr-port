"""The VR lobby's authoring pipeline in one place.

    py -3.13 tools/vr_lobby_assets/lobby_pipeline.py poses             the game meshes the layout stands in (the pod's
                                                                       figure, the table's desk) -> build/lobby-blender/poses (glb)
    py -3.13 tools/vr_lobby_assets/lobby_pipeline.py build             layout -> art/vr_lobby/reference/lobby_reference.blend
    py -3.13 tools/vr_lobby_assets/lobby_pipeline.py render [--samples N] [--size W] [--tag NAME]
                                                                       the player's 360 view, compared with the reference
    py -3.13 tools/vr_lobby_assets/lobby_pipeline.py export            the .blend -> build/lobby-reference/export (glb + manifest)
    py -3.13 tools/vr_lobby_assets/lobby_pipeline.py all [...]         build and render

BLENDER (environment) overrides the Blender executable.
"""
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
BLENDER = os.environ.get('BLENDER', str(Path.home() / 'Desktop/blender-5.3.0-beta+v53.2ebd15112ac6-windows.amd64-release'
                                        '/blender.exe'))
BLEND = ROOT / 'art/vr_lobby/reference/lobby_reference.blend'
LOGS = ROOT / 'build/lobby-reference'


def blender(args, log):
    LOGS.mkdir(parents=True, exist_ok=True)
    t = time.time()
    with open(LOGS / log, 'w', encoding='utf-8', errors='replace') as f:
        r = subprocess.run([BLENDER, '--background'] + args, stdout=f, stderr=subprocess.STDOUT)
    text = (LOGS / log).read_text(encoding='utf-8', errors='replace')
    if r.returncode or 'Traceback' in text:
        tail = text[text.find('Traceback'):] if 'Traceback' in text else text[-3000:]
        raise SystemExit('blender failed (%s):\n%s' % (log, tail[:4000]))
    print('%s: %.0f s' % (log, time.time() - t), flush=True)


def opt(argv, name, default):
    return argv[argv.index(name) + 1] if name in argv else default


def poses():
    """The game meshes the layout stands in for the game's own (lobby_layout.STAND_INS), uncooked to glb: their
    geometry, in the mesh's frame, where lobby_blender.py imports it."""
    import lobby_layout as LL
    import make_construct as C
    out = ROOT / 'build/lobby-blender/poses'
    for mesh, archive in LL.STAND_INS.items():
        glb = out / (mesh[:-len('.mesh')].replace('\\', '/') + '.glb')
        r = subprocess.run([str(C.S.CLI), 'uncook', str(C.GAME / 'archive/pc/content' / archive), '-o', str(out),
                            '-gp', str(C.GAME), '-r', '^%s$' % re.escape(mesh), '--mesh-export-type', 'MeshOnly'],
                           capture_output=True, text=True)
        if not glb.exists():
            raise SystemExit('uncook gave no %s:\n%s' % (glb, (r.stdout + r.stderr)[-2000:]))
        print('poses:', glb.relative_to(ROOT))


def build():
    blender(['--factory-startup', '--python', str(HERE / 'lobby_blender.py')], 'build.log')


def render(argv):
    tag = opt(argv, '--tag', 'latest')
    blender([str(BLEND), '--python', str(HERE / 'lobby_render.py'), '--', '--samples', opt(argv, '--samples', '64'),
             '--size', opt(argv, '--size', '2048'), '--out', 'panorama_' + tag], 'render.log')
    pano = ROOT / 'art/vr_lobby/reference/renders' / ('panorama_%s.png' % tag)
    subprocess.run([sys.executable, '-B', str(HERE / 'match_sheet.py'), str(pano),
                    str(ROOT / 'art/vr_lobby/match' / tag)], check=True)
    return pano


def export():
    blender([str(BLEND), '--python', str(HERE / 'lobby_export.py')], 'export.log')
    print((LOGS / 'export.log').read_text(encoding='utf-8', errors='replace').split('EXPORTED')[-1].splitlines()[0])


def main():
    cmd, argv = sys.argv[1], sys.argv[2:]
    if cmd == 'poses':
        poses()
    if cmd in ('build', 'all'):
        build()
    if cmd in ('render', 'all'):
        render(argv)
    if cmd == 'export':
        export()


if __name__ == '__main__':
    main()
