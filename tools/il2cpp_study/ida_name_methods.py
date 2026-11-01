"""IDA script (idat -a -A -S): names every IL2CPP method of names/methods.tsv at its address, makes functions of the
classes studied, saves the database and quits. No auto-analysis of the whole 76 MB binary."""
import os
import ida_auto
import ida_funcs
import ida_loader
import ida_name
import ida_pro
import idaapi

HERE = os.path.dirname(os.path.abspath(__file__))
# The study folder: names/ (il2cpp_names.py output), idb/, decompiled/ and tools/targets.txt.
STUDY = os.environ.get('IL2CPP_STUDY', os.path.join(HERE, '..'))
NAMES = os.path.join(STUDY, 'names', 'methods.tsv')
OUT = os.path.join(STUDY, 'idb', 'GameAssembly.i64')
STUDIED = {'RemapRig', 'OpenControllerRig', 'ControllerRig', 'PhysicsRig', 'PhysTorso', 'PhysHand', 'Hand', 'Grip',
           'TargetGrip', 'CylinderGrip', 'BoxGrip', 'HandJointConfiguration', 'HandGripPair', 'InterpRig',
           'GameWorldSkeletonRig', 'HeptaRig', 'Rig', 'RigManager', 'HandgunVirtualController', 'BodyVirtualController',
           'AnimationRig', 'ArtRig', 'Gun', 'Magazine', 'MarrowBody', 'SkeletonHand', 'HandPoseAnimator'}

base = idaapi.get_imagebase()
seen = {}
made = 0
for line in open(NAMES, encoding='utf-8'):
    image, ns, typ, method, params, rva = line.rstrip('\n').split('\t')
    if rva == '-':
        continue
    ea = base + int(rva, 16)
    clean = ''.join(c if c.isalnum() or c == '_' else '_' for c in '%s__%s' % (typ.replace('/', '_'), method))
    if ea not in seen:
        seen[ea] = clean
        ida_name.set_name(ea, clean, ida_name.SN_NOWARN | ida_name.SN_NOCHECK | ida_name.SN_FORCE)
    if typ.split('/')[0] in STUDIED and ns.startswith('SLZ'):
        if ida_funcs.add_func(ea):
            made += 1
ida_auto.auto_wait()
print('named', len(seen), 'functions made', made)
ida_loader.save_database(OUT, 0)
ida_pro.qexit(0)
