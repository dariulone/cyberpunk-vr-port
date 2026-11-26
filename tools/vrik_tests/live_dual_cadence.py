"""Check accepted player shots on a handed-off PID using the weapon audit's dual Nue fixture."""
import argparse
from contextlib import contextmanager
import ctypes
from ctypes import wintypes
import importlib.util
import json
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / 'tools/weapon_tests/live'
OBSERVER = "GetMod('CPVR_FingerCadenceTest')"
SIDES = ('right', 'left')
NUE = 'Items.Preset_Nue_Default'


def pulse_train(sides, period, high, count):
    return [(i * period + offset, sides, down) for i in range(count)
            for offset, down in ((0, True), (high, False))]


def trials():
    # Separate the bilateral probes enough to avoid requesting Heywood Hurricane.
    for side in SIDES:
        yield side + '-ordinary', pulse_train((side,), .8, .35, 2), True
    yield 'both', pulse_train(SIDES, .85, .30, 3), True
    for side in SIDES:
        for ms, high in ((90, .064), (100, .068), (160, .095)):
            yield f'{side}-{ms}ms', pulse_train((side,), ms / 1000, high, 7), False


def array(value):
    if value == {}:  # CVRBridge serializes empty Lua tables as objects.
        return []
    if not isinstance(value, list):
        raise ValueError('missing or malformed observer array')
    return value


def check_take(take, schedule, exact, entities):
    if take.get('invalid') or set(take['weapons']) != set(SIDES) or take['frames'] < 1:
        raise ValueError(f"invalid dual take: {take.get('invalid')}")
    summary = {}
    for side in SIDES:
        row = take['weapons'][side]
        if row['item'] != NUE:
            raise ValueError(f'{side} is not the production Nue fixture')
        if row['entity'] != entities[side] or row['down']:
            raise ValueError(f'{side} entity changed or trigger still down')
        receipts = array(row['receipts'])
        edges = array(row['edges'])
        requested = [down for _, hands, down in schedule if side in hands]
        if [edge['down'] for edge in edges] != requested:
            raise ValueError(f'{side}: physical edges differ from the requested trace')
        count = len(receipts)
        spent = row['original'] - row['remaining']
        changes = array(row['ammoChanges'])
        if spent != count or any(change['delta'] != 1 for change in changes) or len(changes) != spent:
            raise ValueError(f'{side}: {count} receipts, {spent} rounds, {len(changes)} magazine changes')
        pulls = len(requested) // 2
        if (exact and count != pulls) or count < min(2, pulls) or count > pulls:
            raise ValueError(f'{side}: {count} accepted shots for {pulls} pulls')
        stamps = [receipt['t'] for receipt in receipts]
        for earlier, later in zip(stamps, stamps[1:]):
            if later < earlier + .100:
                raise ValueError(f'{side}: accepted interval {(later-earlier)*1000:.4f} ms')
        # Match each receipt to a distinct physical pull. A rejected tap can
        # never appear as a second round or as a round after release.
        windows = iter(zip(edges[::2], edges[1::2]))
        for t in stamps:
            if not take['begin'] <= t <= take['prevTime'] or not any(
                    press['lo'] <= t <= release['hi'] for press, release in windows):
                raise ValueError(f'{side}: receipt outside its physical pull')
        summary[side] = {'requested': pulls, 'receipts': count, 'spent': spent, 'entity': row['entity'],
                         'intervalsMs': [(b-a)*1000 for a,b in zip(stamps, stamps[1:])]}
    if all(len(hands) == 2 for _, hands, _ in schedule):
        right, left = (take['weapons'][side]['receipts'] for side in SIDES)
        skews = [abs(a['t'] - b['t'])*1000 for a,b in zip(right,left)]
        if not any(skew < 100 for skew in skews):
            raise ValueError('no independent cross-hand pair within 100 ms')
        summary['crossHandMs'] = skews
    return summary


