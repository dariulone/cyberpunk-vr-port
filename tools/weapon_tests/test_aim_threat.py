"""Run the shipped aim driver and dwell logic with controllable game and XR inputs."""
from pathlib import Path
import unittest

from lupa.luajit21 import LuaRuntime


WEAPON_MOD = Path(__file__).resolve().parents[2] / "mods/cet/CyberpunkVRPort_Weapon"

GAME = """
left, right = 'AttachmentSlots.WeaponLeft', 'AttachmentSlots.WeaponRight'
now, stamp, origin = 0, 0, 1
slots, active, equipped, hits, events = {}, {}, {}, {}, {}
state = {EquipmentLeft=0, EquipmentRight=0, UpperBody=0, SceneTier=0}
tracking = {[left]=true, [right]=true}
shared = {[0]=1, [8]=1, [81]=0}
function GetVRSharedSlot(index) return shared[index] end
TweakDBID = {new=function(name) return name end}
EquipmentManipulationRequestSlot = {Left=left, Right=right}
EquipmentSystem = {GetSlotActiveItem=function(_, slot) return active[slot] end}
ItemID = {IsValid=function(id) return id ~= nil end, GetCombinedHash=function(id) return id.hash end}
gamedataItemType = {Wea_GrenadeLauncher='Wea_GrenadeLauncher'}
WeaponObject = {
    GetWeaponType=function(item) return item.kind end,
    IsOfType=function(item, kind) return item.kind == kind end,
}
UIInventoryItemsManager = {IsItemTypeRangedWeapon=function(kind) return kind == 'Wea_Handgun' end}
Vector4 = {new=function(x,y,z,w) return {x=x,y=y,z=z,w=w} end}
Quaternion = {GetForward=function() return {x=0,y=1,z=0} end}
QueryFilter = {ALL=function() return 'all' end}
TraceResult = {GetHitEntity=function(trace) return trace.entity end}
local psm = {GetInt=function(_, key) assert(state[key] ~= nil); return state[key] end}
player = {
    id='player', GetEntityID=function(self) return {hash=self.id} end,
    IsAttached=function() return true end, IsDead=function() return playerDead end,
    VRPortHasEquippedAimWeapon=function(_, weapon) return equipped[left]==weapon or equipped[right]==weapon end,
    GetPlayerStateMachineBlackboard=function() return psm end,
}
local defs = {
    PlayerStateMachine={SceneTier='SceneTier', UpperBody='UpperBody', EquipmentLeft='EquipmentLeft', EquipmentRight='EquipmentRight'},
    UI_System={IsInMenu='menu'},
}
local requests = {IsPreGame=function() return pregame end, IsGamePaused=function() return paused end}
local ui = {GetBool=function() return menu end}
local boards = {Get=function() return ui end}
local transaction = {GetItemInSlot=function(_, owner, slot) assert(owner == player); return slots[slot] end}
local spatial = {SyncRaycastByQueryFilter=function(_, a, b, filter, flag1, flag2)
    assert(filter == 'all' and flag1 == false and flag2 == false)
    assert(a.x == b.x and b.y - a.y == 30 and a.z == b.z and a.w == 1 and b.w == 1)
    return hits[a.x] ~= nil, {entity=hits[a.x]}
end}
Game = {
    GetSystemRequestsHandler=function() return requests end, GetAllBlackboardDefs=function() return defs end,
    GetBlackboardSystem=function() return boards end, GetTransactionSystem=function() return transaction end,
    GetSpatialQueriesSystem=function() return spatial end, VRWeaponInLeftHand=function() return leftMain end,
    VRMeleeInputFrame=function()
        if noFrame then return {} end
        local l = {w=tracking[left] and origin or 0}
        local r = {w=tracking[right] and origin or 0}
        if not stuckValid and now - stamp > 250 then l.w, r.w = 0, 0 end -- GetGestureHandFrame expiry
        return {{x=stamp % 16777216}, leftMain and r or l, leftMain and l or r, {}, {}}
    end,
}
function npc(id)
    return {
        id=id, attached=true, awake=true,
        IsA=function(_, name) return name == 'NPCPuppet' end,
        IsDead=function(self) return self.dead == true end,
        IsAttached=function(self) return self.attached end, IsActive=function(self) return self.awake and not self.dead end,
        GetEntityID=function(self) return {hash=self.id} end,
        VRPortSetWeaponAim=function(self, on) table.insert(events, {id=self.id, on=on, time=now}) end,
    }
end
a, b = npc('A'), npc('B')
wall = {IsA=function() return false end}
function draw(slot, entity, kind)
    local item = {hash=entity..'-item', kind=kind or 'Wea_Handgun'}
    local gun = {
        attached=true, ranged=true, item=item,
        IsA=function(_, name) return name == 'gameweaponObject' end,
        IsAttached=function(self) return self.attached end, IsRanged=function(self) return self.ranged end,
        GetItemID=function(self) return self.item end, GetEntityID=function() return {hash=entity} end,
        GetMuzzleSlotWorldTransform=function(self) return self.muzzle end,
        muzzle={Orientation={i=0,j=0,k=0,r=1}, position={x=slot==left and 0 or 1, y=0,z=1.5}},
    }
    slots[slot], active[slot], equipped[slot] = gun, item, gun
    return gun
end
gun = draw(right, 'gun1')
hits[1] = a
function tick(ms)
    now = now + ms
    if not frozen then stamp = now end
    aim.update(player, now / 1000)
end
function hold(ms)
    while ms > 0 do
        local step = math.min(ms, 20)
        tick(step)
        ms = ms - step
    end
end
function mature()
    tick(0); hold(2000)
    assert(aim.state.active == 1 and #events == 1 and events[1].on and events[1].time == now)
end
"""


