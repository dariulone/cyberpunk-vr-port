"""Attach to a user-owned OpenXR Simulator run. Uses the installed CVRBridge."""
import argparse
import importlib.util
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
QUERY = """(function()
    local r=assert(GetMod('CyberpunkVRPort_Reload').reload())
    local m=assert(r.magFlick,'mag_flick is not loaded')
    local w=assert(Game.GetPlayer():GetActiveWeapon())
    return {cfg=r.cfgOf(w),entity=tostring(w:GetEntityID().hash),ammo=w:GetMagazineAmmoCount(),
        left=Game.VRWeaponInLeftHand(),mirrored=VRLeftHanded() and VRLeftHandedBindings()>=1,
        trigger=GetVRSharedSlot(Game.VRWeaponInLeftHand() and 154 or 160),
        b=GetVRSharedSlot(157),support=GetVRTwoHandActive(),gameplay=Game.VRWeaponTriggerEnabled(),
        releases=m.releases,flicks=m.flicks,delta=m.lastDelta or 0,dbg=r.dbg()}
end)()"""


def run(pid, case):
    sys.path.insert(0,str(ROOT/'tools/sim_harness'))
    from simxr import Sim, _exports, _modules
    modules={name.lower():(addr,path) for name,addr,path in _modules(pid)}
    if 'cyberpunk2077.exe' not in modules or 'openxr_simulator.dll' not in modules:
        raise RuntimeError('Expected a running simulator game at the supplied PID')
    base,dll=modules['cyberpunkvrport.dll']
    spec=importlib.util.spec_from_file_location('cpvr_bridge',ROOT/'tools/grasp_tests/corpse_live/run.py')
    protocol=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(protocol)
    bridge=protocol.Bridge(Path(dll).parent/'lua/CVRBridge')
    def observe():
        return bridge.call(QUERY,timeout=4)
    sim=Sim(pid,focus_away=False)
    pad=base+_exports(dll)['CyberpunkVR_TestPadButtons']
    saved=sim._i32(pad)
    before=observe()
    if before['cfg']!='constitutional_unity' or 'mag[in/' not in before['dbg'] or before['ammo']<=0:
        raise RuntimeError('Equip and load one Unity before each case')
    side='left' if before['left'] else 'right'
    free='right' if before['left'] else 'left'
    sign=-1 if before['left'] else 1
    button=0x8000 if before['mirrored'] else 0x2000
    holder_grip=0x10000 if before['left'] else 0x20000
    if saved & button or before['b']>.5:
        raise RuntimeError('Release the secondary button first')
    def place(offset):
        sim.hand(side,pos=(sign*(.17+offset),-.28,-.37),ypr=(0,-40,0),mode='body')
    def set_button(down):
        flags=saved|holder_grip
        sim._write(pad,struct.pack('<i',flags|button if down else flags))
    try:
        sim.hands_follow_head(True)
        sim.hand(free,pos=(-sign*.40,-.35,-.17),ypr=(0,0,0),mode='body')
        set_button(False)
        place(0);time.sleep(.4)
        ready=observe()
        if (not ready['gameplay'] or ready['support'] or ready['trigger']>.2 or ready['b']>.5 or
                ready['entity']!=before['entity'] or ready['ammo']!=before['ammo'] or 'mag[in/' not in ready['dbg']):
            raise RuntimeError('Start in gameplay, free hand away, B and firing trigger up: '+str(ready))
        start=time.perf_counter()
        set_button(case!='unarmed')
        # Identical B duration for both releases. Hand-authored simulator input,
        # right/left reflected across body X, fixed controller orientation.
        for i,offset in enumerate((.013,.026,.039,.052,.065,.078,.091,.104,.117,.130),start=1):
            deadline=start+.040+.012*i
            time.sleep(max(0,deadline-time.perf_counter()))
            if case!='still':
                place(offset)
        time.sleep(.12)
        held=observe()
        set_button(False)
        time.sleep(.10)
        after=observe()
        release=int(case!='unarmed')
        flick=int(case=='flick')
        def matches(sample):
            return (sample['entity']==ready['entity'] and sample['releases']==ready['releases']+release
                    and sample['flicks']==ready['flicks']+flick
                    and sample['ammo']==(0 if release else ready['ammo'])
                    and ('mag[gone/' if release else 'mag[in/') in sample['dbg'])
        passed=(matches(held) and matches(after) and (held['b']>.5)==bool(release)
                and after['b']<.5 and (not release or (held['delta']>.5 if flick else held['delta']==0)))
        return dict(pid=pid,case=case,passed=passed,ready=ready,held=held,after=after)
    finally:
        try:
            sim._write(pad,struct.pack('<i',saved))
        finally:
            sim.restore()


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid',type=int)
    parser.add_argument('case',choices=('unarmed','still','flick'))
    parser.add_argument('--out',type=Path)
    args=parser.parse_args()
    result=run(args.pid,args.case)
    report=json.dumps(result,indent=2,allow_nan=False)
    print(report)
    if args.out:
        args.out.parent.mkdir(parents=True,exist_ok=True)
        args.out.write_text(report+'\n',encoding='utf-8')
    sys.exit(0 if result['passed'] else 1)
