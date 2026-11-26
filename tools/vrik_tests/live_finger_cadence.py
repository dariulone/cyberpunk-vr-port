"""Attach to an exclusively handed-off, save-locked simulator. Stage
fixtures/weapon_acceptance/init.lua as CPVR_FingerCadenceTest before boot.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import struct
import sys
import time

from live_weapon_acceptance import PreviewTrigger
from launch_to_save import ForegroundRejected

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/sim_harness'))
from simxr import Sim, _exports, _modules

OBSERVER = "GetMod('CPVR_FingerCadenceTest')"
WEAPON = 'Game.GetPlayer():GetActiveWeapon()'
CASES = {
    'Liberty': ('SemiAuto', .14, .065, 6),
    'Unity': ('SemiAuto', .14, .065, 6),
    'Nue': ('SemiAuto', .14, .065, 6),
    'Sor22': ('SemiAuto', .16, .075, 6),
    'Ajax': ('FullAuto', 0, .85, 1),
    'Kenshin': ('Burst', 0, .22, 1),
    'Nekomata': ('Charge', 0, .40, 1),
}


def focus_preview(trigger):
    for attempt in range(3):
        try:
            trigger.focus()
            return
        except ForegroundRejected:
            if attempt == 2:
                raise
            time.sleep(.15)


def equip(bridge, family):
    record = 'Items.Preset_' + family + '_Default'
    key = json.dumps(record)
    bridge.call("""
        assert(Game.IsSavingLocked())
        local p=Game.GetPlayer()
        local ts=Game.GetTransactionSystem()
        local id=TweakDBID.new(%s)
        local ok,items=ts:GetItemList(p)
        assert(ok)
        CPVRFingerItem=nil
        for _,it in ipairs(items) do
            if ItemID.GetTDBID(it:GetID())==id then CPVRFingerItem=it:GetID();break end
        end
        if not CPVRFingerItem then CPVRFingerItem=ItemID.FromTDBID(id);ts:GiveItem(p,CPVRFingerItem,1) end
        local r=EquipRequest.new()
        r.owner=p;r.slotIndex=0;r.itemID=CPVRFingerItem;r.addToInventory=false
        Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')):QueueRequest(r)
    """ % key, 'exec')
    time.sleep(.9)
    bridge.call("""
        local r=DrawItemRequest.new()
        r.owner=Game.GetPlayer();r.itemID=CPVRFingerItem
        Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')):QueueRequest(r)
        CPVRFingerItem=nil
    """, 'exec')
    time.sleep(1.3)
    assert bridge.call(f'TDBID.ToStringDEBUG(ItemID.GetTDBID({WEAPON}:GetItemID()))') == record


def observe_actions(bridge):
    bridge.call("""
        if CPVRFingerDiagInstalled then return end
        ObserveBefore('SemiAutoDecisions','OnRangeAttackInput',function(_,_,value)
            if CPVRFingerDiag then table.insert(CPVRFingerDiag,{event='action',value=value,t=Game.VRMeleeRealTime()}) end
        end)
        ObserveBefore('ShootEvents','OnEnter',function(self,ctx,si)
            if not CPVRFingerDiag then return end
            local a=CName.new('RangedAttack')
            table.insert(CPVRFingerDiag,{event='receipt',token=tostring(Game.VRCurrentFingerAction()),
                count=si:GetActionPressCount(a),value=si:GetActionValue(a),t=Game.VRMeleeRealTime()})
        end)
        CPVRFingerDiagInstalled=true
    """,'exec')


def measure(bridge, trigger, family, period, high, count, mode, diagnostics):
    bridge.call(f"""local w={WEAPON}
        assert(w:GetCurrentTriggerMode():Type()==gamedataTriggerMode.{mode})
        assert(GetMod('CyberpunkVRPort_Reload').reload().util.setMagAmmo(w,WeaponObject.GetMagazineCapacity(w)))
    """, 'exec')
    trigger.set(False)
    time.sleep(.35)
    if diagnostics:
        bridge.call('CPVRFingerDiag={}','exec')
    assert bridge.call(OBSERVER + '.start(true,true)') is True
    starts = []
    try:
        focus_preview(trigger)
        for _ in range(count):
            start = time.perf_counter()
            trigger.set(True)
            starts.append(start)
            time.sleep(high)
            trigger.set(False)
            time.sleep(max(0, start + period - time.perf_counter()))
        time.sleep(.5)
    finally:
        trigger.set(False)
        take = bridge.call(OBSERVER + '.stop()')
        if diagnostics:
            take['actions'] = bridge.call('CPVRFingerDiag')
            bridge.call('CPVRFingerDiag=nil','exec')
    edges = take['input']['edges']
    events = take['events']
    rounds = take['original'] - take['remaining']
    passed = (not take.get('invalid') and take['input']['vr'] and not take['input']['down']
              and take['item'] == 'Items.Preset_' + family + '_Default' and take['maxGap'] < .1
              and [e['down'] for e in edges] == [True, False] * count
              and rounds == len(events) and all(e['delta'] == 1 for e in events))
    if passed and mode == 'SemiAuto':
        # A receipt can be sampled in the same update bracket as release.
        passed = rounds == count and all(e['hi'] >= press['lo'] and e['lo'] <= release['hi']
                                         for e,press,release in zip(events,edges[::2],edges[1::2]))
    if passed and mode == 'FullAuto':
        passed = rounds > 1 and events[-1]['lo'] <= edges[-1]['hi']
    if passed and mode == 'Burst':
        burst = bridge.call(f'Game.GetStatsSystem():GetStatValue({WEAPON}:GetEntityID(),gamedataStatType.NumShotsInBurst)')
        take['burstSize'] = burst
        passed = rounds == burst and rounds > 1
    if passed and mode == 'Charge':
        passed = rounds == 1 and events[0]['hi'] >= edges[-1]['lo']
    take.update(mode=mode, requested=count, holdSeconds=high, requestedPeriod=period,
                pressOffsets=[s-starts[0] for s in starts], passed=passed)
    print(f'{family} {mode} requested={count} sampled={len(edges)//2} rounds={rounds} passed={passed}', flush=True)
    return take


def gate_checks(bridge, trigger, sim, set_buttons, report, pause_pairs=1):
    def state():
        return bridge.call(f"""(function() local w={WEAPON}
            local m=GetMod('CyberpunkVRPort_Reload').reload()
            return {{ammo=w:GetMagazineAmmoCount(),need=m.wants(w),info=m.dbg(),
                    paused=Game.GetSystemRequestsHandler():IsGamePaused(),vr=Game.VRWeaponTriggerEnabled(),
                    sample=GetVRSharedSlot(160)}} end)()""")

    def load(n):
        bridge.call(f'assert(GetMod("CyberpunkVRPort_Reload").reload().util.setMagAmmo({WEAPON},{n}))','exec')
        time.sleep(.25)
        assert state()['ammo'] == n

    def pull():
        focus_preview(trigger)
        trigger.set(True)
        time.sleep(.085)
        trigger.set(False)
        time.sleep(.25)

    def menu(event):
        bridge.call("""local layer=Game.VRInkLayer(CName.new("inkMenuLayer"))
            for _,c in ipairs(layer:GetGameControllers()) do
                if NameToString(c:GetClassName())=="gameuiInGameMenuGameController" then
                    c:SpawnMenuInstanceEvent(CName.new(%s));return end end
            error("In-game menu controller unavailable")""" % json.dumps(event),'exec')
        time.sleep(.55)

    equip(bridge,'Liberty')
    load(7)
    equip(bridge,'Unity')
    focus_preview(trigger)
    trigger.set(True)
    time.sleep(.22)
    equip(bridge,'Liberty')
    held = state()
    trigger.set(False)
    time.sleep(.35)
    released = state()
    pull()
    fresh = state()
    report('equip while held', [held,released,fresh],
           held['sample'] > .84 and held['ammo'] == released['ammo'] == 7 and fresh['ammo'] == 6)

    for attempt in range(2 * pause_pairs):
        keep_held = bool(attempt % 2)
        load(7)
        before = state()
        menu('OnOpenPauseMenu')
        try:
            paused = state()
            focus_preview(trigger)
            trigger.set(True)
            time.sleep(.16)
            if not keep_held:
                trigger.set(False)
                time.sleep(.12)
        finally:
            menu('OnClosePauseMenu')
        resumed = state()
        trigger.set(False)
        time.sleep(.18)
        bridge.call('CPVRFingerDiag={}','exec')
        assert bridge.call(OBSERVER + '.start(true,true)') is True
        pull()
        trace = bridge.call(OBSERVER + '.stop()')
        trace['actions'] = bridge.call('CPVRFingerDiag')
        bridge.call('CPVRFingerDiag=nil','exec')
        fresh = state()
        fresh['trace'] = trace
        report(f'pause {attempt+1} '+('held through resume' if keep_held else 'released before resume'),
               [before,paused,resumed,fresh], paused['paused'] and not resumed['paused'] and resumed['vr']
               and (resumed['sample']>.84) == keep_held and resumed['ammo'] == before['ammo']
               and fresh['ammo'] == before['ammo']-1
               and [edge['down'] for edge in trace['input']['edges']] == [True,False])

    load(0)
    pull()
    empty = state()
    load(6)
    time.sleep(.35)
    reloaded = state()
    pull()
    fresh = state()
    report('released empty pull then refill', [empty,reloaded,fresh],
           empty['ammo'] == 0 and reloaded['ammo'] == 6 and fresh['ammo'] == 5)

    sim.hand('left',pos=(-.42,-.3,-.15),ypr=(0,0,0),mode='body')
    sim.grip('left',0)
    set_buttons(0x2000)
    time.sleep(.13)
    set_buttons(0)
    time.sleep(.35)
    removed = state()
    pull()
    after = state()
    report('magazine removed', [removed,after], removed.get('need') == after.get('need') == 'out'
           and removed['ammo'] == after['ammo'] == 0)

    equip(bridge,'Grad')
    load(4)
    pull()
    fired = state()
    pull()
    blocked = state()
    report('manual chamber cycle', [fired,blocked],
           fired.get('need') == blocked.get('need') == 'rack' and fired['ammo'] == blocked['ammo'] == 3)


def run(pid, out, families, gates, pause_pairs=1, rapid_period=None, rapid_repeats=1, diagnostics=False):
    modules = {n.lower():(base,Path(dll)) for n,base,dll in _modules(pid)}
    if 'cyberpunk2077.exe' not in modules or 'openxr_simulator.dll' not in modules:
        raise RuntimeError('Expected an owned simulator game PID')
    base, plugin = modules['cyberpunkvrport.dll']
    spec = importlib.util.spec_from_file_location('cpvr_bridge',ROOT/'tools/grasp_tests/corpse_live/run.py')
    protocol = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(protocol)
    bridge = protocol.Bridge(plugin.parent/'lua/CVRBridge')
    assert bridge.call('Game.IsSavingLocked()') is True
    assert bridge.call(OBSERVER + '~=nil') is True
    assert not bridge.call('VRLeftHanded()'), 'PreviewTrigger uses the right-hand preview binding'
    sim = Sim(pid,focus_away=False)
    trigger = PreviewTrigger(sim)
    grip = base + _exports(plugin)['CyberpunkVR_TestPadButtons']
    original = sim._i32(grip)
    data = {'pid':pid,'series':[],'gates':{}}
    def save():
        out.write_text(json.dumps(data,indent=2)+'\n',encoding='utf-8')
    def report(name, states, passed):
        data['gates'][name] = {'states':states,'passed':passed}
        save()
        print(name, [s['ammo'] for s in states], passed, flush=True)
    def set_buttons(extra):
        sim._write(grip, struct.pack('<i',original | 0x20000 | extra))
    try:
        if gates or diagnostics:
            observe_actions(bridge)
        set_buttons(0)
        sim.hand('right',pos=(.18,-.14,-.44),ypr=(0,48,-5),mode='body')
        for family in families:
            equip(bridge,family)
            mode,period,high,count = CASES[family]
            if rapid_period is not None and mode == 'SemiAuto':
                period,high = rapid_period,rapid_period/2
            series = [(period,high,count)]
            if mode == 'SemiAuto':
                series *= rapid_repeats
                series.append((0,1.05,1))
            for period,high,count in series:
                data['series'].append(measure(bridge,trigger,family,period,high,count,mode,diagnostics))
                save()
        if gates:
            gate_checks(bridge,trigger,sim,set_buttons,report,pause_pairs)
    finally:
        trigger.set(False)
        sim._write(grip, struct.pack('<i',original))
        sim.restore()
        if gates or diagnostics:
            bridge.call('CPVRFingerDiag=nil','exec')
    return all(t['passed'] for t in [*data['series'],*data['gates'].values()])


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('pid',type=int)
    p.add_argument('out',type=Path)
    p.add_argument('--families',nargs='+',choices=CASES,default=list(CASES))
    p.add_argument('--gates-only',action='store_true')
    p.add_argument('--trace-actions',action='store_true',help='record game action callbacks and accepted shot states')
    p.add_argument('--pause-pairs',type=int,choices=range(1,11),default=1)
    p.add_argument('--rapid-period',type=float,help='seconds per semi-auto press, 50 percent duty cycle')
    p.add_argument('--rapid-repeats',type=int,choices=range(1,11),default=1)
    args = p.parse_args()
    if args.out.exists():
        p.error('Choose a new results file')
    if args.rapid_period is not None and args.rapid_period<=0:
        p.error('--rapid-period must be positive')
    sys.exit(0 if run(args.pid,args.out,[] if args.gates_only else args.families,args.gates_only,
                      args.pause_pairs,args.rapid_period,args.rapid_repeats,args.trace_actions) else 1)