def load_harness():
    spec = importlib.util.spec_from_file_location('dual_weapon_audit', FIXTURE / 'run.py')
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


@contextmanager
def direct_controllers(pid):
    # The simulator polls Space/Ctrl and overwrites Sim.trigger while its
    # keyboard emulation is enabled and a game/preview window has focus.
    # Use the preview's own Input menu so only the handed-off PID is touched.
    sys.path.insert(0,str(ROOT/'tools/quickboot_tests'))
    from launch_to_save import focus_window, windows_for, user32
    window, = [row['hwnd'] for row in windows_for(pid)
               if row['visible'] and row['title'].startswith('OpenXR Simulator')]
    command_id = 1801  # OpenXR-Simulator ui_enhancements.h, ID_INPUT_ENABLE
    user32.GetMenu.argtypes = [wintypes.HWND]
    user32.GetMenu.restype = wintypes.HMENU
    user32.GetSubMenu.argtypes = [wintypes.HMENU, ctypes.c_int]
    user32.GetSubMenu.restype = wintypes.HMENU
    user32.GetMenuItemCount.argtypes = [wintypes.HMENU]
    user32.GetMenuStringW.argtypes = [wintypes.HMENU,wintypes.UINT,wintypes.LPWSTR,ctypes.c_int,wintypes.UINT]
    user32.GetMenuState.argtypes = [wintypes.HMENU,wintypes.UINT,wintypes.UINT]
    user32.GetMenuState.restype = wintypes.UINT
    bar = user32.GetMenu(window)
    title = ctypes.create_unicode_buffer(128)
    for i in range(user32.GetMenuItemCount(bar)):
        menu = user32.GetSubMenu(bar,i)
        if user32.GetMenuStringW(menu,command_id,title,len(title),0) and 'Send Simulated Input to OpenXR' in title.value.replace('&',''):
            break
    else:
        raise ValueError('simulator input menu unavailable')
    def checked():
        flags = user32.GetMenuState(menu,command_id,0)
        if flags == 0xffffffff:
            raise RuntimeError('simulator input state unavailable')
        return bool(flags & 8)
    def toggle():
        result = ctypes.c_size_t()
        if not user32.SendMessageTimeoutW(window,0x111,command_id,0,2,2000,ctypes.byref(result)):
            raise RuntimeError('simulator input command timed out')
    original = checked()
    try:
        focus_window(window)  # XR only publishes finger-pull identities while focused.
        if original: toggle()
        if checked(): raise RuntimeError('simulator keyboard input still enabled')
        time.sleep(.15)  # allow the simulator's one-time release on OFF
        yield lambda: focus_window(window)
    finally:
        if checked() != original: toggle()
        if checked() != original: raise RuntimeError('simulator keyboard input was not restored')


