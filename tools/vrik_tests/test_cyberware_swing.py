"""Replay short tracked punches without starting the game (requires lupa)."""
from pathlib import Path
from lupa import LuaRuntime

root = Path(__file__).resolve().parents[2]
source = (root / 'mods/cet/CyberpunkVRPort_Weapon/cyberware.lua').read_text()
for hz in (30, 60, 90, 120):
    lua = LuaRuntime(unpack_returned_tuples=True)
    lua.globals().package.path=str(root / 'mods/cet/CyberpunkVRPort_Weapon/?.lua').replace('\\','/')+';'+lua.globals().package.path
    lua.execute('''
        stamp=0; x=0; renderedMotion=0; kind=1; starts={}; contacts={}
        function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
        Game={GetSystemRequestsHandler=function() return {IsGamePaused=function() return false end} end,
          GetVRSharedSlot=function() return 0 end,
          VRCyberwareFrame=function()
            return {v(kind,2,2,stamp),v(0,0,0,stamp),v(x,0,1),v(0,0,1),v(0,0,0,1),v(0,0,0,1),
                    v(x+renderedMotion,0,1),v(x+renderedMotion,.08,1),v(0,0,1),v(0,.08,1),
                    v(x,0,1),v(0,0,1),v(0,0,0,1),v(0,0,0,1)}
          end}
        player={VRCyberwareBeginSwing=function(_,w,side,strong) starts[#starts+1]={stamp,side};return true end,
          VRCyberwareContacts=function(_,w,side,points,previous,radius,passive)
            if not passive then contacts[#contacts+1]={stamp,side,previous[1].x,points[1].x} end
          end, VRMeleeWhoosh=function() end}
        weapon={}
    ''')
    module = lua.execute(source)
    g = lua.globals()
    def frame(pos):
        g.stamp = round(g.stamp + 1000 / hz)
        g.x = pos
        module.update(1 / hz, g.player, g.weapon, 1, 0)
    for _ in range(hz):
        frame(0)
    impact = round(g.stamp + 1000 / hz)
    travel = 3.5 / hz
    frame(travel)
    assert len(g.starts) == 0 and module.state.fullSwingStarts == 1, (hz, 'short punch must be physical')
    assert len(g.contacts) == 1 and g.contacts[1][1] == impact, (hz, 'contact delayed')
    assert g.contacts[1][3] == 0 and abs(g.contacts[1][4] - travel) < 1e-6, 'lost swept interval'
    for _ in range(hz):
        frame(travel)
    assert len(g.contacts) == 1, 'held fist repeatedly damages'
    for i in range(20):
        g.renderedMotion = .1 if i % 2 else -.1
        frame(travel)
    assert module.state.fullSwingStarts == 1 and len(g.contacts) == 1, 'native body/IK motion triggered another punch'
    g.renderedMotion = 0
    frame(travel + 2)
    assert len(g.contacts) == 1, 'tracking teleport damages'
    print(f'{hz}Hz: short punch contacts in same sample; stationary/teleport produce no extra contacts')

# Same thresholds for all three arm weapons, including a violently oscillating
# wire whose owner is stationary/moving slowly. These used to generate sounds.
for kind in (1, 2, 3):
    for hz in (30, 60, 90, 120):
        lua=LuaRuntime(unpack_returned_tuples=True)
        lua.globals().package.path=str(root / 'mods/cet/CyberpunkVRPort_Weapon/?.lua').replace('\\','/')+';'+lua.globals().package.path
        lua.execute('''
          stamp=0;x=0;jiggle=0;whooshes=0;swings=0;kind=1
          function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
          Game={GetVRSharedSlot=function() return 0 end,
            GetSystemRequestsHandler=function() return {IsGamePaused=function() return false end} end,
            VRCyberwareFrame=function() return {
              v(kind,2,2,stamp),v(0,0,0,stamp),v(x,0,1),v(0,0,1),v(0,0,0,1),v(0,0,0,1),
              v(stoppedX or x,0,1),v(stoppedX or x,jiggle,1),v(0,0,1),v(0,0,1),
              v(x,0,1),v(0,0,1),v(0,0,0,1),v(0,0,0,1)} end}
          player={VRCyberwareBeginSwing=function() swings=swings+1;return true end,
            VRCyberwareContacts=function() end,VRMeleeWhoosh=function() whooshes=whooshes+1 end}
          weapon={}
        ''')
        m=lua.execute(source);g=lua.globals();g.kind=kind
        def tick(speed,jiggle=0):
            g.stamp=round(g.stamp+1000/hz);g.x+=speed/hz;g.jiggle=jiggle
            m.update(1/hz,g.player,g.weapon,kind,0)
        for _ in range(hz):tick(0)
        for i in range(hz):tick(.25,.1 if i%2 else -.1)
        assert g.whooshes==0 and g.swings==0,(kind,hz,'slow controller/rope vibration attacks or makes a sound')
        for _ in range(hz):tick(0)
        for _ in range(round(.14*hz)):tick(1.8)
        assert g.whooshes==0 and g.swings==0, 'below Gorilla threshold must stay silent'
        for _ in range(hz):tick(0)
        for _ in range(round(.14*hz)):tick(2.2)
        expected=1 if kind==1 else 0
        assert g.whooshes==expected and (m.state.fullSwingStarts or 0)==expected and g.swings==0,(kind,hz,'Gorilla 2m/s override changed other weapons')
        for _ in range(hz):tick(0)
        for _ in range(round(.14*hz)):tick(3.6)
        assert g.whooshes==expected+1,(kind,hz,'real swing must make exactly one sound',g.whooshes)
        for i in range(hz):tick(0,.1 if i%2 else -.1)
        assert g.whooshes==expected+1,(kind,hz,'residual rope motion repeated the sound')
        # The solved item moves for one frame and then meets a fixed obstacle.
        # The controller continues through the audio window at the original speed.
        for _ in range(hz):tick(0)
        tick(3.6)
        g.stoppedX=g.x
        for _ in range(round(.20*hz)):tick(3.6)
        assert g.whooshes==expected+1,(kind,hz,'blocked item made an air sound')
        print(f'kind={kind} {hz}Hz: slow/settling silent; deliberate swing sounds once')

