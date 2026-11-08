local K=require("modules/ink")
local L=require("modules/locale")
local I=require("modules/inventory_data")
local Preview=require("modules/inventory_preview")
local Pref=require("modules/preferences")
local Images=require("modules/gameplay_data")
local Sort=require("modules/item_sort")
local Wd=require("modules/wardrobe_data")
local Jobs=require("modules/jobs")
local Tooltip=require("modules/item_tooltip")
local B=require("modules/backpack_data")
local HI=require("modules/holster_icons")
-- THE HOLSTERS (CyberpunkVRPort_Holster, immersive holsters): each weapon slot's tile carries a badge -- the figure and
-- its weapon where it hangs -- that opens a row of the places its kind can take (auto, each place, not carried), and
-- the page has a switch that hides every holstered weapon (2026-11-05). The icons are one atlas
-- (tools/vr_lobby_assets/make_holster_icons.py); the names are the player's own side (left-handed: turned over).
local PLACE_NAMES={auto={"АВТО","AUTO"},none={"НЕ\nНОСИТЬ","NOT\nCARRIED"},
  hip_r={"ПРАВОЕ\nБЕДРО","RIGHT\nHIP"},hip_l={"ЛЕВОЕ\nБЕДРО","LEFT\nHIP"},
  thigh_r={"ПРАВАЯ\nНОГА","RIGHT\nTHIGH"},thigh_l={"ЛЕВАЯ\nНОГА","LEFT\nTHIGH"},
  underarm_l={"ПОД ЛЕВОЙ\nРУКОЙ","UNDER\nLEFT ARM"},underarm_r={"ПОД ПРАВОЙ\nРУКОЙ","UNDER\nRIGHT ARM"},
  chest_l={"ГРУДЬ\nСЛЕВА","LEFT\nCHEST"},chest_r={"ГРУДЬ\nСПРАВА","RIGHT\nCHEST"},
  shoulder_r={"ЗА ПРАВЫМ\nПЛЕЧОМ","RIGHT\nSHOULDER"},shoulder_l={"ЗА ЛЕВЫМ\nПЛЕЧОМ","LEFT\nSHOULDER"},
  lower_back={"ПОЯСНИЦА","LOWER\nBACK"}}
-- The holster mod's menu side, while the holsters are the immersive ones (shared [23] is 1 for the simple ones).
local function holsterMod()
  if type(GetVRSharedSlot)=="function" and (GetVRSharedSlot(23) or 0)>0.5 then return nil end
  local ok,m=pcall(GetMod,"CyberpunkVRPort_Holster")
  if ok and type(m)=="table" and type(m.slotInfo)=="function" then return m end
  return nil
end
-- "_w" when the player's body is the woman's: her figure (body_w), and a weapon on the back cut by her outline.
local function figureSuffix()
  local ok,female=pcall(function() return NameToString(Game.GetPlayer():GetResolvedGenderName())=="Female" end)
  return (ok and female) and "_w" or ""
end
-- The figure, then over it the weapon where it hangs; "none" crosses it out, nil (no place free) leaves it alone.
local function placeIcon(p,x,y,size,place,family,col,op)
  local w=figureSuffix()
  K.image(p,x,y,size,size,HI.ATLAS,HI.parts["body"..w] and "body"..w or "body",K.CYAN,op*.6)
  if place=="none" then K.image(p,x,y,size,size,HI.ATLAS,"none",K.RED,op)
  elseif place~=nil then
    local part=place.."_"..family
    if HI.parts[part..w] then part=part..w end
    if HI.parts[part] then K.image(p,x,y,size,size,HI.ATLAS,part,col,op) end
  end
end
return function(C,U)
local V={}
local Wardrobe
local W=1780
local names={Weapon={"ОРУЖИЕ","WEAPONS"},Head={"ГОЛОВА","HEAD"},Face={"ЛИЦО","FACE"},
  OuterChest={"ВЕРХНЯЯ ОДЕЖДА","OUTER TORSO"},InnerChest={"НИЖНЯЯ ОДЕЖДА","INNER TORSO"},
  Legs={"НОГИ","LEGS"},Feet={"ОБУВЬ","FEET"},Outfit={"КОМПЛЕКТ ОДЕЖДЫ","OUTFIT"}}
local quality=K.QUALITY
local function tr(kit,ru,en) return L.tr(ru,en) end
local function label(kit,area) local a=names[area];return a and L.tr(a[1],a[2]) or area end
local function inv(kit)
  local s=C.state(kit);s.inventory=s.inventory or {slot=0,filter="all"}
  s.inventoryContext=s.inventoryContext or I.open()
  return s.inventory,s.inventoryContext
