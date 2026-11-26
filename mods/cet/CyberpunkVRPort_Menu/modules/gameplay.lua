local K = require("modules/ink")
local L=require("modules/locale")
local D = require("modules/gameplay_data")
local Jobs = require("modules/jobs")
local R = require("modules/reference_data")
local G = {}
local Reference
local WIDTH, DETAIL = 820, 900
local LABELS = {
  en = { character="CHARACTER", journal="JOURNAL", codex="DATABASE", timeskip="WAIT",
    attributes="Attributes", perks="Perks", skills="Skills", level="Level", points="Attribute points",
    perkPoints="Perk points", relic="Relic", upgrade="UPGRADE +1", buy="LEARN", refund="REFUND",
    confirm="CONFIRM", cancel="CANCEL", yes="YES", back="BACK", requires="Requires", cost="Cost",
    upgradeQuestion="Spend 1 attribute point?", upgradeHint="Future +1 clicks spend points immediately.",
    active="Active", completed="Completed", failed="Failed", all="All", track="TRACK OBJECTIVE",
    tracked="Tracked", optional="Optional", choose="Select an entry", empty="No entries",
    readError="Unable to read game data", retry="RETRY", skip="SKIP", hours="hours", now="Now",
    target="After skipping", tomorrow="Next day", blocked="Unavailable right now", done="Done",
    characters="Characters", locations="Locations", lore="World", reset="RESET FILTER", xp="XP",
    spend="Spend points?", pending="Applying…", closeDetails="ATTRIBUTES", ability="Abilities" },
  ru = { character="ПЕРСОНАЖ", journal="ЖУРНАЛ", codex="БАЗА ДАННЫХ", timeskip="ОЖИДАНИЕ",
    attributes="Характеристики", perks="Способности", skills="Навыки", level="Уровень", points="Очки характеристик",
    perkPoints="Очки способностей", relic="Биочип", upgrade="ПОВЫСИТЬ +1", buy="ИЗУЧИТЬ", refund="ВЕРНУТЬ ОЧКИ",
    confirm="ПОДТВЕРДИТЬ", cancel="ОТМЕНА", yes="ДА", back="НАЗАД", requires="Требуется", cost="Стоимость",
    upgradeQuestion="Потратить 1 очко характеристики?", upgradeHint="В дальнейшем нажатие +1 сразу потратит очко.",
    active="Активные", completed="Завершённые", failed="Проваленные", all="Все", track="ОТСЛЕЖИВАТЬ ЦЕЛЬ",
    tracked="Отслеживается", optional="Необязательно", choose="Выберите запись", empty="Нет записей",
    readError="Не удалось прочитать данные игры", retry="ПОВТОРИТЬ", skip="ПОДОЖДАТЬ", hours="ч.", now="Сейчас",
    target="После ожидания", tomorrow="Следующий день", blocked="Сейчас недоступно", done="Готово",
    characters="Персонажи", locations="Места", lore="Мир", reset="СБРОСИТЬ ФИЛЬТР", xp="Опыт",
    spend="Потратить очки?", pending="Применение…", closeDetails="ХАРАКТЕРИСТИКИ", ability="Способности" },
}

local function state(kit)
  kit.S.gameplay = kit.S.gameplay or { hours = 1, questFilter = "active", codexFilter = 0 }
  return kit.S.gameplay, L.labels(LABELS.en, LABELS.ru)
end

local function clean(text)
  return (tostring(text or ""):gsub("\\n", "\n"):gsub("<[^>]*>", ""):gsub("\r", ""))
