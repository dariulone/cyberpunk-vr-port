"""Attach-only Ajax release regression. Requires exclusive use of a save-locked simulator."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import time

from live_finger_cadence import ROOT, OBSERVER, WEAPON, equip, focus_preview, observe_actions
from live_weapon_acceptance import PreviewTrigger
from simxr import Sim, _exports, _modules


def replay(bridge, archive):
    take = next(t for t in json.loads(archive.read_text())['series'] if t['input']['auto'] and not t['passed'])
    release = take['input']['edges'][-1]
    rows = [e for e in take['actions'] if e['event']=='receipt' and e['t']<=take['prevTime']]
    assert len(rows)==take['original']-take['remaining'] and all(r['value']==1 for r in rows)
    decisions = []
    try:
        for row in rows:
            up = row['t']>release['hi']
            fresh = row['token']!='0ULL'
            expr = (f'{WEAPON}:VRAutoReleaseBlocks({take["input"]["slot"]},{row["count"]},'
                    f'{0 if up else 1},{str(fresh).lower()})')
            decisions.append(dict(t=row['t'],native=row['value'],count=row['count'],released=up,
                                  blocked=bridge.call(expr)))
    finally:
        # Drop the archived press identity before driving real controller input.
        bridge.call(f'{WEAPON}:VRAutoReleaseBlocks(160,{rows[-1]["count"]+1},0,false)')
    assert sum(d['released'] for d in decisions)==3
    return decisions


def run(pid, archive, out):
    modules = {n.lower():(base,Path(dll)) for n,base,dll in _modules(pid)}
    assert {'cyberpunk2077.exe','openxr_simulator.dll','cyberpunkvrport.dll'} <= modules.keys()
    base, plugin = modules['cyberpunkvrport.dll']
    script=plugin.parent/'scripts/CyberpunkVRPort_NoAnims/vrport_full_auto_release.reds'
    sha=hashlib.sha256(script.read_bytes()).hexdigest()
    assert sha==hashlib.sha256((ROOT/'mods/redscript/CyberpunkVRPort_NoAnims'/script.name).read_bytes()).hexdigest()
    spec=importlib.util.spec_from_file_location('cpvr_bridge',ROOT/'tools/grasp_tests/corpse_live/run.py')
    protocol=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(protocol)
    bridge=protocol.Bridge(plugin.parent/'lua/CVRBridge')
    assert bridge.call('Game.IsSavingLocked()') and not bridge.call('VRLeftHanded()')
    assert bridge.call(OBSERVER+'~=nil') and bridge.call('GetVRSharedSlot(29)')==0
    sim=Sim(pid,focus_away=False)
    trigger=PreviewTrigger(sim)
    flags=base+_exports(plugin)['CyberpunkVR_TestPadButtons']
    old=sim._i32(flags)
    result=dict(pid=pid,scriptSHA256=sha)
    def save():
        out.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    try:
        sim._write(flags,struct.pack('<i',old|0x20000))
        sim.hand('right',pos=(.18,-.14,-.44),ypr=(0,48,-5),mode='body')
        equip(bridge,'Ajax')
        result['replay']=replay(bridge,archive)
        save()
        observe_actions(bridge)
        bridge.call("""
            if not CPVRAutoProbeInstalled then
                ObserveBefore('WeaponTransition','VRReleasedFullAuto',function(_,ctx,si)
                    if not CPVRAutoProbe then return end
                    local key=CName.new('RangedAttack')
                    table.insert(CPVRAutoProbe,{t=Game.VRMeleeRealTime(),raw=GetVRSharedSlot(160),
                        action=si:GetActionValue(key),count=si:GetActionPressCount(key)})
                end)
                CPVRAutoProbeInstalled=true
            end
        """,'exec')
        bridge.call(f'local w={WEAPON};assert(w:GetCurrentTriggerMode():Type()==gamedataTriggerMode.FullAuto);'
                    'assert(GetMod("CyberpunkVRPort_Reload").reload().util.setMagAmmo(w,WeaponObject.GetMagazineCapacity(w)))','exec')
        trigger.set(False)
        time.sleep(.35)
        focus_preview(trigger)
        bridge.call('CPVRAutoProbe={};CPVRFingerDiag={}','exec')
        assert bridge.call(OBSERVER+'.start(true,true)')
        try:
            trigger.set(True)
            time.sleep(.38)
            # Slot 29 contributes RT only to the merged XInput packet. Start
            # while RT is already high, then release the physical XR trigger.
            bridge.call('Game.SetVRMeleeFire(512)','exec')
            time.sleep(.40)
            trigger.set(False)
            time.sleep(.62)
            result['remainingForcedPolls']=bridge.call('GetVRSharedSlot(29)')
        finally:
            trigger.set(False)
            bridge.call('Game.SetVRMeleeFire(0)','exec')
            time.sleep(.22)
            result['take']=bridge.call(OBSERVER+'.stop()')
            result['decisions']=bridge.call('CPVRAutoProbe')
            result['actions']=bridge.call('CPVRFingerDiag')
            save()
        take=result['take']
        edges=take['input']['edges']
        assert [e['down'] for e in edges]==[True,False]
        last=edges[-1]['hi']
        nativeDown=[s for s in result['decisions'] if s['t']>=last and s['raw']<=.20 and s['action']==1]
        shots=[s for s in result['actions'] if s['event']=='receipt' and s['t']<=take['prevTime']]
        result['staleWindow']=nativeDown[-1]['t']-nativeDown[0]['t'] if nativeDown else 0
        result['postReleaseRounds']=sum(e['lo']>last for e in take['events'])
        result['passed']=(all(d['blocked']==d['released'] for d in result['replay']) and
            take['input']['auto'] and take['input']['vr'] and not take.get('invalid') and
            result['remainingForcedPolls']>0 and result['staleWindow']>=.4 and
            len({s['count'] for s in nativeDown+shots})==1 and
            take['original']-take['remaining']==len(shots)==len(take['events'])>=3 and
            result['postReleaseRounds']==0 and bridge.call('GetVRSharedSlot(29)')==0)
        save()
        print(f"Replay blocked {sum(d['blocked'] for d in result['replay'])}/{len(result['replay'])}; "
              f"stale action {result['staleWindow']:.3f}s, {len(shots)} rounds, "
              f"{result['postReleaseRounds']} after release; passed={result['passed']}",flush=True)
        return result['passed']
    finally:
        trigger.set(False)
        sim._write(flags,struct.pack('<i',old))
        sim.restore()
        bridge.call('CPVRAutoProbe=nil;CPVRFingerDiag=nil;Game.SetVRMeleeFire(0)','exec')


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('pid',type=int)
    p.add_argument('archive',type=Path)
    p.add_argument('out',type=Path)
    args=p.parse_args()
    if args.out.exists():
        p.error('Choose a new results file')
    raise SystemExit(0 if run(args.pid,args.archive,args.out) else 1)