def driver():
    lua = LuaRuntime(unpack_returned_tuples=True)
    lua.globals().package.path = (WEAPON_MOD / "?.lua").as_posix()
    lua.execute(GAME)
    lua.execute("aim = require('aim_threat')")
    return lua


class AimThreatTests(unittest.TestCase):
    def test_full_two_seconds_without_ads(self):
        driver().execute("""
            tick(0); hold(1999)
            assert(#events == 0)
            tick(1)
            assert(#events == 1 and events[1].on and events[1].time == 2000)
            hold(1000)
            assert(#events <= 11 and events[#events].on and aim.state.active == 1)
        """)

    def test_one_frame_interruptions_break_pending_and_active_dwell(self):
        # The interruption is shorter than the idle acquisition poll.
        cases = {
            "holstered, old object still attached": ("active[right]=nil", "active[right]=gun.item"),
            "same model, different active item": ("active[right]={hash='other',kind=gun.item.kind}", "active[right]=gun.item"),
            "unequip cycle": ("state.EquipmentRight=1", "state.EquipmentRight=0"),
            "unequipped": ("state.EquipmentRight=2", "state.EquipmentRight=0"),
            "forced empty hands": ("state.UpperBody=5", "state.UpperBody=0"),
            "temporary unequip": ("state.UpperBody=4", "state.UpperBody=0"),
            "empty slot": ("slots[right]=nil", "slots[right]=gun"),
            "unequip event with stale slot and active ID": ("equipped[right]=nil", "equipped[right]=gun"),
            "detached weapon": ("gun.attached=false", "gun.attached=true"),
            "missing muzzle": ("saved=gun.muzzle; gun.muzzle=nil", "gun.muzzle=saved"),
            "invalid rotation": ("gun.muzzle.Orientation.r=0", "gun.muzzle.Orientation.r=1"),
            "invalid position": ("gun.muzzle.position.x=0/0", "gun.muzzle.position.x=1"),
            "lost hand": ("tracking[right]=false", "tracking[right]=true"),
            "lost shared pose": ("shared[8]=0", "shared[8]=1"),
            "head aim or unavailable tracking": ("noFrame=true", "noFrame=false"),
            "wall first": ("hits[1]=wall", "hits[1]=a"),
            "miss": ("hits[1]=nil", "hits[1]=a"),
            "different NPC": ("hits[1]=b", "hits[1]=a"),
            "target unconscious": ("a.awake=false", "a.awake=true"),
            "target dead": ("a.dead=true", "a.dead=false"),
            "target detached": ("a.attached=false", "a.attached=true"),
            "pause": ("paused=true", "paused=false"),
            "pregame": ("pregame=true", "pregame=false"),
            "shared menu": ("shared[81]=1", "shared[81]=0"),
            "game menu": ("menu=true", "menu=false"),
            "cinematic": ("state.SceneTier=3", "state.SceneTier=1"),
            "player dead": ("playerDead=true", "playerDead=false"),
        }
        for name, (interrupt, restore) in cases.items():
            for mature_first in (False, True):
                with self.subTest(name=name, mature=mature_first):
                    lua = driver()
                    lua.execute("mature(); events={}" if mature_first else "tick(0); hold(1999)")
                    lua.execute(interrupt)
                    lua.execute("tick(1); assert(aim.state.active == 0)")
                    if mature_first:
                        lua.execute("assert(#events == 1 and not events[1].on and events[1].id == 'A')")
                    else:
                        lua.execute("assert(#events == 0)")
                    lua.execute(restore)
                    lua.execute("""
                        events={}; tick(1); hold(1999); assert(#events == 0)
                        hold(120); assert(aim.state.active == 1 and events[1].on and events[1].id == 'A')
                        assert(aim.state.error == nil)
                    """)

    def test_non_firearms_never_trace(self):
        for change in (
            "slots[right]={IsA=function() return false end}",  # e.g. a carried prop
            "gun.item.kind='Wea_Fists'", "gun.item.kind='Wea_Knife'",
            "gun.item.kind='Cyb_Launcher'", "gun.item.kind='Gad_Grenade'",
            "gun.item.kind='Wea_VehiclePowerWeapon'", "gun.ranged=false",
        ):
            with self.subTest(change=change):
                lua = driver()
                lua.execute(change)
                lua.execute("tick(0); hold(2500); assert(#events == 0 and aim.state.traces == 0 and aim.state.error == nil)")

    def test_weapon_hand_origin_and_player_identity(self):
        for change in (
            # A respawned entity for the same inventory item.
            "local old=gun.item; gun=draw(right,'replacement'); gun.item=old; active[right]=old",
            # The very same entity moving to the other hand.
            "slots[right],active[right]=nil,nil; slots[left],active[left]=gun,gun.item; leftMain=true",
            "origin=origin+1", "player.id='new-player'",
        ):
            with self.subTest(change=change):
                lua = driver()
                lua.execute("tick(0); hold(1999)")
                lua.execute(change)
                lua.execute("tick(1); hold(1999); assert(#events == 0); hold(20); assert(events[1].on)")

    def test_left_hand_only_and_dual_wield(self):
        driver().execute("""
            leftMain=true; slots[right], active[right]=nil,nil; state.EquipmentRight=2
            draw(left,'left','Wea_GrenadeLauncher'); hits[0]=b
            tracking[right]=false; shared[8]=0
            mature(); assert(events[1].id == 'B')
            tracking[left]=false; tick(1); assert(not events[2].on)
        """)
        driver().execute("""
            draw(left,'other'); hits[0]=a
            mature(); hits[1]=nil; tick(1); assert(aim.state.active == 1 and #events == 1)
            hits[0]=wall; tick(1); assert(aim.state.active == 0 and #events == 2 and not events[2].on)
        """)
        driver().execute("""
            mature(); draw(left,'other'); hits[0]=a; tick(1); hold(300)
            hits[1]=wall; tick(1); assert(aim.state.active == 0 and not events[#events].on)
            events={}; hold(1600); assert(#events == 0); hold(120); assert(events[1].on)
        """)
        driver().execute("""
            draw(left,'other'); hits[0]=b; tick(0); hold(2000)
            assert(aim.state.active == 2 and #events == 2 and events[1].id ~= events[2].id)
            events={}; state.EquipmentLeft=1; tick(1)
            assert(aim.state.active == 1 and #events == 1 and not events[1].on and events[1].id == 'B')
        """)

    def test_stale_and_discontinuous_samples(self):
        driver().execute("""
            stuckValid=true -- A frozen producer can keep both valid bits high.
            tick(0); hold(1999); frozen=true; hold(240)
            assert(#events == 0, 'replaying the last pose crossed the threshold')
            hold(100); frozen=false; tick(1); hold(1999); assert(#events == 0)
            hold(120); assert(events[1].on)
            frozen=true; hold(300); assert(not events[#events].on and aim.state.active == 0)
        """)
        driver().execute("""
            mature(); tick(500); assert(#events == 2 and not events[2].on)
            hold(1999); assert(aim.state.active == 0); hold(2); assert(aim.state.active == 1)
            tick(-400); assert(not events[#events].on and aim.state.active == 0)
        """)
        driver().execute("now=16777000; mature()")  # sample clock wrap within a hold

    def test_left_primary_exposed_through_weapon_right(self):
        # Test both equipment representations while only the RIGHT attachment
        # contains the muzzle. Frame[3] is the physical left hand in both.
        for left_equipment in (False, True):
            with self.subTest(left_equipment=left_equipment):
                lua = driver()
                lua.execute("leftMain=true; equipped[left],equipped[right]=gun,nil")
                if left_equipment:
                    lua.execute("active[left],active[right]=gun.item,nil; state.EquipmentRight=2")
                else:
                    lua.execute("state.EquipmentLeft=2")
                lua.execute("""
                    tracking[left]=false; tick(0); hold(2200)
                    assert(#events == 0 and aim.state.traces == 0, 'right tracking qualified the left barrel')
                    tracking[left]=true; tracking[right]=false; shared[8]=0
                    hold(2300); assert(aim.state.active == 1 and events[1].on)
                    events={}; shared[0]=0; shared[8]=1; tracking[right]=true; tick(1)
                    assert(#events == 1 and not events[1].on and aim.state.active == 0)
                    shared[0]=1; events={}; hold(1999); assert(#events == 0); hold(140)
                    assert(aim.state.active == 1 and events[1].on)
                """)
        driver().execute("""
            mature(); events={}; leftMain=true; tick(1)
            assert(#events == 1 and not events[1].on, 'same logical slot inherited the right hand dwell')
            hold(1999); assert(aim.state.active == 0); hold(20); assert(aim.state.active == 1)
        """)
        driver().execute("""
            leftMain=true; active[left]=gun.item; state.EquipmentLeft=1
            tick(0); hold(2300); assert(aim.state.traces == 0, 'right slot bypassed left unequip')
        """)
        driver().execute("""
            slots[left],active[left]=gun,gun.item
            tracking[right]=false; tick(0); hold(2300)
            assert(aim.state.traces == 0, 'one right-hand gun was qualified by its left slot alias')
            leftMain=true; hold(2300); assert(aim.state.active == 1)
            tracking[left]=false; tracking[right]=true; tick(1); assert(aim.state.active == 0)
        """)

    def test_frozen_mature_stamp_cannot_refresh_or_reactivate(self):
        driver().execute("""
            mature(); events={}; frozen=true; stuckValid=true
            hold(240); assert(#events == 0, 'duplicate pose renewed a mature lease')
            hold(20); assert(#events == 1 and not events[1].on)
            hold(2200); assert(#events == 1 and aim.state.active == 0)
            events={}; frozen=false; tick(1); hold(1999); assert(#events == 0)
            hold(120); assert(aim.state.active == 1 and events[1].on)
        """)

    def test_player_disappearance_and_despawn_cleanup(self):
        driver().execute("""
            mature(); aim.update(nil, now/1000); assert(#events == 2 and not events[2].on)
            events={}; mature()
            a.VRPortSetWeaponAim=function() error('despawned target') end
            hits[1]=nil; tick(1); assert(aim.state.active == 0 and aim.state.error == nil)
        """)


if __name__ == "__main__":
    unittest.main()
