local K=require("modules/ink")
local L=require("modules/locale")
local A=require("modules/appearance")
local V={}
-- The game's mirror prompt icon (ChoiceIcons.MirrorIcon): a person before a mirror. Opaque box measured off
-- mappin_icons.xbm, alpha > 40.
local MIRROR={K.ATLAS.mappin,"mirror",64,64,2,5,61,59}
-- `x` is the panel's left edge on the menu canvas. The real mirror is the preview: the menu opens centred on it,
-- so the panel stands to its right.
function V.draw(kit,p,x)
  local s=A.current();if not s or not s.rows then return end
  local y=kit.P1Y
  kit.glass(p,820,1140,x,y)
  K.text(p,24,8,620,70,L.tr("ВНЕШНОСТЬ","APPEARANCE"),40,K.CYAN)
  -- The game's own character editor, for what the panel does not offer.
  local NX,NY,NS=676,10,130
  local nbg=K.roundFill(p,NX,NY,NS,NS,K.ICE,.12,12)
  local icon=kit.glyph(p,NX+NS/2,NY+NS/2,NS*1.25,MIRROR,K.CYAN,.85)
  kit.button("p1",kit.at(x,y),NX,NY,NS,NS,function()
    if A.native() then kit.close() end
  end,function(h) nbg:SetOpacity(h and .35 or .12);icon:SetOpacity(h and 1.0 or .85) end).key="appearanceNative"
  local rows={}
  for _,entry in ipairs(s.rows) do
    local _,value=A.value(entry)
    rows[#rows+1]={h=116,build=function(c,it,v)
      K.text(c,18,0,v.cw-36,44,K.clip(entry.label,38),30,K.TEXT)
      K.text(c,80,48,v.cw-160,48,K.clip(value,31),28,K.CYAN,"center")
      for _,direction in ipairs({-1,1}) do
        local bx=direction<0 and 12 or v.cw-72
        local bg=K.roundFill(c,bx,48,60,48,K.ICE,.12,6)
        K.text(c,bx,48,60,48,direction<0 and "<" or ">",32,K.CYAN,"center")
        kit.button("p1",kit.at(v.x,it.y,v),bx,48,60,48,function()
          A.change(entry,direction)
          kit.rebuild(false,false)
        end,function(h) bg:SetOpacity(h and .35 or .12) end).key="appearance:"..entry.key..":"..direction
      end
    end}
  end
  kit.list(p,"p1","appearance",x,y,150,792,rows,830,kit.S.keepScroll)
  K.text(p,24,980,772,40,L.tr("Изменения видны в зеркале","Changes are shown in the mirror"),24,K.TEXT)
  local function button(bx,label,accept,key)
    kit.action(p,"p1",kit.at(x,y),bx,1030,370,76,label,true,function()
      if A.action(accept) then kit.close() end
    end,K.CYAN).key=key
  end
  button(25,L.tr("ОТМЕНА","CANCEL"),false,"appearanceCancel")
  button(425,L.tr("ПРИНЯТЬ","ACCEPT"),true,"appearanceAccept")
end
return V
