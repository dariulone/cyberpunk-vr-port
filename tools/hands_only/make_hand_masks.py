"""Which render chunks of the player's arm meshes are the HANDS: the table the hands-only mode cuts the arms with.

    py -3.13 -B tools/hands_only/make_hand_masks.py            all steps
    py -3.13 -B tools/hands_only/make_hand_masks.py extract    every player arm mesh out of the archives, as JSON
    py -3.13 -B tools/hands_only/make_hand_masks.py report     each mesh's chunks and what they are skinned to
    py -3.13 -B tools/hands_only/make_hand_masks.py lua        mods/cet/CyberpunkVRPort_HandsOnly/data/hand_masks.lua

WHY. Hands only (xr_body_hands_only) draws the player's hands and nothing else of the body. The arms are one mesh per
arm (or one for both) from the shoulder to the fingertips, split into render chunks by the material and by the region
the artists cut: the shoulder and upper arm, the forearm, the hand. A component draws the chunks its `chunkMask` has
set, so the arms keep only the hand chunks. Which chunks those are is read here from the meshes themselves: each
vertex belongs to the bone that weighs it most, and a chunk whose vertices are the hand's (85 % of them on the
hand and finger bones) is a hand chunk; one that runs on up the forearm is not. The game also sets chunk masks on the arms itself (the
holstered Gorilla Arms are drawn as two components with complementary masks), so the mode ANDs its mask into the
game's rather than writing over it.

Pieces of cyberware that are what the arm DOES -- the Mantis Blades' blades, the Monowire's wire, the Projectile Launch System's shells,
fragments and ammunition -- are kept whole: they come out of the forearm when used, and hiding them would hide the
weapon.

THE TABLE: the mesh's depot path hash (FNV-1a 64, what a component's `mesh.hash` reads) -> the mask of the chunks to
keep (bit i = chunk i; a mesh has far fewer than 32 chunks).
"""
import base64
import collections
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
WORK = ROOT / 'build/hands-only-20261103/all'
LUA_OUT = ROOT / 'mods/cet/CyberpunkVRPort_HandsOnly/data/hand_masks.lua'
BS = chr(92)
# Every arm mesh of the player: the two bodies' arms (and their decals, nails, fists) and the arm cyberware.
SEP = BS + BS                                  # a backslash, in a regular expression
NAME = '[^' + SEP + ']+'
PATTERNS = [p.replace('/', SEP).replace('<n>', NAME) for p in (
    'base/characters/common/player_base_bodies/<n>/.*/a0_<n>[.]mesh',
    'base/characters/cyberware/player/a0_<n>/(.*/)?a0_<n>[.]mesh',
    'ep1/characters/cyberware/player/(.*/)?a0_<n>[.]mesh',
)]
SOURCES = [GAME / 'archive/pc/content', GAME / 'archive/pc/ep1']
SIZES = {'PT_Short4N': 8, 'PT_UByte4': 4, 'PT_UByte4N': 4, 'PT_Float16_2': 4, 'PT_Dec4': 4, 'PT_Color': 4,
         'PT_Float16_4': 8, 'PT_Float3': 12}
WHOLE = ('_blade', 'shells', 'ammo', 'fragment', 'nails', 'fists', '_whip1_')


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode != 0:
        sys.stdout.write(r.stdout[-1500:] + r.stderr[-1500:])
    return r


def fnv1a64(text):
    h = 0xCBF29CE484222325
    for b in text.encode('utf-8'):
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def cmd_extract():
    raw = WORK / 'raw'
    rx = '^(' + '|'.join(PATTERNS) + ')$'
    for src in SOURCES:
        run(CLI, 'unbundle', src, '-o', raw, '-r', rx, '-v', 'Minimal')
    meshes = sorted(raw.rglob('*.mesh'))
    flat = WORK / 'flat'
    flat.mkdir(parents=True, exist_ok=True)
    index = {}
    for m in meshes:
        rel = m.relative_to(raw).as_posix().replace('/', BS)
        name = rel.replace(BS, '__')
        (flat / name).write_bytes(m.read_bytes())
        index[name] = rel
    (WORK / 'index.json').write_text(json.dumps(index, indent=1), encoding='utf-8')
    out = WORK / 'json'
    out.mkdir(parents=True, exist_ok=True)
    run(CLI, 'convert', 'serialize', flat, '-o', out, '-v', 'Minimal')
    print('%d meshes, %d serialized' % (len(meshes), len(list(out.glob('*.json')))))


