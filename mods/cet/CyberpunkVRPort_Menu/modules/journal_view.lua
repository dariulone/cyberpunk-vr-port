local K=require("modules/ink")
local Loc=require("modules/locale")
local D=require("modules/gameplay_data")
local R=require("modules/reference_data")
local L=require("modules/library_data")
return function(C,U)
local J={}
local function text(kit,ru,en) return Loc.tr(ru,en) end
local function mark(p,x,y,size,filled,col)
  K.image(p,x,y,size,size,K.ATLAS.phone,"circle",col,1)
  if not filled then K.image(p,x+3,y+3,size-6,size-6,K.ATLAS.phone,"circle",K.BLACK,1) end
  K.text(p,x,y-1,size,size,"!",math.floor(size*.75+.5),filled and K.BLACK or col,"center")
end
local function book(p,x,y,size,col)
  K.roundEdge(p,x,y,size*.73,size,col,.9,2)
  K.rect(p,x+size*.18,y+3,2,size-6,col,1)
  K.text(p,x+size*.2,y,size*.5,size,"!",math.floor(size*.66+.5),col,"center")
end
-- TRACKING SAYS SO. A tap on an active objective tracks it, and nothing on the row said that. Each active
-- objective now carries its state at its right end: TRACK (an outlined chip with a crosshair, lit with the
-- row's hover) or TRACKING (filled yellow, the colour the tracked quest already has in the list and on its
-- distance). The chip is drawing only: the whole row stays the button, as before.
-- Russian uses the game's own words (Отслеживать / Отслеживается, as in the menu's LABELS), and every language its
-- own: the chip and the list's TRACKED label are as wide as the words need (set per draw, the language can change
-- in the settings).
local CHIP_W,CHIP_H=176,46
local TAG_W=140
-- How wide a label draws, estimated per glyph from the font the language uses (K.FONTS): Rajdhani caps (English,
-- Russian) .62 of the size, Blender caps (the other Latin languages, Ukrainian) .72, a CJK glyph a full em, Thai
-- and Arabic narrower, Thai's marks above and below the line nothing. An ink text sizes itself only when it is laid
-- out, after the chip under it is drawn, so the chip is sized from this.
local function labelW(str,size)
  local latin=K.FONTS[Loc.code]~=nil and .72 or .62
  local w=0
  for ch in string.gmatch(str,"[%z\1-\127\194-\244][\128-\191]*") do
    local b1,b2,b3=ch:byte(1,3)
    local cp=b1<128 and b1 or b1<224 and (b1-192)*64+(b2-128) or b1<240 and ((b1-224)*64+(b2-128))*64+(b3-128) or 0x10000
    if cp==32 then w=w+.3
    elseif cp>=0x2E80 then w=w+1
    elseif cp>=0x0E00 and cp<0x0E80 then
      if not (cp==0x0E31 or (cp>=0x0E34 and cp<=0x0E3A) or (cp>=0x0E47 and cp<=0x0E4E)) then w=w+.55 end
    elseif cp>=0x0600 and cp<0x0700 then
      if not (cp>=0x064B and cp<=0x065F) then w=w+.5 end
    else w=w+latin end
  end
  return w*size
end
local function crosshair(p,x,y,size,col)
  K.image(p,x,y,size,size,K.ATLAS.phone,"circle",col,1)
  K.image(p,x+2,y+2,size-4,size-4,K.ATLAS.phone,"circle",K.BLACK,1)
  local m=size/2
  U.line(p,x+m,y-4,x+m,y+5,col,1,2);U.line(p,x+m,y+size-5,x+m,y+size+4,col,1,2)
  U.line(p,x-4,y+m,x+5,y+m,col,1,2);U.line(p,x+size-5,y+m,x+size+4,y+m,col,1,2)
end
-- returns the chip's fill for the hover to light (nil for the tracked one, which stays as it is)
local function trackChip(kit,p,x,y,tracked)
  if tracked then
    K.roundFill(p,x,y,CHIP_W,CHIP_H,K.YELLOW,.9,6)
    K.text(p,x,y,CHIP_W,CHIP_H,Loc.tr("ОТСЛЕЖИВАЕТСЯ","TRACKING"),23,K.BLACK,"center")
    return nil
  end
  local bg=K.roundFill(p,x,y,CHIP_W,CHIP_H,K.CYAN,.08,6)
  K.roundEdge(p,x,y,CHIP_W,CHIP_H,K.CYAN,.8,6)
  crosshair(p,x+17,y+14,18,K.CYAN)
  K.text(p,x+46,y,CHIP_W-54,CHIP_H,Loc.tr("ОТСЛЕЖИВАТЬ","TRACK"),23,K.CYAN,"center")
  return bg
