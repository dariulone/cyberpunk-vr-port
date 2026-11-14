"""Verify visual mirror actors never enter the physical hand NPC list."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
p=Path(__file__).resolve().parents[2]/'mods/cet/CyberpunkVRPort_HandCollision/init.lua'
s=p.read_text(encoding='utf-8');a=s.index('local function npcAlive(e)');b=s.index('\nlocal function npcRefresh',a)
L=LuaRuntime(unpack_returned_tuples=True)
L.execute('ScriptedPuppet={IsAlive=function(e)return e.alive end};TDBID={ToStringDEBUG=function(v)return v end}')
f=L.execute(s[a:b]+'\nreturn function(record,alive,npc) local e={alive=alive,IsA=function()return npc end,GetRecordID=function()return record end};return npcAlive(e)end')
for r in ('Character.q110_tpp_male_v','Character.q110_tpp_female_v','Character.johnny_mirror'):assert not f(r,True,True)
for r in ('Character.Judy','Character.Silverhand','Character.TenantMale'):assert f(r,True,True)
assert not f('Character.Judy',False,True) and not f('Character.Judy',True,False)
print('PASS mirror clones excluded; normal/story NPCs retained; dead and non-NPC excluded')