def run(pid, out):
    harness = load_harness()
    modules = {n.lower(): Path(p) for n, _, p in harness._modules(pid)}
    if not {'cyberpunk2077.exe','openxr_simulator.dll','cyberpunkvrport.dll'} <= modules.keys():
        raise ValueError('expected the handed-off simulator game PID')
    audit = harness.Audit(argparse.Namespace(pid=pid, out=out, game=modules['cyberpunk2077.exe'].parents[2]))
    if audit.plugin.resolve() != modules['cyberpunkvrport.dll'].parent.resolve():
        raise ValueError('loaded plugin does not match fixture path')
    report = {'pid':pid, 'cases':{}, 'passed':False}
    try:
        if not audit.call(f"Game.IsSavingLocked() and Game.GetPlayer()~=nil and WeaponAudit==nil and {OBSERVER}~=nil"):
            raise ValueError('expected the save-locked boot and staged cadence observer')
        audit.execute((FIXTURE / 'bridge.lua').read_text(encoding='utf-8'))
        audit.loaded = True
        audit.execute('WeaponAudit.acquireSaveLock(' + json.dumps(audit.save_lock_name) + ',true)')
        audit.save_lock_confirmed = True
        audit.original = audit.call('WeaponAudit.begin()')
        audit.save('original', audit.original)
        audit.sim = harness.Sim(pid, focus_away=False)
        audit.begun = True
        audit.neutral()
        for side, x in [('right',.22), ('left',-.22)]:
            audit.sim.hand(side,pos=(x,-.2,-.4),quat=(.7071067812,0,0,.7071067812),mode='body')
        audit.execute('WeaponAudit.perkReady(true)')
        created = {side:audit.call(f'WeaponAudit.create("{NUE}",{i})')['item']['key']
                   for i,side in enumerate(SIDES,1)}
        equipped = audit.loadout(1,2)
        audit.save('equipped', equipped)
        if (not equipped['dual'] or equipped['flags'] != 1 or equipped['frenzy']
                or len(set(created.values())) != 2 or
                any(equipped[s]['item']['key'] != created[s] or equipped[s]['ammo'] < 7 for s in SIDES)):
            raise ValueError(f'production pair not ready: {equipped}')
        entities = {side:equipped[side]['entity'] for side in SIDES}
        if len(set(entities.values())) != 2:
            raise ValueError('both slots report one entity')

        with direct_controllers(pid) as focus:
            for side in SIDES:
                audit.sim.grip(side,1)
            for name, schedule, exact in trials():
                focus()
                audit.execute('WeaponAudit.fill()')
                time.sleep(.85)
                if not audit.call("""(function()
                    if not Game.VRWeaponTriggerEnabled() or GetVRSharedSlot(49)<.5 or GetVRSharedSlot(155)<.5 then return false end
                    local p=Game.GetPlayer();local ts=Game.GetTransactionSystem()
                    for _,s in ipairs({'Right','Left'}) do local w=ts:GetItemInSlot(p,TweakDBID.new('AttachmentSlots.Weapon'..s))
                      if not w or w:GetCurrentTriggerMode():Type()~=gamedataTriggerMode.SemiAuto then return false end end
                    return true end)()"""):
                    raise ValueError(f'{name}: VR trigger route, physical grips or semi-auto mode unavailable')
                ready = audit.call('WeaponAudit.sample()')
                if (not ready['dual'] or ready['flags'] != 1 or ready['frenzy'] or
                        any(ready[s]['entity'] != entities[s] or ready[s]['ammo'] != ready[s]['capacity'] for s in SIDES)):
                    raise ValueError(f'{name}: routing or magazine not ready: {ready}')
                if not audit.call(OBSERVER + '.startHands()'):
                    raise RuntimeError('per-hand observation did not start')
                try:
                    start = time.perf_counter()+.05
                    for at, sides, down in schedule:
                        time.sleep(max(0,start+at-time.perf_counter()))
                        for side in sides:
                            audit.sim.trigger(side,int(down))
                    time.sleep(.5)
                finally:
                    for side in SIDES:
                        audit.sim.trigger(side,0)
                    take = audit.call(OBSERVER + '.stopHands()')
                    audit.save(name,take)
                report['cases'][name] = check_take(take,schedule,exact,entities)
                audit.save('cadence',report)
                print(name, report['cases'][name], flush=True)
    except BaseException as exc:
        report['error'] = str(exc)
        raise
    finally:
        # A failed preflight can load the Lua helpers before a lock or
        # restoration snapshot exists. There is then no owned lock to release.
        if audit.loaded and not audit.save_lock_confirmed and audit.call('WeaponAudit and not WeaponAudit.saveLockRequested and not WeaponAudit.original'):
            audit.execute('WeaponAudit=nil')
            audit.loaded=False
        report['cleanupErrors'] = audit.cleanup()
        report['passed'] = 'error' not in report and not report['cleanupErrors'] and len(report['cases']) == len(list(trials()))
        audit.save('cadence',report)
    return report['passed']


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid', type=int)
    parser.add_argument('output', type=Path, help='new evidence directory')
    args=parser.parse_args()
    sys.exit(0 if run(args.pid,args.output) else 1)
