-- The game's complete dropdown and comparison policy, shared by item grids.
local K=require("modules/ink")
local I=require("modules/inventory_data")
local S={}
local options
function S.options()
  if options then return options end
  options={}
  for _,entry in ipairs(SortingDropdownData.GetDefaultDropdownOptions()) do
    local mode=FromVariant(entry.identifier)
    options[#options+1]={key=tostring(mode):match(": (%w+)"),mode=mode,
      label=K.upper(I.loc(NameToString(entry.labelKey))),direction=entry.direction}
  end
  return options
end
function S.option(key)
  key=({quality="QualityAsc",name="NameAsc",weight="WeightDesc"})[key] or key or "QualityAsc"
  for _,entry in ipairs(S.options()) do if entry.key==key then return entry end end
  return S.option("QualityAsc")
end
function S.apply(items,key)
  local out={};local mode=S.option(key);local view=BackpackDataView.new();view.itemSortMode=mode.mode
  local inventory=Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem"))
  for _,item in ipairs(items) do
    if not item.ui then item.ui=inventory:GetPlayerItemFromAnySource(item.raw or InventoryItemData.GetGameItemData(item.data)) end
    if not item.sortData then item.sortData=WrappedInventoryItemData.new();item.sortData.Item=item.ui end
    out[#out+1]=item
  end
  table.sort(out,function(a,b)
    -- Trade displays the contextual buy/sell/buyback quote; sort that actual price.
    if (mode.key=="PriceAsc" or mode.key=="PriceDesc") and a.price and b.price and a.price~=b.price then
      if mode.key=="PriceAsc" then return a.price<b.price end
      return a.price>b.price
    end
    if view:SortItemNew(a.sortData,b.sortData) then return true end
    if view:SortItemNew(b.sortData,a.sortData) then return false end
    return a.key<b.key
  end)
  return out
end
-- Same native comparator as apply(), but bounded work for large vendor inventories.
-- CET does not expose coroutines; an explicit merge state can stop between comparisons.
function S.begin(items,key)
  local mode=S.option(key);local view=BackpackDataView.new();view.itemSortMode=mode.mode
  local inventory=Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem"))
  local fastQuality=(mode.key=="QualityAsc" or mode.key=="QualityDesc") and type(UnicodeStringCompare)=="function"
  local function less(a,b)
    if fastQuality then
      -- BackpackDataView's quality order is favourite, integer quality, then
      -- UnicodeStringCompare(name). Read each item's keys once instead of
      -- allocating two native compare builders for every pair.
      local x,y=a.quickQuality,b.quickQuality
      if x.favorite~=y.favorite then return x.favorite end
      if x.quality~=y.quality then
        if mode.key=="QualityAsc" then return x.quality>y.quality end
        return x.quality<y.quality
      end
      local order=UnicodeStringCompare(x.name,y.name)
      if order~=0 then return order<0 end
      return a.key<b.key
    end
    if (mode.key=="PriceAsc" or mode.key=="PriceDesc") and a.price and b.price and a.price~=b.price then
      if mode.key=="PriceAsc" then return a.price<b.price end
      return a.price>b.price
    end
    if view:SortItemNew(a.sortData,b.sortData) then return true end
    if view:SortItemNew(b.sortData,a.sortData) then return false end
    return a.key<b.key
  end
  return {source=items,out={},prepare=1,inventory=inventory,less=less,fastQuality=fastQuality,width=1,left=1,dest={}}
end
function S.step(t,deadline)
  local n=#t.source
  for _=1,1024 do
    if t.prepare then
      local item=t.source[t.prepare]
      if item then
        if not item.ui then item.ui=t.inventory:GetPlayerItemFromAnySource(item.raw or InventoryItemData.GetGameItemData(item.data)) end
        if t.fastQuality then
          local favorite=item.favorite;if favorite==nil then favorite=item.ui:IsPlayerFavourite() end
          item.quickQuality={favorite=favorite,quality=item.qualityInt or item.ui:GetQualityInt(),name=item.name or item.ui:GetName()}
        elseif not item.sortData then item.sortData=WrappedInventoryItemData.new();item.sortData.Item=item.ui end
        t.out[#t.out+1]=item;t.prepare=t.prepare+1
      else t.prepare=nil end
    elseif t.width>=n then return t.out
    elseif t.left>n then
      t.out,t.dest=t.dest,{};t.width=t.width*2;t.left=1;t.i=nil
    else
      if not t.i then
        t.i=t.left;t.mid=math.min(t.left+t.width,n+1);t.j=t.mid
        t.right=math.min(t.left+2*t.width,n+1);t.k=t.left
      end
      if t.i<t.mid and (t.j>=t.right or t.less(t.out[t.i],t.out[t.j])) then
        t.dest[t.k]=t.out[t.i];t.i=t.i+1
      else t.dest[t.k]=t.out[t.j];t.j=t.j+1 end
      t.k=t.k+1
      if t.i>=t.mid and t.j>=t.right then t.left=t.left+2*t.width;t.i=nil end
    end
    if os.clock()>=deadline then break end
  end
end
local function arrow(p,x,y,direction,color)
  if direction==DropdownItemDirection.None then return {} end
  local sign=direction==DropdownItemDirection.Up and -1 or 1
  local function line(x1,y1,x2,y2)
    local dx,dy=x2-x1,y2-y1;local w=K.rect(p,x1,y1,math.sqrt(dx*dx+dy*dy),2,color,1)
    w:SetRenderTransformPivot(Vector2.new({X=0,Y=0}));w:SetRotation(math.deg(math.atan2(dy,dx)));return w
  end
  return {line(x,y-9,x,y+9),line(x-7,y+sign*2,x,y+sign*9),line(x,y+sign*9,x+7,y+sign*2)}
end
function S.control(kit,p,owner,key,x,y,w,value,set,redraw)
  local option=S.option(value);local open=owner.sortPopup and owner.sortPopup.key==key
  local text=K.text(p,x+28,y,w-54,42,K.clip(option.label,22),25,K.TEXT)
  local marks=arrow(p,x+12,y+21,option.direction,K.CYAN)
  local edge=K.rect(p,x+4,y+41,w-8,1,K.CYAN,.25)
  kit.chevron(p,x+w-13,y+21,6,2,open and "up" or "down",K.CYAN,.8)
  local b=kit.button(owner.sortPopup and "sort" or "p1",kit.at(kit.P1X,kit.P1Y),x,y,w,44,function()
    if open then owner.sortPopup=nil else owner.sortPopup={key=key} end
    redraw()
  end,function(h)
    local color=h and K.YELLOW or K.CYAN;text:SetTintColor(K.color(h and K.YELLOW or K.TEXT));K.tint(marks,color,1)
    edge:SetTintColor(K.color(color));edge:SetOpacity(h and .8 or .25)
  end);b.key=key;b.context=b.click -- Grip/right-click opens the sort options too
  if open then owner.sortPopup={key=key,x=x,y=y,w=w,value=option.key,set=set,redraw=redraw} end
end
function S.popup(kit,p,owner)
  local popup=owner.sortPopup;if not popup or not popup.set then return end
  kit.S.modalGroup="sort"
  kit.S.cfBack=function() owner.sortPopup=nil;popup.redraw() end
  local w,rowH=430,48;local h=#S.options()*rowH+12
  local x=math.max(12,math.min(1780-w-12,popup.x+popup.w-w))
  local y=math.min(popup.y+52,1195-h)
  local host=K.canvas(p,x,y,w,h)
  K.roundFill(host,0,0,w,h,K.BLACK,1,4);K.cutFrame(host,0,0,w,h,K.CYAN,.7,10,2)
  for n,entry in ipairs(S.options()) do
    local yy=6+(n-1)*rowH;local chosen=entry.key==popup.value
    local bg=K.roundFill(host,5,yy,w-10,rowH-3,chosen and K.YELLOW or K.NAVY,chosen and .17 or .18,3)
    local color=chosen and K.YELLOW or K.TEXT
    local marks=arrow(host,24,yy+rowH/2,entry.direction,color)
    local label=K.text(host,44,yy,w-60,rowH-3,entry.label,27,color)
    local b=kit.button("sort",kit.at(kit.P1X+x,kit.P1Y+y),5,yy,w-10,rowH-3,function()
      owner.sortPopup=nil;popup.set(entry.key);popup.redraw()
    end,function(hov)
      bg:SetTintColor(K.color((hov or chosen) and K.YELLOW or K.NAVY));bg:SetOpacity(hov and .22 or chosen and .17 or .18)
      local c=hov and K.YELLOW or color;label:SetTintColor(K.color(c));K.tint(marks,c,1)
    end);b.key=popup.key..":"..entry.key
  end
end
return S
