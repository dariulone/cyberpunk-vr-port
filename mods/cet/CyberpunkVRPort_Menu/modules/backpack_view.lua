-- The backpack page of the inventory (inventory_view draws it while s.backpack is set): the crafting components and
-- "disassemble junk" on the left, the game's filters, sorting and every backpack item on the right, the game's own
-- tooltip for the item pointed at, and the hold actions of the fullscreen backpack as buttons -- favourite, drop,
-- disassemble. SELECTION mode (the button beside the sort order) marks any number of items and drops or disassembles
-- them at once, whole stacks, each batch confirmed by a second press (2026-11-04: "select several items to
-- disassemble or drop a batch at once").
local K=require("modules/ink")
local L=require("modules/locale")
local B=require("modules/backpack_data")
local I=require("modules/inventory_data")
local Images=require("modules/gameplay_data")
local Tooltip=require("modules/item_tooltip")
local Sort=require("modules/item_sort")
local Jobs=require("modules/jobs")
return function(C,U)
local V={}
local W,H=1780,1305
local GRID_X,GRID_W,GRID_Y,GRID_H=350,1406,230,800
local CARD_W,CARD_H,ROW_H=454,178,190
local colors={Common=K.GREY,Uncommon={.2,.85,.5},Rare={.25,.6,1},Epic={.75,.3,1},Legendary={1,.62,.16}}
local function tr(ru,en) return L.tr(ru,en) end
-- the game's own word for it where it has one: it is in the game's language already
local function loc(key,ru,en) local t=I.loc(key);if t~="" and t~=key then return t end;return tr(ru,en) end
local function state(kit,s)
  s.bp=s.bp or {filter="AllItems",sort="QualityAsc",quantity=1,tips={},marked={}}
  s.bp.marked=s.bp.marked or {}
  return s.bp
end
local function icon(p,x,y,w,h,id,col,opacity)
  local image=Images.image(TweakDBID.new(id))
  if image then return K.image(p,x,y,w,h,image.atlas,image.part,col or K.CYAN,opacity or 1) end
end
-- A five-pointed star outline: the favourite mark (the menu's fonts have no star glyph).
function V.star(p,cx,cy,r,color,alpha,width)
  local pts={}
  for i=0,9 do
    local a=math.rad(-90+i*36);local rr=i%2==0 and r or r*.45
    pts[#pts+1]={cx+rr*math.cos(a),cy+rr*math.sin(a)}
  end
  local out={}
  for i=1,10 do local a,b=pts[i],pts[i%10+1];out[#out+1]=U.line(p,a[1],a[2],b[1],b[2],color,alpha or 1,width or 2) end
  return out
end
local star=V.star
local function card(kit,s,c,it,view,v,x)
  local col=colors[v.quality] or K.CYAN
  local marked=s.multi and s.marked[v.key]
  local bg=K.roundFill(c,x,4,CARD_W,CARD_H,col,marked and .13 or .045,4)
  local edge=K.cutFrame(c,x,4,CARD_W,CARD_H,(marked or s.selected==v.key) and K.YELLOW or K.CYAN,marked and 1 or .35,10,marked and 3 or 1)
  if s.multi then
    -- the selection box, ticked when marked
    K.cutFrame(c,x+CARD_W-52,44,34,34,marked and K.YELLOW or K.CYAN,marked and 1 or .6,5,2)
    if marked then U.line(c,x+CARD_W-45,61,x+CARD_W-38,69,K.YELLOW,1,4);U.line(c,x+CARD_W-38,69,x+CARD_W-25,51,K.YELLOW,1,4) end
  end
  K.rect(c,x+2,8,5,CARD_H-8,col,1)
  U.picture(kit,c,x+14,27,CARD_W-28,92,v.image)
  K.text(c,x+12,119,CARD_W-24,28,K.clip(v.name,33),23,col)
  if v.favorite then star(c,x+24,22,10,K.YELLOW,1,2) end
  if v.new then K.image(c,x+CARD_W-20,13,9,9,K.ATLAS.phone,"circle",K.CYAN,1) end
  if v.equipped then U.line(c,x+CARD_W-50,15,x+CARD_W-43,22,K.CYAN,1,3);U.line(c,x+CARD_W-43,22,x+CARD_W-31,8,K.CYAN,1,3) end
  if v.comparison==ItemComparisonState.Better or v.comparison==ItemComparisonState.Worse then
    local up=v.comparison==ItemComparisonState.Better;local sign=up and -1 or 1;local color=up and {.2,.95,.5} or K.RED
    U.line(c,x+CARD_W-29,40-sign*5,x+CARD_W-22,40+sign*3,color,1,3);U.line(c,x+CARD_W-22,40+sign*3,x+CARD_W-15,40-sign*5,color,1,3)
  end
  local tx=x+14
  if v.weapon then
    if not v.melee then
      K.rect(c,tx,155,3,15,K.CYAN,1);K.rect(c,tx+6,155,3,15,K.CYAN,1)
      K.text(c,tx+15,149,65,28,v.ammo>999 and "999+" or tostring(v.ammo),22,K.CYAN);tx=tx+78
    end
    if v.typeIcon then K.image(c,tx,150,25,25,v.typeIcon.atlas,v.typeIcon.part,v.iconic and K.YELLOW or K.CYAN,1) end
    local avail=x+CARD_W-12-(tx+28)
    K.text(c,tx+28,149,avail,28,K.clip(K.upper(v.typeName or ""),math.max(7,math.floor(avail/10))),20,v.iconic and K.YELLOW or K.GREY)
  else
    K.text(c,tx,149,320,28,v.junk and loc("UI-Filters-Junk","Мусор","Junk") or "",21,K.GREY)
    K.text(c,x+CARD_W-110,149,95,28,v.quantity>1 and ("x"..v.quantity) or "",22,K.CYAN,"right")
  end
  local b=kit.button("p1",kit.at(view.x,it.y,view),x,4,CARD_W,CARD_H,function()
    if s.multi then
      s.marked[v.key]=not s.marked[v.key] or nil;s.batchArmed=nil
      C.redraw(kit);return
    end
    s.selected=v.key;s.quantity=1;s.confirm=nil
    I.inspect({ui=Game.GetScriptableSystemsContainer():Get(CName.new("UIScriptableSystem"))},v);v.new=false
    C.redraw(kit)
  end,function(on)
    bg:SetTintColor(K.color(on and K.YELLOW or col));bg:SetOpacity((on or marked) and .13 or .045);K.tint(edge,(on or marked or s.selected==v.key) and K.YELLOW or K.CYAN,(on or marked) and 1 or .35)
    if on then s.hover=v.key;s.hoverAt=kit.S.time elseif s.hover==v.key then s.hover=nil;s.hoverAt=kit.S.time end
  end);b.key="backpackItem:"..v.key
end
local function materials(kit,p)
  local m=B.materials()
  local function grid(list,y)
    for i,mat in ipairs(list) do
      local x,yy=24+((i-1)%2)*158,y+math.floor((i-1)/2)*128
      K.roundFill(p,x,yy,148,118,K.NAVY,.18,4);K.cutFrame(p,x,yy,148,118,K.CYAN,.3,8,1)
      if mat.image then K.image(p,x+24,yy+8,100,70,mat.image.atlas,mat.image.part,K.WHITE,1) end
      K.text(p,x+8,yy+80,132,34,mat.quantity>999 and "999+" or tostring(mat.quantity),27,mat.quantity>0 and {.25,.95,.55} or K.GREY)
      local b=kit.button("p1",kit.at(kit.P1X,kit.P1Y),x,yy,148,118,nil,nil);b.key="backpackMaterial:"..i
    end
    return y+math.ceil(#list/2)*128
  end
  K.text(p,24,170,320,44,K.upper(loc("UI-ResourceExports-CraftingMaterials","Компоненты","Components")),27,K.RED)
  local y=grid(m.common,222)
  K.text(p,24,y+12,320,44,K.upper(loc("UI-ResourceExports-DeamonComponents","Компоненты скриптов","Script components")),27,K.RED)
  grid(m.hacking,y+64)
end
local function sortKey(bp) return bp.filter..":"..bp.sort end
function V.draw(kit,p,s,back)
  local bp=state(kit,s);bp.tips={};bp.hover=nil;bp.shown=nil
  kit.glass(p,W,H,kit.P1X,kit.P1Y)
  local snap=Jobs.get("backpack")
  if not snap then Jobs.run("backpack",B.snapshotJob,true);Jobs.finish("backpack",.03);snap=Jobs.get("backpack") end
  U.title(p,K.upper(loc("UI-PanelNames-BACKPACK","Рюкзак","Backpack"))..(snap and ("  /  "..#snap.list) or ""),W)
  local b=U.action(kit,p,kit.P1X,24,105,306,tr("К СНАРЯЖЕНИЮ","EQUIPMENT"),back,true,56,27);b.key="backpackBack"
  materials(kit,p)
  -- disassemble the junk: a second press within three seconds confirms
  local junk=B.junkCount(snap)
  local armed=bp.junkArmed and kit.S.time-bp.junkArmed<3
  K.text(p,24,1030,306,60,junk>0 and (tr("Мусор: ","Junk: ")..junk) or tr("Мусора нет","No junk"),25,K.GREY,"center")
  b=U.action(kit,p,kit.P1X,24,1100,306,armed and tr("ПОДТВЕРДИТЬ","CONFIRM") or K.upper(loc("UI-UserActions-DisassembleJunk","Разобрать мусор","Disassemble junk")),function()
    if not (bp.junkArmed and kit.S.time-bp.junkArmed<3) then bp.junkArmed=kit.S.time;C.redraw(kit);return end
    bp.junkArmed=nil
    local n=B.disassembleJunk(snap)
    kit.toast(tr("Разобрано предметов: ","Items disassembled: ")..n);bp.refreshAt=kit.S.time+.3;C.redraw(kit)
  end,junk>0,68,armed and 30 or 26);b.key="backpackJunk"
  if not snap then return end
  if not snap.categories[bp.filter] and bp.filter~="AllItems" then bp.filter="AllItems" end
  -- the filter buttons the backpack has items for, and the sort order
  local caption=K.text(p,GRID_X+10,178,600,40,"",23,K.GREY)
  local n=0
  for _,name in ipairs(B.FILTERS) do
    local f=ItemFilterCategory[name]
    if name=="AllItems" or snap.categories[name] then
      local x=GRID_X+n*73;n=n+1;local on=bp.filter==name
      local label=I.loc(NameToString(ItemFilterCategories.GetLabelKey(f)))
      if on then caption:SetText(label) end
      local bgf=K.roundFill(p,x,105,66,56,on and K.YELLOW or K.NAVY,on and .12 or .18,3)
      icon(p,x+9,110,48,46,ItemFilterCategories.GetIcon(f),on and K.YELLOW or K.CYAN)
      local fb=kit.button("p1",kit.at(kit.P1X,kit.P1Y),x,105,66,56,function()
        bp.filter=name;bp.selected=nil;bp.confirm=nil;C.state(kit).resetList=true;C.redraw(kit)
      end,function(h) bgf:SetTintColor(K.color((h or on) and K.YELLOW or K.NAVY));bgf:SetOpacity(h and .18 or on and .12 or .18);if h then caption:SetText(label) end end)
      fb.key="backpackFilter:"..name
    end
  end
  -- selection mode on/off; off forgets the marks
  b=U.action(kit,p,kit.P1X,GRID_X+GRID_W-300-12-200,105,200,bp.multi and tr("ГОТОВО","DONE") or tr("ВЫДЕЛЕНИЕ","SELECT"),function()
    bp.multi=not bp.multi;bp.marked={};bp.batchArmed=nil;bp.selected=nil;bp.confirm=nil;C.redraw(kit)
  end,true,56,24);b.key="backpackMulti"
  Sort.control(kit,p,bp,"backpackSort",GRID_X+GRID_W-300,112,300,bp.sort,function(value)
    bp.sort=value;bp.selected=nil;bp.confirm=nil;C.state(kit).resetList=true
  end,function() C.redraw(kit) end)
  -- the grid, once its sorted list is in (V.update sorts a slice a frame; no loading line meanwhile)
  local shown=bp.sorted and bp.sortedKey==sortKey(bp) and bp.sortedSeq==bp.snapSeq and bp.sorted
  if not shown then bp.sorting=bp.sorting or {key=sortKey(bp)} end
  bp.colOf={}
  if shown then
    local rows={}
    for first=1,#shown,3 do
      local group={};for i=first,math.min(#shown,first+2) do group[#group+1]=shown[i];bp.colOf[shown[i].key]=i-first+1 end
      rows[#rows+1]={h=ROW_H,build=function(c,it,view) for i,v in ipairs(group) do card(kit,bp,c,it,view,v,(i-1)*(CARD_W+12)) end end}
    end
    if #rows==0 then rows[1]={h=130,build=function(c) K.text(c,10,20,GRID_W-20,90,tr("Нет предметов в этой категории","No items in this category"),29,K.GREY,"center") end} end
    local host=K.canvas(p,GRID_X,0,GRID_W,1200)
    C.list(kit,host,"p1","p1",kit.P1X+GRID_X,GRID_Y,GRID_W,rows,GRID_H,kit.S.keepScroll):enableMask()
  end
  -- the marked items' actions (selection mode), or the selected item's
  local v=bp.selected and snap.byKey[bp.selected]
  if not v or bp.multi then bp.selected=nil;v=nil end
  if bp.multi then
    local list,count={},0
    for key in pairs(bp.marked) do
      local m=snap.byKey[key]
      if m then list[#list+1]=m;count=count+m.quantity else bp.marked[key]=nil end
    end
    local armedFor=bp.batchArmed and kit.S.time-bp.batchArmed.at<3 and bp.batchArmed.kind or nil
    K.text(p,GRID_X,1052,GRID_W,46,#list>0 and (tr("Выбрано: ","Selected: ")..#list..(count>#list and ("  ("..tr("шт. ","pcs ")..count..")") or "")) or tr("Выберите предметы","Pick the items"),32,#list>0 and K.YELLOW or K.GREY)
    b=U.action(kit,p,kit.P1X,GRID_X,1110,200,tr("ВСЁ","ALL"),function()
      for _,m in ipairs(shown or {}) do bp.marked[m.key]=true end
      bp.batchArmed=nil;C.redraw(kit)
    end,shown~=nil and #shown>0,68,28);b.key="backpackMarkAll"
    b=U.action(kit,p,kit.P1X,GRID_X+212,1110,200,tr("СНЯТЬ","NONE"),function()
      bp.marked={};bp.batchArmed=nil;C.redraw(kit)
    end,#list>0,68,28);b.key="backpackMarkNone"
    local function batch(kind,fn,ru,en)
      if armedFor~=kind then bp.batchArmed={kind=kind,at=kit.S.time};C.redraw(kit);return end
      bp.batchArmed=nil
      local items,skipped=fn(list)
      kit.toast(tr(ru,en)..items..(skipped>0 and (tr(", пропущено: ",", skipped: ")..skipped) or ""))
      bp.marked={};bp.refreshAt=kit.S.time+.3;C.redraw(kit)
    end
    b=U.action(kit,p,kit.P1X,GRID_X+GRID_W-604,1110,290,armedFor=="drop" and tr("ПОДТВЕРДИТЬ","CONFIRM") or K.upper(loc("UI-UserActions-Drop","Бросить","Drop")),function()
      batch("drop",B.dropMany,"Выброшено: ","Dropped: ")
    end,#list>0,68,28);b.key="backpackDropMarked"
    b=U.action(kit,p,kit.P1X,GRID_X+GRID_W-302,1110,302,armedFor=="disassemble" and tr("ПОДТВЕРДИТЬ","CONFIRM") or K.upper(loc("UI-UserActions-Disassemble","Разобрать","Disassemble")),function()
      batch("disassemble",B.disassembleMany,"Разобрано: ","Disassembled: ")
    end,#list>0,68,28);b.key="backpackDisassembleMarked"
  elseif v then
    local a=B.actions(v);bp.quantity=math.min(bp.quantity or 1,a.max)
    K.text(p,GRID_X,1052,GRID_W,46,K.clip(v.name,70),32,colors[v.quality] or K.CYAN)
    local x=GRID_X
    if a.canFavorite then
      local fav=a.favorite
      local fb=U.action(kit,p,kit.P1X,x,1110,90,"",function()
        B.setFavorite(v,not fav);v.favorite=not fav;bp.refreshAt=kit.S.time+.3;C.redraw(kit)
      end,true);fb.key="backpackFavorite"
      star(p,x+45,1144,22,fav and K.YELLOW or K.CYAN,1,fav and 4 or 2)
      x=x+102
    end
    if a.max>1 then
      b=U.action(kit,p,kit.P1X,x,1110,80,"−",function() bp.quantity=math.max(1,bp.quantity-1);bp.confirm=nil;C.redraw(kit) end,bp.quantity>1);b.key="backpackQuantityMinus"
      K.text(p,x+84,1110,110,68,tostring(bp.quantity),32,K.CYAN,"center")
      b=U.action(kit,p,kit.P1X,x+198,1110,80,"+",function() bp.quantity=math.min(a.max,bp.quantity+1);bp.confirm=nil;C.redraw(kit) end,bp.quantity<a.max);b.key="backpackQuantityPlus"
      b=U.action(kit,p,kit.P1X,x+290,1110,150,tr("МАКС.","MAX"),function() bp.quantity=a.max;bp.confirm=nil;C.redraw(kit) end,true,68,28);b.key="backpackQuantityMax"
    end
    local function done(ok,ru,en)
      if ok then kit.toast(tr(ru,en));bp.selected=nil;bp.confirm=nil;bp.refreshAt=kit.S.time+.3 else kit.toast(kit.T.blocked) end
      C.redraw(kit)
    end
    b=U.action(kit,p,kit.P1X,GRID_X+GRID_W-604,1110,290,K.upper(loc("UI-UserActions-Drop","Бросить","Drop")),function()
      done(B.drop(v,bp.quantity),"Предмет выброшен","Item dropped")
    end,a.drop,68,28);b.key="backpackDrop"
    local confirm=bp.confirm==v.key
    b=U.action(kit,p,kit.P1X,GRID_X+GRID_W-302,1110,302,confirm and tr("ПОДТВЕРДИТЬ","CONFIRM") or K.upper(loc("UI-UserActions-Disassemble","Разобрать","Disassemble")),function()
      -- an iconic weapon asks first, as the game does
      if a.iconic and bp.confirm~=v.key then bp.confirm=v.key;C.redraw(kit);return end
      done(B.disassemble(v,bp.quantity),"Предмет разобран","Item disassembled")
    end,a.disassemble,68,28);b.key="backpackDisassemble"
    if confirm then K.text(p,GRID_X+GRID_W-604,1052,604,46,tr("Культовое оружие будет разобрано","The iconic weapon will be disassembled"),24,K.YELLOW,"right") end
  else K.text(p,GRID_X,1100,GRID_W,80,tr("Наведите на предмет, чтобы увидеть подробности; выберите его для действий","Point at an item for its details; select it for actions"),27,K.GREY,"center") end
  -- the tooltip layer, over the side of the grid away from the item pointed at
  bp.tipHost=K.canvas(p,0,0,W,1100)
  bp.block=kit.button("p2",kit.at(kit.P1X,kit.P1Y),0,186,640,850,nil,nil);bp.block.blocker=true;bp.block.visible=false
  bp.closeHost=K.canvas(p,0,0,60,50);bp.closeHost:SetVisible(false)
  K.roundFill(bp.closeHost,0,0,60,50,K.BLACK,1,7);U.line(bp.closeHost,18,12,42,36,K.CYAN,1,3);U.line(bp.closeHost,42,12,18,36,K.CYAN,1,3)
  bp.close=kit.button("p2",kit.at(kit.P1X,kit.P1Y),0,0,60,50,function() bp.selected=nil;bp.hover=nil;bp.confirm=nil;C.redraw(kit) end,nil)
  bp.close.key="backpackTooltipClose";bp.close.visible=false
  Sort.popup(kit,p,bp)
end
function V.update(kit,s)
  local bp=s and s.bp;if not bp then return end
  -- a fresh read after the menu's own action, or when the inventory signature moved; the old one stays drawn
  local _,sig=Jobs.get("inventorySig")
  local sigSeq=sig and sig.seq
  if (bp.refreshAt and kit.S.time>=bp.refreshAt) or (sigSeq and bp.sigSeq and sigSeq~=bp.sigSeq) then
    bp.refreshAt=nil;Jobs.run("backpack",B.snapshotJob,true)
  end
  bp.sigSeq=sigSeq
  local snap,rec=Jobs.get("backpack")
  if rec and rec.seq~=bp.snapSeq then bp.snapSeq=rec.seq;bp.sorted=nil;bp.sorting={key=sortKey(bp)} end
  if bp.sorting and snap then
    local job=bp.sorting
    if job.key~=sortKey(bp) then job=nil;bp.sorting={key=sortKey(bp)};return end
    job.task=job.task or Sort.begin(B.filtered(snap,bp.filter),bp.sort)
    local sorted=Sort.step(job.task,os.clock()+.004)
    if sorted then bp.sorted=sorted;bp.sortedKey=job.key;bp.sortedSeq=bp.snapSeq;bp.sorting=nil;C.redraw(kit) end
    return
  end
  Tooltip.fit(bp.tips)
  local key=bp.hover or bp.selected
  if bp.tipHost and key~=bp.shown and kit.S.time-(bp.hoverAt or 0)>.12 then
    bp.tipHost:RemoveAllChildren();bp.tips={};bp.shown=key;bp.block.visible=false;bp.close.visible=false;bp.closeHost:SetVisible(false)
    local v=key and snap and snap.byKey[key]
    if v then
      -- the first column's items get it over the third column, the others over the components
      local x=(bp.colOf and bp.colOf[key]==1) and W-652 or 12
      local ui=Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem")):GetPlayerItem(v.id) or v.ui
      local resolver=InventoryItemPreferredComparisonResolver.Make(Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem")))
      local compare=not ui:IsEquipped() and resolver:GetPreferredComparisonItem(ui) or nil
      if compare and compare:GetID()==v.id then compare=nil end
      local t=Tooltip.spawn(kit,bp.tipHost,ui,ItemDisplayContext.Backpack,x+12,194,616,830,compare,nil,true)
      if t then bp.tips[#bp.tips+1]=t end
      bp.block.x=kit.P1X+x;bp.block.visible=true
      if bp.selected==key then
        K.move(bp.closeHost,x+572,194);bp.closeHost:SetVisible(true);bp.close.x=kit.P1X+x+572;bp.close.y=kit.P1Y+194;bp.close.visible=true
      end
    end
  end
end
function V.leave(s) if s and s.bp then s.bp.tips={};s.bp.tipHost=nil end end
return V
end
