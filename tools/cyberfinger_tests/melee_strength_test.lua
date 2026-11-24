-- Exercise the actual motion/contact detectors, not only the modifier helper.
package.path=assert(arg[1])..'/?.lua;'..package.path
local function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
Vector4={new=v}
local cases=0
for _,hz in ipairs({30,90}) do
  for _,profile in ipairs({'missing','inactive','cyberfinger'}) do
    for side=0,1 do
      for kind=0,3 do -- ordinary fists/weapons, Gorilla Arms, blades, wire
        for _,triggers in ipairs({{0,0},{1,0},{0,1},{1,1},{.5,.5},{0,.51}}) do
          local stamp,sequence,x,starts,sounds,contacts=0,0,0,{},0,0
          local left,right=triggers[1],triggers[2]
          VRCyberFingerEnabled=profile~='missing' and function() return profile=='cyberfinger' end or nil
          Game={GetVRSharedSlot=function(slot)
              assert(slot==154 or slot==160,'unexpected input slot')
              return slot==154 and left or right
            end,
            VRWeaponInLeftHand=function() return side==0 end,
            GetSystemRequestsHandler=function() return {IsGamePaused=function() return false end} end,
            VRMeleeInputFrame=function()
              return {v(stamp,sequence,0),v(0,0,1),v(x,0,1),v(0,0,0,1),v(0,0,0,1)}
            end,
            VRCyberwareFrame=function()
              local l,r=side==0 and x or 0,side==1 and x or 0
              return {v(kind,2,2,sequence),v(0,0,0,stamp),v(l,0,1),v(r,0,1),v(0,0,0,1),v(0,0,0,1),
                v(l,0,1),v(l,.08,1),v(r,0,1),v(r,.08,1),v(l,0,1),v(r,0,1),v(0,0,0,1),v(0,0,0,1)}
            end}
          local player={VRCyberwareBeginSwing=function(_,weapon,hand,strong)
              starts[#starts+1]={side=hand,strong=strong};return true
            end,
            VRCyberwareUpdateSwing=function() end,
            VRMeleeWhoosh=function() sounds=sounds+1 end,
            VRMeleeSweepHit=function(_,weapon,p,t,op,ot,radius,passive)
              if not passive then contacts=contacts+1 end
            end,
            VRCyberwareContacts=function(_,weapon,hand,p,op,radius,passive)
              if not passive then contacts=contacts+1 end
            end}
          WeaponObject={IsOfType=function(id,kind)return id==kind end}
          gamedataItemType={Wea_Katana="katana"}
          local weapon={GetItemID=function()return "club" end,VRMeleeReach=function() return 1 end}
          package.loaded.ordinary_melee=nil;package.loaded.cyberware=nil
          local module=require('cyberware')
          local function tick(speed)
            sequence=sequence+1;stamp=math.floor(sequence*1000/hz+.5);x=x+speed/hz
            if kind==0 then module.ordinary(1/hz,player,weapon,v(x,0,1),v(0,1,0,0),v(0,0,0))
            else module.update(1/hz,player,weapon,kind,0) end
          end
          -- Holding either/both triggers and toggling them without motion must
          -- not create attacks, active contacts or swing sounds.
          for _=1,hz do tick(0) end
          left,right=0,0;tick(0);left,right=triggers[1],triggers[2]
          for _=1,hz do tick(0) end
          assert(#starts==0 and contacts==0 and sounds==0,'trigger alone attacked')
          for _=1,math.floor(.15*hz+.5) do tick(6) end
          local expected=triggers[side+1]>.5
          if profile=='cyberfinger' then expected=left>.5 or right>.5 end
          local label=string.format('%s kind=%d side=%d triggers=%.2f/%.2f %dHz',profile,kind,side,left,right,hz)
          assert(#starts==1 and starts[1].strong==expected,'wrong physical attack strength: '..label)
          assert(starts[1].side==(kind==0 and 1 or side),'wrong attacking hand: '..label)
          assert(contacts>0 and sounds==1,'physical stroke lost contact/sound: '..label)
          for _=1,hz do tick(0) end
          local settled=contacts
          for _=1,hz do tick(0) end
          assert(#starts==1 and sounds==1 and contacts==settled,'held modifier repeats damage: '..label)
          cases=cases+1
        end
      end
    end
  end
end
print(cases..' physical melee replays passed: either-trigger strength for CyberFinger only; motion still required')
