"""Save-locked CPVR simulator harness probe, called as run(out, sim, pad, call, ex, equip, tag).

The owner starts and closes the game, stages fixtures/weapon_acceptance/init.lua as the temporary
module CPVR_AcceptanceTelemetry, and restores the XR runtime. The probe itself never issues a shot.
"""
import json
import struct
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/quickboot_tests'))
from launch_to_save import focus_window, user32, windows_for


class PreviewTrigger:
    """The focused preview maps Space to the right controller; never press it in another window."""

    def __init__(self, sim):
        (self.window,) = [w['hwnd'] for w in windows_for(sim.pid)
                          if w['visible'] and w['title'].startswith('OpenXR Simulator')]
        self.sim = sim
        self.held = False

    def focus(self):
        focus_window(self.window)

    def set(self, down):
        if down and user32.GetForegroundWindow() != self.window:
            raise RuntimeError('The simulator preview lost focus')
        if down or self.held:
            user32.keybd_event(32, 0x39, 0 if down else 2, 0)
        self.held = down
        self.sim.trigger('right', int(down))


def check_air(sim, pad, call, ex, equip):
    def counters():
        return list(struct.unpack('<8i', pad.read('CyberpunkVR_HapticStats', 32)))

    def motion():
        return call("""(function()
            local p=Game.GetPlayer(); local w=p:GetActiveWeapon()
            local d=GetMod('CyberpunkVRPort_Weapon').CyberwareStatus()
            return {whooshes=d.whooshes or 0,hits=p.vrMeleeConfirmedHits,
                    serial=w:VRBladeConfirmedSerial(),phase=d.phase}
        end)()""")

    def place(x):
        sim.hand('right', pos=(x,-.12,-.49), ypr=(0,-52,-12), mode='body')

    sim.hands_follow_head(True)
    sim.head(0, 12)
    equip('Katana')
    place(.05)
    time.sleep(1.25)
    before, h0, enabled = motion(), counters(), pad.i32('CyberpunkVR_Haptics')
    for direction in (1,-1,1,-1):
        base = .05 if direction == 1 else .37
        for i in range(1, 5):
            place(base + direction * .08 * i)
            time.sleep(.016)
        time.sleep(.17)
    time.sleep(.25)
    after, h1 = motion(), counters()
    # Positive control on the measuring channel, NOT evidence of a physical collision or vibration.
    ex('VRHapticEvent(14,2)')
    time.sleep(.16)
    control = counters()
    passed = bool(enabled and after['whooshes'] > before['whooshes'] and before['hits'] == after['hits']
                  and before['serial'] == after['serial'] and h1[0] == h0[0] and h1[1:3] == h0[1:3]
                  and h1[7] == h0[7] and control[0] > h1[0] and control[7] > h1[7])
    return {'before':before, 'after':after, 'hapticsEnabled':enabled, 'hapticBefore':h0,
            'hapticAfter':h1, 'positiveControl':control}, passed


def check_magazine(take, family):
    events = take['events']
    return (take['item'] == f'Items.Preset_{family}_Default' and not take.get('invalid') and take['frames'] > 10
            and take['original'] - take['remaining'] == len(events) == min(take['original'], 6) and len(events) >= 4
            and all(e['delta'] == 1 and 0 <= e['hi'] - e['lo'] <= .10 for e in events) and take['peakDeg'] > 0)


def run(out, sim, pad, call, ex, equip, tag, series=None):
    if series is None:
        series = [('Unity',.215), ('Nue',.242), ('Sor22',.295)]
    air, passed = check_air(sim, pad, call, ex, equip)
    result = {'probe':'CPVR weapon acceptance', 'tag':tag, 'magazines':{}, 'air':air, 'airPass':passed}
    path = out / (tag + '-weapon-acceptance.json')

    def save():
        path.write_text(json.dumps(result, indent=2), encoding='utf-8')

    print('air', air, flush=True)
    save()
    trigger = PreviewTrigger(sim)
    old_buttons = pad.i32('CyberpunkVR_TestPadButtons')
    try:
        pad.set_i32('CyberpunkVR_TestPadButtons', old_buttons | 0x20000)
        sim.hand('right', pos=(.18,-.14,-.44), ypr=(0,48,-5), mode='body')
        sim.head(0, 17)
        for family, period in series:
            equip(family)
            trigger.focus()
            trigger.set(False)
            time.sleep(.35)
            # Warm-up is intentionally outside the magazine/pose measurement.
            trigger.set(True)
            time.sleep(.14)
            trigger.set(False)
            time.sleep(.55)
            assert call("GetMod('CPVR_AcceptanceTelemetry').start()") is True
            pad.write('CyberpunkVR_DebugRecoilPeakDeg', struct.pack('<f', 0))
            pulls = []
            for _ in range(6):
                start = time.perf_counter()
                pulls.append(start)
                trigger.set(True)
                time.sleep(.085)
                trigger.set(False)
                time.sleep(max(0, period - (time.perf_counter() - start)))
            time.sleep(.4)
            take = call("GetMod('CPVR_AcceptanceTelemetry').stop()")
            take['inputIntervalsMs'] = [round((b-a)*1000, 2) for a,b in zip(pulls,pulls[1:])]
            take['peakDeg'], = struct.unpack('<f', pad.read('CyberpunkVR_DebugRecoilPeakDeg', 4))
            take['settledDeg'], = struct.unpack('<f', pad.read('CyberpunkVR_DebugRecoilAppliedDeg', 4))
            result['magazines'][family] = take
            save()
            print('ammo', family, take, flush=True)
        result['magPass'] = bool(result['magazines']) and all(check_magazine(t, f) for f,t in result['magazines'].items())
        save()
        print('archive', path, 'airPass', passed, 'magPass', result['magPass'], flush=True)
        assert passed and result['magPass'], 'See the per-control observations in the JSON.'
    finally:
        trigger.set(False)
        pad.set_i32('CyberpunkVR_TestPadButtons', old_buttons)
