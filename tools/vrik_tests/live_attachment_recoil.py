"""Measure attachment changes on drawn weapons in the handed-off OpenXR simulator."""
import argparse
from contextlib import contextmanager
import importlib.util
import json
import math
from pathlib import Path
from statistics import median
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
OBSERVER = "GetMod('CPVR_AttachmentRecoilTest').snapshot()"
PRIMARY, PARTNER = 'primary', 'partner'
PREFIX = 'Items.w_muzzle_brake_'
SHOTS = 5


def expected(muzzle):
    if not muzzle or muzzle['record'] in (PREFIX+'03', PREFIX+'07'):
        return 1.0
    if muzzle['record'] == PREFIX+'01_legendary':
        return .8
    if muzzle['record'] in (PREFIX+'01', PREFIX+'08'):
        return 1 - .075 - .025 * muzzle['quality']
    raise ValueError(f'Unexpected installed muzzle {muzzle}')


def phases(case):
    changes = [(PRIMARY, None)]
    if case == 'rifle':
        return changes + [(PRIMARY, PREFIX+s) for s in ('01', '01_legendary')] + [
            (PRIMARY, None), (PRIMARY, PREFIX+'03'), (PRIMARY, None)]
    for side in ([PRIMARY, PARTNER] if case == 'dual' else [PRIMARY]):
        changes.extend([(side, PREFIX+'08'), (side, PREFIX+'07'), (side, None)])
    return changes


def validate(snapshot, initial, wanted):
    if not snapshot['locked'] or any(snapshot[k] != initial[k] for k in ('leftOnly', 'supported')):
        raise ValueError('Save lock, handedness or grip changed')
    for side in (PRIMARY, PARTNER):
        w = snapshot[side]
        if bool(w) != bool(initial[side]):
            raise ValueError(f'{side} hand occupancy changed')
        if not w:
            continue
        if any(w[k] != initial[side][k] for k in ('instance', 'record', 'other')):
            raise ValueError(f'{side} weapon instance or other installed parts changed')
        record = w['muzzle']['record'] if w['muzzle'] else None
        if record != wanted[side] or w.get('failure') or w.get('publishedEntity') != w['entity']:
            raise ValueError(f'{side} slot/profile pending: {record}, {w.get("failure")}')
        if not math.isclose(w['factor'], expected(w['muzzle']), abs_tol=.001):
            raise ValueError(f'{side} published {w["factor"]}, expected {expected(w["muzzle"])}')


def check_shot(before, after, firing):
    for side in (PRIMARY, PARTNER):
        w = before[side]
        if not w:
            continue
        end = after[side]
        if any(w[k] != end[k] for k in ('instance', 'entity', 'muzzle', 'other', 'factor')):
            raise ValueError(f'{side} equipment changed during shot')
        rounds = int(side == firing)
        if w['ammo'] - end['ammo'] != rounds or (end['seq'] - w['seq']) % 2**32 != rounds:
            raise ValueError(f'{side} magazine or native impulse count disagrees with the trigger')
    peak = after[firing]['peak']
    if not math.isfinite(peak) or peak <= 0:
        raise ValueError(f'Invalid sampled peak {peak}')
    return peak


def wait_for(read, check, timeout=4):
    deadline = time.monotonic() + timeout
    while True:
        snap = read()
        try:
            check(snap)
            return snap
        except ValueError:
            if time.monotonic() >= deadline:
                raise
            time.sleep(.05)


def require_ammo(snap, side, count):
    if snap[side]['ammo'] != count:
        raise ValueError(f'{side} magazine has {snap[side]["ammo"]}, expected {count}')


def command(bridge, side, instance, code):
    # Recheck ownership at execution, including cleanup. Bind only Lua locals.
    lookup = 'p:GetActiveWeapon()' if side == PRIMARY else 'GameObject.GetLeftActiveWeapon(p)'
    bridge.call(f"""
        assert(Game.IsSavingLocked())
        local p=Game.GetPlayer(); local w=assert({lookup}); local id=w:GetItemID()
        assert(tostring(ItemID.GetCombinedHash(id))=={json.dumps(instance)})
        local ts=Game.GetTransactionSystem()
        local slot=TweakDBID.new('AttachmentSlots.PowerModule')
        {code}
    """, 'exec')


