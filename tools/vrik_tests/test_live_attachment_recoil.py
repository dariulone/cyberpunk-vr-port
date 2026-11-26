"""Offline runner tests. All weapon, controller, clock and bridge inputs are fixtures."""
from copy import deepcopy
import unittest
from unittest.mock import patch

import live_attachment_recoil as run


class Fixture:
    def __init__(self, case):
        self.case = case
        self.state = dict(locked=True, leftOnly=case=='left-pistol', supported=False, partner=False)
        for side in ['primary', 'partner'] if case == 'dual' else ['primary']:
            self.state[side] = dict(record='test', instance=side, entity=side, publishedEntity=side, slot=True,
                                    other='', ammo=8, muzzle=False, seq=2**32-1, peak=64, factor=1.)

    def call(self, query):
        assert query == run.OBSERVER
        return deepcopy(self.state)

    def shoot(self, hand):
        side = 'partner' if hand == 'left' and self.case == 'dual' else 'primary'
        if self.case == 'left-pistol':
            assert hand == 'left'
        w = self.state[side]
        w['seq'] = (w['seq'] + 1) % 2**32
        w['peak'] = (8 if side == 'primary' else 11) * w['factor']
        w['ammo'] -= 1

    def change(self, side, original, record):
        w = self.state[side]
        assert w['muzzle'] == original['muzzle']
        w['muzzle'] = dict(record=record, instance=record, quality=3) if record else False
        factors = {'01':.85, '01_legendary':.8, '03':1, '08':.85, '07':1}
        w['factor'] = factors[record.removeprefix(run.PREFIX)] if record else 1

    def refill(self, side, original, count):
        assert self.state[side]['instance'] == original['instance']
        self.state[side]['ammo'] = count


class Measurement(unittest.TestCase):
    def measure(self, f, shoot, result, supported=False):
        with patch.object(run.time, 'sleep'), patch.object(run, 'change', Fixture.change), \
                patch.object(run, 'refill', Fixture.refill):
            run.measure(f, shoot, f.case, supported, result, lambda: None)

    def test_all_routes(self):
        for case in ('rifle', 'right-pistol', 'left-pistol', 'dual'):
            with self.subTest(case=case):
                f = Fixture(case)
                result = dict(phases=[], cleanupErrors=[])
                self.measure(f, f.shoot, result)
                self.assertEqual(len(result['phases']), len(run.phases(case)))
                for phase in result['phases']:
                    for take in phase['hands'].values():
                        self.assertTrue(take['passed'])
                        self.assertEqual(len(take['shots']), 5)
                self.assertEqual(result['cleanupErrors'], [])
                self.assertEqual(result['restored']['primary']['ammo'], 8)

    def test_cross_hand_and_abort_cleanup(self):
        f = Fixture('dual')
        result = dict(phases=[], cleanupErrors=[])
        def faulty_shot(hand):
            f.shoot(hand)
            if f.state['primary']['muzzle']:
                f.shoot('left')
        with self.assertRaisesRegex(ValueError, 'partner magazine'):
            self.measure(f, faulty_shot, result)
        self.assertFalse(result['restored']['primary']['muzzle'])
        self.assertEqual(result['restored']['partner']['ammo'], 8)
        self.assertEqual(result['cleanupErrors'], [])

    def test_unreduced_pose_fails_even_when_factor_is_correct(self):
        f = Fixture('rifle')
        def faulty_shot(hand):
            f.shoot(hand)
            f.state['primary']['peak'] = 8
        result = dict(phases=[], cleanupErrors=[])
        self.measure(f, faulty_shot, result)
        self.assertFalse(result['phases'][1]['hands']['primary']['passed'])
        self.assertTrue(result['phases'][3]['hands']['primary']['passed'])

    def test_supported(self):
        f = Fixture('rifle')
        f.state['supported'] = True
        result = dict(phases=[], cleanupErrors=[])
        self.measure(f, f.shoot, result, supported=True)
        self.assertTrue(result['restored']['supported'])

    def test_compiled_bridge_commands_and_boot_observer(self):
        from lupa.luajit21 import LuaRuntime
        lua = LuaRuntime()
        fixture = run.ROOT/'tools/vrik_tests/fixtures/attachment_recoil/init.lua'
        observer = lua.execute(fixture.read_text(encoding='utf-8'))
        lua.execute("""
            Game={GetPlayer=function() return {} end, IsSavingLocked=function() return true end,
                VRMeleeRealTime=function() return 42 end, GetTransactionSystem=function() return {
                    GetItemData=function() return {HasPartInSlot=function() return false end,
                        GetEmptySlotsOnItem=function() return {'AttachmentSlots.PowerModule'} end,
                        GetUsedSlotsOnItem=function() return {} end} end} end}
            local function gun(id) return {GetItemID=function() return id end,
                GetEntityID=function() return {hash=id} end,GetMagazineAmmoCount=function() return 10 end} end
            GameObject={GetRightActiveWeapon=function() return gun('right') end,
                GetLeftActiveWeapon=function() return gun('left') end}
            local same=function(x) return x end
            TweakDBID={new=same}; TDBID={ToStringDEBUG=same}; ItemID={GetTDBID=same,GetCombinedHash=same}
            GetVRTwoHandActive=function() return false end
            GetVRLastShotPeak=function(side) return {x=3+side,y=42,z=1} end
            GetMod=function() return {AttachmentRecoilStatus=function() return {
                primary=1,partner=.8,primaryWid='right',partnerWid='left'} end} end
        """)
        snap = observer.snapshot()
        self.assertEqual(snap.primary.seq, 65578)
        self.assertEqual((snap.primary.peak, snap.partner.peak), (4,3))
        self.assertEqual(snap.partner.entity, snap.partner.publishedEntity)
        self.assertTrue(snap.primary.slot)
        lua.execute('GameObject.GetRightActiveWeapon=function() end')
        self.assertTrue(observer.snapshot().leftOnly)
        self.assertFalse(observer.snapshot().partner)
        class Compiler:
            def call(self, code, kind):
                assert kind == 'exec'
                lua.eval('function(code) assert(loadstring(code)) end')(code)
        bridge = Compiler()
        w = dict(instance='sample', muzzle=False)
        run.change(bridge, run.PRIMARY, w, run.PREFIX+'01')
        w['muzzle'] = dict(instance='part')
        run.change(bridge, run.PARTNER, w, run.PREFIX+'07')
        run.change(bridge, run.PARTNER, w, None)
        run.refill(bridge, run.PRIMARY, w, 8)


if __name__ == '__main__':
    unittest.main()
