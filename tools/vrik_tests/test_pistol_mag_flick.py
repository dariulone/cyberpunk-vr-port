"""Exercise the production Lua gesture with timestamped XR frames, without a game."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

ROOT = Path(__file__).resolve().parents[2]


def test_flick():
    lua = LuaRuntime()
    lua.globals().flick = lua.execute((ROOT / 'mods/cet/CyberpunkVRPort_Reload/reload/mag_flick.lua').read_text())
    lua.execute('''
        local side, enabled, paused, trigger, pose, frame, yaw, speedLimit
        local pistol={GetItemID=function() return 1 end}
        gamedataItemType={Wea_Handgun=1}
        WeaponObject={IsOfType=function(id,t) return id==t end}
        Game={VRWeaponTriggerEnabled=function() return enabled end,
              VRWeaponInLeftHand=function() return side==0 end,
              GetSystemRequestsHandler=function() return {IsGamePaused=function() return paused end} end,
              VRMeleeThrowFrame=function() return frame end}
        GetVRSharedSlot=function(slot)
            assert(slot==(side==0 and 154 or 160))
            return trigger
        end
        local function point(x,y,z) return {x=x,y=y,z=z} end
        local stamp,seq,origin,body
        local function sample(dx,dy,dz,ms,headX)
            seq=seq+1; stamp=(stamp+(ms or 20))%16777216
            pose=point(pose.x+dx,pose.y+dy,pose.z+dz)
            frame={point(stamp,seq,origin),pose,point(math.cos(yaw),math.sin(yaw),0),
                   point(0,0,-1),point(0,0,-1),point(headX or 0,0,0)}
        end
        local function setup(hand)
            side=hand or 1; enabled=true; paused=false; trigger=0; yaw=0; speedLimit=5
            stamp=16777165; seq=0; origin=17; pose=point(0,0,0);body={0,0,-1}
            sample(0,0,0);flick.cancel()
        end
        local function arm() flick.begin(pistol,side,11) end
        local function update(held,drop,hand,dt)
            flick.update(drop or 11,hand or side,held~=false,dt or .02,body,speedLimit)
        end
        local function stroke(n,dx)
            for i=1,n do sample(dx,0,0);update() end
        end
        for hand=0,1 do
            setup(hand);yaw=math.pi/2;sample(0,0,0);arm()
            local sign=hand==1 and 1 or -1
            local before=flick.flicks;stroke(3,.020*sign);assert(flick.flicks==before)
            stroke(1,.020*sign);assert(flick.flicks==before+1 and flick.lastDelta>1)
            assert(math.abs(body[1])<1e-6 and sign*body[2]>1 and body[3]==-1)
            local vy=body[2];stroke(10,.03*sign);assert(body[2]==vy and flick.flicks==before+1)
        end
        -- No new edge, still, rotation only, ordinary forward/slow motion, body lean,
        -- wrong direction, single jump, repeated XR publication, and reversal.
        local function negative(fn)
            setup();local before=flick.flicks;arm();fn()
            assert(flick.flicks==before and body[1]==0 and body[2]==0)
        end
        negative(function() flick.cancel();stroke(7,.03) end)
        negative(function() for i=1,8 do sample(0,0,0);frame[4]=point(1,0,0);update() end end)
        negative(function() stroke(10,.008) end)
        negative(function() for i=1,5 do sample(0,0,-.03);update() end end)
        negative(function() for i=1,6 do sample(.022,0,0,20,i*.022);update() end end)
        negative(function() stroke(7,-.022) end)
        negative(function() stroke(1,.18);stroke(5,.03) end)
        negative(function() sample(.035,0,0);update();for i=1,6 do update() end end)
        negative(function() stroke(2,.028);stroke(1,-.02);stroke(2,.028) end)
        for _,cancel in ipairs({
            function() update(false) end, function() update(true,12) end,
            function() update(true,11,0) end, function() update(true,11,1,.11) end,
            function() origin=18;sample(.02,0,0);update() end,
            function() sample(.02,0,0,300);update() end,
            function() frame=nil;update() end,
            function() enabled=false;update();enabled=true end,
            function() paused=true;update();paused=false end,
            function() trigger=.9;update();trigger=0 end,
            function() frame[2]=point(0/0,0,0);update() end,
            function() stroke(12,0) end
        }) do
            negative(function() stroke(1,.025);cancel();stroke(6,.025) end)
        end
        setup();speedLimit=1.5;arm();stroke(4,.025)
        assert(math.sqrt(body[1]^2+body[3]^2)<=speedLimit+1e-7 and flick.lastDelta>0)
        setup();local before=flick.flicks
        flick.begin({GetItemID=function() return 2 end},side,11);stroke(5,.023);assert(flick.flicks==before)
        setup();Game.VRMeleeThrowFrame=nil;arm();stroke(5,.023);assert(flick.flicks==before)
    ''')


if __name__ == '__main__':
    test_flick()
    print('PASS pistol magazine gesture, physical hands, duplicate samples and ownership changes')