def change(bridge, side, w, target):
    current = w['muzzle']
    guard = "local data=assert(ts:GetItemData(p,id)); "
    if current:
        guard += ("assert(data:HasPartInSlot(slot) and tostring(ItemID.GetCombinedHash("
                  "InnerItemData.GetItemID(data:GetItemPart(slot))))==" + json.dumps(current['instance']) + "); ")
    else:
        guard += ("assert(not data:HasPartInSlot(slot) or "
                  "InnerItemData.GetStaticData(data:GetItemPart(slot)):TagsContains('DummyPart')); ")
    if target:
        request = 'SwapItemPart' if current else 'InstallItemPart'
        body = f"""
            local part
            for _,candidate in ipairs(ts:GetItemsInstallableInSlot(p,id,slot)) do
                if ItemID.GetTDBID(candidate)==TweakDBID.new({json.dumps(target)}) then part=candidate; break end
            end
            assert(part,'Missing compatible owned attachment {target}')
            local req={request}.new(); req:Set(p,id,part,slot)
        """
    else:
        body = 'local req=RemoveItemPart.new(); req:Set(p,id,slot)\n'
    command(bridge, side, w['instance'], guard + body +
            "Game.GetScriptableSystemsContainer():Get(CName.new('ItemModificationSystem')):QueueRequest(req)")


def refill(bridge, side, w, count):
    command(bridge, side, w['instance'], f"""
        assert({count} <= WeaponObject.GetMagazineCapacity(w))
        local evt=SetAmmoCountEvent.new(); evt.ammoTypeID=WeaponObject.GetAmmoType(w)
        evt.count={count}; w:QueueEvent(evt)
    """)


@contextmanager
def preview_keys(pid, hands, supported):
    from launch_to_save import focus_window, user32, windows_for
    window, = [row['hwnd'] for row in windows_for(pid)
               if row['visible'] and row['title'].startswith('OpenXR Simulator')]
    previous = user32.GetForegroundWindow()
    trigger = {'right': (0x20, 0x39), 'left': (0xA2, 0x1D)}
    grip = {'right': (0x46, 0x21), 'left': (0xA4, 0x38)}
    held = []

    def press(key):
        if user32.GetForegroundWindow() != window or user32.GetAsyncKeyState(key[0]) & 0x8000:
            raise RuntimeError('Preview lost focus or simulator key already held')
        held.append(key)
        user32.keybd_event(*key, 0, 0)

    def release(key):
        user32.keybd_event(*key, 2, 0)
        held.remove(key)

    def shoot(hand):
        key = trigger[hand]
        if user32.GetForegroundWindow() != window:
            focus_window(window)
        press(key)
        try:
            time.sleep(.085)
        finally:
            release(key)
        # A window change after delivery cannot tell us if a round fired.
        # The before/after magazine and per-hand impulse counters verify it.

    try:
        focus_window(window)
        # Dual mode already holds both weapons. The simulator's left grip is
        # Alt, which opens the window menu when paired with the right trigger (Space).
        for hand in (['right','left'] if supported else hands[:1]):
            press(grip[hand])
        time.sleep(.55)
        yield shoot
    finally:
        for key in reversed(held.copy()):
            release(key)
        if previous and previous != window and user32.GetForegroundWindow() == window:
            user32.SetForegroundWindow(previous)


