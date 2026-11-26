"""Run the real optics scan and restoration, not just the visibility policy table."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

root=Path(__file__).resolve().parents[2]
lua=LuaRuntime(unpack_returned_tuples=True)
lua.globals().source_root=str(root/'mods/cet/CyberpunkVRPort_Optics').replace('\\','/')
lua.execute('''
package.path = source_root..'/?.lua;'..package.path
function registerForEvent() end
CName = {new=function(s) return s end}
function NameToString(s) assert(type(s)=='string','invalid visibility property'); return s end
gamedataItemType={Wea_SniperRifle='sniper'}
gamedataWeaponEvolution={Smart='smart'}
gamedataStatType={ZoomLevel=1}
components={}
function component(name,cls,gate,enabled)
    local c={name=name,cls=cls,visibilityAnimationParam=gate,enabled=enabled,writes=0}
    function c:GetName()return self.name end
    function c:IsA(t)return t==self.cls or (self.cls=='entPhysicalMeshComponent' and t=='entMeshComponent')end
    function c:IsEnabled()return self.enabled end
    function c:Toggle(v)self.enabled=v;self.writes=self.writes+1 end
    -- Reading this field from a plain MeshComponent is an API error in CET.
    if not gate then setmetatable(c,{__index=function(_,k)
        if k=='visibilityAnimationParam' then error('MeshComponent has no visibilityAnimationParam')end
    end})end
    table.insert(components,c);return c
end
glass=component('w_rifle_sniper__nokota_osprey__base1_3648','entSkinnedMeshComponent','invisibleWhenAiming',false)
cap=component('w_att__scope_sniper_01_5183','entSkinnedMeshComponent','invisibleWhenAiming',true)
tube=component('native_tube','entPhysicalMeshComponent','visibleWhenAiming',false)
plain=component('plain_housing','entMeshComponent',nil,true)
mag=component('magazine','entSkinnedMeshComponent','showMagazine',false)
weapon={}
function weapon:GetEntityID()return {hash=1}end
function weapon:IsRanged()return true end
function weapon:HasScope()return false end
function weapon:GetWeaponRecord()return {ItemType=function()return {Type=function()return 'handgun'end}end,Evolution=function()return nil end}end
player={FindComponentByName=function()return nil end}
Game={VREntityComponents=function()return components end,FindEntityByID=function()return weapon end,
    GetStatsSystem=function()return {GetStatValue=function()return 1 end}end}
function upvalue(fn,wanted)
    for i=1,100 do local n,v=debug.getupvalue(fn,i);if not n then break end;if n==wanted then return v end end
    error('missing production upvalue '..wanted)
end
''')
mod=lua.execute((root/'mods/cet/CyberpunkVRPort_Optics/init.lua').read_text(encoding='utf-8'))
lua.globals().mod=mod
lua.execute('''
local scan=upvalue(mod.update,'scan')
local restore=upvalue(scan,'restoreWeapon')
scan(player,weapon)
assert(glass.enabled and tube.enabled and plain.enabled)
assert(cap.enabled and not mag.enabled)
assert(glass.visibilityAnimationParam=='None' and tube.visibilityAnimationParam=='None')
assert(mag.visibilityAnimationParam=='showMagazine')
local writes=glass.writes+cap.writes+tube.writes
scan(player,weapon)
assert(writes==glass.writes+cap.writes+tube.writes,'repeated scan toggled mesh again')
restore()
assert(not glass.enabled and cap.enabled and not tube.enabled)
assert(glass.visibilityAnimationParam=='invisibleWhenAiming' and tube.visibilityAnimationParam=='visibleWhenAiming')
local shell=component('w_att__scope_long_01_5273','entSkinnedMeshComponent','invisibleWhenAiming',false)
local main=component('w_att__scope_long_01_017225','entSkinnedMeshComponent','None',true)
main.chunkMask=255
component('vr_optic_low','entMeshComponent',nil,false)
component('vr_optic_surface','entMeshComponent',nil,false)
component('vr_optic_panel','entMeshComponent',nil,false)
local native,variant=require('optics/visibility').housing(shell.name)
shell.mesh={hash=native}
local changes=0
Game.VRComponentChangeResource=function(c,path) changes=changes+1;c.mesh={hash=path};return true end
Game.VRComponentRefreshAppearance=function() end
Game.VRComponentChangeAppearance=function(c,name)c.meshAppearance=name;return true end
weapon.HasScope=function()return true end
mod.policy.cameraThreshold=4
scan(player,weapon);assert(shell.enabled and shell.mesh.hash==variant and changes==1)
scan(player,weapon);assert(changes==1,'stable shader path requested a second resource load')
shell.mesh={hash=native} -- same item after the game rebuilt its appearance
scan(player,weapon);assert(shell.mesh.hash==variant and changes==2)
mod.policy.cameraThreshold=1
scan(player,weapon);assert(shell.enabled and shell.mesh.hash==native and changes==3)
mod.policy.cameraThreshold=4
scan(player,weapon);restore()
assert(shell.mesh.hash==native and not shell.enabled and shell.visibilityAnimationParam=='invisibleWhenAiming')
components={}
local old=component('w_smg__budget_guillotine__base1_1413','entSkinnedMeshComponent','visibleWhenAiming',true)
local lens=component('vr_builtin_guillotine','entSkinnedMeshComponent','None',false)
weapon.HasScope=function()return false end
scan(player,weapon)
assert(mod.path=='none' and old.enabled and not lens.enabled,'incomplete integrated camera hid the native sight')
local panel=component('vr_builtin_guillotine_panel','WorldWidgetComponent',nil,false)
scan(player,weapon)
assert(mod.path=='camera' and mod.profile=='guillotine' and not old.enabled and lens.enabled and panel.enabled)
local stable=old.writes+lens.writes+panel.writes
scan(player,weapon)
assert(old.writes+lens.writes+panel.writes==stable,'integrated sight flickered on rescan')
weapon.HasScope=function()return true end
scan(player,weapon)
assert(mod.path=='collimator' and old.enabled and not lens.enabled and not panel.enabled,'mounted sight did not take over')
weapon.HasScope=function()return false end
scan(player,weapon);restore()
assert(old.enabled and not lens.enabled and not panel.enabled,'native visibility was not restored')
''')
print('PASS scope housings, shader/camera transitions, appearance rebuild and restore')
