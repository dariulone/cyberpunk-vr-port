"""IDA script (idat -A -S on GameAssembly.i64): decompiles the functions listed in tools/targets.txt
("Type Method RVA" per line) into decompiled/<Type>__<Method>.c, then quits without saving."""
import os
import ida_funcs
import ida_hexrays
import ida_pro
import idaapi

HERE = os.path.dirname(os.path.abspath(__file__))
# The study folder: names/ (il2cpp_names.py output), idb/, decompiled/ and tools/targets.txt.
STUDY = os.environ.get('IL2CPP_STUDY', os.path.join(HERE, '..'))
OUT = os.path.join(STUDY, 'decompiled')
os.makedirs(OUT, exist_ok=True)
base = idaapi.get_imagebase()
ida_hexrays.init_hexrays_plugin()
done = 0
for line in open(os.path.join(STUDY, 'tools', 'targets.txt'), encoding='utf-8'):
    parts = line.split()
    if len(parts) != 3:
        continue
    typ, method, rva = parts
    ea = base + int(rva, 16)
    if not ida_funcs.get_func(ea):
        ida_funcs.add_func(ea)
    path = os.path.join(OUT, '%s__%s.c' % (typ, method))
    try:
        cf = ida_hexrays.decompile(ea)
        text = str(cf) if cf else '// decompilation failed'
    except Exception as e:
        text = '// decompilation failed: %s' % e
    with open(path, 'w', encoding='utf-8') as f:
        f.write('// %s.%s @ RVA %s\n%s\n' % (typ, method, rva, text))
    done += 1
print('decompiled', done)
ida_pro.qexit(0)
