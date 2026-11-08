"""Runs the handful generator for a gun: prepare.py's input, handful.exe, the pictures to PNG, the data into
reload/handful/<gun>.lua. Orchestration and file I/O only; the geometry is handful.cpp's.

    py -3.13 -B run.py [tactician|carnage ...] [--no-install]
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..'))
OUT = os.path.join(HERE, 'out')
DEST = os.path.join(REPO, 'mods', 'cet', 'CyberpunkVRPort_Reload', 'reload', 'handful')


def ppm_to_png(src, dst):
    from PIL import Image
    Image.open(src).save(dst)
    os.remove(src)


def main(argv):
    install = '--no-install' not in argv
    guns = [a for a in argv if not a.startswith('--')] or ['tactician', 'carnage']
    exe = os.path.join(HERE, 'handful.exe')
    if not os.path.exists(exe):
        sys.exit('build handful.exe first (build.bat)')
    os.makedirs(OUT, exist_ok=True)
    for gun in guns:
        txt = os.path.join(OUT, gun + '.txt')
        subprocess.check_call([sys.executable, '-B', os.path.join(HERE, 'prepare.py'), gun, txt])
        prefix = os.path.join(OUT, gun)
        subprocess.check_call([exe, txt, prefix])
        for name in sorted(os.listdir(OUT)):
            if name.startswith(gun + '_') and name.endswith('.ppm'):
                ppm_to_png(os.path.join(OUT, name), os.path.join(OUT, name[:-4] + '.png'))
        if install:
            os.makedirs(DEST, exist_ok=True)
            with open(prefix + '.lua', encoding='utf-8') as s, open(os.path.join(DEST, gun + '.lua'), 'w',
                                                                    encoding='utf-8', newline='\n') as d:
                d.write(s.read())
            print('installed', os.path.join(DEST, gun + '.lua'))


if __name__ == '__main__':
    main(sys.argv[1:])
