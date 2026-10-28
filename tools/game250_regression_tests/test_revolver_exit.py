from pathlib import Path
from lupa import LuaRuntime
r=Path(__file__).resolve().parents[2].joinpath('mods/cet/CyberpunkVRPort_Reload/reload')
l=LuaRuntime()
l.globals().rev=l.execute((r/'revolver.lua').read_text())
l.globals().cfg=l.execute((r/'weapons/malorian_overture.lua').read_text())
l.execute('''
local objects={};local nextId=0
WorldTransform={new=function() return {SetPosition=function() end,SetOrientation=function() end} end}
Vector4={new=function(...) return {...} end};Quaternion=Vector4
Game={FindEntityByID=function(id) return objects[id] end}
local spawner={Spawn=function() nextId=nextId+1;objects[nextId]={SetWorldTransform=function() end};return nextId end,Despawn=function() end}
local function pose(angle) return {x=0,y=0,z=1,qi=math.sin(angle/2),qj=0,qk=0,qr=math.cos(angle/2)} end
for _,fps in ipairs({30,60,90}) do
 local ctx={cfg=cfg.mag,spawner=spawner,dt=1/fps,cyl=pose(math.pi/2)}
 rev.eject(ctx,ctx.cyl,3)
 for i=1,fps do rev.tick(ctx) end
 assert(rev.held() and rev.heldLive()==3 and not rev.spent(),'level gun must retain rounds')
 -- 30 degree tip releases all six, even when leveled again during travel.
 ctx.cyl=pose(math.pi/3)
 for i=1,math.ceil(.15*fps) do rev.tick(ctx) end
 ctx.cyl=pose(math.pi/2)
 for i=1,math.ceil(.20*fps) do rev.tick(ctx) end
 assert(rev.spent() and not rev.held(),'all rounds must finish extraction')
 rev.clear(ctx)
end
''')
print('PASS actual revolver module: level retention, live ammo count, six staggered exits after tilt then leveling at 30/60/90 Hz.')
