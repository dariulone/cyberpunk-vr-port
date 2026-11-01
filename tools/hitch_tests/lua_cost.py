"""lua_cost.py CAPTURE: GameThread samples inside LuaVM::Update split by what the leaf was: game code (exe), the
binding layer (sol / RTTIHelper / Scripting / Converter / ClassType / handles), the LuaJIT VM and JIT traces, GC and
allocation, the CRT heap, and other; as % of GameThread and ms per frame. Then the top game entry points
(the first exe frame below the binding) by inclusive samples."""
import collections
import json
import sys

v = json.load(open(sys.argv[1], encoding='utf-8'))
gt = v['gt']
frames = len(v.get('frames') or []) or 1
secs = gt[-1][0] - gt[0][0]
n = len(gt)
P = 'cyberpunkvrport.dll!'
BIND = ('sol::', 'RTTIHelper', 'Scripting::', 'Converter', 'ClassType', 'ClassReference', 'StrongReference',
        'WeakReference', 'Type::', 'LuaRED', 'RTTILocator', 'tsl::', 'FunctionOverride', 'ClassStatic', 'GameOptions',
        'Enum', 'luaL_', 'lua_', 'TiltedPhoques', 'Single', 'Array', 'CNamePool', 'RED4ext::')
GC = ('lj_gc', 'lj_mem', 'lj_alloc', 'lj_tab_free', 'lj_func_free', 'lj_udata_free', 'gc_', 'luaJIT_profile_stop')
VM = ('lj_vm', 'lj_BC', 'lj_meta', 'lj_tab', 'lj_str', 'lj_buf', 'lj_strfmt', 'lj_ff', 'lj_lib', 'lj_cf', 'lj_dispatch',
      'lj_trace', 'lj_record', 'lj_snap', 'lj_opt', 'lj_asm', 'lj_ir', 'lj_crecord', 'lj_mcode', 'lj_')
cat = collections.Counter()
entry = collections.Counter()
for r in gt:
    s = r[1]
    if not any('LuaVM::Update' in f for f in s):
        continue
    leaf = s[0]
    name = leaf.split('!')[-1]
    if leaf.startswith('exe+'):
        c = 'game code'
    elif leaf.startswith(P) and name.startswith(GC):
        c = 'gc/alloc'
    elif leaf.startswith(P) and name.startswith(VM):
        c = 'luajit vm'
    elif leaf.startswith(P) and any(name.startswith(b) or b in name[:40] for b in BIND):
        c = 'binding'
    elif leaf == '?':
        c = 'jit trace (?)'
    elif leaf.startswith(('ntdll', 'ucrt', 'VCRUNTIME', 'KERNEL')):
        c = 'crt/os: ' + leaf.split('!')[-1][:24]
    elif leaf.startswith(P):
        c = 'plugin other: ' + name[:40]
    else:
        c = 'other: ' + leaf[:40]
    # GC anywhere above the leaf (frees inside the sweep) counts as gc
    if c.startswith(('crt/os', 'binding', 'plugin other')) and any(f.startswith(P) and f.split('!')[-1].startswith(GC) for f in s[:8]):
        c = 'gc/alloc'
    cat[c] += 1
    # the first exe frame from the top of the binding = the game entry the Lua called
    for i, f in enumerate(s):
        if f.startswith('exe+') and i + 1 < len(s) and s[i + 1].startswith(P):
            entry[f] += 1
            break
tot = sum(cat.values())
print('Lua samples %d = %.1f%% GameThread, %.2f ms/frame' % (tot, 100 * tot / n, tot / n * secs * 1000 / frames))
for c, k in cat.most_common(25):
    print('  %5.1f%% of Lua  %5.2f ms/frame  %s' % (100 * k / tot, k / n * secs * 1000 / frames, c))
print('top game entry points under Lua (inclusive, ms/frame):')
for f, k in entry.most_common(15):
    print('  %5.2f  %s' % (k / n * secs * 1000 / frames, f))
