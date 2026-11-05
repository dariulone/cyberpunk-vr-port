"""A car's cabin wheel turns further for the same steering input: the hands then turn further for the same road-wheel angle.

The hands hold the cabin wheel one to one (WheelRange.hpp: the hands' share of the cabin wheel's full lock is turned
into the input that stands the cabin wheel at the hands' angle). So the steering's sensitivity IS the cabin wheel's
gain: `mulFactor` of the animAnimNode_FloatTrackDirectConnConstraint that turns `steering_wheel_turn` by
`steer_turn_rate` on RotY. It was doubled (1 -> 2, build_wheel_graphs.py, full lock 65 -> 130 degrees); players found
that too sharp (2026-11-05: "the wheel is too sharp, make it a little less") -> 2.5, full lock 162.5 degrees. Road wheels
and the drive model are untouched: only the cabin wheel and with it the hands turn further.

    py -3.13 tools/driving_tests/wheel_ratio_graph.py [factor]

Every road-car graph in the one-archive pack tree (and its copy in mods/archive/build): serialized, the one field
changed, deserialized, serialized again and compared with the source in every other field.
"""
import json
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACK = ROOT / 'build/one-archive-20261027/pack/cyberpunkvrport'
COPY = ROOT / 'mods/archive/build'
WORK = ROOT / 'build/wheel-ratio-20261105'
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
FACTOR = float(sys.argv[1]) if len(sys.argv) > 1 else 2.5
OLD = 2.0


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit('%s failed:\n%s%s' % (args[:3], r.stdout[-2000:], r.stderr[-2000:]))


def nodes(o):
    if isinstance(o, dict):
        yield o
        for v in o.values():
            yield from nodes(v)
    elif isinstance(o, list):
        for v in o:
            yield from nodes(v)


def name(node, field):
    return (node.get(field) or {}).get('name', {}).get('$value')


def wheel_nodes(doc):
    return [n for n in nodes(doc['Data']) if n.get('$type') == 'animAnimNode_FloatTrackDirectConnConstraint'
            and name(n, 'transformIndex') == 'steering_wheel_turn']


graphs = sorted(p for p in PACK.rglob('*.animgraph') if 'vehicles' in p.parts)
print('road-car graphs in the pack:', len(graphs))
if WORK.exists():
    shutil.rmtree(WORK)
for sub in ('src', 'json', 'out', 'check'):
    (WORK / sub).mkdir(parents=True)
flat = {}
for g in graphs:
    rel = g.relative_to(PACK)
    key = rel.as_posix().replace('/', '__')
    shutil.copyfile(g, WORK / 'src' / key)
    flat[key] = rel
run(CLI, 'convert', 'serialize', WORK / 'src', '-o', WORK / 'json')
changed = 0
for key, rel in flat.items():
    j = WORK / 'json' / (key + '.json')
    doc = json.loads(j.read_text(encoding='utf-8'))
    ws = wheel_nodes(doc)
    assert len(ws) == 1, (rel, len(ws))
    n = ws[0]
    assert name(n, 'floatTrackIndex') == 'steer_turn_rate' and n['channel'] == 'RotY', rel
    if abs(n['mulFactor'] - FACTOR) < 1e-6:
        continue
    assert abs(n['mulFactor'] - OLD) < 1e-6, (rel, n['mulFactor'])
    n['mulFactor'] = FACTOR
    j.write_text(json.dumps(doc, indent=1), encoding='utf-8')
    changed += 1
print('mulFactor %.2f -> %.2f in %d graphs' % (OLD, FACTOR, changed))
run(CLI, 'convert', 'deserialize', WORK / 'json', '-o', WORK / 'out')
# the rebuilt files, serialized again, against the sources in every field but the one
outs = {p.name: p for p in (WORK / 'out').rglob('*.animgraph')}
assert len(outs) == len(flat), (len(outs), len(flat))
shutil.copytree(WORK / 'out', WORK / 'check', dirs_exist_ok=True)
run(CLI, 'convert', 'serialize', WORK / 'check', '-o', WORK / 'check')
for key, rel in flat.items():
    src = json.loads((WORK / 'json' / (key + '.json')).read_text(encoding='utf-8'))['Data']
    back = next((WORK / 'check').rglob(key + '.json'))
    got = json.loads(back.read_text(encoding='utf-8'))['Data']
    assert got == src, 'the round trip changed more than mulFactor: %s' % rel
print('round trip: every graph equal to its source but the one field')
for key, rel in flat.items():
    data = outs[key].read_bytes()
    for base in (PACK, COPY):
        dst = base / rel
        if base is COPY and not dst.exists():
            continue
        dst.write_bytes(data)
print('written into the pack tree and mods/archive/build')
