"""Offline checks for the dual-wield cadence observer and production fixture."""
import copy
import unittest

from lupa.luajit21 import LuaRuntime
from live_dual_cadence import FIXTURE, NUE, SIDES, check_take, pulse_train, trials


def observed(schedule):
    entities = {'left':'second', 'right':'first'}
    take = {'frames':100,'begin':0,'prevTime':5,'weapons':{}}
    for side in SIDES:
        edges = [{'lo':t, 'hi':t+.01, 'down':down} for t,hands,down in schedule if side in hands]
        shots = [{'t':e['hi']+.01} for e in edges if e['down']]
        take['weapons'][side] = dict(item=NUE, entity=entities[side], down=False, original=10,
            remaining=10-len(shots), edges=edges or {}, receipts=shots or {},
            ammoChanges=[{'delta':1} for _ in shots] or {})
    return take,entities


class CadenceTest(unittest.TestCase):
    def test_isolation_and_both(self):
        for sides in [('left',), ('right',), SIDES]:
            with self.subTest(sides=sides):
                schedule=pulse_train(sides,.85,.3,3)
                take,entities=observed(schedule)
                summary=check_take(take,schedule,True,entities)
                for side in SIDES:
                    self.assertEqual(summary[side]['spent'],3 if side in sides else 0)
                if len(sides)==2:
                    self.assertEqual(summary['crossHandMs'],[0,0,0])

    def test_floor_checks_receipts_and_accepts_dropped_pulls(self):
        schedule=pulse_train(('left',),.09,.064,7)
        take,entities=observed(schedule)
        with self.assertRaisesRegex(ValueError,'accepted interval'):
            check_take(take,schedule,False,entities)
        row=take['weapons']['left']
        row['receipts']=row['receipts'][::2]
        row['ammoChanges']=row['ammoChanges'][::2]
        row['remaining']=6
        self.assertEqual(check_take(take,schedule,False,entities)['left']['receipts'],4)
        row['ammoChanges']=[]
        with self.assertRaisesRegex(ValueError,'magazine'):
            check_take(take,schedule,False,entities)

    def test_zero_wrong_hand_and_delayed_shots_fail(self):
        schedule=pulse_train(('left',),.85,.3,2)
        original,entities=observed(schedule)
        for case in ['zero','other-hand','late','same-pull','wrong-entity']:
            with self.subTest(case=case):
                take=copy.deepcopy(original)
                left=take['weapons']['left']
                if case=='zero': left.update(receipts={},ammoChanges={},remaining=10)
                if case=='other-hand':
                    take['weapons']['right'].update(receipts=[{'t':.1}],ammoChanges=[{'delta':1}],remaining=9)
                if case=='late': left['receipts'][-1]['t']=1.5
                if case=='same-pull': left['receipts'][-1]['t']=.20
                if case=='wrong-entity': left['entity']=entities['right']
                with self.assertRaises(ValueError): check_take(take,schedule,False,entities)

    def test_production_pair_and_boot_lock(self):
        lua=LuaRuntime()
        lua.execute("""
            CName={new=function(s)return s end}; TweakDBID=CName
            gamedataNewPerkType={Cool_Central_Milestone_2=7};gamedataDevelopmentPointType={Primary=0}
            gamedataEquipmentArea={Weapon=1}; EItemOperationType={ADD=1,REMOVE=2}
            gameEquipAnimationType={Instant=1}
            local req={new=function()return {} end}
            GameplayEquipRequest=req; DrawItemRequest=req; SaveLockRequest=req
            engine=true; native=true; requests={}
            manager={IsSavingLocked=function() return native end,
              QueueRequest=function(_,r)table.insert(requests,r)end}
            Game={IsSavingLocked=function()return engine end,GetPlayer=function()
              return {GetEntityID=function()return {hash=121}end}end,
              GetAutoSaveSystem=function()return {InvalidateAutoSaveRequests=function()end}end,
              GetScriptableSystemsContainer=function()return {Get=function()return manager end}end}
            rank=0; pd={IsNewPerkUnlocked=function()return true end,GetNewPerkMaxLevel=function()return 3 end,
              IsNewPerkBought=function()return rank end, BuyNewPerk=function(_,_,force)assert(force);rank=rank+1;return true end}
            PlayerDevelopmentSystem={GetData=function()return pd end}
        """)
        lua.execute((FIXTURE/'bridge.lua').read_text(encoding='utf-8'))
        lua.execute("""
            local a=WeaponAudit
            native=false;assert(not pcall(a.acquireSaveLock,'cpvr_weapon_audit_deadbeef',true))
            native=true; a.acquireSaveLock('cpvr_weapon_audit_deadbeef',true)
            assert(#requests==1 and requests[1].operation==EItemOperationType.ADD)
            a.original={entity='121'};a.created={'nue-one','nue-two'}
            a.perkReady(true);assert(rank==1)
            a.loadout(1,2);a.draw(1)
            assert(requests[2].itemID=='nue-one' and requests[2].slotIndex==0)
            assert(requests[3].itemID=='nue-two' and requests[3].slotIndex==3)
            assert(requests[2].blockUpdateWeaponActiveSlots and requests[3].blockUpdateWeaponActiveSlots)
            assert(requests[4].itemID=='nue-one' and requests[4].equipAnimationType==gameEquipAnimationType.Instant)
            native=false;assert(not pcall(a.guard));native=true
            engine=false;assert(not pcall(a.guard))
        """)


if __name__=='__main__':
    unittest.main()
