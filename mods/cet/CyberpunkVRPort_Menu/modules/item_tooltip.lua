-- Original game tooltip widgets, shared by exchange panels on the world-space canvas.
--
-- A GAME TOOLTIP THAT DOES NOT SHOW IS NEVER A BLACK PLATE (issue #188: "just a black space where the item
-- information should be", at a ripperdoc). The game's tooltip is spawned invisible and shown once it has laid out
-- (T.fit); a spawn or a SetData that throws (a UI mod replacing the tooltip's .inkwidget, a controller that is not
-- the one the data was made for) or a tooltip that never lays out left the panel's black plate with nothing on it.
-- Either way the menu's own card -- the item's name, quality, price and description -- stands in (T.card), and the
-- reason goes to the CET log once per item (T.report).
local K=require("modules/ink")
local T={}
-- how long a game tooltip may take to lay out before the card stands in for it (seconds)
local LAYOUT_WAIT=.75
local reported={}

local function now() return os.clock() end

local function itemName(ui)
  local name="?"
  pcall(function() name=ui:GetName() end)
  return tostring(name)
end

function T.report(ui,why)
  local key=itemName(ui).."|"..tostring(why)
  if reported[key] then return end
  reported[key]=true
  print("[VRMenu] the game tooltip for "..itemName(ui).." did not show ("..tostring(why).."); the menu's card stands in")
end

-- characters, not bytes (UTF-8 continuation bytes are not counted)
local function ulen(s) return select(2,s:gsub("[^\128-\191]","")) end

local function wrap(text,chars)
  local lines={}
  for para in (tostring(text or ""):gsub("\r","").."\n"):gmatch("(.-)\n") do
    local line=""
    for word in para:gmatch("%S+") do
      if line=="" then line=word
      elseif ulen(line)+1+ulen(word)<=chars then line=line.." "..word
      else lines[#lines+1]=line;line=word end
    end
    lines[#lines+1]=line
  end
  while #lines>0 and lines[#lines]=="" do lines[#lines]=nil end
  return lines
end

-- The menu's own card for tip t (its ui, parent, box and price): what the game tooltip would have led with.
function T.card(t)
  if t.fell then return end
  t.fell=true
  if t.widget then pcall(function() t.widget:SetVisible(false) end) end
  local ui=t.ui
  local name,quality,desc=itemName(ui),"",""
  pcall(function() quality=ui:GetQualityText() end)
  pcall(function() desc=ui:GetDescription() end)
  local x,y,w=t.x,t.y,t.w
  local lines=wrap(desc,math.max(12,math.floor((w-40)/(24*.62))))
  local h=20+56+(quality~="" and 40 or 0)+(t.price and 40 or 0)+#lines*32+20
  h=math.min(h,t.h)
  local host=K.canvas(t.parent,0,0,10,10)
  t.card=host
  K.roundFill(host,x,y,w,h,K.BLACK,1,8)
  local cy=y+20
  K.text(host,x+20,cy,w-40,46,name,34,K.YELLOW);cy=cy+56
  if quality~="" then K.text(host,x+20,cy,w-40,34,quality,24,K.GREY);cy=cy+40 end
  if t.price then K.text(host,x+20,cy,w-40,34,"€$ "..tostring(math.floor(t.price+.5)),26,K.CYAN);cy=cy+40 end
  for _,l in ipairs(lines) do
    if cy+32>y+h then break end
    K.text(host,x+20,cy,w-40,32,l,24,K.TEXT);cy=cy+32
  end
end

-- A tip that stands for the game tooltip of `ui` in box x,y,w,h on `parent`, its card shown at once (`why` logged).
function T.fallback(ui,parent,x,y,w,h,price,why)
  local t={ui=ui,parent=parent,x=x,y=y,w=w,h=h,price=price,born=now()}
  T.report(ui,why)
  T.card(t)
  return t
end

-- Each frame for a tip that has not laid out yet: past LAYOUT_WAIT the card stands in. True once it has.
function T.watch(t)
  if t.fell then return true end
  if t.laid or now()-(t.born or now())<LAYOUT_WAIT then return false end
  T.report(t.ui,"no layout in "..LAYOUT_WAIT.." s")
  T.card(t)
  return true
end

local function spawnGame(kit,p,ui,context,x,y,w,h,compare,price,backed)
  local data=UIInventoryItemTooltipWrapper.Make(ui,ItemDisplayContextData.Make(Game.GetPlayer(),context,false))
  if compare then data.comparisonData=UIInventoryItemComparisonManager.Make(ui,compare) end
  if price then data.overridePrice=price end
  local path,library=[[base\gameplay\gui\common\tooltip\itemtooltip.inkwidget]],"itemTooltip"
  if ui:IsWeapon() then path,library=[[base\gameplay\gui\common\tooltip\new_itemtooltip.inkwidget]],"itemTooltip"
  elseif ui:GetItemData():HasTag(CName.new("Cyberdeck")) then path,library=[[base\gameplay\gui\common\tooltip\cyberdecktooltip.inkwidget]],"cyberdeckTooltip"
  elseif ui:GetItemType()==gamedataItemType.Prt_Program then path,library=[[base\gameplay\gui\common\tooltip\programtooltip.inkwidget]],"programTooltip" end
  local back=backed and K.canvas(p,0,0,10,10) or nil
  local widget=kit.menuController():SpawnFromExternal(p,ResRef.FromString(path),CName.new(library))
  if not widget then return nil,"no widget from "..library end
  local t={widget=widget,x=x,y=y,w=w,h=h,back=back,ui=ui,parent=p,price=price,born=now()}
  widget:SetAnchor(inkEAnchor.TopLeft);widget:SetAnchorPoint(Vector2.new({X=0,Y=0}));widget:SetRenderTransformPivot(Vector2.new({X=0,Y=0}))
  widget:SetScale(Vector2.new({X=.001,Y=.001}));widget:SetOpacity(.001)
  K.move(widget,x,y)
  local controller=widget:GetController()
  if not controller then return nil,"no controller on "..library end
  controller:SetData(data)
  return t
end

-- `backed`: a black plate behind the tooltip, sized to it once it has laid out (T.fit) -- for a tooltip over other
-- content; a plate drawn at the full box before that stood empty around a smaller tooltip.
function T.spawn(kit,p,ui,context,x,y,w,h,compare,price,backed)
  local ok,t,why=pcall(spawnGame,kit,p,ui,context,x,y,w,h,compare,price,backed)
  if ok and t then return t end
  return T.fallback(ui,p,x,y,w,h,price,ok and why or t)
end
-- The weapon tooltip widget given ready tooltip data (a MinimalItemTooltipData snapshot), as the upgrade screen
-- feeds it: what it shows is read when the data is made, not when the widget draws.
function T.spawnData(kit,p,data,x,y,w,h)
  local ok,t=pcall(function()
    local widget=kit.menuController():SpawnFromExternal(p,ResRef.FromString([[base\gameplay\gui\common\tooltip\new_itemtooltip.inkwidget]]),CName.new("itemTooltip"))
    if not widget then return nil end
    widget:SetAnchor(inkEAnchor.TopLeft);widget:SetAnchorPoint(Vector2.new({X=0,Y=0}));widget:SetRenderTransformPivot(Vector2.new({X=0,Y=0}))
    widget:SetScale(Vector2.new({X=.001,Y=.001}));widget:SetOpacity(.001)
    K.move(widget,x,y);widget:GetController():SetData(data)
    return {widget=widget,x=x,y=y,w=w,h=h,born=now(),laid=false,noCard=true}
  end)
  if not ok then print("[VRMenu] the upgrade tooltip did not spawn: "..tostring(t));return nil end
  return t
end
function T.fit(tips)
  for _,t in ipairs(tips or {}) do
    if not t.fell then
      local ok,z=pcall(function() return t.widget:GetDesiredSize() end)
      if ok and z.X>1 and z.Y>1 then
        t.laid=true
        local scale=math.min(t.w/z.X,t.h/(z.Y+65));t.widget:SetScale(Vector2.new({X=scale,Y=scale}));K.move(t.widget,t.x,t.y+35*scale);t.widget:SetOpacity(1)
        local bw,bh=math.floor(z.X*scale+.5),math.floor(z.Y*scale+.5)
        if t.back and (bw~=t.backW or bh~=t.backH) then
          t.backW,t.backH=bw,bh;t.back:RemoveAllChildren()
          K.roundFill(t.back,t.x-10,t.y+35*scale-10,bw+20,bh+20,K.BLACK,1,8)
        end
      elseif not t.noCard and t.ui then
        T.watch(t)
      end
    end
  end
end
return T
