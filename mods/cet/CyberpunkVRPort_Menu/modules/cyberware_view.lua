local K=require("modules/ink")
local L=require("modules/locale")
local D=require("modules/cyberware_data")
local I=require("modules/inventory_data")
local Images=require("modules/gameplay_data")
local Anatomy=require("modules/cyberware_anatomy")
local Tooltip=require("modules/item_tooltip")
return function(C,U)
local V={};local W=1780
local colors={Common=K.GREY,Uncommon={.2,.85,.5},Rare={.2,.6,1},Epic={.75,.3,1},Legendary=K.YELLOW}
local function tr(kit,ru,en) return L.tr(ru,en) end
local function state(kit)
  local g=C.state(kit);g.cyber=g.cyber or {slot=0};local s=g.cyber
  s.ctx=s.ctx or D.open();return s
end
local function redraw(kit) C.state(kit).resetList=true;C.redraw(kit) end
local function action(kit,p,x,y,w,ru,en,fn,enabled,key)
  local b=U.action(kit,p,kit.P1X,x,y,w,tr(kit,ru,en),fn,enabled);b.key=key;return b
end
local function hover(kit,key,area,on)
  local s=state(kit)
  if on then s.hover=key;s.hoverArea=area;s.hoverAt=kit.S.time
  elseif s.hover==key then s.hover=nil;s.hoverArea=nil;s.hoverAt=kit.S.time end
