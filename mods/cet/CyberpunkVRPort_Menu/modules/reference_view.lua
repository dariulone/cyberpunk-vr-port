-- Native hub presentation adapted to the SAO panel's world-space ink canvas.
local K=require("modules/ink")
local L=require("modules/locale")
local D=require("modules/gameplay_data")
local R=require("modules/reference_data")
local Pref=require("modules/preferences")
return function(C)
local V={}
local W=1780
local function tr(kit,ru,en) return L.tr(ru,en) end
local function line(p,x1,y1,x2,y2,color,alpha,width)
  local dx,dy=x2-x1,y2-y1
  local q=K.rect(p,x1,y1,math.sqrt(dx*dx+dy*dy),width or 3,color,alpha or 1)
  q:SetRenderTransformPivot(Vector2.new({X=0,Y=0}))
  q:SetRotation(math.deg(math.atan2(dy,dx)))
  return q
end
local function hex(p,x,y,w,h,color,alpha)
  local cut=math.min(w*.18,h*.5)
  local points={{x+cut,y},{x+w-cut,y},{x+w,y+h/2},{x+w-cut,y+h},{x+cut,y+h},{x,y+h/2}}
  local edges={}
  for i,a in ipairs(points) do local b=points[i%6+1];edges[#edges+1]=line(p,a[1],a[2],b[1],b[2],color,alpha,3) end
  return edges
end
-- How far a ray from the centre of a hex drawn by hex() (w x h) runs before it meets the outline.
local function hexReach(w,h,dx,dy)
  local cut=math.min(w*.18,h*.5)
  local points={{-w/2+cut,-h/2},{w/2-cut,-h/2},{w/2,0},{w/2-cut,h/2},{-w/2+cut,h/2},{-w/2,0}}
  local best=math.huge
  for i,a in ipairs(points) do
    local b=points[i%6+1];local ex,ey=b[1]-a[1],b[2]-a[2]
    local den=dx*ey-dy*ex
    if math.abs(den)>1e-9 then
      local t,u=(a[1]*ey-a[2]*ex)/den,(a[1]*dy-a[2]*dx)/den
      if t>0 and u>=0 and u<=1 then best=math.min(best,t) end
    end
  end
  return best
end
local function icon(p,x,y,w,h,image,color)
  if image then return K.image(p,x,y,w,h,image.atlas,image.part,color or K.CYAN,1) end
end
local function picture(kit,p,x,y,w,h,image,view,item)
  if not image then return end
  local s=C.state(kit)
  local pic=K.image(p,x,y,1,1,image.atlas,image.part,K.WHITE,.001)
  pic:SetFitToContent(true);pic:SetContentHAlign(inkEHorizontalAlign.Center);pic:SetContentVAlign(inkEVerticalAlign.Center)
  pic:SetRenderTransformPivot(Vector2.new({X=0,Y=0}));pic:SetScale(Vector2.new({X=.001,Y=.001}))
  s.images[#s.images+1]={widget=pic,view=view,item=item,w=w,h=h,x=x,y=y,noHit=view==nil}
end
local function action(kit,p,sx,x,y,w,label,fn,enabled,h,fs)
  h=h or 68
  local bg=K.roundFill(p,x,y,w,h,K.NAVY,enabled==false and .035 or .18,4)
  local edge=K.cutFrame(p,x,y,w,h,K.CYAN,enabled==false and .18 or .45,9,1.5)
  local text=K.text(p,x,y,w,h,label,fs or 32,enabled==false and K.GREY or K.TEXT,"center")
  return kit.button("p1",kit.at(sx,kit.P1Y),x,y,w,h,function()
    if enabled~=false then fn() else kit.toast(kit.T.blocked) end
  end,function(h)
    if enabled==false then return end
    bg:SetTintColor(K.color(h and K.YELLOW or K.NAVY));bg:SetOpacity(h and .16 or .18)
    K.tint(edge,h and K.YELLOW or K.CYAN,h and 1 or .45);text:SetTintColor(K.color(h and K.YELLOW or K.TEXT))
  end)
end
-- A small button on a panel's title line, its right edge at x1: low enough and far enough in to keep its
-- frame inside the glass, which sits inset from the panel's edge (menu.lua glassRect); callers keep x1 left of
-- the frame's corner mark (panel width - 72).
local HEAD_Y,HEAD_BH,HEAD_FS=22,48,28
local function headAction(kit,p,sx,x1,label,fn,enabled,w)
  if not w then
    -- characters, not bytes: UTF-8 continuation bytes do not count
    local _,cont=string.gsub(label,"[\128-\191]","")
    w=math.floor((#label-cont)*HEAD_FS*.56)+48
  end
  return action(kit,p,sx,x1-w,HEAD_Y,w,label,fn,enabled,HEAD_BH,HEAD_FS)
end
local function title(p,text,w)
  K.text(p,28,12,w-56,60,K.clip(text,49),42,K.WHITE)
  K.rect(p,28,82,w-56,1,K.CYAN,.32)
end
local function stopVideos(s)
  for _,video in ipairs(s.videos or {}) do pcall(function() video:Stop() end) end
  s.videos={}
end
V.stopVideos=stopVideos

function V.skills(kit,p,d)
  local s=C.state(kit)
  kit.glass(p,W,1140,kit.P1X,kit.P1Y);title(p,tr(kit,"РАЗВИТИЕ НАВЫКОВ","SKILL PROGRESSION"),W)
  headAction(kit,p,kit.P1X,W-72,tr(kit,"НАЗАД","BACK"),function() s.skills=false;C.redraw(kit) end)
  local info=K.text(p,30,1010,W-60,88,tr(kit,"Наведите на награду, чтобы прочитать её эффект","Point at a reward to read its effect"),31,K.CYAN)
  for i,skill in ipairs(d.skills) do
    local y=130+(i-1)*165
    local ratio=skill.level>=skill.max and 1 or skill.xp/math.max(1,skill.xp+skill.remaining)
    K.text(p,30,y,420,48,skill.level.."  "..skill.name,36,K.WHITE)
    K.rect(p,30,y+62,400,4,K.ICE,.2);K.rect(p,30,y+62,400*ratio,4,K.CYAN,1)
    K.text(p,30,y+76,400,35,skill.xp.." / "..(skill.xp+skill.remaining).."   "..math.floor(ratio*100).."%",26,K.GREY,"right")
    for j,reward in ipairs(skill.rewards) do
      local x=470+(j-1)*105
      local col=reward.unlocked and K.YELLOW or K.RED
      local bg=K.roundFill(p,x,y+8,92,94,col,reward.unlocked and .12 or .035,8)
      K.roundEdge(p,x,y+8,92,94,col,.55,8)
      K.text(p,x,y+12,92,40,tostring(reward.level),31,col,"center")
      K.text(p,x,y+54,92,40,"+",33,col,"center")
      kit.button("p1",kit.at(kit.P1X,kit.P1Y),x,y+8,92,94,function()
        info:SetText(table.concat(C.wrap(skill.name.." / "..reward.level..": "..reward.text,95),"\n"))
      end,function(h)
        bg:SetOpacity(h and .25 or .1)
        if h then info:SetText(table.concat(C.wrap(skill.name.." / "..reward.level..": "..reward.text,95),"\n")) end
      end)
    end
  end
end

-- Slots are native tree positions, and every link comes from RequiresPerks, including OR branches.
local nativeSlots=require("modules/perk_layout")
local function slotPosition(perk,index,width)
  local position=assert(nativeSlots[perk.slot], "Unknown native perk slot: "..tostring(perk.slot))
  local scale=math.min((width-140)/2808,850/1532)
  local left=(width-2808*scale)/2
  return left+(position[1]-516)*scale,240+(position[2]-421)*scale,math.max(45,math.min(position[3],position[4])*scale)
end
local function nodeFrame(p,x,y,size,color,opacity,inset)
  local r=size/2;local cut=size*.21;inset=inset or 0
  local pts={{x-r+cut,y-r},{x+r-cut,y-r},{x+r,y-r+cut},{x+r,y+r-cut},{x+r-cut,y+r},{x-r+cut,y+r},{x-r,y+r-cut},{x-r,y-r+cut}}
  local parts={}
  for i,a in ipairs(pts) do local b=pts[i%8+1];parts[#parts+1]=line(p,a[1],a[2],b[1],b[2],color,opacity,inset>0 and inset or 3) end
  return parts
end
local function wire(p,from,to,color,opacity)
  local points
  if math.abs(from.x-to.x)<3 or math.abs(from.y-to.y)<12 then points={{from.x,from.y},{to.x,to.y}}
  else
    local middle=(from.y+to.y)/2;local direction=to.x>from.x and 1 or -1
    local vertical=to.y>from.y and 1 or -1;local bend=math.min(15,math.abs(to.x-from.x)/4)
    points={{from.x,from.y},{from.x,middle-vertical*bend},{from.x+direction*bend,middle},{to.x-direction*bend,middle},{to.x,middle+vertical*bend},{to.x,to.y}}
  end
  for offset=-2,2,2 do for i=1,#points-1 do local a,b=points[i],points[i+1];line(p,a[1]+offset,a[2],b[1]+offset,b[2],color,opacity,1) end end
end

local function buyAttribute(kit,kind)
  C.perform(kit,function()
    if not D.buyAttribute(kind) then return false end
    if not Pref.attributeUpgradeConfirmed then Pref.confirmAttributeUpgrade() end
    return true
  end)
end

local function confirmAttribute(kit,s,t,prompt)
  local a=prompt.attribute
  local w,h=1160,380
  local x,y=(kit.CW-w)/2,(kit.CH-h)/2
  local card=K.canvas(kit.S.layerPanel,x,y,w,h)
  local origin=kit.at(x,y)
  kit.S.modalGroup="attributePrompt"
  K.roundFill(card,0,0,w,h,K.NAVY,.94,12)
  kit.glass(card,w,h,x,y)
  K.text(card,40,26,w-80,64,t.upgradeQuestion,46,K.WHITE,"center")
  K.text(card,40,96,w-80,64,a.name.."   "..a.value.." → "..(a.value+1),42,K.CYAN,"center")
  K.text(card,40,166,w-80,90,table.concat(C.wrap(t.upgradeHint,54),"\n"),32,K.TEXT,"center")
  local function resolve(accept)
    if s.attributePrompt~=prompt then return end
    s.attributePrompt=nil
    if accept then buyAttribute(kit,a.type) else C.redraw(kit) end
  end
  kit.action(card,"attributePrompt",origin,68,272,490,76,t.yes,true,function() resolve(true) end)
  kit.action(card,"attributePrompt",origin,w-558,272,490,76,t.cancel,true,function() resolve(false) end)
end

function V.tree(kit,p,p2,a,d)
  local s,t=C.state(kit)
  if s.attributePrompt then return confirmAttribute(kit,s,t,s.attributePrompt) end
  local hasDetails=s.perk~=nil
  local treeW,detailX,detailW=hasDetails and 1050 or W,kit.P1X+1090,690
  K.move(p2,detailX,kit.P1Y)
  kit.glass(p,treeW,1190,kit.P1X,kit.P1Y);title(p,a.name.."  "..(a.relic and d.relic or a.value),treeW)
  K.text(p,375,16,650,48,t.perkPoints..": "..d.perks.."  /  "..t.relic..": "..d.relic,26,K.YELLOW,"right")
  action(kit,p,kit.P1X,20,100,480,hasDetails and tr(kit,"< ВЕТКА","< TREE") or tr(kit,"ХАРАКТЕРИСТИКИ","ATTRIBUTES"),function()
    if hasDetails then s.perk=nil else s.attribute=nil end
    s.confirm=nil;C.redraw(kit)
  end)
  if a.type~=gamedataStatType.Espionage then
    action(kit,p,kit.P1X,520,100,132,"+1",function()
      if Pref.attributeUpgradeConfirmed then
        buyAttribute(kit,a.type)
      else
        s.confirm=nil
        s.attributePrompt={attribute=a}
        C.redraw(kit)
      end
    end,a.canBuy,68,44)
    K.text(p,672,100,treeW-690,68,t.points..": "..d.points,28,K.CYAN)
  end
  -- kept between clicks (a read is ~10 ms); gameplay.lua drops it with the character read
  local perks=C.fetch(kit,"perks:"..tostring(a.type),function() return function() return true,R.perks(a) end end,math.huge,.05) or {}
  local positions={};local selected
  for i,perk in ipairs(perks) do
    local x,y,size=slotPosition(perk,i,treeW);positions[perk.key]={x=x,y=y,size=size,perk=perk}
    if perk.key==s.perk then selected=perk end
  end
  -- The wires, frames, icons and badges are ~1100 widgets (~35 ms of ink calls): kept while nothing they
  -- show changes, so a click only re-tints the nodes (ink.lua K.kept).
  local sig={a.name,treeW,tostring(a.relic)}
  for _,perk in ipairs(perks) do sig[#sig+1]=perk.key..":"..perk.level.."/"..perk.max..(perk.locked and "L" or "") end
  local key="tree:"..table.concat(sig,"|");local slot=tostring(a.type)..":"..treeW
  s.treeKeys=s.treeKeys or {}
  if s.treeKeys[slot] and s.treeKeys[slot]~=key then K.poolDrop(s.treeKeys[slot]) end
  s.treeKeys[slot]=key
  local nodes,reused=K.kept(key,p,0,0,treeW,1190,function(c)
    for _,perk in ipairs(perks) do
      local to=positions[perk.key]
      for _,parent in ipairs(perk.parents) do
        local from=positions[parent]
        if from then
          local col=from.perk.level>=from.perk.max and K.YELLOW or K.RED
          wire(c,from,to,col,perk.level>0 and .9 or .25)
        end
      end
    end
    local out={}
    for _,perk in ipairs(perks) do
      local pos=positions[perk.key];local x,y,size=pos.x,pos.y,pos.size
      local col=perk.level>0 and (a.relic and {0.35,0.55,1.0} or K.YELLOW) or (perk.locked and K.RED or K.CYAN)
      local bg=K.roundFill(c,x-size/2,y-size/2,size,size,K.BLACK,.82,12)
      if perk.level>0 then nodeFrame(c,x,y,size+14,col,.12,10) end
      local edges=nodeFrame(c,x,y,size+4,col,perk.locked and .25 or 1)
      if size>70 then nodeFrame(c,x,y,size+16,col,.3) end
      icon(c,x-size*.36,y-size*.36,size*.72,size*.72,perk.icon,col)
      if perk.slot:find("Milestone",1,true) or perk.max>1 then
        K.roundFill(c,x-26,y+size/2-11,52,27,col,perk.level>0 and .8 or .12,3)
        K.text(c,x-26,y+size/2-11,52,27,perk.level.."/"..perk.max,21,perk.level>0 and K.BLACK or col,"center")
      end
      out[perk.key]={bg=bg,edges=edges,col=col,rest=perk.locked and .25 or 1}
    end
    return out
  end)
  for _,perk in ipairs(perks) do
    local pos=positions[perk.key];local x,y,size=pos.x,pos.y,pos.size;local n=nodes[perk.key]
    -- a kept tree may carry the last view's hover or selection (only those nodes are reset)
    if reused and n.dirty then K.tint(n.edges,n.col,n.rest);n.bg:SetTintColor(K.color(K.BLACK));n.bg:SetOpacity(.82);n.dirty=false end
    local b=kit.button("p1",kit.at(kit.P1X,kit.P1Y),x-size/2-6,y-size/2-6,size+12,size+12,function()
      s.perk=perk.key;s.confirm=nil;C.redraw(kit)
    end,function(h)
      K.tint(n.edges,h and K.WHITE or n.col,h and 1 or n.rest)
      n.bg:SetTintColor(K.color(h and K.CYAN or K.BLACK));n.bg:SetOpacity(h and .22 or .82)
      n.dirty=h and true or false
    end)
    b.contains=function(px,py)
      local w=size+12;local cut=w*.21
      return px+py>=cut and px+w-py>=cut and w-px+py>=cut and 2*w-px-py>=cut
    end
    b.key=perk.key
    if s.perk==perk.key then K.tint(n.edges,K.WHITE,1);n.dirty=true end
  end
  s.perkCount=#perks;s.treePositions=positions
  if not a.relic then
    local scale=math.min((treeW-140)/2808,850/1532)
    for _,tier in ipairs({{20,580.5,"МАСТЕР","MASTER"},{15,1011,"ЭКСПЕРТ","EXPERT"},{9,1457,"ПРОФИ","PRO"},{4,1841,"НОВИЧОК","ROOKIE"}}) do
      local y=240+(tier[2]-421)*scale;local locked=a.value<tier[1]
      K.text(p,treeW-135,y-24,115,32,tr(kit,tier[3],tier[4]),18,locked and K.RED or K.CYAN,"right")
      if locked then K.text(p,treeW-135,y+10,115,28,tostring(a.value).." / "..tier[1],18,K.RED,"right") end
    end
    for _,gate in ipairs({{9,1650},{15,1234},{20,795}}) do
      if a.value<gate[1] then
        local y=240+(gate[2]-421)*scale
        line(p,20,y,treeW-20,y,K.RED,.7,2)
        K.roundEdge(p,treeW/2-9,y-12,18,23,K.RED,1,3)
        break
      end
    end
  end
  if not selected then return end
  kit.glass(p2,detailW,1190,detailX,kit.P1Y)
  title(p2,selected.name,detailW)
  local rows={};local top=100
  if ResRef.IsValid(selected.video) then
    local video=inkVideoWidget.new();video:SetAnchor(inkEAnchor.TopLeft);video:SetAnchorPoint(Vector2.new({X=0,Y=0}))
    video:SetMargin(inkMargin.new({left=20,top=100,right=0,bottom=0}));video:SetSize(Vector2.new({X=detailW-40,Y=(detailW-40)*9/16}))
    video:Reparent(p2);video:SetVideoPath(selected.video);video:SetLoop(true);video:Play()
    s.videos[#s.videos+1]=video;top=480
  end
  C.textRows(kit,rows,selected.name,detailW,K.CYAN,34)
  C.textRows(kit,rows,t.requires..": "..a.name.." "..selected.required.."  /  "..t.cost..": "..selected.cost,detailW,K.YELLOW,28)
  for _,level in ipairs(D.perkDescription(selected)) do
    C.textRows(kit,rows,t.level.." "..level.level.."\n"..level.text.."\n",detailW,level.level<=selected.level and K.YELLOW or K.TEXT,30)
  end
  C.list(kit,p2,"p2","p2",detailX,top,detailW,rows,1040-top)
  if s.confirm then
    action(kit,p2,detailX,18,1110,360,t.confirm,function() C.perform(kit,function() return D.changePerk(a,selected.type,s.confirm=="refund") end) end)
    action(kit,p2,detailX,392,1110,280,t.cancel,function() s.confirm=nil;C.redraw(kit) end)
  else
    action(kit,p2,detailX,18,1110,300,t.buy,function() s.confirm="buy";C.redraw(kit) end,selected.canBuy)
    action(kit,p2,detailX,336,1110,336,t.refund,function() s.confirm="refund";C.redraw(kit) end,selected.canSell)
  end
end

function V.character(kit,p,p2)
  -- Kept between clicks (a read is ~4.5 ms); gameplay.lua drops it when the character's points change.
  local s,t=C.state(kit)
  local d=C.fetch(kit,"character",function() return function() return true,R.character() end end,math.huge,.05)
  if not d then return end
  s.snapshot=d
  if s.skills then return V.skills(kit,p,d) end
  local chosen
  for _,a in ipairs(d.attributes) do if tostring(a.type)==s.attribute then chosen=a end end
  if chosen then return V.tree(kit,p,p2,chosen,d) end
  kit.glass(p,820,1140,kit.P1X,kit.P1Y);title(p,t.character,820)
  for i,counter in ipairs({{t.attributes,d.points,K.CYAN},{t.perks,d.perks,K.YELLOW},{t.relic,d.relic,K.CYAN}}) do
    local x=22+(i-1)*266
    K.text(p,x,96,244,30,counter[1],22,counter[3],"center")
    K.text(p,x,126,244,42,tostring(counter[2]),34,counter[3],"center")
  end
  local places={Strength={190,380},Reflexes={410,255},TechnicalAbility={630,380},Intelligence={190,770},Cool={630,770},Espionage={410,915}}
  local detail=d.attributes[1]
  for _,a in ipairs(d.attributes) do if tostring(a.type)==s.attributeHover then detail=a end end
  kit.glass(p2,900,1140,kit.P2X,kit.P1Y)
  -- the title line as title() draws it, so both panels share their margins
  local detailTitle=K.text(p2,28,12,844,60,K.clip(detail.name,40),42,K.CYAN)
  K.rect(p2,28,82,844,1,K.CYAN,.32)
  local detailHost=K.canvas(p2,0,96,900,1000)
  local function show(a)
    detailTitle:SetText(K.clip(a.name,40));detailHost:RemoveAllChildren()
    icon(detailHost,28,18,100,100,a.icon,K.CYAN)
    K.text(detailHost,152,35,692,65,a.relic and t.relic or (t.level.." "..a.value.." / "..a.max),36,K.CYAN)
    local lines=C.wrap(a.description,47)
    K.text(detailHost,28,146,844,math.min(750,#lines*43),table.concat(lines,"\n"),32,K.TEXT)
    K.text(detailHost,28,935,844,48,tr(kit,"НАЖМИТЕ, ЧТОБЫ ОТКРЫТЬ ВЕТКУ","CLICK TO OPEN THE PERK TREE"),30,K.CYAN)
  end
  show(detail)
  local center={410,575}
  for _,a in ipairs(d.attributes) do
    local name=tostring(a.type):match(": (%w+)");local xy=places[name]
    if xy then
      -- the link runs between the two outlines, a little short of each
      local dx,dy=xy[1]-center[1],xy[2]-center[2];local len=math.sqrt(dx*dx+dy*dy);dx,dy=dx/len,dy/len
      local a,b=hexReach(160,160,dx,dy)+10,len-hexReach(290,164,-dx,-dy)-10
      if b>a then line(p,center[1]+dx*a,center[2]+dy*a,center[1]+dx*b,center[2]+dy*b,K.CYAN,.23,3) end
    end
  end
  hex(p,330,495,160,160,K.CYAN,.8);K.text(p,330,520,160,70,tostring(d.level),54,K.CYAN,"center")
  K.text(p,330,587,160,40,t.level,28,K.CYAN,"center")
  for i,a in ipairs(d.attributes) do
    local name=tostring(a.type):match(": (%w+)");local xy=places[name]
    if xy then
      local x,y=xy[1]-145,xy[2]-82
      local c=K.canvas(p,x,y,290,164)
      local fill=K.roundFill(c,24,0,242,164,K.NAVY,.18,8)
      local edge=hex(c,0,0,290,164,K.CYAN,.55)
      K.text(c,20,12,250,44,a.name,28,K.CYAN,"center")
      icon(c,92,70,56,56,a.icon,K.CYAN)
      K.text(c,155,72,100,60,a.relic and "R" or tostring(a.value),40,K.CYAN)
      local b=kit.button("p1",kit.at(kit.P1X,kit.P1Y),x,y,290,164,function()
        s.attribute=tostring(a.type);s.perk=nil;s.confirm=nil;s.skills=false;C.redraw(kit)
      end,function(h)
        K.tint(edge,h and K.YELLOW or K.CYAN,h and 1 or .55);fill:SetOpacity(h and .18 or .06)
        if h then s.attributeHover=tostring(a.type);show(a) end
      end)
      b.contains=function(px,py) return px>=math.abs(py-82)*.35 and px<=290-math.abs(py-82)*.35 end
      if kit.S.anim1 then kit.slideIn("p1",c,x,y,(i-1)*.025,0,26) end
    end
  end
  action(kit,p,kit.P1X,25,1050,770,tr(kit,"РАЗВИТИЕ НАВЫКОВ","SKILL PROGRESSION"),function() s.skills=true;C.redraw(kit) end)
end

function V.tarot(kit,p,p2)
  local s=C.state(kit);local cards,count,total=R.tarot();s.tarotCount=count
  local selected
  for _,card in ipairs(cards) do if card.index==s.tarot then selected=card end end
  local collectionW=1280
  local panelW=selected and selected.found and W or collectionW
  kit.glass(p,panelW,1305,kit.P1X,kit.P1Y);title(p,tr(kit,"КАРТЫ ТАРО","TAROT CARDS").."  "..count.." / "..total,panelW)
  if selected and selected.found then
    action(kit,p,kit.P1X,20,100,280,tr(kit,"К КОЛЛЕКЦИИ","COLLECTION"),function() s.tarot=nil;C.redraw(kit) end)
    picture(kit,p,80,200,530,900,selected.image)
    K.text(p,700,125,1000,66,selected.name,42,K.CYAN)
    local rows={};C.textRows(kit,rows,selected.description,1000,K.TEXT,34)
    local host=K.canvas(p,680,0,1040,1100)
    C.list(kit,host,"p2","p2",kit.P1X+680,220,1040,rows,820)
    return
  end
  local rows={}
  for first=1,#cards,5 do
    local group={};for i=first,math.min(#cards,first+4) do group[#group+1]=cards[i] end
    rows[#rows+1]={h=450,build=function(c,it,v)
      for i,card in ipairs(group) do
        local x=7+(i-1)*248
        local edge=K.roundEdge(c,x,5,228,440,card.found and K.CYAN or K.GREY,.6,8)
        picture(kit,c,x+14,11,200,400,card.image)
        K.text(c,x+8,411,212,30,card.found and K.clip(card.name,19) or "?",24,card.found and K.CYAN or K.GREY,"center")
        kit.button("p1",kit.at(v.x,it.y,v),x,5,228,440,function()
          if card.found then s.tarotOffset=v.offset;s.tarot=card.index;C.redraw(kit) end
        end,function(h) K.tint(edge,card.found and K.CYAN or K.GREY,h and 1 or .6) end)
      end
    end}
  end
  local view=C.list(kit,p,"p1","p1",kit.P1X,110,collectionW,rows,1125,s.tarotOffset or kit.S.keepScroll)
  view:enableMask()
  local onScroll=view.onScroll
  view.onScroll=function(v) s.tarotOffset=v.offset;if onScroll then onScroll(v) end end
end

local function comparison(kit,p,height,enabled,toggle,key)
  local label=enabled and tr(kit,"СРАВНЕНИЕ: ВКЛ","COMPARE: ON") or tr(kit,"СРАВНЕНИЕ: ВЫКЛ","COMPARE: OFF")
  local b=action(kit,p,kit.P1X,24,height-95,430,label,toggle,true)
  b.key=key
  return b
end
local helpers={line=line,picture=picture,action=action,headAction=headAction,title=title,comparison=comparison}
V.summary=require("modules/summary_view")(C,helpers)
local Journal=require("modules/journal_view")(C,helpers)
V.journal=Journal.journal
V.shards=Journal.shards
V.timeskip=require("modules/wait_view")(C,helpers)
local Inventory=require("modules/inventory_view")(C,helpers)
V.inventory=Inventory.draw
V.inventoryUpdate=Inventory.update
V.inventoryLeave=Inventory.leave
local Cyberware=require("modules/cyberware_view")(C,helpers)
V.cyberware=Cyberware.draw
V.cyberwareUpdate=Cyberware.update
V.cyberwareLeave=Cyberware.leave
V.cyberwareBeforeBuild=Cyberware.beforeBuild
local Crafting=require("modules/crafting_view")(C,helpers)
V.crafting=Crafting.draw
V.craftingUpdate=Crafting.update
V.craftingLeave=Crafting.leave
local Trade=require("modules/trade_view")(C,helpers)
V.trade=Trade.draw
V.tradeUpdate=Trade.update
V.tradeLeave=Trade.leave
V.tradeBeforeBuild=Trade.beforeBuild
local Stash=require("modules/stash_view")(C,helpers)
V.stash=Stash.draw
V.stashUpdate=Stash.update
V.stashLeave=Stash.leave
V.stashBeforeBuild=Stash.beforeBuild
local Load=require("modules/load_view")(C,helpers)
V.load=Load.draw
V.loadUpdate=Load.update
V.loadLeave=Load.leave
V.loadBeforeBuild=Load.beforeBuild
function V.libraryNav(kit,p)
  headAction(kit,p,kit.P1X,820-72,tr(kit,"ЧИПЫ","SHARDS"),function() kit.openSection("shards") end)
end

return V
end