def measure(bridge, shoot, case, supported, report, save):
    read = lambda: bridge.call(OBSERVER)
    initial = read()
    report['initial'] = initial
    sides = [PRIMARY, PARTNER] if case == 'dual' else [PRIMARY]
    hands = ['left' if case == 'left-pistol' else 'right', 'left']
    wanted = {PRIMARY: None, PARTNER: None}
    if (not initial[PRIMARY] or initial['leftOnly'] != (case == 'left-pistol')
            or bool(initial[PARTNER]) != (case == 'dual') or initial['supported'] != supported):
        raise ValueError('Draw the fixture for this case and check the support grip')
    validate(initial, initial, wanted)
    if any(not initial[s]['slot'] or initial[s]['ammo'] <= SHOTS for s in sides):
        raise ValueError('Each drawn gun needs a muzzle slot and at least six loaded rounds')
    if case == 'dual' and initial[PRIMARY]['record'] != initial[PARTNER]['record']:
        raise ValueError('Dual fixture requires two instances of the same pistol record')
    baselines, touched = {}, set()
    fired = False
    try:
        for side, target in phases(case):
            prev = read()
            validate(prev, initial, wanted)
            start = time.monotonic()
            if wanted[side] != target:
                touched.add(side)
                change(bridge, side, prev[side], target)
                wanted[side] = target
            ready = wait_for(read, lambda s: validate(s, initial, wanted))
            row = dict(changed=side, requested=target, readySeconds=time.monotonic()-start, ready=ready, hands={})
            report['phases'].append(row)
            for firing, hand in zip(sides, hands):
                take = row['hands'][firing] = {'shots': []}
                for _ in range(SHOTS):
                    before = read()
                    validate(before, initial, wanted)
                    fired = True
                    shoot(hand)
                    time.sleep(1.0)
                    after = read()
                    take['shots'].append(dict(before=before, after=after))
                    save()
                    validate(after, initial, wanted)
                    take['shots'][-1]['peakDeg'] = check_shot(before, after, firing)
                mid = median(s['peakDeg'] for s in take['shots'])
                ratio = mid / baselines.setdefault(firing, mid)
                expect = expected(ready[firing]['muzzle'])
                take.update(medianDeg=mid, ratio=ratio, expected=expect, passed=abs(ratio-expect) <= .03)
                count = initial[firing]['ammo']
                refill(bridge, firing, initial[firing], count)
                wait_for(read, lambda s: require_ammo(s, firing, count))
                save()
    finally:
        for s in sides:
            try:
                w = read()[s]
                if w['instance'] != initial[s]['instance']:
                    raise RuntimeError('Fixture weapon changed')
                if s in touched and w['muzzle']:
                    if w['muzzle']['record'] not in {target for _, target in phases(case)}:
                        raise RuntimeError('Unexpected muzzle during cleanup')
                    change(bridge, s, w, None)
                if fired and w['ammo'] != initial[s]['ammo']:
                    refill(bridge, s, w, initial[s]['ammo'])
            except Exception as exc:
                report['cleanupErrors'].append(f'{s}: {exc}')
        def restored(snap):
            validate(snap, initial, {PRIMARY: None, PARTNER: None})
            for s in sides:
                require_ammo(snap, s, initial[s]['ammo'])
        try:
            report['restored'] = wait_for(read, restored)
        except Exception as exc:
            report['cleanupErrors'].append(str(exc))
        save()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('pid', type=int)
    p.add_argument('output', type=Path)
    p.add_argument('--case', choices=['rifle','right-pistol','left-pistol','dual'], default='rifle')
    p.add_argument('--supported', action='store_true', help='fixture hands are positioned for the support grip')
    a = p.parse_args()
    if a.supported and a.case == 'dual':
        p.error('Support grip requires a single weapon')
    sys.path[:0] = [str(ROOT/'tools/sim_harness'), str(ROOT/'tools/quickboot_tests')]
    from simxr import _modules
    modules = {name.lower(): Path(path) for name, _, path in _modules(a.pid)}
    if not {'cyberpunk2077.exe','openxr_simulator.dll','cyberpunkvrport.dll'} <= modules.keys():
        p.error('Supply the handed-off simulator game PID')
    spec = importlib.util.spec_from_file_location('protocol', ROOT/'tools/grasp_tests/corpse_live/run.py')
    protocol = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(protocol)
    bridge = protocol.Bridge(modules['cyberpunkvrport.dll'].parent/'lua/CVRBridge')
    if not bridge.call(OBSERVER)['locked']:
        p.error('The runtime fixture must have saving locked')
    data = dict(pid=a.pid, case=a.case, supported=a.supported, phases=[], cleanupErrors=[], passed=False)
    a.output.touch(exist_ok=False)
    def save():
        a.output.write_text(json.dumps(data, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    try:
        hands = ['right','left'] if a.case == 'dual' else ['left' if a.case == 'left-pistol' else 'right']
        with preview_keys(a.pid, hands, a.supported) as shoot:
            measure(bridge, shoot, a.case, a.supported, data, save)
        data['passed'] = not data['cleanupErrors'] and all(
            hand['passed'] for row in data['phases'] for hand in row['hands'].values())
    except BaseException as exc:
        data['error'] = f'{type(exc).__name__}: {exc}'
        raise
    finally:
        save()
    print(f"{a.case}: {'PASS' if data['passed'] else 'FAIL'} ({a.output})")
    return 0 if data['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
