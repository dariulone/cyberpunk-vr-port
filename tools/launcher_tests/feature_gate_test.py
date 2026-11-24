"""Run real Lua menu gates and disabled-physics cleanup with isolated engine stubs."""
from pathlib import Path
import unittest
from lupa.luajit21 import LuaRuntime

ROOT=Path(__file__).resolve().parents[2]
MENU=ROOT/'mods/cet/CyberpunkVRPort_Menu'

class FeatureGates(unittest.TestCase):
    def setUp(self):
        self.lua=LuaRuntime(unpack_returned_tuples=True)
        self.lua.execute('enabled=true; function VRInterfaceEnabled() return enabled end')
        self.available=self.lua.execute((MENU/'modules/availability.lua').read_text(encoding='utf-8'))
        self.lua.globals().availability=self.available
        self.lua.execute('function require(name) assert(name=="modules/availability"); return availability end')

    def test_gate_handles_native_absence_and_failure(self):
        self.assertTrue(self.available.enabled())
        self.lua.execute('enabled=false')
        self.assertFalse(self.available.enabled())
        self.lua.execute('function VRInterfaceEnabled() error("offline") end')
        self.assertFalse(self.available.enabled())
        self.lua.execute('VRInterfaceEnabled=nil')
        self.assertTrue(self.available.enabled())

    def test_disabled_vendor_and_storage_do_not_touch_engine(self):
        self.lua.execute('enabled=false; Game=setmetatable({}, {__index=function() error("engine touched") end})')
        for name,args in [('vendor_visit',(None,None,None)),('stash_visit',(None,None,False))]:
            module=self.lua.execute((MENU/f'modules/{name}.lua').read_text(encoding='utf-8'))
            self.assertFalse(module.begin(*args))

    def test_disabled_storage_calls_native(self):
        self.lua.execute('enabled=false; hooks={}; function Override(c,n,f) hooks[n]=f end; FromVariant=function(v) return v end')
        module=self.lua.execute((MENU/'modules/stash_visit.lua').read_text(encoding='utf-8'))
        module.bind(self.lua.table())
        self.lua.execute('wrapped=0; assert(hooks.OnOpenStorage({}, {}, function(v) wrapped=wrapped+1; return 91 end)==91); assert(wrapped==1)')

    def test_disabled_vendor_does_not_hide_native_layer(self):
        self.lua.execute('''enabled=false; hooks={}; function Override(c,n,f) hooks[c..":"..n]=f end
function ObserveBefore(...) end; function ObserveAfter(...) end
function NameToString(n) return n end
CName={new=function(n) return n end}
Game=setmetatable({}, {__index=function() error("engine touched") end})''')
        module=self.lua.execute((MENU/'modules/vendor_visit.lua').read_text(encoding='utf-8'))
        module.bind(self.lua.table())
        self.lua.execute('''local n=0
local function wrapped(...) n=n+1; return 17 end
assert(hooks['MenuScenario_Idle:OnLeaveScenario']({},'MenuScenario_Vendor',wrapped)==17)
hooks['UISystem:RequestVendorMenu']({},nil,'MenuScenario_Vendor',wrapped)
assert(n==2)''')

    def test_direct_open_rejected_before_world_access(self):
        source=(MENU/'modules/menu.lua').read_text(encoding='utf-8')
        start=source.index('function M.open(cam,initialSection)')
        end=source.index('\nfunction M.close()',start)
        self.lua.execute('M={}; Availability=availability; enabled=false')
        self.lua.execute(source[start:end])
        self.assertFalse(self.lua.globals().M.open())

    def test_disabled_physics_releases_all_owners_and_query_flags(self):
        source=(ROOT/'mods/cet/CyberpunkVRPort_HandCollision/init.lua').read_text(encoding='utf-8')
        start=source.index('    suspend = function()')
        end=source.index('    setPhys = function(',start)
        self.lua.execute('''K={PULL={[0]={},[1]={}}, FORCED={[0]=1,[1]=1}, CORPSE_GRASPED={[0]=true,[1]=true}, FAST={},
NPC_GRIP={[0]={npc=1,part=2,rec={held=true}},[1]={npc=2,part=3,rec={held=true}}}, QUERY_BACK={}}
NPC_BODY={heldRec={[0]={},[1]={}}}; K.HAIR={}; K.BREAST={}; Vector4={new=function(...) return {...} end}
released=0; corpses=0; npcs=0; queries=0; carried=true
function carryRelease(side,speed) assert(not speed); released=released+1 end
function K.corpseLetGo(side) K.CORPSE_GRASPED[side]=nil; corpses=corpses+1 end
function VRNpcGrip(...) npcs=npcs+1 end
function VRBodySetQueryable(body,on) assert(on);queries=queries+1 end
function K.carryState(on) carried=on end
function K.weightTick() weightCleared=true end
hairRec={e=1,hairStrands='weights'}; breastRec={e=2}; K.HAIR[hairRec]={1,2,3};K.BREAST[breastRec]=true
function VRHairForce(e,v) assert(e==1 and v[1]==0);hairCleared=true end
function VRHairStrands(e,s) assert(e==1 and s=='');strandsCleared=true end
function VRDangleForce(e,v) assert(e==2 and v[1]==0);dangleCleared=true end
rec={body=99,queryOff=true}; K.QUERY_BACK[rec]=1''')
        cleanup=self.lua.execute('return {'+source[start:end]+'}')
        cleanup.suspend()
        self.lua.execute('''assert(released==2 and corpses==2 and npcs==2 and queries==1 and not carried and weightCleared)
assert(next(K.NPC_GRIP)==nil and next(K.PULL)==nil and next(K.FORCED)==nil)
assert(next(K.QUERY_BACK)==nil and next(NPC_BODY.heldRec)==nil and not rec.queryOff)
assert(hairCleared and strandsCleared and dangleCleared and next(K.HAIR)==nil and next(K.BREAST)==nil)''')

if __name__=='__main__': unittest.main()