end
local function action(kit,p,sx,x,y,w,ru,en,fn,enabled,key)
  local b=U.action(kit,p,sx,x,y,w,tr(kit,ru,en),fn,enabled);b.key=key;return b
end
local function redraw(kit) C.state(kit).resetList=true;C.redraw(kit) end
local function pending(kit,result)
  if result then
    C.state(kit).inventoryRefreshAt=kit.S.time+.3
    C.state(kit).inventoryPreviewDirty=true
  else kit.toast(tr(kit,"Сейчас недоступно","Not available right now")) end
end
local function tile(kit,p,H,x,y,w,h,item,caption,fn,selected,key,onHover)
  local col=item and quality[item.quality] or K.CYAN;col=col or K.CYAN
  local weapon=item and item.area==gamedataEquipmentArea.Weapon
  local idleOpacity=.045
  local bg=K.roundFill(p,x,y,w,h,selected and K.YELLOW or col,selected and .12 or idleOpacity,4)
  local edge=K.cutFrame(p,x,y,w,h,selected and K.YELLOW or K.CYAN,selected and 1 or .35,10,1)
  K.rect(p,x+2,y+5,6,h-10,col,1)
  if item then
    local weapon=item.area==gamedataEquipmentArea.Weapon
    U.picture(kit,p,x+14,y+(weapon and 28 or 8),w-28,h-(weapon and 96 or 61),item.image)
    K.text(p,x+14,y+h-(weapon and 61 or 50),w-28,weapon and 31 or 44,K.clip(item.name,math.max(9,math.floor((w-28)/13))),weapon and 23 or 25,col,"center")
    if item.new then K.image(p,x+w-22,y+10,10,10,K.ATLAS.phone,"circle",K.CYAN,1) end
    if item.equipped then
      U.line(p,x+w-63,y+15,x+w-55,y+23,K.CYAN,1,3)
      U.line(p,x+w-55,y+23,x+w-43,y+9,K.CYAN,1,3)
    end
    if weapon then
      local g=C.state(kit);local ctx=g.inventoryContext
      local parts=I.slots(ctx,item)
      local ox=key and key:find("^weaponSlot") and 48 or 16
      local attachments={}
      for i,part in ipairs(parts) do
        K.image(p,x+ox+(i-1)*19,y+12,12,12,K.ATLAS.phone,"circle",K.RED,1)
        if not part.item then K.image(p,x+ox+3+(i-1)*19,y+15,6,6,K.ATLAS.phone,"circle",K.BLACK,1) end
        if part.id==TweakDBID.new("AttachmentSlots.Scope") or part.id==TweakDBID.new("AttachmentSlots.PowerModule") then attachments[#attachments+1]=part end
      end
      local compare=I.compare(ctx,item,g.inventoryComparison)
      if compare==ItemComparisonState.Better or compare==ItemComparisonState.Worse then
        local up=compare==ItemComparisonState.Better;local c=up and {.2,.95,.5} or K.RED
        local cy=y+40;local sign=up and -1 or 1
        U.line(p,x+w-30,cy-sign*5,x+w-23,cy+sign*3,c,1,3)
        U.line(p,x+w-23,cy+sign*3,x+w-16,cy-sign*5,c,1,3)
      end
      local tx=x+15
      if not item.melee then
        K.rect(p,tx,y+h-25,3,14,K.CYAN,1);K.rect(p,tx+6,y+h-25,3,14,K.CYAN,1)
        K.text(p,tx+16,y+h-31,80,28,tostring(item.ammo),23,K.CYAN);tx=tx+94
      end
      if item.typeIcon then K.image(p,tx,y+h-29,28,25,item.typeIcon.atlas,item.typeIcon.part,item.iconic and K.YELLOW or K.CYAN,1) end
      -- a narrow tile (the left-hand slot) keeps the ammo and the type icon only
      if w<200 then attachments={} end
      local available=x+w-14-#attachments*34-(tx+32)
      if available>=40 then
        K.text(p,tx+32,y+h-31,available,28,K.clip(K.upper(item.typeName),math.max(8,math.floor(available/11))),21,item.iconic and K.YELLOW or K.CYAN)
      end
      for i,part in ipairs(attachments) do
        local icon=Images.image(TweakDBID.new(NameToString(UIItemsHelper.GetSlotShadowIcon(part.id,gamedataItemType.Invalid,gamedataEquipmentArea.Invalid))))
        if icon then K.image(p,x+w-14-#attachments*34+(i-1)*34,y+h-31,30,28,icon.atlas,icon.part,part.item and K.CYAN or K.RED,part.item and 1 or .5) end
      end
    elseif item.iconic then K.text(p,x+17,y+5,32,34,"◆",25,K.YELLOW,"center") end
  else K.text(p,x+10,y+15,w-20,h-30,caption or "+",30,K.GREY,"center") end
  local b=kit.button("p1",H,x,y,w,h,fn,function(hover)
    bg:SetTintColor(K.color((hover or selected) and K.YELLOW or col));bg:SetOpacity(hover and .16 or selected and .12 or idleOpacity)
    K.tint(edge,(hover or selected) and K.YELLOW or K.CYAN,hover and 1 or selected and .85 or .35)
    if onHover then onHover(hover) end
  end);b.key=key;return b
end
Wardrobe=require("modules/wardrobe_view")(C,U,tile)
local Backpack=require("modules/backpack_view")(C,U)
-- the game's own word for it where it has one
local function loc(key,ru,en) local t=I.loc(key);if t~="" and t~=key then return t end;return L.tr(ru,en) end
-- A weapon slot's holster badge, (x, y) its top-left: where the weapon hangs, a tap opens the places it can take.
local function holsterBadge(kit,p,origin,x,y,slot,H)
  local ok,info=pcall(H.slotInfo,slot)
  if not ok or info==nil then return end
  local s=inv(kit)
  local place=info.choice=="none" and "none" or info.place
  local bg=K.roundFill(p,x,y,84,84,K.BLACK,.62,6)
  local edge=K.cutFrame(p,x,y,84,84,K.CYAN,.4,7,1)
  -- the dual wielding pair hangs on both hips over any choice: both guns shown, nothing to pick
  if info.pair then
    placeIcon(p,x+4,y+4,76,info.place,info.family,K.YELLOW,1)
    local other=(info.place=="hip_r" and "hip_l" or "hip_r").."_"..info.family
    if HI.parts[other..figureSuffix()] then other=other..figureSuffix() end
    if HI.parts[other] then K.image(p,x+4,y+4,76,76,HI.ATLAS,other,K.YELLOW,1) end
    K.text(p,x+4,y+60,74,22,"x2",18,K.CYAN,"right")
  else
    placeIcon(p,x+4,y+4,76,place,info.family,K.YELLOW,1)
    if info.choice=="auto" then K.text(p,x+4,y+60,74,22,"A",18,K.CYAN,"right") end
  end
  local b=kit.button("p1",origin,x,y,84,84,function()
    if info.pair then kit.toast(tr(kit,"Две пушки висят на бёдрах","Dual wielding: both guns hang on the hips"));return end
    s.holsterPick=slot;C.redraw(kit)
  end,function(h)
    K.tint(edge,h and K.YELLOW or K.CYAN,h and 1 or .4);bg:SetOpacity(h and .8 or .62)
  end)
  b.key="holsterSlot"..slot
end
-- The row of places for weapon slot `slot`, over its tile: auto, each place its kind can take, not carried.
local function holsterPicker(kit,p,origin,x,y,w,h,slot,H)
  local s=inv(kit)
  local ok,info=pcall(H.slotInfo,slot)
  if not ok or info==nil then s.holsterPick=nil;return end
  local opts={{name="auto",shown=info.choice=="auto" and info.place or info.auto,label="auto"}}
  for _,c in ipairs(info.choices) do opts[#opts+1]={name=c.name,shown=c.shown,label=c.shown} end
  opts[#opts+1]={name="none",shown="none",label="none"}
  K.roundFill(p,x,y,w,h,K.BLACK,.94,6);K.cutFrame(p,x,y,w,h,K.YELLOW,.85,10,1.5)
  -- a tap beside the places closes the row
  kit.button("p1",origin,x,y,w,h,function() s.holsterPick=nil;C.redraw(kit) end,nil).key="holsterPickClose"
  local cw=math.floor((w-16)/#opts);local size=math.min(88,cw-14)
  for i,o in ipairs(opts) do
    local cx=x+8+(i-1)*cw;local selected=info.choice==o.name
    -- the one chosen framed, not filled: its icon stays readable
    local cell=K.roundFill(p,cx+2,y+10,cw-4,h-20,K.CYAN,.05,4)
    if selected then K.cutFrame(p,cx+2,y+10,cw-4,h-20,K.YELLOW,.95,8,2) end
    placeIcon(p,cx+math.floor((cw-size)/2),y+16,size,o.shown,info.family,selected and K.YELLOW or K.CYAN,1)
    local nm=PLACE_NAMES[o.label] or {o.label,o.label}
    K.text(p,cx+2,y+20+size,cw-4,h-30-size,L.tr(nm[1],nm[2]),18,selected and K.YELLOW or K.TEXT,"center")
    kit.button("p1",origin,cx+2,y+10,cw-4,h-20,function()
      if H.setPlace(slot,o.name) then s.holsterPick=nil;C.redraw(kit)
      else kit.toast(tr(kit,"Не удалось сохранить настройку","Unable to save preference")) end
    end,function(hv)
      cell:SetTintColor(K.color(hv and K.YELLOW or K.CYAN));cell:SetOpacity(hv and .16 or .05)
    end).key="holsterPick:"..o.name
  end
end
local function choose(kit,area,slot)
  local s=inv(kit);s.area=area;s.slot=slot or 0;s.part=nil;s.selection=nil;s.filter="all";s.confirm=nil;s.holsterPick=nil
  if area=="Weapon" then s.weaponSlot=slot end
  redraw(kit)
end
-- The hover callback of an equipped tile on the main page: its item's tooltip after a short rest (V.update).
local function hoverTip(kit,item)
  if not item then return nil end
  local state=C.state(kit);state.invByKey=state.invByKey or {};state.invByKey[item.key]=item
  return function(on)
    if on then state.invHover=item.key;state.invHoverAt=kit.S.time
    elseif state.invHover==item.key then state.invHover=nil;state.invHoverAt=kit.S.time end
  end
end
function V.draw(kit,p,p2)
  local state=C.state(kit);local s,ctx=inv(kit)
  kit.glass(p,W,1305,kit.P1X,kit.P1Y)
  if s.wardrobe and Wd.available() then
    state.inventoryComparison=nil
    Wardrobe.draw(kit,p,ctx,s,function() s.wardrobe=nil;s.wardrobeDelete=nil;redraw(kit) end)
    return
  end
  s.wardrobe=nil
  state.invTipHost=nil;state.invTips={};state.invHover=nil;state.invTipShown=nil
  state.invTipRect=nil;state.invTipBlock=nil;state.invByKey={}
  if s.backpack then
    state.inventoryComparison=nil;Preview.release(kit)
    Backpack.draw(kit,p,s,function() s.backpack=nil;redraw(kit) end)
    return
  end
  U.title(p,tr(kit,"СНАРЯЖЕНИЕ","INVENTORY"),W)
  local Hol=not s.area and holsterMod() or nil
  action(kit,p,kit.P1X,24,1210,Hol and 300 or 404,Pref.characterModel and "МОДЕЛЬ: ВКЛ" or "МОДЕЛЬ: ВЫКЛ",Pref.characterModel and "MODEL: ON" or "MODEL: OFF",function()
    if Pref.setCharacterModel(not Pref.characterModel) then Preview.release(kit);C.redraw(kit)
    else kit.toast(tr(kit,"Не удалось сохранить настройку","Unable to save preference")) end
  end,true,"inventoryModelToggle")
  if Hol then
    -- every holstered weapon unseen on the body (still there to draw), or seen again
    local hidden=Hol.hidden()==true
    U.action(kit,p,kit.P1X,334,1210,300,tr(kit,hidden and "ХОЛСТЕРЫ: СКРЫТЫ" or "ХОЛСТЕРЫ: ВИДНЫ",hidden and "HOLSTERS: HIDDEN" or "HOLSTERS: SHOWN"),function()
      if Hol.setHidden(not hidden) then C.redraw(kit)
      else kit.toast(tr(kit,"Не удалось сохранить настройку","Unable to save preference")) end
    end,true,68,26).key="inventoryHolsterToggle"
  end
  state.inventoryComparison=nil
  local origin=kit.at(kit.P1X,kit.P1Y)
  if not s.area then
    K.text(p,24,107,450,45,label(kit,"Weapon"),32,K.WHITE)
    -- with the dual-wielding perk the FIRST slot gets its left-hand partner beside it -- the only one the game
    -- dual wields (I.DUAL_SLOT)
    local dual=I.dualWield()
    I.clearStrayLeft(ctx)
    for slot=0,2 do
      local item=I.equipped(ctx,"Weapon",slot)
      local paired=dual and slot==I.DUAL_SLOT
      tile(kit,p,origin,24,163+slot*222,paired and 300 or 454,205,item,tr(kit,"СЛОТ ","SLOT ")..(slot+1),function() choose(kit,"Weapon",slot) end,false,"weaponSlot"..slot,hoverTip(kit,item))
      K.text(p,36,170+slot*222,50,34,tostring(slot+1),27,K.CYAN)
      if paired then
        local left=I.equipped(ctx,"Weapon",slot+3)
        tile(kit,p,origin,334,163+slot*222,144,205,left,tr(kit,"ЛЕВАЯ","LEFT"),function() choose(kit,"Weapon",slot+3) end,false,"weaponSlotLeft"..slot,hoverTip(kit,left))
        K.text(p,344,170+slot*222,40,34,tr(kit,"Л","L"),27,K.CYAN)
      end
    end
    Preview.show(kit,p,486,100,760,1080)
    -- the holster badges just right of the slots, over the preview's empty edge (drawn after it, on top), and the row
    -- of places, wider than a slot so every name fits (2026-11-05: "a little to the right of the slot")
    if Hol then
      for slot=0,2 do
        if I.equipped(ctx,"Weapon",slot) then holsterBadge(kit,p,origin,488,163+slot*222+60,slot,Hol) end
      end
      if s.holsterPick~=nil then holsterPicker(kit,p,origin,24,163+s.holsterPick*222,760,205,s.holsterPick,Hol) end
    end
    for i,area in ipairs(I.clothes) do
      local x,y=1260+((i-1)%2)*250,155+math.floor((i-1)/2)*258
      K.text(p,x,y-47,234,43,label(kit,area),25,K.WHITE)
      local worn=I.equipped(ctx,area,0)
      tile(kit,p,origin,x,y,234,194,worn,"+",function() choose(kit,area,0) end,false,"clothingSlot"..area,hoverTip(kit,worn))
    end
    -- EquipmentEx's wardrobe, only when EquipmentEx is installed
    if Wd.available() then
      U.action(kit,p,kit.P1X,1260,1210,484,K.upper(Wd.text("Gameplay-Devices-DisplayNames-Wardrobe")),function()
        s.wardrobe=true;s.wardrobeDelete=nil;redraw(kit)
      end).key="inventoryWardrobe"
    end
    U.action(kit,p,kit.P1X,646,1210,440,K.upper(loc("UI-PanelNames-BACKPACK","Рюкзак","Backpack")),function()
      s.backpack=true;s.area=nil;redraw(kit)
    end).key="inventoryBackpack"
    -- the game's tooltip for an equipped item pointed at, over the character preview (V.update fills it)
    state.invTipRect={490,110,752,1070};state.invTipHost=K.canvas(p,0,0,W,1200)
    if I.wardrobe() then
      K.text(p,24,922,450,110,table.concat(C.wrap(tr(kit,"Внешний вид задан комплектом гардероба","Appearance is set by your wardrobe outfit"),28),"\n"),28,K.YELLOW)
      action(kit,p,kit.P1X,24,1060,454,"СНЯТЬ КОМПЛЕКТ","REMOVE OUTFIT",function() pending(kit,I.clearWardrobe(ctx)) end,ctx.manager:IsWardrobeEnabled(),"wardrobeRemove")
    end
    return
  end
  action(kit,p,kit.P1X,24,105,320,"К СНАРЯЖЕНИЮ","EQUIPMENT",function() s.area=nil;s.part=nil;s.selection=nil;redraw(kit) end,true,"inventoryBack")
  local leftSlot=s.area=="Weapon" and (s.slot or 0)>=3
  local slotName=leftSlot and (s.slot-2).."  "..tr(kit,"ЛЕВАЯ РУКА","LEFT HAND") or tostring((s.slot or 0)+1)
  K.text(p,680,110,700,58,label(kit,s.area)..(s.area=="Weapon" and "  /  "..slotName or ""),34,K.CYAN)
  Sort.control(kit,p,s,"inventorySort",1448,112,280,s.sort,function(value)
    s.sort=value;s.selection=nil;s.confirm=nil;state.resetList=true
  end,function() C.redraw(kit) end)
  local equipped=I.equipped(ctx,s.area,s.slot)
  state.inventoryComparison=equipped
  local parts=I.slots(ctx,equipped);local selectedSlot
  if s.area=="Weapon" then
    Preview.release(kit)
    tile(kit,p,origin,24,200,554,255,equipped,"+",function() s.part=nil;s.selection=nil;redraw(kit) end,not s.part,"equippedWeapon")
    K.text(p,24,484,554,48,tr(kit,"НАСАДКИ И МОДИФИКАЦИИ","ATTACHMENTS AND MODS"),29,K.WHITE)
    if #parts==0 then K.text(p,24,553,554,110,tr(kit,"У этого оружия нет доступных слотов","This weapon has no available attachment slots"),29,K.GREY,nil,true) end
    for i,part in ipairs(parts) do
      local x,y=24+((i-1)%3)*188,570+math.floor((i-1)/3)*205
      if s.part==part.key then selectedSlot=part end
      tile(kit,p,origin,x,y,178,155,part.item,"+",function() s.part=part.key;s.selection=nil;s.confirm=nil;redraw(kit) end,s.part==part.key,"attachmentSlot"..i)
      local slotLabel=(part.id==TweakDBID.new("AttachmentSlots.Scope") or part.id==TweakDBID.new("AttachmentSlots.PowerModule")) and part.name or tr(kit,"Модификация","Mod")
      K.text(p,x,y-38,178,34,K.clip(slotLabel,15),24,K.CYAN,"center")
    end
    if s.part and not selectedSlot then s.part=nil end
  else
    Preview.show(kit,p,0,215,640,900,s.area)
    if equipped then K.text(p,25,180,550,45,K.clip(equipped.name,36),29,quality[equipped.quality] or K.CYAN,"center") end
    if I.wardrobe() then K.text(p,24,1018,554,70,tr(kit,"Внешний вид: комплект гардероба","Appearance: wardrobe outfit"),26,K.YELLOW) end
  end
  if equipped and not selectedSlot then
    action(kit,p,kit.P1X,24,1110,554,"СНЯТЬ","UNEQUIP",function() pending(kit,I.unequip(ctx,s.area,s.slot)) end,I.canChange(ctx,s.area,s.slot),"inventoryUnequip")
  elseif selectedSlot and selectedSlot.item then
    action(kit,p,kit.P1X,24,1110,554,"СНЯТЬ НАСАДКУ","REMOVE ATTACHMENT",function() pending(kit,I.remove(ctx,equipped.id,selectedSlot.id)) end,selectedSlot.removable,"attachmentRemove")
    if not selectedSlot.removable then K.text(p,24,1005,554,78,tr(kit,"Установленный мод нельзя снять","This installed mod cannot be removed"),28,K.GREY,nil,true) end
  end
  local items=selectedSlot and I.parts(ctx,equipped,selectedSlot) or I.items(ctx,s.area)
  local shown={}
  if s.area=="Weapon" and not selectedSlot then
    local filters={{"all","ВСЁ","ALL"},{"ranged","ОГНЕСТРЕЛ","RANGED"},{"melee","БЛИЖНИЙ БОЙ","MELEE"}}
    for i,f in ipairs(filters) do
      local x=680+(i-1)*352
      action(kit,p,kit.P1X,x,180,336,f[2],f[3],function() s.filter=f[1];s.selection=nil;redraw(kit) end,true,"inventoryFilter"..f[1])
      if i>1 then
        local id=UIItemsHelper.GetWeaponTypeIcon(i==2 and gamedataItemType.Wea_Handgun or gamedataItemType.Wea_Katana)
        local icon=Images.image(TweakDBID.new(NameToString(id)))
        if icon then K.image(p,x+12,199,34,30,icon.atlas,icon.part,K.CYAN,1) end
      end
      if s.filter==f[1] then K.rect(p,x+15,244,306,3,K.CYAN,1) end
    end
  elseif selectedSlot then K.text(p,680,178,1048,60,selectedSlot.name,30,K.YELLOW) end
  -- the left hand takes only a gun that can be dual wielded, and only beside one in the right hand
  local partnerOK=not leftSlot or I.canDual(I.equipped(ctx,"Weapon",s.slot-3))
  for _,item in ipairs(items) do
    local melee=item.melee
    if (not leftSlot or selectedSlot or I.canDual(item))
      and (s.filter=="all" or selectedSlot or s.area~="Weapon" or (s.filter=="melee" and melee) or (s.filter=="ranged" and not melee)) then shown[#shown+1]=item end
  end
  shown=Sort.apply(shown,s.sort)
  local selected
  for _,item in ipairs(shown) do if s.selection==item.key then selected=item end end
  local gridW,cols=1080,(s.area=="Weapon" and not selectedSlot) and 2 or 5
  local cellW=math.floor((gridW-46-(cols-1)*12)/cols)
  local rowH=cols==2 and 210 or 205
  local rows={}
  for first=1,#shown,cols do
    local group={};for i=first,math.min(#shown,first+cols-1) do group[#group+1]=shown[i] end
    rows[#rows+1]={h=rowH,build=function(c,it,v)
      for i,item in ipairs(group) do
        tile(kit,c,kit.at(v.x,it.y,v),(i-1)*(cellW+12),4,cellW,rowH-12,item,nil,function() s.selection=item.key;s.confirm=nil;I.inspect(ctx,item);C.redraw(kit) end,s.selection==item.key,"inventoryItem:"..item.key,function(on)
          if on then state.invHover=item.key;state.invHoverAt=kit.S.time
          elseif state.invHover==item.key then state.invHover=nil;state.invHoverAt=kit.S.time end
        end)
      end
    end}
  end
  local host=K.canvas(p,660,0,gridW,1200)
  C.list(kit,host,"p1","p1",kit.P1X+660,270,gridW,rows,660,kit.S.keepScroll):enableMask()
  if selected then
    K.text(p,680,1005,1048,44,K.clip(selected.name,60),33,quality[selected.quality] or K.CYAN)
    local detail=selected.mod and tr(kit,"Модификация оружия","Weapon mod") or selected.type==gamedataItemType.Wea_Katana and tr(kit,"Катана","Katana") or selected.description
    if s.area=="Weapon" and not selectedSlot then detail=tr(kit,"Урон в секунду: ","DPS: ")..selected.dps.."    "..tr(kit,"Боеприпасы: ","Ammo: ")..selected.ammo end
    K.text(p,680,1055,1048,47,K.clip(C.clean(detail),83),27,K.GREY)
    local enabled=selected.allowed and (selectedSlot~=nil or (I.canChange(ctx,s.area,s.slot) and partnerOK))
    local ru,en=selectedSlot and "УСТАНОВИТЬ" or "ЭКИПИРОВАТЬ",selectedSlot and "INSTALL" or "EQUIP"
    if s.confirm then ru,en="ЗАМЕНИТЬ МОД","REPLACE MOD";K.text(p,680,1050,1048,55,tr(kit,"Предыдущий мод будет уничтожен","The previous mod will be destroyed"),28,K.YELLOW) end
    action(kit,p,kit.P1X,680,1110,500,ru,en,function()
      local result
      if selectedSlot then result=I.install(ctx,equipped.id,selectedSlot.id,selected.id,s.confirm==selected.key)
      else result=I.equip(ctx,s.area,s.slot,selected.id) end
      if result=="confirm" then s.confirm=selected.key;C.redraw(kit) else s.confirm=nil;pending(kit,result) end
    end,enabled,"inventoryApply")
    -- the backpack's hold actions as buttons: favourite (weapons), drop, disassemble
    local a=B.actions({id=selected.id})
    if a.canFavorite then
      local fav=a.favorite
      U.action(kit,p,kit.P1X,1192,1110,90,"",function() B.setFavorite({id=selected.id},not fav);pending(kit,true);C.redraw(kit) end,true).key="inventoryFavorite"
      Backpack.star(p,1237,1144,22,fav and K.YELLOW or K.CYAN,1,fav and 4 or 2)
    end
    local function done(ok,ru,en) if ok then kit.toast(tr(kit,ru,en));s.selection=nil;s.disarm=nil end;pending(kit,ok) end
    U.action(kit,p,kit.P1X,1294,1110,226,K.upper(loc("UI-UserActions-Drop","Бросить","Drop")),function()
      done(B.drop({id=selected.id},1),"Предмет выброшен","Item dropped")
    end,a.drop,68,27).key="inventoryDrop"
    local arm=s.disarm==selected.key
    local disLabel=K.upper(loc("UI-UserActions-Disassemble","Разобрать","Disassemble"))
    if arm then disLabel=tr(kit,"ПОДТВЕРДИТЬ","CONFIRM") end
    U.action(kit,p,kit.P1X,1532,1110,224,disLabel,function()
      if a.iconic and s.disarm~=selected.key then s.disarm=selected.key;C.redraw(kit);return end
      done(B.disassemble({id=selected.id},1),"Предмет разобран","Item disassembled")
    end,a.disassemble,68,27).key="inventoryDisassemble"
  elseif not partnerOK and not selectedSlot then
    K.text(p,680,1035,1048,90,tr(kit,"Сначала возьмите в слот ","First equip slot ")..(s.slot-2)..tr(kit," оружие для двух рук"," with a dual-wield gun"),31,K.YELLOW,"center")
  else K.text(p,680,1035,1048,90,#shown==0 and tr(kit,"Нет подходящих предметов","No compatible items") or tr(kit,"Выберите предмет","Select an item"),31,K.GREY,"center") end
  state.inventoryItems=shown;state.inventoryParts=parts
  state.invByKey={};for _,item in ipairs(shown) do state.invByKey[item.key]=item end
  state.invEquipped=equipped
  -- The game's own item tooltip (all stats, mods, requirements, comparison) for the item pointed at or
  -- selected, over the left column below its back button; V.update fills it once the pointer has rested on an item.
  state.invTipHost=K.canvas(p,0,0,660,1200);state.invTips={};state.invTipShown=nil
  state.invTipBlock=kit.button("p2",kit.at(kit.P1X,kit.P1Y),12,186,640,924,nil,nil);state.invTipBlock.blocker=true;state.invTipBlock.visible=false
  state.invTipCloseHost=K.canvas(p,0,0,60,50);state.invTipCloseHost:SetVisible(false)
  K.roundFill(state.invTipCloseHost,0,0,60,50,K.BLACK,1,7);U.line(state.invTipCloseHost,18,12,42,36,K.CYAN,1,3);U.line(state.invTipCloseHost,42,12,18,36,K.CYAN,1,3)
  state.invTipClose=kit.button("p2",kit.at(kit.P1X,kit.P1Y),578,194,60,50,function()
    s.selection=nil;s.confirm=nil;state.invHover=nil;C.redraw(kit)
  end,nil);state.invTipClose.key="inventoryTooltipClose";state.invTipClose.visible=false
  Sort.popup(kit,p,s)
end

-- Fill the tooltip layer for the hovered (after a short rest) or selected item, compared with the
-- equipped one; empty when neither -- the left column shows through again.
local function tooltip(kit,state)
  if not state.invTipHost then return end
  Tooltip.fit(state.invTips)
  local s=state.inventory or {}
  local key=state.invHover or s.selection
  if key==state.invTipShown or kit.S.time-(state.invHoverAt or 0)<.15 then return end
  state.invTipShown=key
  state.invTipHost:RemoveAllChildren();state.invTips={}
  local item=key and state.invByKey and state.invByKey[key]
  local inventory=Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem"))
  local ui=item and inventory:GetPlayerItem(item.id)
  local show=ui~=nil
  if state.invTipBlock then
    state.invTipBlock.visible=show;state.invTipClose.visible=show and s.selection~=nil;state.invTipCloseHost:SetVisible(show and s.selection~=nil)
  end
  if not show then return end
  -- the picker's plate covers the whole column (the preview would show round a smaller one); the main page's
  -- tooltip gets a plate of its own size
  local r=state.invTipRect or {12,186,640,924}
  if not state.invTipRect then K.roundFill(state.invTipHost,r[1],r[2],r[3],r[4],K.BLACK,1,8) end
  local equipped=state.invEquipped
  local compare=not state.invTipRect and equipped and equipped.id~=item.id and inventory:GetPlayerItem(equipped.id) or nil
  local t=Tooltip.spawn(kit,state.invTipHost,ui,ItemDisplayContext.Backpack,r[1]+12,r[2]+8,r[3]-40,r[4]-24,compare,nil,state.invTipRect~=nil)
  if t then state.invTips[#state.invTips+1]=t end
  if state.invTipCloseHost and state.invTipBlock then K.move(state.invTipCloseHost,578,194) end
end
function V.update(kit)
  local state=C.state(kit);local ctx=state.inventoryContext
  if not ctx then return end
  Preview.update(kit)
  if state.inventory and state.inventory.backpack then Backpack.update(kit,state.inventory)
  elseif state.invTipHost then tooltip(kit,state) end
  local function read(front) Jobs.run("inventorySig",function() return I.signatureJob(ctx) end,front) end
  if state.inventoryRefreshAt and kit.S.time>=state.inventoryRefreshAt then
    -- the menu's own request: show its result now, and read the backpack again behind it
    state.inventoryRefreshAt=nil;I.invalidate(ctx);read(true);C.redraw(kit);return
  end
  -- The backpack and loadout are read a slice a frame (700+ items cost ~10 ms at once); a changed
  -- result redraws, the first one after entering is only the baseline.
  local _,rec=Jobs.get("inventorySig")
  local seq=rec and rec.seq
  if seq~=state.inventorySeq then
    local first=state.inventorySeq==nil
    state.inventorySeq=seq
    if not first then I.invalidate(ctx);C.redraw(kit);return end
  end
  if not Jobs.busy("inventorySig") and Jobs.age("inventorySig")>1 then read(false) end
end
function V.leave(kit)
  local s=C.state(kit);Preview.release(kit)
  I.close(s.inventoryContext);s.inventoryContext=nil;s.inventoryRefreshAt=nil;s.inventoryPoll=nil;s.inventorySeq=nil
  if s.inventory then s.inventory.wardrobe=nil;s.inventory.wardrobeDelete=nil;s.inventory.holsterPick=nil;Backpack.leave(s.inventory) end
end
return V
end
