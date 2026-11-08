"""Input for handful.exe: the left hand (tools/grasp_tests/rig.txt, the live plugin's reference hands) with a gun's
shell-carry finger pose on it, the carry frame and the shell's size. File I/O only; the geometry is handful.cpp's.

    py -3.13 -B prepare.py <gun> <out.txt>      gun: tactician | carnage
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..'))
RIG = os.path.join(REPO, 'tools', 'grasp_tests', 'rig.txt')
RELOAD = os.path.join(REPO, 'mods', 'cet', 'CyberpunkVRPort_Reload', 'reload')

# The left hand's bones the pose and the model use: 2.5 indices (DumpPlayerBoneNames) and the pose files' names.
NAMES = {
    23: 'LeftHand', 26: 'WeaponLeft',
    33: 'LeftInHandThumb', 34: 'LeftInHandIndex', 35: 'LeftInHandMiddle', 36: 'LeftInHandRing', 37: 'LeftInHandPinky',
    45: 'LeftHandThumb1', 46: 'LeftHandIndex1', 47: 'LeftHandMiddle1', 48: 'LeftHandRing1', 49: 'LeftHandPinky1',
    55: 'LeftHandThumb2', 56: 'LeftHandIndex2', 57: 'LeftHandMiddle2', 58: 'LeftHandRing2', 59: 'LeftHandPinky2',
    65: 'LeftHandIndex3', 66: 'LeftHandMiddle3', 67: 'LeftHandRing3', 68: 'LeftHandPinky3',
}

# Per gun: its config file, the shell mesh's radius and length (mesh bbox, build/shotgun-reload-20261019/meshx), and
# how many shells a handful holds.
GUNS = {
    'tactician': ('constitutional_tactician.lua', 0.0104, 0.0507, 4),
    'carnage': ('budget_carnage.lua', 0.0122, 0.0720, 2),
}

NUM = r'(-?[0-9.]+(?:e-?[0-9]+)?)'


def carry(cfg_text):
    m = re.search(r'carry\s*=\s*\{\s*off\s*=\s*\{\s*' + r'\s*,\s*'.join([NUM] * 3) + r'\s*\}\s*,\s*rot\s*=\s*\{\s*' +
                  r'\s*,\s*'.join([NUM] * 4) + r"\s*\}\s*,\s*pose\s*=\s*'([^']+)'", cfg_text)
    if not m:
        sys.exit('no carry = { off, rot, pose } in the config')
    return [float(v) for v in m.groups()[:7]], m.group(8)


def pose(name):
    rows = {}
    for line in open(os.path.join(RELOAD, 'poses', name + '.lua'), encoding='utf-8'):
        m = re.match(r"\s*\{\s*'(\w+)'\s*,\s*" + r'\s*,\s*'.join([NUM] * 4), line)
        if m:
            rows[m.group(1)] = [float(v) for v in m.groups()[1:]]
    return rows


def main(gun, out):
    cfg_name, radius, length, count = GUNS[gun]
    text = open(os.path.join(RELOAD, 'weapons', cfg_name), encoding='utf-8').read()
    frame, pose_name = carry(text)
    p = pose(pose_name)
    bones = {}
    for line in open(RIG, encoding='utf-8'):
        f = line.split()
        if f and f[0] == 'bone' and int(f[1]) in NAMES:
            bones[int(f[1])] = (int(f[2]), [float(v) for v in f[4:7]], [float(v) for v in f[7:11]])
    missing = [i for i in NAMES if i not in bones and i != 23]
    if missing:
        sys.exit('rig.txt lacks bones %s' % missing)
    with open(out, 'w', encoding='utf-8') as o:
        o.write('# %s: carry pose %s\n' % (gun, pose_name))
        for i in sorted(NAMES):
            if i == 23:
                o.write('bone 23 -1 0 0 0 0 0 0 1 LeftHand\n')
                continue
            parent, pos, rot = bones[i]
            if NAMES[i] in p:
                rot = p[NAMES[i]]
            o.write('bone %d %d %.7f %.7f %.7f %.7f %.7f %.7f %.7f %s\n' % ((i, parent) + tuple(pos) + tuple(rot) +
                                                                       (NAMES[i],)))
        o.write('slot 26\n')
        o.write('shell %.5f %.5f\n' % (radius, length))
        o.write('carry %s\n' % ' '.join('%.6f' % v for v in frame))
        o.write('count %d\n' % count)
        o.write('gun %s\n' % gun)
    print('wrote', out, 'pose', pose_name, 'bones', len(NAMES), 'posed', sum(1 for n in NAMES.values() if n in p))


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
