-- CyberpunkVRPort -- the two braindance HUD pieces that neither an asset nor redscript can carry.
--
-- Most of the braindance layout IS baked into the widgets: braindance.inkwidget holds the seven panels,
-- tutorial_braindance.inkwidget holds the overlay centring and its silenced decor, subtitles.inkwidget
-- holds the 0.7. The scanner's braindance-only half size lives in the redscript mod
-- CyberpunkVRPort_ScannerHud, which already re-applies on every scanner open and already owns a scale for
-- those panels. Two things are left here, each for a reason that was measured rather than assumed:
--
--   1. THE "Сейчас вы не можете этого сделать" PLATE. Its container, NotificationRoot, is a spawn slot
--      (SpawnLibraryItemController, library item `notification_layer`) declared in a HUD-level widget
--      that is in neither the notifications nor the tutorial folder: 63 widget files from both were
--      extracted and searched by name, and it is in none of them. There is no asset to edit.
--   2. THE BOX BEHIND "ЗАКРЫТЬ". The button has no box in the asset -- two widgets have to be CREATED,
--      and an .inkwidget edit can change numbers, not add widgets.
--
-- WHY NOT REDSCRIPT. Both sit in other subtrees than any scriptable controller, so reaching them needs a
-- walk UP and then down, and `scc` rejects the only method that would do it: "method 'GetParentWidget'
-- not found on 'inkWidget'" -- present in RTTI, absent from the script bindings. CET can enumerate the
-- ink layers directly, which is how every number below was measured in the first place.
--
-- WHY THIS IS EVENT-DRIVEN AND NOT A TIMER. The first version walked every ink layer twice a second from
-- onUpdate and froze the game -- reported as "каждый кадр игра фризит", and correctly: a full HUD walk
-- is thousands of widgets. Nothing runs outside a braindance now; the walk is armed by the braindance
-- controller's own OnInitialize, the budget is finite, and it stops as soon as both pieces are placed.

local NOTIF_SCALE  = 0.8      -- read back out of the game after the picture was approved
local NOTIF_Y      = 250.0    -- 200 on screen: the container above it carries a 0.80 scale
local BTN_W        = 285.0    -- the button row: icon 64 + label 181
local BTN_H        = 64.0
local BTN_MR       = 204.0    -- its own margins, so the box is placed from the button, not by taste
local BTN_MB       = 250.0
local PAD_X        = 26.0
local PAD_Y        = 20.0
local FRAME        = 4.0

local ATTEMPTS     = 24       -- the tutorial overlay appears later than the braindance itself
local INTERVAL     = 0.5

-- Tints copied from the plates' own bg and fg. The plates build their box out of the cell_bg / tut_fg /
-- box_glow2 parts of one texture atlas, and a widget created at runtime CANNOT be given that atlas --
-- neither SetAtlasResource nor the atlasTexture field accepts it, and the images then draw nothing at
-- all. A tinted rectangle draws, so the colour matches even though the texture and the glow do not.
local TINT_FRAME   = { Red = 0.0,  Green = 1.18, Blue = 0.92, Alpha = 1.0 }
local TINT_FILL    = { Red = 0.02, Green = 0.03, Blue = 0.07, Alpha = 1.0 }

local armed     = false
local left      = 0
local timer     = 0.0
local plateDone = false
local boxDone   = false

local function inBraindance()
  local on = false
  pcall(function()
    local sys = Game.GetScriptableSystemsContainer():Get(CName.new("BraindanceSystem"))
    if sys ~= nil then on = sys:GetIsInBraindance() end
  end)
  return on
end

local function V2(x, y)
  return Vector2.new({ X = x, Y = y })
end

-- Both names in ONE walk, and it stops the moment it has them: an earlier version walked the tree once
-- per name, which doubled the cost of the case that finds nothing -- and finding nothing is the normal
-- case for a name whose widget is not up yet.
local function collect(w, depth, out)
  if depth > 10 then return end
  local n = 0
  pcall(function() n = w:GetNumChildren() end)
  for i = 0, n - 1 do
    if out.plate and out.hints then return end
    local c = nil
    pcall(function() c = w:GetWidgetByIndex(i) end)
    if c then
      local nm = ""
      pcall(function() nm = tostring(c:GetName().value) end)
      if nm == "NotificationRoot" then
        out.plate = out.plate or c
      elseif nm == "input_hints" then
        out.hints = out.hints or c
      end
      collect(c, depth + 1, out)
    end
  end
end