end
local function length(s) local _, n = s:gsub("[^\128-\191]", ""); return n end
local function wrap(text, count)
  local lines = {}
  for para in (clean(text) .. "\n"):gmatch("(.-)\n") do
    local line = ""
    for token in para:gmatch("%S+") do
      local word = token
      if line ~= "" and length(line) + length(word) + 1 > count then lines[#lines+1] = line; line = "" end
      -- Long URLs/identifiers must not bleed into the neighbouring panel.
      while length(word) > count do
        if line ~= "" then lines[#lines+1] = line; line = "" end
        -- Split at a UTF-8 character boundary, without K.clip's ellipsis.
        local pos, n = 1, 0
        for at in word:gmatch("()[^\128-\191]") do n=n+1; if n==count+1 then pos=at; break end end
        if pos == 1 then break end
        lines[#lines+1] = word:sub(1,pos-1); word=word:sub(pos)
      end
      line = line == "" and word or line .. " " .. word
    end
    lines[#lines+1] = line
  end
  return lines
end

local function textRows(kit, rows, text, width, color, size)
  size = size or 34
  for _, line in ipairs(wrap(text, math.floor((width - 64) / (size * .56)))) do
    rows[#rows+1] = { h = size + 12, build = function(c, it, v)
      K.text(c, 22, 0, v.cw-44, size+12, line, size, color or K.TEXT)
      -- The visible glass/text row can be dragged to scroll, but does not trigger an action.
      if line ~= "" then kit.button("p2", kit.at(v.x, it.y, v), 0, 0, v.cw, size+12, nil, nil) end
    end }
  end
end

local function row(kit, group, label, sub, click, selected, color)
  return { h = 110, build = function(c, it, v)
    local bg = K.roundFill(c, 0, 0, v.cw, 96, K.ICE, .07, 20)
    K.text(c, 24, 2, v.cw-48, 53, K.clip(clean(label), 31), 38, color or K.TEXT)
    K.text(c, 24, 52, v.cw-48, 35, K.clip(clean(sub), 42), 28, K.GREY)
    kit.button(group, kit.at(v.x, it.y, v), 0, 0, v.cw, 96, click, function(hover)
      bg:SetTintColor(K.color(hover and K.CYAN or selected and K.RED or K.ICE))
      bg:SetOpacity(hover and .25 or selected and .20 or .07)
    end)
    if selected then bg:SetTintColor(K.color(K.RED)); bg:SetOpacity(.20) end
  end }
end

local function button(kit,p,x,y,w,label,enabled,fn,group,sx)
  return kit.action(p,group or "p1",kit.at(sx or kit.P1X,kit.P1Y),x,y,w,76,label,enabled,fn,K.CYAN)
end
local function heading(kit,p,w,title,sx)
  kit.glass(p,w,1160,sx,kit.P1Y)
  K.text(p,32,8,w-64,76,K.clip(title,32),42,K.WHITE)
  K.rect(p,24,90,w-48,2,K.ICE,.2)
end
local function list(kit,p,group,key,sx,top,w,rows,height,keep)
  -- While its data is still being read a list stays empty: no "no entries" in the meantime.
  if #rows==0 and not state(kit).reading then rows={row(kit,group,L.labels(LABELS.en, LABELS.ru).empty,"",nil)} end
  return kit.list(p,group,key,sx,kit.P1Y,top,w-28,rows,height,keep)
end
local function redraw(kit) kit.rebuild(false,false) end

-- The data a panel draws, read by a background job (modules/jobs.lua): the last result at once, with a
-- refresh queued behind it once it is `period` seconds old; the first time, the read finished now if it
-- fits `budget`, else the panel draws without it (s.reading) and redraws when it lands. G.update
-- redraws when a watched key's result changes (opts.changed handles it instead, when given).
local function fetch(kit,key,producer,period,budget,opts)
  local s=state(kit)
  local value,rec=Jobs.get(key)
  if value==nil and opts and opts.background then Jobs.run(key,producer)
  elseif value==nil then
    Jobs.run(key,producer,true)
    if Jobs.finish(key,budget or .012) then value,rec=Jobs.get(key) else s.reading=true end
  elseif Jobs.age(key)>(period or 2) then Jobs.run(key,producer,true) end
  s.watch=s.watch or {}
  s.watch[key]={producer=producer,period=period or 2,seq=rec and rec.seq,changed=opts and opts.changed}
  return value
end
G.fetch=fetch

local function perform(kit,fn)
  local s,t=state(kit)
  local ok,result=pcall(fn)
  s.confirm=nil
  -- tracking an objective changes the quest's rows: read them again behind the toast
  if kit.S.section=="journal" then
    local hash=s.quest
    if hash then Jobs.run("quest:"..tostring(hash),function() return R.questDetail(hash) end,true) end
    Jobs.run("quests",R.questList,true)
  end
  if kit.S.section=="character" then Jobs.drop("character");Jobs.dropPrefix("perks:") end
  if not ok then print("[VRMenu] action: "..tostring(result)) end
  kit.toast(ok and result and t.pending or t.blocked)
  s.pendingUntil=kit.S.time+(kit.S.section=="timeskip" and 3 or .35)
  kit.S.refreshAt=s.pendingUntil
  redraw(kit)
end
local function filters(kit,p,items,current,change)
  local w=(WIDTH-40-(#items-1)*12)/#items
  for i,item in ipairs(items) do
    local x=20+(i-1)*(w+12)
    local bg=K.roundFill(p,x,108,w,60,item[1]==current and K.CYAN or K.ICE,.14,20)
    K.text(p,x,108,w,60,item[2],26,item[1]==current and K.CYAN or K.TEXT,"center")
    kit.button("p1",kit.at(kit.P1X,kit.P1Y),x,108,w,60,function() change(item[1]) end,function(h) bg:SetOpacity(h and .3 or .14) end)
  end
end

function G.codex(kit,p,p2)
  local s,t=state(kit)
  heading(kit,p,WIDTH,t.codex,kit.P1X)
  filters(kit,p,{{0,t.all},{1,t.lore},{2,t.characters},{3,t.locations}},s.codexFilter,function(f)
    s.codexFilter=f;s.codex=nil;s.resetList=true;redraw(kit)
  end)
  local rows,selected={},nil
  -- the full list is read again only when the cheap walk of the codex sees an entry come or go
  fetch(kit,"codexWatch",D.codexWatch,4,0,{background=true,changed=function(first) if not first then Jobs.run("codex",D.codexList) end end})
  local entries=fetch(kit,"codex",D.codexList,math.huge) or {};s.codexCount=#entries
  for _,e in ipairs(entries) do
    if e.hash==s.codex then selected=e end
    if s.codexFilter==0 or s.codexFilter==e.category then
      rows[#rows+1]=row(kit,"p1",e.name,({[1]=t.lore,[2]=t.characters,[3]=t.locations})[e.category] or "",function()
        s.codex=e.hash;D.visit(e.hash);e.fresh=false;redraw(kit)
      end,s.codex==e.hash,e.fresh and K.YELLOW or nil)
    end
  end
  list(kit,p,"p1","p1",kit.P1X,188,WIDTH,rows,800,kit.S.keepScroll)
  heading(kit,p2,DETAIL,t.codex,kit.P2X)
  local details={}
  if selected then
    textRows(kit,details,selected.name,DETAIL,K.WHITE,40)
    local image=D.image(selected.image)
    if image and image.part~="" then
      details[#details+1]={h=320,build=function(c,it,v)
        -- Keep native image proportions: journal character portraits and landscape art differ.
        local pic=K.image(c,22,10,1,1,image.atlas,image.part,K.WHITE,.001)
        pic:SetFitToContent(true)
        pic:SetContentHAlign(inkEHorizontalAlign.Center)
        pic:SetContentVAlign(inkEVerticalAlign.Center)
        pic:SetRenderTransformPivot(Vector2.new({X=0,Y=0}))
        pic:SetScale(Vector2.new({X=.001,Y=.001}))
        s.images[#s.images+1]={widget=pic,view=v,item=it,w=v.cw-44,h=290}
      end}
    end
    textRows(kit,details,selected.description,DETAIL)
  else textRows(kit,details,t.choose,DETAIL,K.GREY) end
  list(kit,p2,"p2","p2",kit.P2X,110,DETAIL,details,880)
end

-- A queued purchase may land between the click's redraw and its scheduled refresh.
-- Invalidate the snapshot before drawing or recording the new point totals.
local function refreshCharacter(s)
  local ok,signature=pcall(D.signature,"character")
  if not ok or signature==s.signature then return false end
  Jobs.drop("character");Jobs.dropPrefix("perks:")
  s.confirm=nil
  s.attributePrompt=nil
  s.signature=signature
  return true
end

function G.build(kit,p,p2)
  local s,t=state(kit)
  s.images={};s.watch={};s.reading=nil
  if s.resetList then kit.S.keepScroll=nil;s.resetList=nil end
  if kit.S.section=="character" then refreshCharacter(s) end
  local ok,err=pcall(G[kit.S.section],kit,p,p2)
  local viewKey=kit.S.section..":"..tostring(s.attribute)..":"..tostring(s.skills)..":"..tostring(s.tarot)
    ..":"..tostring(s.perk~=nil)..":"..tostring(s.journalInspect~=nil)
  if ok and s.viewKey~=viewKey and (kit.S.section=="character" or kit.S.section=="tarot") then
    local m1,m2=p:GetMargin(),p2:GetMargin()
    kit.slideIn("p1",p,m1.left,m1.top,0,0,22)
    kit.slideIn("p2",p2,m2.left,m2.top,.03,28,0)
    kit.S.panelInteractiveAt=math.max(kit.S.panelInteractiveAt,kit.S.time+.22)
  end
  s.viewKey=viewKey
  s.error=not ok and tostring(err) or nil
  s.lastPoll=kit.S.time
  if not ok then
    print("[VRMenu] "..kit.S.section..": "..tostring(err))
    kit.toast(t.readError)
    button(kit,p,20,1045,WIDTH-40,t.retry,true,function() redraw(kit) end)
  end
end

function G.leave(kit)
  local s=state(kit);s.confirm=nil;s.attributePrompt=nil;s.lastPoll=nil;s.signature=nil;s.images={};s.watch={};s.reading=nil
end

-- Keys the panel on screen drew from: a newer result redraws it (or goes to its own handler), and a
-- result older than its period is read again behind the panel.
local function watch(kit,s)
  local busy=false
  for _, hand in ipairs(kit.S.hands) do if hand.drag or hand.pend or hand.scroll then busy=true end end
  if not busy then for _, view in pairs(kit.S.views) do if view:moving() then busy=true end end end
  local stale=false
  for key,w in pairs(s.watch or {}) do
    local _,rec=Jobs.get(key)
    local seq=rec and rec.seq
    if seq~=w.seq then
      if w.changed then
        local first=w.seq==nil;w.seq=seq
        if w.changed(first,rec and rec.value)==true then stale=true end
      else stale=true end
    end
    if not Jobs.busy(key) and Jobs.age(key)>w.period then Jobs.run(key,w.producer) end
  end
  -- never pull a list out from under a finger; the redraw waits until the drag ends
  if stale and not busy then
    local detail=kit.S.views.p2
    local offset=detail and detail.offset
    redraw(kit)
    if offset and kit.S.views.p2 then kit.S.views.p2:scrollTo(offset) end
  end
end

function G.update(kit)
  local s=state(kit)
  watch(kit,s)
  for _,image in ipairs(s.images or {}) do
    if not image.fitted then
      local size=image.widget:GetDesiredSize()
      if size.X>1 and size.Y>1 then
        local scale=math.min(image.w/size.X,image.h/size.Y)
        local x,y=(image.x or 22)+(image.w-size.X*scale)/2,(image.y or 10)+(image.h-size.Y*scale)/2
        image.widget:SetFitToContent(false)
        image.widget:SetContentHAlign(inkEHorizontalAlign.Fill)
        image.widget:SetContentVAlign(inkEVerticalAlign.Fill)
        K.size(image.widget,size.X*scale,size.Y*scale)
        image.widget:SetScale(Vector2.new({X=1,Y=1}))
        K.move(image.widget,x,y)
        image.widget:SetOpacity(1)
        if not image.noHit then kit.button("p2",kit.at(image.view.x,image.item.y,image.view),x,y,size.X*scale,size.Y*scale,nil,nil) end
        image.fitted=true
      end
    end
  end
  if kit.S.section=="inventory" then Reference.inventoryUpdate(kit);return end
  if kit.S.section=="cyberware" then Reference.cyberwareUpdate(kit);return end
  if kit.S.section=="crafting" then Reference.craftingUpdate(kit);return end
  if kit.S.section=="trade" then Reference.tradeUpdate(kit);return end
  if kit.S.section=="stash" then Reference.stashUpdate(kit);return end
  if kit.S.section=="load" then Reference.loadUpdate(kit);return end
  if kit.S.time-(s.lastPoll or 0)<1 then return end
  s.lastPoll=kit.S.time
  for _, hand in ipairs(kit.S.hands) do if hand.drag or hand.pend or hand.scroll then return end end
  for _, view in pairs(kit.S.views) do if view:moving() then return end end
  if kit.S.section=="timeskip" then
    local ok,c=pcall(D.clock)
    if ok and s.clock and (c.minute~=s.clock.minute or c.hour~=s.clock.hour or c.allowed~=s.clock.allowed or c.hidden~=s.clock.hidden) then redraw(kit) end
  elseif kit.S.section=="character" then
    -- (the journal, codex, shards and tarot are watched through their jobs above)
    if refreshCharacter(s) then
      local detail=kit.S.views.p2
      local offset=detail and detail.offset
      redraw(kit)
      if offset and kit.S.views.p2 then kit.S.views.p2:scrollTo(offset) end
    end
  end
end

Reference=require("modules/reference_view")({state=state,clean=clean,wrap=wrap,row=row,
  textRows=textRows,list=list,heading=heading,button=button,perform=perform,redraw=redraw,fetch=fetch})
G.character=Reference.character
G.inventory=Reference.inventory
G.cyberware=Reference.cyberware
G.crafting=Reference.crafting
G.trade=Reference.trade
G.stash=Reference.stash
G.load=Reference.load
G.journal=Reference.journal
G.shards=Reference.shards
G.tarot=Reference.tarot
G.timeskip=Reference.timeskip
G.summary=Reference.summary
local buildCodex=G.codex
function G.codex(kit,p,p2) buildCodex(kit,p,p2);Reference.libraryNav(kit,p) end

function G.beforeBuild(kit)
  local s=state(kit)
  Reference.stopVideos(s)
  Reference.cyberwareBeforeBuild(kit)
  Reference.tradeBeforeBuild(kit)
  Reference.stashBeforeBuild(kit)
  Reference.loadBeforeBuild(kit)
end
local leave=G.leave
function G.leave(kit)
  local s=state(kit);Reference.stopVideos(s);Reference.inventoryLeave(kit);Reference.cyberwareLeave(kit);Reference.craftingLeave(kit);Reference.tradeLeave(kit);Reference.stashLeave(kit);Reference.loadLeave(kit);leave(kit)
end
function G.updateSummary(kit)
  local s=state(kit)
  if not kit.S.summaryVisible then return end
  if kit.S.time-(s.summaryAt or 0)<2 or not kit.S.layerSummary then return end
  s.summaryAt=kit.S.time
  local ok,data=pcall(require("modules/reference_data").summary)
  if not ok then return end
  local function stamp(d)
    if not d then return "" end
    local v={math.floor(d.seconds/60),math.floor(d.health),math.floor(d.armor),math.floor(d.dps)}
    for _,p in ipairs(d.progress) do v[#v+1]=p.level;v[#v+1]=p.xp end
    for _,p in ipairs(d.other) do v[#v+1]=p.value end
    return table.concat(v,"|")
  end
  if stamp(data)~=stamp(s.summary) then Reference.summary(kit,kit.S.layerSummary) end
end
-- On a menu open: read what the quest group draws behind the opening animation, so its first panel
-- has its data before a finger reaches it. Results kept from an earlier open are refreshed instead.
function G.prefetch()
  if not Jobs.has("quests") or Jobs.age("quests")>2 then Jobs.run("quests",R.questList) end
  if not Jobs.has("codex") then Jobs.run("codex",D.codexList) end
  if not Jobs.has("shards") or Jobs.age("shards")>5 then Jobs.run("shards",require("modules/library_data").shardList) end
end
-- Background reads get this many seconds of a frame (os.clock counts whole milliseconds).
function G.step() Jobs.step(.0015) end

function G.cancelTransition(kit)
  kit.S.waitTransition=nil
  if type(VRMenuBlink)=="function" then pcall(VRMenuBlink,0) end
  if type(VRMenuCapture)=="function" then pcall(VRMenuCapture,false) end
end
function G.transition(kit,dt)
  local transition=kit.S.waitTransition
  if not transition then return false end
  local elapsed=kit.S.time-transition.started
  local function ease(t) t=math.max(0,math.min(1,t));return t*t*(3-2*t) end
  if transition.phase=="closing" then
    VRMenuBlink(ease(elapsed))
    if elapsed>=1 then
      local now=GameTime.GetSeconds(Game.GetTimeSystem():GetGameTime())
      transition.target=now+transition.hours*3600
      local ok,result=pcall(D.skip,transition.hours)
      transition.applied=ok and result
      transition.phase="holding";transition.at=kit.S.time
      if not ok then print("[VRMenu] wait: "..tostring(result)) end
    end
  elseif transition.phase=="holding" then
    VRMenuBlink(1)
    local now=GameTime.GetSeconds(Game.GetTimeSystem():GetGameTime())
    if kit.S.time-transition.at>=.4 and (not transition.applied or now>=transition.target or kit.S.time-transition.at>=8) then
      transition.phase="opening";transition.at=kit.S.time
    end
  else
    VRMenuBlink(1-ease(kit.S.time-transition.at))
    if kit.S.time-transition.at>=1 then G.cancelTransition(kit) end
  end
  return true
end
return G
