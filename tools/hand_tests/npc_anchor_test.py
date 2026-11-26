"""Exercise the production Lua segment mapping and native NPC driver path."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
root=Path(__file__).resolve().parents[2]
source=(root/'mods/cet/CyberpunkVRPort_HandCollision/init.lua').read_text(encoding='utf-8')
def span(a,b):
    i=source.index(a);return source[i:source.index(b,i)]
lua=LuaRuntime(unpack_returned_tuples=True)
lua.execute('''
NPC_BODY={kinds={},grips={},segments={}};sent=0;pointValid=true
function npcPt()if pointValid then return 1,2,3 end end
''')
select=lua.execute(span('local npcUid, npcPart','local function npcFoot')+'''
return function(kind,first)
 npcPart=0;sent=0;NPC_BODY.kind=kind;NPC_BODY.grip=1
 npcSend(function()sent=sent+1 end,first,first,0,0,0,0,first,first,0,0,0,0,.04,false)
 return NPC_BODY.segments[1],npcPart
end
''')
for kind,first,expected in [(0,4,0),(1,5,0),(0,6,1),(1,7,1),(0,8,2),(1,9,2),(2,3,0),(3,3,0),(4,1,None),(5,2,None)]:
    assert select(kind,first)==(expected,1)
lua.globals().pointValid=False
assert select(1,7)==(1,1) and lua.globals().sent==0,'missing slot shifted the stable capsule-to-bone mapping'
lua.execute('''
K={NPC_GRIP={}};calls={};Vector4={new=function(x,y,z,w)return{x=x,y=y,z=z,w=w}end}
function VRNpcGripBind(npc,side,part,segment,owner)
 calls[#calls+1]={npc=npc,side=side,part=part,segment=segment,owner=owner};return .003
end
function VRNpcGrip(npc,side,part,target,turn)released=target.w==0 end
function handReal()error('old wrist-offset path called')end
function worldPos()error('Lua sampled a second NPC frame')end
''')
lua.execute(span('K.npcGripTick = function','K.npcTick = function'))
lua.execute('K.NPC_GRIP[1]={nativeAnchor=true,npc=123,phys=true,part=1,segment=0,owner=108}')
lua.globals().K.npcGripTick(1,True,.016,100,200,300,0,0,0,1)
assert len(lua.globals().calls)==1 and lua.globals().calls[1].segment==0
assert lua.globals().K.NPC_GRIP[1] is not None
lua.globals().K.npcGripTick(1,False,.016,100,200,300,0,0,0,1)
assert lua.globals().released and lua.globals().K.NPC_GRIP[1] is None
print('NPC bone anchors: upper/forearm/hand mapping, stable missing-slot indices, native-only driving and release PASS')