def region(bone):
    l = (bone or '').lower()
    if any(k in l for k in ('hand', 'thumb', 'index', 'middle', 'ring', 'pinky', '_ind', '_mid', '_rng', '_pnk',
                            'finger', 'palm', 'knuckle')):
        return 'hand'
    if 'wrist' in l:
        return 'wrist'
    if 'forearm' in l or 'elbow' in l:
        return 'forearm'
    if any(k in l for k in ('arm', 'deltoid', 'triceps', 'biceps', 'shl', 'shoulder', 'scapula', 'clav')):
        return 'upper'
    return 'body'


def chunks(path):
    d = json.loads(path.read_text(encoding='utf-8'))
    r = d['Data']['RootChunk']
    names = [b.get('$value') for b in r.get('boneNames', [])]
    rb = r['renderResourceBlob']['Data']
    h = rb['header']
    buf = base64.b64decode(rb['renderBuffer']['Bytes'])
    out = []
    for c in h['renderChunkInfos']:
        el = c['chunkVertices']['vertexLayout']['elements']['Elements']
        per = [e for e in el if e['streamIndex'] == 0 and e['type'] != 'PT_Invalid']
        stride = sum(SIZES[e['type']] for e in per)
        n = c['numVertices']
        off0 = c['chunkVertices']['byteOffsets']['Elements'][0]
        ni = sum(1 for e in per if e['usage'] == 'PS_SkinIndices') * 4
        cnt = collections.Counter()
        if ni and n:
            rows = buf[off0:off0 + n * stride]
            for v in range(n):
                row = rows[v * stride:(v + 1) * stride]
                idx = row[8:8 + ni]
                w = row[8 + ni:8 + 2 * ni]
                k = max(range(ni), key=lambda i: w[i])
                cnt[region(names[idx[k]] if idx[k] < len(names) else '')] += 1
        out.append((n, cnt))
    return out


def classify(name, chs):
    """The keep mask: a chunk mostly hand and wrist ring, or every chunk of a piece that is the arm's weapon."""
    if any(k in name for k in WHOLE):
        return (1 << len(chs)) - 1, 'whole'
    mask = 0
    for i, (n, cnt) in enumerate(chs):
        # the hand's own vertices only: a chunk that runs on up the forearm (the Gorilla Arms' skin) is cut away
        if n and cnt['hand'] >= 0.85 * n:
            mask |= 1 << i
    return mask, 'hand chunks'


def table():
    index = json.loads((WORK / 'index.json').read_text(encoding='utf-8'))
    rows = []
    for name, rel in sorted(index.items()):
        j = WORK / 'json' / (name + '.json')
        if not j.exists():
            continue
        chs = chunks(j)
        mask, why = classify(rel.lower(), chs)
        rows.append((rel, fnv1a64(rel.lower()), mask, why, chs))
    return rows


def cmd_report():
    for rel, h, mask, why, chs in table():
        print('%s  %d  keep %s (%s)' % (rel.split(BS)[-1], h, bin(mask), why))
        for i, (n, cnt) in enumerate(chs):
            print('    %d: %5d %s' % (i, n, dict(cnt.most_common(4))))


def cmd_lua():
    rows = table()
    lines = ['-- Generated by tools/hands_only/make_hand_masks.py: do not edit.',
             '-- The player\'s arm meshes, by the hash of their depot path (a component\'s mesh.hash), -> the mask of the',
             '-- render chunks that are the hands (bit i = chunk i) and the mesh\'s chunk count.',
             'return {']
    for rel, h, mask, why, chs in rows:
        lines.append('  ["%d"] = { %d, %d }, -- %s' % (h, mask, len(chs), rel.split(BS)[-1]))
    lines.append('}')
    LUA_OUT.parent.mkdir(parents=True, exist_ok=True)
    LUA_OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print('%s: %d meshes' % (LUA_OUT.relative_to(ROOT), len(rows)))


if __name__ == '__main__':
    for step in sys.argv[1:] or ['extract', 'report', 'lua']:
        {'extract': cmd_extract, 'report': cmd_report, 'lua': cmd_lua}[step]()