# Ordinary weapons share the tracked gate. Test model-only motion separately
# from physical wrist motion, and steering a two-hand grip about a still wrist.
for hz in (30, 60, 90, 120):
    lua=LuaRuntime(unpack_returned_tuples=True)
    lua.globals().package.path=str(root / 'mods/cet/CyberpunkVRPort_Weapon/?.lua').replace('\\','/')+';'+lua.globals().package.path
    lua.execute('''
      stamp=0;x=0;renderedX=0;theta=0;held=false;swings=0;whooshes=0;activeContacts=0
      function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
      Vector4={new=v}
      Game={VRWeaponInLeftHand=function() return false end,GetVRSharedSlot=function() return 0 end,
        GetSystemRequestsHandler=function() return {IsGamePaused=function() return false end} end,
        VRMeleeInputFrame=function() return {
          v(stamp,stamp,held and 1 or 0),v(x+.2*math.sin(theta),.2*math.cos(theta),1,1),v(x,0,1,1),
          v(0,0,0,1),v(0,0,0,1)} end}
      player={VRCyberwareBeginSwing=function() swings=swings+1;return true end,
        VRCyberwareUpdateSwing=function() end,
        VRMeleeSweepHit=function(_,w,p,t,oldP,oldT,r,passive) if not passive then activeContacts=activeContacts+1 end end,
        VRMeleeWhoosh=function() whooshes=whooshes+1 end}
      weapon={VRMeleeReach=function() return 1 end,VRBladeKind=function() return -1 end}
      function position() return v(renderedX,0,1) end
      function forward() return v(math.sin(theta),math.cos(theta),0,0) end
      origin=v(0,0,0)
    ''')
    m=lua.execute(source);g=lua.globals()
    def tick():
        g.stamp=round(g.stamp+1000/hz)
        m.ordinary(1/hz,g.player,g.weapon,g.position(),g.forward(),g.origin)
    for _ in range(hz):tick()
    for i in range(hz):g.renderedX=.10 if i%2 else -.10;tick()
    assert g.swings==g.whooshes==g.activeContacts==0, (hz,'model-only motion produced attack')
    g.renderedX=0
    for _ in range(hz):tick()
    for _ in range(round(.12*hz)):g.x+=6.5/hz;g.renderedX=g.x;tick()
    assert g.swings==0 and m.state.fullSwingStarts==g.whooshes==1 and g.activeContacts>0,(hz,'physical ordinary swing missing')
    for _ in range(hz):tick()
    g.held=True
    for _ in range(hz):tick()
    # Right controller stays still; the left rotates the physical grip line.
    for _ in range(round(.12*hz)):g.theta+=8/hz;tick()
    assert g.swings==0 and m.state.fullSwingStarts==g.whooshes==2,(hz,'support-hand-only steering missing')
    print(f'ordinary {hz}Hz: tracked wrist and support-hand swings; model-only motion silent')

# CET has one callback slot per event per mod. Loading the actual entry point
# catches accidental replacement of weapon/profile updates by combat updates.
lua=LuaRuntime(unpack_returned_tuples=True)
lua.globals().package.path=str(root / 'mods/cet/CyberpunkVRPort_Weapon/?.lua').replace('\\','/')+';'+lua.globals().package.path
lua.execute('''
  events={}
  function registerForEvent(name,callback)
    assert(not events[name], 'duplicate CET event: '..name)
    events[name]=callback
  end
  Game={GetPlayer=function() return nil end,VRCyberwareSelect=function(kind) assert(kind==0) end,
    VRHandEquipment=function(mask) assert(mask==3) end}
  spdlog={info=function(message) error(message) end}
''')
lua.execute((root / 'mods/cet/CyberpunkVRPort_Weapon/init.lua').read_text(encoding='utf-8'))
lua.execute("assert(events.onUpdate);for i=1,300 do events.onUpdate(1/60) end")
print('CET entry point: one update handler; menu without player does not install weapon hooks')