-- The plate is a PERSISTENT slot: it sits in the tree with visibility off while nothing is being
-- announced, so placing it once is enough. Proven live -- the placement survived the plate being shown
-- and hidden repeatedly.
local function placePlate(plate)
  pcall(function() plate:SetAnchor(inkEAnchor.Centered) end)
  pcall(function() plate:SetAnchorPoint(V2(0.5, 0.5)) end)
  pcall(function() plate:SetMargin(inkMargin.new({ left = 0.0, top = 0.0, right = 0.0, bottom = 0.0 })) end)
  pcall(function() plate:SetRenderTransformPivot(V2(0.5, 0.5)) end)
  pcall(function() plate:SetScale(V2(NOTIF_SCALE, NOTIF_SCALE)) end)
  pcall(function() plate:SetTranslation(V2(0.0, NOTIF_Y)) end)
  return true
end

local function rect(parent, w, h, mr, mb, tint)
  local x = nil
  pcall(function() x = NewObject("inkRectangleWidget") end)
  if not x then return end
  pcall(function() x:SetAnchor(inkEAnchor.BottomRight) end)
  pcall(function() x:SetAnchorPoint(V2(1.0, 1.0)) end)
  pcall(function() x:SetSize(V2(w, h)) end)
  pcall(function() x:SetMargin(inkMargin.new({ left = 0.0, top = 0.0, right = mr, bottom = mb })) end)
  pcall(function() x:SetTintColor(HDRColor.new(tint)) end)
  pcall(function() x:SetOpacity(1.0) end)
  pcall(function() x:SetVisible(true) end)
  pcall(function() parent:AddChildWidget(x) end)
end

-- Counted, not named: SetName on a widget created this way does not stick -- the children read back
-- unnamed -- so "have I built this already" is answered by the child count. The authored input_hints
-- holds exactly one child, its button row.
local function buildCloseBox(hints)
  local n = 0
  pcall(function() n = hints:GetNumChildren() end)
  if n ~= 1 then
    return n > 1        -- already built: stop trying
  end
  local row = nil
  pcall(function() row = hints:GetWidgetByIndex(0) end)
  if not row then return false end
  rect(hints, BTN_W + 2 * PAD_X + 2 * FRAME, BTN_H + 2 * PAD_Y + 2 * FRAME,
       BTN_MR - PAD_X - FRAME, BTN_MB - PAD_Y - FRAME, TINT_FRAME)
  rect(hints, BTN_W + 2 * PAD_X, BTN_H + 2 * PAD_Y,
       BTN_MR - PAD_X, BTN_MB - PAD_Y, TINT_FILL)
  -- Appending puts the rectangles ON TOP of the button, which is what happened the first time. The row
  -- goes back to the end of the list so it draws last.
  local m = 0
  pcall(function() m = hints:GetNumChildren() end)
  pcall(function() hints:ReorderChild(row, m - 1) end)
  return true
end

local function pass()
  local sys = nil
  pcall(function() sys = Game.GetInkSystem() end)
  if not sys then return end
  local layers = nil
  pcall(function() layers = sys:GetLayers() end)
  if not layers then return end
  local out = {}
  for i = 1, #layers do
    if out.plate and out.hints then break end
    local vw = nil
    pcall(function() vw = layers[i]:GetVirtualWindow() end)
    if vw then collect(vw, 0, out) end
  end
  if out.plate and not plateDone then
    plateDone = placePlate(out.plate)
  end
  if out.hints and not boxDone then
    boxDone = buildCloseBox(out.hints)
  end
end

-- VERIFIED, not trusted. BraindanceGameController is a HUD game controller: it is built with the rest
-- of the HUD and initialises on every load, braindance or not. Arming on that alone spent the whole
-- budget walking the ink tree after each save load -- reported as "какие-то фризы где-то секунд 5-10
-- после загрузки в сейв". The hook is still the cheapest trigger; BraindanceSystem is the judge.
local function arm()
  if not inBraindance() then return end
  armed     = true
  left      = ATTEMPTS
  timer     = 0.0
  plateDone = false
  boxDone   = false
end

local function disarm()
  armed = false
  left  = 0
end

registerForEvent("onInit", function()
  -- The braindance controller is the state signal: it exists exactly while a braindance runs, so the
  -- work is armed and disarmed by its own lifetime and nothing has to poll for the state.
  Observe("BraindanceGameController", "OnInitialize", arm)
  Observe("BraindanceGameController", "OnUnInitialize", disarm)
end)

registerForEvent("onUpdate", function(dt)
  if not armed then return end
  if plateDone and boxDone then
    disarm()
    return
  end
  timer = timer - dt
  if timer > 0.0 then return end
  timer = INTERVAL
  -- Re-checked on every attempt: a braindance that ends before the budget is spent must stop the walk,
  -- and a hook that fired without a braindance must cost one system lookup rather than 24 walks.
  if not inBraindance() then
    disarm()
    return
  end
  left = left - 1
  if left <= 0 then
    disarm()
    return
  end
  pcall(pass)
end)
