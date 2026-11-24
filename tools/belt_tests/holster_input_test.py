"""Run the real holster input/update functions with game and geometry boundaries mocked."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

root = Path(__file__).resolve().parents[2]
source = (root / "mods/cet/CyberpunkVRPort_Holster/modules/holsters.lua").read_text(encoding="utf-8")
lua = LuaRuntime(unpack_returned_tuples=True)
lua.execute("""
    grips = {[49]=0, [155]=0}
    function GetVRSharedSlot(slot) return grips[slot] or 0 end
    function VRLeftHanded() return false end
    playerPresent, dataPresent, suspended = true, true, false
    player = {}
    Game = {GetPlayer=function() if playerPresent then return player end end}
    require = function() return {} end
    io.open = function() return nil end
    function replace(fn, name, value)
        for i=1,100 do
            local n, old = debug.getupvalue(fn, i)
            if not n then break end
            if n == name then debug.setupvalue(fn, i, value); return old end
        end
        error('missing real-module boundary: ' .. name)
    end
""")
lua.globals().holsters = lua.execute(source)
# Exercise the other component helpers too: their pcalls otherwise swallow a
# missing legacy method instead of producing the update-aborting root failure.
helpers = source[source.index("local function showEntity(e, on)"):
                 source.index("-- The copy where its place puts it")]
lua.globals().component_helpers = lua.execute(
    "return function(S)\n" + helpers +
    "\nreturn {show=showEntity, hide=hideReal, restore=unhideReal, root=root}\nend")
lua.execute("""
    local h = holsters
    local function noop() end
    local hand = replace(h.update, 'hand', noop)
    replace(h.update, 'hand', hand)
    grabs, cancellations = {}, 0
    replace(hand, 'grab', function(_, _, _, side) grabs[#grabs+1]=side end)
    replace(hand, 'pullTick', noop)
    replace(h.update, 'playerData', function() return {}, dataPresent and {} or nil end)
    replace(h.update, 'inHands', function() return {}, {} end)
    replace(h.update, 'sceneNow', function() return suspended end)
    local realSettleShells = replace(h.update, 'settleShells', noop)
    for _, name in ipairs({'scan', 'settle', 'revealReal', 'learnHolds', 'releaseTick', 'graspTick'}) do
        replace(h.update, name, noop)
    end
    replace(hand, 'endPull', function() cancellations=cancellations+1;h._S.pull=nil end)
    function tick(enabled) h.update(.016, enabled ~= false) end
    function neutral()
        grips[49], grips[155]=0,0
        playerPresent, dataPresent, suspended=true,true,false
        tick();grabs={}
    end
    -- Ordinary input still fires once per press, in either hand.
    neutral()
    for _, slot in ipairs({49,155}) do
        local before=#grabs
        grips[slot]=1;tick();tick()
        assert(#grabs==before+1, 'held active grip repeats or press was lost')
        grips[slot]=0;tick()
    end
    assert(grabs[1]==1 and grabs[2]==0, 'physical hand identity changed')
    print('PASS normal per-hand grip edges')

    function suspension(kind, on)
        if kind=='scene' then suspended=on
        elseif kind=='player' then playerPresent=not on
        elseif kind=='data' then dataPresent=not on end
    end
    for _, kind in ipairs({'scene','disabled','player','data'}) do
        neutral()
        suspension(kind,true)
        grips[49]=1;grips[155]=1;tick(kind~='disabled')
        suspension(kind,false);tick()
        assert(#grabs==0, kind .. ': grip pressed while suspended was deferred until resume')
        grips[49],grips[155]=0,0;tick()
        grips[49],grips[155]=1,1;tick()
        assert(#grabs==2, kind .. ': fresh grip after resume did not work')
        -- A release sampled during suspension must rearm the next active press.
        suspension(kind,true)
        grips[49],grips[155]=0,0;tick(kind~='disabled')
        suspension(kind,false)
        grips[49],grips[155]=1,1;tick()
        assert(#grabs==4, kind .. ': release during suspension was lost')
        print('PASS suspended grip sampling: ' .. kind)
    end
    for _, kind in ipairs({'scene','disabled','player','data'}) do
        neutral()
        local before=cancellations
        h._S.pull={side=1,key='copy'}
        suspension(kind,true);tick(kind~='disabled')
        assert(cancellations==before+1 and h._S.pull==nil, kind .. ': suspended pull survived')
    end
    print('PASS pull cancellation across suspension gates')

    -- A clean bundled install has the native component enumerator, not
    -- Codeware's entity:GetComponents extension. Keep the real shell path.
    neutral()
    CName={new=function(s)return s end}
    NameToString=function(s)return s end
    Vector4={new=function(x,y,z,w)return {x=x,y=y,z=z,w=w} end}
    Quaternion={new=function(i,j,k,r)return {i=i,j=j,k=k,r=r} end}
    local function component(name, kind, enabled)
        return {name=name,kind=kind,enabled=enabled,
            IsA=function(self,t)return t==self.kind or t=='entIPlacedComponent' end,
            IsEnabled=function(self)return self.enabled end,
            GetName=function(self)return self.name end,
            Toggle=function(self,on)self.enabled=on end,
            SetLocalTransform=function(self,p,q)self.position=p;self.rotation=q end}
    end
    local rootComponent=component('root','entIPlacedComponent',true)
    local mesh=component('shell_mesh','entIVisualComponent',true)
    local hidden=component('authored_hidden','entIVisualComponent',false)
    local light=component('light','entLightComponent',true)
    mesh.parentTransform={};hidden.parentTransform={};light.parentTransform={}
    local entity={components={rootComponent,mesh,hidden,light},
        FindComponentByName=function()return nil end}
    assert(entity.GetComponents==nil, 'fixture accidentally supplies Codeware')
    local reads=0
    Game.VREntityComponents=function(e)reads=reads+1;return e.components end
    player.FindComponentByName=function()return {} end
    VRDetachFromSlot=function()end
    VRAttachToSlot=function()return true end
    local shell={entity=entity,comp='slots',slot='hips',
        pose={p={1,2,3},q={0,0,0,1}}}
    h._S.shells={leg_r=shell}
    replace(h.update, 'settleShells', realSettleShells)
    grips[49]=1;tick()
    assert(shell.attached and rootComponent.position.z==3, 'native enumeration did not seat shell')
    assert(#grabs==1 and grabs[1]==1, 'shell update prevented grip input without Codeware')
    suspended=true;tick()
    assert(not mesh.enabled and not light.enabled and not hidden.enabled, 'scene failed to hide shell components')
    suspended=false;tick()
    assert(mesh.enabled and light.enabled and not hidden.enabled, 'scene restore changed authored visibility')
    assert(reads>=3, 'shell root/visibility did not use native enumeration')
    print('PASS native-only shell enumeration reaches grip input and preserves visibility')
    local helpers=component_helpers(h._S)
    entity.GetEntityID=function()return {hash=42}end
    assert(helpers.show(entity,false)==3, 'copy visual/light enumeration failed silently')
    assert(not mesh.enabled and not light.enabled, 'copy remained visible')
    assert(helpers.show(entity,true)==3, 'copy reveal enumeration failed silently')
    hidden.enabled=false
    helpers.hide('weapon',entity)
    assert(not mesh.enabled and not light.enabled and not hidden.enabled, 'real weapon hide failed silently')
    local record=h._S.hiddenReal.weapon
    assert(record.names.shell_mesh and record.names.light and not record.names.authored_hidden, 'real weapon visibility record incorrect')
    helpers.restore(record,entity)
    assert(mesh.enabled and light.enabled and not hidden.enabled, 'real weapon restore changed authored visibility')
    helpers.hide('weapon',entity)
    entity.GetEntityID=function()return {hash=43}end
    helpers.restore(record,entity)
    assert(not mesh.enabled, 'replaced entity was restored with old visibility record')
    assert(helpers.root(entity)==rootComponent, 'single placed root not selected')
    local second=component('other_root','entIPlacedComponent',true)
    entity.components[#entity.components+1]=second
    assert(helpers.root(entity)==nil, 'ambiguous placed roots accepted')
    entity.FindComponentByName=function()return rootComponent end
    assert(helpers.root(entity)==rootComponent, 'placeholder root priority lost')
    print('PASS native-only copy/weapon enumeration, visibility and root ownership')
""")

# Real placement and shell modules: no generated geometry and no game process needed.
geometry = LuaRuntime(unpack_returned_tuples=True)
geometry.globals().holster_root = str(root / "mods/cet/CyberpunkVRPort_Holster").replace("\\", "/")
geometry.execute(r"""
    package.path = holster_root .. '/?.lua;' .. package.path
    local P = require('modules/placements')
    local SH = require('modules/shells')
    local D = require('data/shells')
    local H = require('modules/holsters')
    -- Authored shell-fit quaternions are rounded to six decimal places.
    local function close(a,b) assert(math.abs(a-b)<3e-6, tostring(a)..' != '..tostring(b)) end
    local function vec(a,b) for i=1,#b do close(a[i],b[i]) end end
    local function up(fn,name)
        for i=1,100 do
            local n,v=debug.getupvalue(fn,i)
            if n==name then return v end
            if n==nil then break end
        end
        error('missing production helper: '..name)
    end
    local compose=up(SH.wanted,'compose')
    local copyWorld=up(up(H.near,'grabSpan'),'copyWorld')
    CName={new=function(v)return v end}
    Vector4={new=function(x,y,z,w)return {x=x,y=y,z=z,w=w}end}
    WorldPosition={ToVector4=function(v)return v end}
    VRWorldToModel=function(v)return v end
    local component={GetSlotTransform=function()
        return true,{Position={x=0,y=0,z=0},Orientation={i=0,j=0,k=0,r=1}}
    end}
    local player={FindComponentByName=function()return component end}
    Game={GetPlayer=function()return player end}
    for _,left in ipairs({false,true}) do
        VRLeftHanded=function()return left end
        local side=left and 'l' or 'r'
        local hand=left and 0 or 1
        local sign=left and -1 or 1
        assert(P.kind('Wea_Handgun')=='pistol' and P.kind('Wea_Revolver')=='pistol')
        for _,name in ipairs({'hip_r','thigh_r'}) do
            local p=P.get(name,left,false)
            local old=name=='hip_r' and {.17,-.06,.23*sign} or {-.24,-.047,.19*sign}
            vec(p.pos,{old[1]-.03,old[2]-.02,old[3]})
            assert(p.reach==(name=='hip_r' and .15 or .12))
            for _,body in ipairs({'m','w'}) do
                assert(SH.place(p,body)==p, 'shell override moved handgun away from shared placement')
                for _,entry in ipairs({{'pistol','','Wea_Handgun'},{'revolver','','Wea_Revolver'},
                        {'overture','Overture','Wea_Revolver'},{'malorian','Silverhand','Wea_Handgun'}}) do
                    local c={place=p,kind='pistol',look=entry[2],typeName=entry[3],attached=true}
                    local shell=SH.wanted({c},body)[(name=='hip_r' and 'leg_' or 'thigh_')..side]
                    assert(shell and shell.comp==p.comp and shell.slot==p.slot)
                    local seated=compose(shell.pose,D.leg[side][entry[1]][body].fit)
                    vec(seated.p,p.pos);vec(seated.q,p.rot)
                    local w,q=copyWorld(player,c,0)
                    vec(w,p.pos);vec(q,p.rot)
                    c.entity={GetWorldPosition=function()return {x=w[1],y=w[2],z=w[3]}end}
                    H._S.copies={c}
                    close(H.near(w[1],w[2],w[3],hand),0)
                    assert(H.near(w[1]+p.reach+.001,w[2],w[3],hand)==nil)
                    assert(H.near(w[1],w[2],w[3],1-hand)==nil)
                end
            end
        end
        vec(P.get('thigh_r',left,true).pos,{-.24,-.047,.19*sign})
        vec(P.get('hip_l',left,true).pos,{.105,-.147,-.178*sign})
        vec(P.get('hip_r',left,true).pos,{.17,-.06,.23*sign})
        vec(P.get('underarm_l',left,false).pos,{-.10,-.11,-.20*sign})
        vec(P.get('shoulder_r',left,false).pos,{.10,.125,.12*sign})
        vec(P.get('lower_back',left,false).pos,{.04,.15,.14*sign})
        local hip={place=P.get('hip_r',left,false),kind='pistol'}
        local thigh={place=P.get('thigh_r',left,false),kind='pistol'}
        local shells=SH.wanted({hip,thigh},'w')
        assert(shells['leg_'..side] and shells['thigh_'..side], 'same-side shells overwrote each other')
        assert(next(SH.wanted({{place=P.get('thigh_r',left,true),kind='knife'}},'w'))==nil)
    end
    print('PASS handgun hip/thigh -3cm up +2cm forward: both hands/bodies, all shell fits, copies and reach')
    print('PASS knife/melee/underarm/long-gun positions unchanged; independent hip and thigh shells')
""")
