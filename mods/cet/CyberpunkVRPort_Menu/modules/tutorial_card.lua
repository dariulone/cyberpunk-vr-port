-- Present text-only tutorial notifications on the same world surface as the VR menu.
-- The original controller owns input, quest facts, time dilation and dismissal.
local Space = require("modules/space")
local Ink = require("modules/ink")
local M, card = {}, nil
local tag = "VRPortReadingCard"
local function release()
  local c = card
  card = nil
  if not c then return end
  if c.masked then pcall(function()
    if c.source:GetOpacity() == 0 then c.source:SetOpacity(c.opacity) end
  end) end
  if c.id then pcall(function() Game.VRSpawnDelete(c.id) end) end
end
M.release = release

local function widget(ref)
  return ref and inkWidgetRef.Get(ref)
end

local function readText(w)
  local text = w:GetText()
  if type(text) == "string" and text ~= "" and not text:match("^LocKey#%d+$") then return text end
  local key = w:GetLocalizationKey()
  if key and type(key) ~= "string" then key = key.value end
  if type(key) == "string" and key ~= "" then return GetLocalizedText(key) end
  return ""
end

local function visible(w)
  while w do
    if not w:IsVisible() or w:GetOpacity() <= .01 then return false end
    w = w:GetParentWidget()
  end
  return true
end

local function build(root, title, body, footer, glyph)
  root:RemoveAllChildren()
  Ink.rect(root, 120, 40, 2960, 1520, Ink.NAVY, .95)
  Ink.rect(root, 120, 40, 2960, 18, Ink.CYAN, 1)
  local function text(x, y, width, height, value, size, color)
    local w = Ink.text(root, x, y, width, height, value, size, color, "left")
    Ink.size(w, width, height)
    local wrap = w.wrappingInfo
    wrap.wrappingAtPosition = width
    wrap.autoWrappingEnabled = true
    w.wrappingInfo = wrap
    w:SetVerticalAlignment(textVerticalAlignment.Top)
    return w
  end
  if glyph then Ink.image(root, 220, 1368, 110, 110, glyph.atlas, glyph.part, Ink.WHITE) end
  return {
    {w=text(220,100,2760,225,title,100,Ink.CYAN), height=225},
    {w=text(220,340,2760,990,body,78,Ink.TEXT), height=990},
    {w=text(glyph and 370 or 220,1360,glyph and 2610 or 2760,145,footer,70,Ink.WHITE), height=145}
  }
end