end
local function filterIcon(p,x,y,kind,col)
  if kind=="main" then mark(p,x,y,30,true,col)
  elseif kind=="active" then
    for i=0,2 do K.rect(p,x,y+3+i*10,4,4,col,1);K.rect(p,x+9,y+3+i*10,25,3,col,1) end
  elseif kind=="activities" then
    K.image(p,x,y,32,32,K.ATLAS.phone,"circle",col,1);K.image(p,x+3,y+3,26,26,K.ATLAS.phone,"circle",K.BLACK,1)
    U.line(p,x+9,y+23,x+23,y+9,col,1,4);U.line(p,x+16,y+11,x+23,y+9,col,1,3)
  else
    K.image(p,x,y,32,32,K.ATLAS.phone,"circle",col,1);K.image(p,x+3,y+3,26,26,K.ATLAS.phone,"circle",K.BLACK,1)
    U.line(p,x+6,y+16,x+13,y+22,col,1,3);U.line(p,x+13,y+22,x+26,y+9,col,1,3)
  end
end

local function distanceText(kit,d)
  return d>=1000 and string.format("%.1f",d/1000)..text(kit,"км","km") or math.floor(d+.5)..text(kit,"м","m")
end

function J.journal(kit,p,p2)
  local s,t=C.state(kit);local inspect=s.journalInspect
  CHIP_W=math.max(176,math.ceil(labelW(Loc.tr("ОТСЛЕЖИВАЕТСЯ","TRACKING"),23))+64,math.ceil(labelW(Loc.tr("ОТСЛЕЖИВАТЬ","TRACK"),23))+66)
  TAG_W=math.max(140,math.ceil(labelW(Loc.tr("ОТСЛЕЖИВАЕТСЯ","TRACKED"),22))+28)
  local listW=820
  local detailX=kit.P2X
  local detailW=900
  K.move(p2,detailX,kit.P1Y)
  -- Walking changes only distances: those rows get their new text in place, anything else redraws.
  s.distanceWidgets={}
  local quests=C.fetch(kit,"quests",R.questList,2,.012,{changed=function(_,list)
    if not list or list.structure~=s.questStructure then return true end
    for _,q in ipairs(list) do
      s.distances[q.hash]=q.distance
      local w=s.distanceWidgets[q.hash]
      if w and q.distance then pcall(function() w:SetText(distanceText(kit,q.distance)) end) end
    end
  end}) or {}
  -- rows built later (scrolled into view) show the newest distance too
  s.distances={}
  s.questStructure=quests.structure
  local counts={active=0,main=0,activities=0,completed=0}
  local selected
  for _,q in ipairs(quests) do
    q.active=q.state==gameJournalEntryState.Active
    q.main=q.story or q.kind:find("SideQuest",1,true)~=nil or q.kind:find("MinorQuest",1,true)~=nil
    if q.hash==s.quest then selected=q end
    if q.active then counts.active=counts.active+1;local key=q.main and "main" or "activities";counts[key]=counts[key]+1
    else counts.completed=counts.completed+1 end
  end
  if not counts[s.questFilter] then s.questFilter="active" end
  kit.glass(p,listW,1130,kit.P1X,kit.P1Y)
  U.title(p,t.journal,listW)
  U.headAction(kit,p,kit.P1X,listW-72,text(kit,"ЧИПЫ","SHARDS"),function() kit.openSection("shards") end)
  local tabs={"active","main","activities","completed"};local tw=(listW-40-24)/4
  for i,key in ipairs(tabs) do
    local x=20+(i-1)*(tw+8);local on=s.questFilter==key
    local colors={active=K.CYAN,main=K.YELLOW,activities={.2,.85,.65},completed=K.CYAN}
    local col=on and K.CYAN or colors[key]
    local bg=K.roundFill(p,x,100,tw,60,col,on and .16 or .035,8)
    filterIcon(p,x+13,115,key,col)
    K.text(p,x+46,100,tw-50,60,tostring(counts[key]),30,col,"center")
    kit.button("p1",kit.at(kit.P1X,kit.P1Y),x,100,tw,60,function()
      s.questFilter=key;s.quest=nil;s.journalInspect=nil;s.resetList=true;C.redraw(kit)
    end,function(h) bg:SetOpacity(h and .25 or on and .16 or .035) end)
  end
  local rows={};s.questCount=0;s.questCounters=counts;s.questRows={}
  for _,q in ipairs(quests) do
    local show=(s.questFilter=="active" and q.active) or (s.questFilter=="main" and q.active and q.main)
      or (s.questFilter=="activities" and q.active and not q.main) or (s.questFilter=="completed" and not q.active)
    if show then
      s.questCount=s.questCount+1;s.questRows[#s.questRows+1]={hash=q.hash,name=q.name,story=q.story,tracked=q.tracked,distance=q.distance}
      rows[#rows+1]={h=106,build=function(c,it,v)
        local bg=K.roundFill(c,0,0,v.cw,96,q.tracked and K.YELLOW or K.ICE,q.tracked and .12 or .035,7)
        local edge=K.roundEdge(c,0,0,v.cw,96,s.quest==q.hash and K.YELLOW or K.CYAN,s.quest==q.hash and 1 or .35,7)
        mark(c,20,27,34,q.story,q.active and K.YELLOW or K.GREY)
        if q.tracked then
          K.roundFill(c,-53,16,38,64,K.YELLOW,.14,5)
          book(c,-48,28,30,K.YELLOW)
        end
        local reserved=q.distance and q.active and 110 or 8
        local available=v.cw-78-reserved
        K.text(c,70,5,available,42,K.clip(q.name,math.floor(available/16)),29,K.CYAN)
        -- the tracked quest says so in words too, not only in yellow
        local tag=q.tracked and q.active and TAG_W or 0
        K.text(c,70,49,v.cw-90-tag,37,K.clip(q.objective,math.floor((v.cw-90-tag)/13)),25,K.GREY)
        if tag>0 then K.text(c,v.cw-tag-14,49,tag,37,Loc.tr("ОТСЛЕЖИВАЕТСЯ","TRACKED"),22,K.YELLOW,"right") end
        if q.distance and q.active then
          if q.tracked then K.roundFill(c,v.cw-112,3,108,43,K.YELLOW,.9,5) end
          s.distanceWidgets[q.hash]=K.text(c,v.cw-110,3,101,43,distanceText(kit,s.distances[q.hash] or q.distance),27,q.tracked and K.BLACK or K.CYAN,"center")
        end
        kit.button("p1",kit.at(v.x,it.y,v),0,0,v.cw,96,function()
          s.quest=q.hash;s.journalInspect=nil;D.visit(q.hash);C.redraw(kit)
        end,function(h) bg:SetOpacity(h and .22 or q.tracked and .12 or .035);K.tint(edge,h and K.YELLOW or s.quest==q.hash and K.YELLOW or K.CYAN,h and 1 or .5) end)
      end}
    end
  end
  C.list(kit,p,"p1","p1",kit.P1X,178,listW,rows,855,kit.S.keepScroll)
  kit.glass(p2,detailW,1130,detailX,kit.P1Y)
  local details={};local detail
  if s.quest then
    local hash=s.quest
    detail=C.fetch(kit,"quest:"..tostring(hash),function() return R.questDetail(hash) end,2,.03)
  end
  local q=detail and detail.quest
  if q then
    -- Title and objective are separate rows, with the same breathing room as the native journal.
    C.textRows(kit,details,K.upper(q.name),detailW,K.WHITE,34)
    details[#details+1]={h=28,build=function() end}
    -- ONLY OBJECTIVES GET A CHIP. The native details panel makes a trackable row of each JournalQuestObjective only
    -- (QuestLogDetailsPanel / QuestDetailsObjectiveController); sub-objectives are drawn indented under their
    -- objective, without a chip, and a tap on one tracks the objective it belongs to (D.quest lists each objective's
    -- subs right after it).
    -- WHICH ONE IS TRACKED comes from JournalManager:GetTrackedEntry(), not from the rows' tracked flag: the wrapper's
    -- QuestObjectiveWrapper.IsTrackedInHierarchy() is true only when one of its SUB-objectives is the tracked entry,
    -- never for the objective itself, so the flag is false on almost every objective row.
    local trackedHash=0
    pcall(function()
      local jm=Game.GetJournalManager();local e=jm:GetTrackedEntry()
      if e~=nil then trackedHash=jm:GetEntryHash(e) end
    end)
    local trackedRow={};local parent
    for _,o in ipairs(q.objectives) do
      if not o.sub then parent=o end
      if o.hash==trackedHash or o.tracked then trackedRow[(o.sub and parent or o).hash]=true end
    end
    parent=nil
    for _,o in ipairs(q.objectives) do
      if not o.sub then parent=o end
      if o.active then
        local sub=o.sub and parent~=nil
        local target=sub and parent.hash or o.hash
        local tracked=not sub and trackedRow[o.hash]==true
        local indent=sub and 36 or 0
        local chipW=sub and 0 or CHIP_W+16
        local lines=C.wrap(K.upper(o.name),math.floor((detailW-100-indent-chipW)/17));local h=#lines*41+26
        details[#details+1]={h=h+12,build=function(c,it,v)
          local bg=K.roundFill(c,indent,0,v.cw-indent,h,K.CYAN,.05,6)
          if sub then K.rect(c,indent+22,25,10,10,K.CYAN,.8) else book(c,16,15,27,tracked and K.YELLOW or K.CYAN) end
          K.text(c,indent+66,11,v.cw-90-indent-chipW,#lines*41,table.concat(lines,"\n"),sub and 27 or 30,K.CYAN)
          local chip=not sub and trackChip(kit,c,v.cw-CHIP_W-14,math.floor((h-CHIP_H)/2),tracked) or nil
          kit.button("p2",kit.at(v.x,it.y,v),indent,0,v.cw-indent,h,function() C.perform(kit,function() return D.track(target) end) end,function(hov)
            bg:SetOpacity(hov and .2 or .05)
            if chip then chip:SetOpacity(hov and .3 or .08) end
          end)
        end}
      end
    end
    s.questActions=detail.actions
    for _,entry in ipairs(s.questActions) do
      details[#details+1]={h=76,build=function(c,it,v)
        local bg=K.roundFill(c,0,0,v.cw,64,K.CYAN,.055,8)
        -- A message bubble and a document outline are drawn directly; no missing atlas parts.
        K.roundEdge(c,18,16,29,25,K.CYAN,1,3)
        K.rect(c,22,42,9,3,K.CYAN,1)
        K.text(c,64,0,v.cw-85,64,K.clip(K.upper(entry.name),math.floor((v.cw-85)/17)),29,K.CYAN)
        kit.button("p2",kit.at(v.x,it.y,v),0,0,v.cw,64,function()
          if entry.kind=="message" then
            kit.S.phone={tab="messages",showRead=true,thread=entry.thread,filter={hash=entry.contact,name=entry.name}}
            kit.openSection("contacts")
          else
            if L.readShard(entry.hash) then s.shard=entry.hash;s.shardGroup=nil;kit.openSection("shards") end
          end
        end,function(h) bg:SetOpacity(h and .2 or .055) end)
      end}
    end
    C.textRows(kit,details,"\n"..q.description.."\n",detailW,K.TEXT,31)
    local links=detail.links;s.questLinks=links
    local columns=detailW>=800 and 3 or 2;local cell=(detailW-58)/columns
    for first=1,#links,columns do
      local group={};for i=first,math.min(#links,first+columns-1) do group[#group+1]=links[i] end
      details[#details+1]={h=190,build=function(c,it,v)
        for i,link in ipairs(group) do
          local x=14+(i-1)*cell
          U.picture(kit,c,x,6,cell-16,120,link.image)
          K.text(c,x,130,cell-16,44,K.clip(K.upper(link.name),math.floor((cell-16)/13)),24,K.CYAN)
          local edge=K.roundEdge(c,x,5,cell-16,122,K.CYAN,0,3)
          kit.button("p2",kit.at(v.x,it.y,v),x,5,cell-16,171,function()
            s.journalInspect=link.hash;D.visit(link.hash);C.redraw(kit)
          end,function(h) K.tint(edge,K.CYAN,h and 1 or 0) end)
        end
      end}
    end
    for _,o in ipairs(q.objectives) do if not o.active then C.textRows(kit,details,o.name,detailW,K.GREY,25) end end
  elseif not (s.quest and s.reading) then C.textRows(kit,details,t.choose,detailW,K.GREY) end
  C.list(kit,p2,"p2","p2",detailX,20,detailW,details,1010)
  if inspect then
    local x,w=kit.P1X,1780;local pane=K.canvas(kit.S.layerPanel,x,kit.P1Y,w,1160)
    -- Erase the underlying ink and block its controls without giving the blank modal a cursor.
    K.roundFill(pane,0,0,w,1130,K.BLACK,1,18)
    kit.glass(pane,w,1130,x,kit.P1Y)
    local blocker=kit.button("p2",kit.at(x,kit.P1Y),0,0,w,1130,nil,nil);blocker.blocker=true
    local entry=L.codex(inspect);local rows3={}
    -- the close box sits inside the glass, left of the frame's corner mark (as reference_view headAction)
    local closeBg=K.roundFill(pane,w-144,22,72,48,K.CYAN,.12,6)
    U.line(pane,w-120,34,w-96,58,K.CYAN,1,3);U.line(pane,w-96,34,w-120,58,K.CYAN,1,3)
    local close=kit.button("p2",kit.at(x,kit.P1Y),w-144,22,72,48,function() s.journalInspect=nil;C.redraw(kit) end,function(h) closeBg:SetOpacity(h and .3 or .12) end)
    close.key="journalClose"
    if entry then
      rows3[#rows3+1]={h=360,build=function(c,it,v) U.picture(kit,c,20,10,v.cw-40,330,D.image(entry.image),v,it) end}
      C.textRows(kit,rows3,K.upper(entry.name),w,K.CYAN,40)
      C.textRows(kit,rows3,entry.description,w,K.TEXT,34)
    else C.textRows(kit,rows3,t.empty,w,K.GREY) end
    C.list(kit,pane,"p2","p3",x,86,w,rows3,950):enableMask()
    if s.inspectShown~=inspect then kit.slideIn("p2",pane,x,kit.P1Y,.02,36,0) end
  end
  s.inspectShown=inspect
end

function J.shards(kit,p,p2)
  local s,t=C.state(kit);local groups=C.fetch(kit,"shards",L.shardList,5) or {};s.shardGroups=groups
  kit.glass(p,820,1140,kit.P1X,kit.P1Y);U.title(p,text(kit,"ЧИПЫ","SHARDS"),820)
  U.headAction(kit,p,kit.P1X,820-72,text(kit,"БАЗА ДАННЫХ","DATABASE"),function() kit.openSection("codex") end)
  local selected,selectedGroup
  if s.shard then
    for _,group in ipairs(groups) do for _,entry in ipairs(group.entries) do if entry.hash==s.shard then selected=entry;selectedGroup=group.key end end end
  end
  if s.shardGroup==nil and selected then s.shardGroup=selectedGroup end
  local rows={}
  for _,group in ipairs(groups) do
    local fresh=0;for _,entry in ipairs(group.entries) do if entry.fresh then fresh=fresh+1 end end
    rows[#rows+1]=C.row(kit,"p1",(s.shardGroup==group.key and "- " or "+ ")..K.upper(group.name).."  ("..#group.entries..")",fresh>0 and (text(kit,"Новые: ","New: ")..fresh) or "",function()
      s.shardGroup=s.shardGroup==group.key and false or group.key;s.resetList=true;C.redraw(kit)
    end,s.shardGroup==group.key,fresh>0 and K.CYAN or K.GREY)
    if s.shardGroup==group.key then
      for _,entry in ipairs(group.entries) do
        rows[#rows+1]=C.row(kit,"p1","  "..entry.name,entry.crypted and text(kit,"Зашифровано","Encrypted") or "",function()
          local read=L.readShard(entry.hash)
          if read then s.shard=entry.hash;C.redraw(kit) end
        end,s.shard==entry.hash,entry.fresh and K.YELLOW or K.TEXT)
      end
    end
  end
  C.list(kit,p,"p1","p1",kit.P1X,100,820,rows,935,kit.S.keepScroll)
  kit.glass(p2,900,1140,kit.P2X,kit.P1Y)
  local details={}
  if selected then
    C.textRows(kit,details,K.upper(selected.name),900,K.CYAN,38)
    C.textRows(kit,details,"\n"..selected.text,900,K.TEXT,32)
    if selected.crypted then
      U.action(kit,p2,kit.P2X,20,1050,860,text(kit,"ВЗЛОМАТЬ ЧИП","CRACK SHARD"),function()
        kit.close();L.crack(selected.hash)
      end)
    end
  else C.textRows(kit,details,text(kit,"[ Запись не выбрана ]","[ No entry selected ]"),900,K.CYAN,38) end
  C.list(kit,p2,"p2","p2",kit.P2X,30,900,details,selected and selected.crypted and 930 or 1020)
end
return J
end
