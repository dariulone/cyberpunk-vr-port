"""graph_shape.py [--gen N] [--wait S] [--chain]: the shape of the render graphs the game runs
(src/Stereo/RenderGraphShape.cpp, game started with CVR_GRAPH_SHAPE=1).

Sets CyberpunkVR_GraphShapeGen, waits, and prints every recorded graph: runs, nodes, dependency edges, the
longest chain of dependent nodes and the widest level. --chain also lists the longest chain's nodes by work
function RVA, with how much of the chain is a plain sequence (each node's only prerequisite is the one before)."""
import argparse
import collections
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'live'))
import probe as P  # noqa: E402

SLOTS, MAX_NODES, MAX_EDGES = 8, 512, 2048
HEAD = struct.Struct('<QQ6I')
SLOT = HEAD.size + MAX_NODES * 4 + MAX_EDGES * 4

ap = argparse.ArgumentParser()
ap.add_argument('--gen', type=int, default=int(time.time()) & 0x7FFFFFFF)
ap.add_argument('--wait', type=float, default=2.0)
ap.add_argument('--chain', action='store_true')
ap.add_argument('--nodes', action='store_true', help='every node: job index, work RVA, prerequisites, dependants')
a = ap.parse_args()

t = P.Target(P.find_pid(), writable=True)
ex = P.exports(t, t.module_base('cyberpunkvrport.dll'))
assert 'CyberpunkVR_GraphShapes' in ex, 'plugin without the graph shape probe'
t.write(ex['CyberpunkVR_GraphShapeGen'], struct.pack('<I', a.gen))
time.sleep(a.wait)
raw = t.read(ex['CyberpunkVR_GraphShapes'], SLOTS * SLOT)
t.write(ex['CyberpunkVR_GraphShapeGen'], struct.pack('<I', 0))

for i in range(SLOTS):
    off = i * SLOT
    graph, runs, gen, nodes, edges, depth, width, _ = HEAD.unpack_from(raw, off)
    if not graph or gen != a.gen:
        continue
    if nodes == 0xFFFFFFFF:
        print('graph %016X  runs %d  (unreadable)' % (graph, runs))
        continue
    work = struct.unpack_from('<%dI' % MAX_NODES, raw, off + HEAD.size)
    ed = struct.unpack_from('<%dH' % (MAX_EDGES * 2), raw, off + HEAD.size + MAX_NODES * 4)
    ed = [(ed[2 * k], ed[2 * k + 1]) for k in range(edges)]
    print('graph %016X  runs %4d  nodes %3d  edges %4d  longest chain %3d  widest level %3d' % (graph, runs, nodes, edges, depth, width))
    n = min(nodes, MAX_NODES)
    pred = collections.defaultdict(list)
    succ = collections.defaultdict(list)
    for x, y in ed:
        pred[y].append(x)
        succ[x].append(y)
    if a.nodes:
        print('   ' + ' '.join('%d:%06X(%d/%d)' % (v, work[v], len(pred[v]), len(succ[v])) for v in range(n)))
    if not a.chain:
        continue
    # longest path by node count
    indeg = {v: len(pred[v]) for v in range(n)}
    order = [v for v in range(n) if not indeg[v]]
    best = {v: 1 for v in range(n)}
    back = {v: None for v in range(n)}
    k = 0
    while k < len(order):
        v = order[k]
        k += 1
        for w in succ[v]:
            if best[v] + 1 > best[w]:
                best[w], back[w] = best[v] + 1, v
            indeg[w] -= 1
            if not indeg[w]:
                order.append(w)
    end = max(range(n), key=lambda v: best[v])
    chain = []
    while end is not None:
        chain.append(end)
        end = back[end]
    chain.reverse()
    plain = sum(1 for v in chain[1:] if len(pred[v]) == 1)
    print('   chain of %d: %d with a single prerequisite; nodes without prerequisites %d, without dependants %d' % (
        len(chain), plain, sum(1 for v in range(n) if not pred[v]), sum(1 for v in range(n) if not succ[v])))
    print('   ' + ' '.join('%06X' % work[v] for v in chain))