-- The game creates several layout variants, including hidden ones. Use only
-- the display that is actually visible. Illustrated and video tutorials keep
-- their original presentation so no part of their instructions can be lost.
local function open(display)
  local source = display:GetRootWidget()
  if not source then return end
  if card and card.display == display then return end
  if not visible(source) then return end
  for _, key in ipairs({"m_image","m_video_1024x576","m_video_1280x720","m_video_720x405","m_video_1360x768","m_video_1500x847"}) do
    local media = widget(display[key])
    if media and visible(media) then return end
  end
  local title, message = widget(display.m_title), widget(display.m_message)
  if not title or not message then return end
  local heading, body = readText(title), readText(message)
  if type(heading) ~= "string" or heading == "" or type(body) ~= "string" or body == "" then return end
  local hint = widget(display.m_inputHint)
  local footer = ""
  local glyph, action
  local displays = 0
  if hint and visible(hint) then
    -- The stock hint contains the localized action label and its input glyph.
    -- Only replace cards whose active hint we can reproduce.
    local labels = {}
    local function collect(w)
      if not w or not w:IsVisible() or w:GetOpacity() <= .01 then return end
      local controller = w:GetController()
      if controller and controller:IsA('inkInputDisplayController') then
        displays = displays + 1
        action = controller:GetInputAction()
      end
      if w:IsA(CName.new("inkTextWidget")) then
        local t = readText(w)
        if t and t ~= "" then labels[#labels+1] = t end
      elseif w:IsA(CName.new("inkImageWidget")) and NameToString(w:GetName()) == "inputIcon" then
        local part = w:GetTexturePart()
        if part and part.value and part.value ~= "None" and w.textureAtlas then
          glyph = {atlas=w.textureAtlas,part=part}
        end
      elseif w:IsA(CName.new("inkCompoundWidget")) then
        for i=0,w:GetNumChildren()-1 do
          collect(w:GetWidgetByIndex(i))
        end
      end
    end
    collect(hint)
    if #labels == 0 and not glyph then return end
    footer = table.concat(labels, "  ")
  end
  local cam = Space.view()
  if not cam or Space.dot(cam.f,cam.f) < .99 then return end
  release()
  local centre = Space.add(cam.p, Space.mul(cam.f, .95))
  centre[3] = centre[3] - .04
  local tr = Space.transformFor(centre, cam.r, {0,0,1})
  local id = Game.VRSpawnTemplate([[vrport\ui\vr_menu.ent]], CName.new(""),
      WorldPosition.ToVector4(tr.Position), tr:GetOrientation(), {CName.new(tag)})
  if id and id.hash ~= 0 then
    card = {id=id, display=display, source=source, title=heading, body=body, footer=footer, glyph=glyph, age=0,
            action=displays == 1 and action or nil, hintTimer=0}
  end
end

local pending
function M.init()
  -- SetupView/SetData can run before the intro has made the chosen panel visible.
  ObserveAfter("TutorialPopupDisplayController", "SetData", function(self)
    pending = pending or {}
    pending[#pending+1] = {display=self, age=0}
  end)
  local function close() pending=nil; release() end
  ObserveBefore("TutorialPopupGameController", "CloseTutorial", close)
  ObserveBefore("TutorialPopupGameController", "OnUninitialize", close)
  ObserveBefore("PlayerPuppet", "OnDetach", close)
end

local function update(dt)
  if not Game.VRPhysicalOpticsEnabled() then pending=nil; release(); return end
  if pending then
    for i=#pending,1,-1 do
      local item=pending[i]
      item.age=item.age+dt
      if item.age>.05 then
        local previous=card
        open(item.display)
        if card~=previous or item.age>1 then table.remove(pending,i) end
      end
    end
    if #pending==0 then pending=nil end
  end
  local c=card
  if not c then return end
  local parent=c.source:GetParentWidget()
  if not parent or not visible(parent) or not c.source:IsVisible() then release(); return end
  if c.action and type(VRCyberFingerHint) == "function" then
    c.hintTimer = c.hintTimer - dt
    if c.hintTimer <= 0 then
      c.hintTimer = .25
      local player = Game.GetPlayer()
      local label = ""
      if (player and player:PlayerLastUsedPad()) or VRCyberFingerHint(CName.new("VRPortCyberFingerMenuPointer"),false) ~= "" then
        label = VRCyberFingerHint(c.action,false)
      end
      local caption = c.footer
      if label ~= "" then caption = caption .. "  [" .. label .. "]" end
      if c.caption ~= caption then
        c.caption = caption
        if c.rows then c.rows[3].w:SetText(caption) end
      end
    end
  end
  if c.masked then
    if c.source:GetOpacity()>0 then c.source:SetOpacity(0) end
    return
  end
  c.age=c.age+dt
  if c.age>3 then release(); return end
  local entity=Game.FindEntityByID(c.id)
  if not entity then return end
  local comp=entity:FindComponentByName(CName.new("vrp_menu_ui"))
  local root=comp and comp:GetWidget()
  if not root then return end
  if not c.rows then c.rows=build(root,c.title,c.body,c.caption or c.footer,c.glyph); return end
  -- Leave the original readable until the presentation is sized and drawn.
  if not VRMenuSurfaceReady(comp,root) or c.source:GetOpacity()<.95 then return end
  for _,row in ipairs(c.rows) do
    if row.w:GetDesiredSize().Y>row.height then release(); return end
  end
  local mesh=entity:FindComponentByName(CName.new("display"))
  if not mesh then return end
  mesh:Toggle(true)
  c.opacity=c.source:GetOpacity()
  c.source:SetOpacity(0)
  c.masked=true
end

function M.update(dt)
  if not pending and not card then return end
  local ok, err=pcall(update,dt and dt>0 and dt or .016)
  if not ok then pending=nil; release(); print("[VR tutorial] "..tostring(err)) end
end
return M