end
local function card(kit,p,H,x,y,size,item,area,slot,click,key,unavailable)
  local s=state(kit);local col=item and colors[item.quality] or K.CYAN;col=col or K.CYAN
  if unavailable then col=K.RED end
  local bg=K.roundFill(p,x,y,size,size,col,unavailable and .23 or .045,4)
  local edge=K.cutFrame(p,x,y,size,size,unavailable and K.RED or K.CYAN,unavailable and .75 or .35,9,1)
  if item then
    U.picture(kit,p,x+9,y+12,size-18,size-25,item.image)
    K.rect(p,x+2,y+5,5,size-10,col,1)
    if item.upgrade and item.upgrade.allowed then
      local a=K.canvas(p,x+size-32,y+7,26,26)
      K.roundEdge(a,0,0,26,26,K.YELLOW,1,3)
      U.line(a,6,18,13,10,K.YELLOW,1,3);U.line(a,13,10,20,18,K.YELLOW,1,3)
      s.pulses[#s.pulses+1]=a
    end
    if item.new then K.image(p,x+12,y+7,9,9,K.ATLAS.phone,"circle",K.CYAN,1) end
    if item.equipped then U.line(p,x+size-28,y+size-18,x+size-20,y+size-10,K.CYAN,1,3);U.line(p,x+size-20,y+size-10,x+size-9,y+size-25,K.CYAN,1,3) end
  else K.text(p,x,y,size,size,"+",34,K.GREY,"center") end
  local b=kit.button("p1",H,x,y,size,size,click,function(on)
    bg:SetTintColor(K.color(unavailable and K.RED or on and K.YELLOW or col));bg:SetOpacity(unavailable and .23 or on and .14 or .045)
    K.tint(edge,unavailable and K.RED or on and K.YELLOW or K.CYAN,on and 1 or unavailable and .75 or .35)
    hover(kit,item and item.key or "area:"..area,area,on)
  end);b.key=key;return b
end
-- The game's tooltip for a cyberware item; a spawn or SetData that fails, or a tooltip that never lays out, shows
-- the menu's own card instead of the empty black plate (modules/item_tooltip.lua, issue #188).
local function spawnTooltip(kit,parent,item,x,y,w,h,upgrade,base)
  local context=ItemDisplayContextData.Make(Game.GetPlayer(),item.vendor and ItemDisplayContext.Vendor or ItemDisplayContext.GearPanel,false)
  if D.session(kit) then context:AddTag(CName.new("Ripperdoc")) end
  if upgrade then context:AddTag(CName.new("CyberwareUpgrade")) end
  local data=UIInventoryItemTooltipWrapper.Make(item.ui,context)
  if base then data.comparisonData=UIInventoryItemComparisonManager.Make(base.ui,item.ui) end
  local isDeck=item.raw:HasTag(CName.new("Cyberdeck"))
  local path=isDeck and [[base\gameplay\gui\common\tooltip\cyberdecktooltip.inkwidget]] or [[base\gameplay\gui\common\tooltip\itemtooltip.inkwidget]]
  local library=isDeck and "cyberdeckTooltip" or "itemTooltip"
  local widget,why=Tooltip.widget(kit,parent,path,library)
  if not widget then return nil,why or ("no widget from "..library) end
  widget:SetAnchor(inkEAnchor.TopLeft);widget:SetAnchorPoint(Vector2.new({X=0,Y=0}))
  widget:SetRenderTransformPivot(Vector2.new({X=0,Y=0}));widget:SetScale(Vector2.new({X=.001,Y=.001}))
  widget:SetMargin(inkMargin.new({left=x,top=y,right=0,bottom=0}));widget:SetOpacity(.001)
  local controller=widget:GetController()
  if not controller then return nil,"no controller on "..library end
  controller:SetData(data)
  return {widget=widget,x=x,y=y,w=w,h=h,ui=item.ui,parent=parent,price=item.price,born=os.clock()}
end
local function tooltip(kit,parent,item,x,y,w,h,upgrade,base)
  if not item or not item.ui then return end
  local s=state(kit)
  local tip=Tooltip.create(function() return spawnTooltip(kit,parent,item,x,y,w,h,upgrade,base) end,
    item.ui,parent,x,y,w,h,item.price)
  s.tips[#s.tips+1]=tip
end
local function bar(kit,p,x,kind,data)
  local capacity=kind=="capacity";local col=capacity and K.YELLOW or K.CYAN
  local max=capacity and math.max(data.max,data.used) or math.max(data.armorMax,data.armor)
  local value=capacity and data.used or data.armor
  local baseline=capacity and data.base or data.armorMax
  local sy,sh=220,750
  K.text(p,x-55,120,170,70,capacity and tr(kit,"ЛИМИТ","CAPACITY") or tr(kit,"БРОНЯ","ARMOR"),25,col,"center")
  for i=0,49 do
    local v=(i+1)/50*max;local active=v<=value
    K.rect(p,x,sy+sh-(i+1)*15,55,7,capacity and v>baseline and K.RED or col,active and .9 or .13)
  end
  local y=sy+sh-value/math.max(max,1)*sh
  K.rect(p,x-10,y,83,3,col,1)
  K.text(p,x-55,sy+sh+16,170,66,math.floor(value+.5).." / "..math.floor((capacity and data.max or data.armorMax)+.5),28,col,"center")
  if capacity and data.extra>0 then local by=sy+sh-data.base/math.max(max,1)*sh;K.rect(p,x-20,by,95,3,K.RED,1) end
  local b=kit.button("p1",kit.at(kit.P1X,kit.P1Y),x-35,120,125,940,function()
    local g=C.state(kit);g.skills=false;g.attribute=tostring(gamedataStatType.TechnicalAbility);g.perk=nil;kit.openSection("character")
  end,function(on) hover(kit,"bar:"..kind,nil,on) end);b.key="cyberBar:"..kind
end
local function barTip(kit,p,kind)
  local s=state(kit);local d=D.stats();local rows={};local col=kind=="capacity" and K.YELLOW or K.CYAN
  local title=kind=="capacity" and tr(kit,"ЛИМИТ ИМПЛАНТОВ","CYBERWARE CAPACITY") or tr(kit,"БРОНЯ","ARMOR")
  K.roundFill(p,0,0,660,830,K.BLACK,1,12);K.roundEdge(p,0,0,660,830,col,.9,12)
  K.text(p,25,20,610,60,title,36,col)
  local text
  if kind=="capacity" then
    text=tr(kit,"Установлено: ","Installed: ")..math.floor(d.used+.5).." / "..math.floor(d.max+.5).."\n\n"..
      tr(kit,"Базовый лимит: ","Base capacity: ")..math.floor(d.base+.5).."\n"..
      tr(kit,"Настоящий киберпанк: +","Edgerunner: +")..math.floor(d.extra+.5).."\n\n"..
      tr(kit,"Превышение базового лимита: ","Capacity above the base limit: ")..math.floor(d.over+.5).."\n"..
      tr(kit,"Уменьшение запаса здоровья: -","Health reduction: -")..math.floor(d.healthLoss+.5).."\n"..
      tr(kit,"Вероятность ярости: ","Fury chance: ")..string.format("%.1f%%",d.over*.1)
  else
    text=math.floor(d.armor+.5).." / "..math.floor(d.armorMax+.5).."\n\n"..
      tr(kit,"Броня определяет вашу устойчивость к урону. Её можно повысить имплантами категорий «Скелет» и «Кожа».","Armor determines your resistance to damage. Increase it with Skeleton and Integumentary System cyberware.").."\n\n"..
      tr(kit,"Снижение урона при максимальной броне для вашего уровня: ","Damage reduction at the armor cap for your level: ")..string.format("%.0f%%",d.reduction).."\n\n"..
      tr(kit,"Влияющая способность: Лицензия на хром, уровень 2.","Related perk: License to Chrome, rank 2.")
  end
  local lines=C.wrap(text,39)
  K.text(p,25,104,610,#lines*39,table.concat(lines,"\n"),30,K.TEXT)
  local perk=TweakDBInterface.GetNewPerkRecord(TweakDBID.new(kind=="capacity" and "NewPerks.Tech_Master_Perk_3" or "NewPerks.Tech_Central_Milestone_3"))
  local icon=Images.image(perk:PerkIcon():GetID())
  if icon then K.image(p,25,640,70,70,icon.atlas,icon.part,K.YELLOW,1) end
  K.text(p,110,640,520,70,kind=="capacity" and tr(kit,"НАСТОЯЩИЙ КИБЕРПАНК","EDGERUNNER") or tr(kit,"ЛИЦЕНЗИЯ НА ХРОМ — 2","LICENSE TO CHROME — 2"),27,K.YELLOW)
  K.text(p,25,735,610,70,tr(kit,"Нажмите на шкалу: дерево способностей","Click the meter to open the perk tree"),27,K.CYAN,nil,true)
end
function V.draw(kit,p,p2)
  local s=state(kit);s.pulses={};s.tips={};s.hover=nil;s.hoverShown=nil;s.poll=kit.S.time
  s.snapshot=D.snapshot(s.ctx,kit);s.stamp=D.signature()
  local data=s.snapshot
  kit.glass(p,W,1210,kit.P1X,kit.P1Y);U.title(p,tr(kit,"КИБЕРИМПЛАНТЫ","CYBERWARE"),W)
  local origin=kit.at(kit.P1X,kit.P1Y)
  if s.upgrade then
    local up=s.upgrade
    K.text(p,25,106,1550,50,tr(kit,"УЛУЧШЕНИЕ — ВЫБЕРИТЕ БОНУСЫ","UPGRADE — SELECT BONUSES"),34,K.YELLOW)
    local close=U.headAction(kit,p,kit.P1X,W-72,"",function() s.upgrade=nil;redraw(kit) end,true,72);close.key="cyberUpgradeClose"
    U.line(p,W-120,34,W-96,58,K.CYAN,1,3);U.line(p,W-96,34,W-120,58,K.CYAN,1,3)
    local options={{base=up.base}}
    for i=1,3 do local ui=up.data["option"..i.."InventoryItem"];if ui then options[#options+1]={item=D.item(s.ctx,ui:GetItemData(),false,ui),index=i} end end
    local width=(W-50)/#options
    for i,entry in ipairs(options) do
      local x=20+(i-1)*width
      if entry.base then tooltip(kit,p,entry.base,x,175,width-12,870,true)
      else
        tooltip(kit,p,entry.item,x,175,width-12,870,true,up.base)
        local edge=K.roundEdge(p,x,165,width-12,890,K.YELLOW,up.choice==entry.index and 1 or .15,8)
        local b=kit.button("p1",origin,x,165,width-12,890,function() up.choice=entry.index;C.redraw(kit) end,function(h) K.tint(edge,K.YELLOW,h and 1 or up.choice==entry.index and 1 or .15) end)
        b.key="cyberUpgradeOption"..entry.index
      end
    end
    local current=D.upgrade(up.base);local enough=current and current.allowed
    K.text(p,25,1080,730,90,tr(kit,"Компоненты: ","Components: ")..up.up.have.." / "..up.up.cost.materialCount,30,enough and K.CYAN or K.RED)
    action(kit,p,810,1100,930,"УЛУЧШИТЬ","UPGRADE",function()
      if D.applyUpgrade(kit,s.ctx,up) then s.upgrade=nil;s.refreshAt=kit.S.time+.6 else kit.toast(tr(kit,"Недостаточно ресурсов или изменились условия","Insufficient resources or requirements changed")) end
    end,enough and D.session(kit)~=nil,"cyberUpgradeConfirm")
    return
  end
  bar(kit,p,65,"capacity",data.stats);bar(kit,p,W-110,"armor",data.stats)
  local group
  for _,g in ipairs(data.groups) do if g.area==s.area then group=g end end
  if group then
    action(kit,p,180,105,270,"К ИМПЛАНТАМ","CYBERWARE",function() s.area=nil;s.selected=nil;redraw(kit) end,true,"cyberBack")
    K.text(p,475,107,600,60,group.name,32,K.CYAN)
    local function step(delta)
      local index=1;for i,g in ipairs(data.groups) do if g.area==s.area then index=i;break end end
      s.area=data.groups[(index-1+delta)%#data.groups+1].area;s.slot=0;s.selected=nil;redraw(kit)
    end
    action(kit,p,1110,105,90,"","",function() step(-1) end,true,"cyberPrevious")
    action(kit,p,1220,105,90,"","",function() step(1) end,true,"cyberNext")
    U.line(p,1164,124,1146,139,K.CYAN,1,3);U.line(p,1146,139,1164,154,K.CYAN,1,3)
    U.line(p,1256,124,1274,139,K.CYAN,1,3);U.line(p,1274,139,1256,154,K.CYAN,1,3)
    for i,entry in ipairs(group.slots) do card(kit,p,origin,185+(i-1)*151,195,138,entry.item,group.area,entry.index,function() s.slot=entry.index;s.selected=entry.item and entry.item.key;C.redraw(kit) end,"cyberSlot"..entry.index) end
    s.body=Anatomy.new(p,965,170,650,865);Anatomy.focus(s.body,group.area,true)
    local rows={};local columns=4;local size=170
    for first=1,#group.items,columns do
      local items={};for i=first,math.min(first+columns-1,#group.items) do items[#items+1]=group.items[i] end
      rows[#rows+1]={h=192,build=function(c,it,v)
        for i,item in ipairs(items) do
          local allowed,excess=D.check(kit,s.ctx,item,group.area,s.slot or 0)
          local unavailable=not allowed and not item.equipped
          card(kit,c,kit.at(v.x,it.y,v),(i-1)*184,6,size,item,group.area,s.slot,function() s.selected=item.key;I.inspect(s.ctx,item);C.redraw(kit) end,"cyberItem:"..item.key,unavailable)
          if unavailable then
            local x=(i-1)*184
            K.roundFill(c,x+5,130,size-10,41,K.BLACK,.85,5)
            K.image(c,x+9,133,34,34,[[base\gameplay\gui\common\icons\atlas_common.inkatlas]],"ico_lock",K.RED,1)
            K.text(c,x+44,133,size-50,36,excess>0 and ("+"..math.ceil(excess).." "..tr(kit,"ЛИМИТ","CAPACITY")) or tr(kit,"ТРЕБУЕТСЯ","REQUIRES"),21,K.RED,"center")
          end
        end
      end}
    end
    local host=K.canvas(p,166,0,778,1080)
    C.list(kit,host,"p1","p1",kit.P1X+166,370,778,rows,620,kit.S.keepScroll):enableMask()
    local selected=data.byKey[s.selected]
    if selected then
      local ok,excess=D.check(kit,s.ctx,selected,group.area,s.slot or 0)
      if excess>0 then K.text(p,180,1015,740,65,tr(kit,"Не хватает лимита: ","Capacity shortfall: ")..math.ceil(excess),29,K.RED) end
      if data.vendor then
        if selected.equipped then
          action(kit,p,190,1110,730,"УЛУЧШИТЬ","UPGRADE",function() s.upgrade=D.prepareUpgrade(kit,s.ctx,selected);if s.upgrade then C.redraw(kit) end end,selected.upgrade~=nil,"cyberUpgrade")
        else
          local ru=selected.vendor and ("КУПИТЬ И УСТАНОВИТЬ  €$ "..selected.price) or "УСТАНОВИТЬ"
          local en=selected.vendor and (L.t("BUY AND INSTALL").."  €$ "..selected.price) or "INSTALL"
          action(kit,p,190,1110,730,ru,en,function() if D.equip(kit,s.ctx,selected,group.area,s.slot or 0) then s.refreshAt=kit.S.time+.6 else kit.toast(kit.T.blocked) end end,ok,"cyberEquip")
        end
      end
    end
  else
    s.body=Anatomy.new(p,620,115,530,1000)
    for i,g in ipairs(data.groups) do
      local left=i%2==1;local x=left and 180 or 1165;local y=155+math.floor((i-1)/2)*188
      local title=K.text(p,x,y-40,430,40,K.clip(g.name,30),26,K.RED,left and "right" or "left")
      local bx=left and x+430-#g.slots*138 or x
      for j,entry in ipairs(g.slots) do card(kit,p,origin,bx+(j-1)*138,y,128,entry.item,g.area,entry.index,function() s.area=g.area;s.slot=entry.index;s.selected=nil;redraw(kit) end,"cyberArea:"..g.area..":"..entry.index) end
    end
  end
  if not data.vendor then K.text(p,180,1110,1430,70,tr(kit,"Посетите рипера, чтобы установить или улучшить импланты.","Visit a ripperdoc to install or upgrade cyberware."),29,K.CYAN,"center") end
  s.tipHost=K.canvas(p,0,0,W,1210)
  s.tipBlock=kit.button("p2",origin,980,165,690,925,nil,nil);s.tipBlock.blocker=true;s.tipBlock.visible=false
  s.tipCloseVisual=K.canvas(p,1582,120,66,54);s.tipCloseVisual:SetVisible(false)
  K.roundFill(s.tipCloseVisual,0,0,66,54,K.BLACK,1,6)
  U.line(s.tipCloseVisual,21,15,45,39,K.RED,1,3);U.line(s.tipCloseVisual,45,15,21,39,K.RED,1,3)
  s.tipClose=kit.button("p2",origin,1582,120,66,54,function()
    s.selected=nil;s.hover=nil;s.hoverShown=nil;s.tipHost:RemoveAllChildren();s.tips={};s.tipBlock.visible=false;s.tipClose.visible=false;s.tipCloseVisual:SetVisible(false)
  end,nil);s.tipClose.key="cyberTooltipClose";s.tipClose.visible=false
end
function V.update(kit)
  local s=state(kit);local now=kit.S.time;local dt=math.min(.1,now-(s.lastFrame or now));s.lastFrame=now
  for _,p in ipairs(s.pulses or {}) do p:SetOpacity(.45+.55*(.5+.5*math.sin(now*5))) end
  if s.body and not s.upgrade then Anatomy.update(s.body,dt) end
  Tooltip.fit(s.tips)
  if s.refreshAt and now>=s.refreshAt then s.refreshAt=nil;I.invalidate(s.ctx);C.redraw(kit);return end
  if D.updatePurchase(kit,s.ctx) then s.refreshAt=now+.4 end
  if not s.upgrade and s.tipHost then
    local key=s.hover or s.selected
    if key~=s.hoverShown and now-(s.hoverAt or 0)>.12 then
      s.tipHost:RemoveAllChildren();s.tips={};s.hoverShown=key
      s.tipBlock.visible=false;s.tipClose.visible=false;s.tipCloseVisual:SetVisible(false)
      local item=key and s.snapshot.byKey[key]
      if item then
        local x=s.area and 980 or 1020
        -- Overview cards alternate left/right. Put the tooltip opposite its source, like the native
        -- ripperdoc: its blocker on a right-hand card caused hover-out, hide, hover-in, repeat.
        if not s.area then
          for i,group in ipairs(s.snapshot.groups) do
            if group.area==s.hoverArea or gamedataEquipmentArea[group.area]==item.area then
              if i%2==0 then x=180 end
              break
            end
          end
        end
        K.roundFill(s.tipHost,x,165,650,925,K.BLACK,1,10)
        tooltip(kit,s.tipHost,item,x+10,180,630,890)
        s.tipBlock.x=kit.P1X+x;s.tipBlock.y=kit.P1Y+165;s.tipBlock.w=650;s.tipBlock.h=925;s.tipBlock.visible=true
        K.move(s.tipCloseVisual,x+562,120);s.tipClose.x=kit.P1X+x+562
        s.tipClose.visible=true;s.tipCloseVisual:SetVisible(true)
      elseif key and key:find("^bar:") then
        local pane=K.canvas(s.tipHost,520,230,660,830);barTip(kit,pane,key:sub(5))
        s.tipBlock.x=kit.P1X+520;s.tipBlock.y=kit.P1Y+230;s.tipBlock.w=660;s.tipBlock.h=830;s.tipBlock.visible=true
      end
      if s.body then Anatomy.focus(s.body,s.area or s.hoverArea,s.area~=nil) end
    end
  end
  if now-(s.poll or 0)>1 then
    s.poll=now
    if D.signature()~=s.stamp or (D.session(kit)~=nil)~=s.snapshot.vendor then I.invalidate(s.ctx);C.redraw(kit) end
  end
end
function V.beforeBuild(kit)
  local s=C.state(kit).cyber;if s then s.body=nil;s.tipHost=nil;s.tips={};s.pulses={} end
end
function V.leave(kit)
  local g=C.state(kit);local s=g.cyber
  if s then D.close(s.ctx);g.cyber=nil end
end
return V
end
