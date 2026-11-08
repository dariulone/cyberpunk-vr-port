-- The menu itself: placement, the pointers, the screens. init.lua drives it (M.init / M.update /
-- M.release) and can read it again off disk with GetMod("CyberpunkVRPort_Menu").hot().

local K = require("modules/ink")
local D = require("modules/data")
local V = require("modules/scroll")
local Stick = require("modules/stick_scroll")
local Hud = require("modules/hud_fade")
local Set = require("modules/settings")
local Msg = require("modules/messages")
local Gameplay = require("modules/gameplay")
local InventoryData = require("modules/inventory_data")
local CyberwareData = require("modules/cyberware_data")
local VendorVisit = require("modules/vendor_visit")
local StashVisit = require("modules/stash_visit")
local LoadData = require("modules/load_data")
local PICS = require("modules/pictures")
local L = require("modules/locale")
local Space = require("modules/space")

local M = {}
-- For the lobby's menu in the main menu, where this menu does not open: the port's settings as data, and the
-- scrolling list (modules/scroll.lua) its pages scroll in.
M.settingsData = require("modules/settings_data")
M.scrollKit = V
local retiring = {}                    -- entity IDs whose asynchronous spawn outlived close()

local ENT = "vrport\\ui\\vr_menu.ent"
local BEAM_ENT = "vrport\\ui\\vr_menu_beam.ent"
local GLASS_ENT = "vrport\\ui\\vr_menu_glass.ent"
local OPAQUE_ENT = "vrport\\ui\\vr_menu_opaque.ent"
local ICON_ENT = "vrport\\ui\\vr_menu_icon.ent"
local UI = "vrp_menu_ui"

local CW, CH, PX = 3200, 1600, 3200       -- canvas pixels, pixels per metre
local W, H = CW / PX, CH / PX              -- 1.00 x 0.50 m
local BEAM_LEN = K.BEAM.len

-- Layout, canvas pixels. The initial column drops into the centre. Opening a section slides it
-- left to COLX; every submenu then opens on its right.
local COLX = 1100                          -- the icon column's centre line
local COL_HOME = CW / 2 - COLX             -- closed sections: the column is centred on the viewer
local ICON = 108
-- The close ring sits apart, above the rest and smaller, out of the way of a hand going for the others.
local COL = { close = 120, person = 270, map = 390, quests = 510, contacts = 630, vehicles = 750,
  radio = 870, timeskip = 990, game = 1110, settings = 1230 }
local CLOSE_ICON = 100
-- The money and weight strip beside the close ring: its rim, and the glass inset inside the rim.
local RES_H, RES_INSET = 60, 5
local COLY = (COL.person + COL.settings) / 2 -- the column's middle
local P1X, P1Y, P1W = 1280, 250, 820
local HEAD = 96
local ROW = 112                            -- a list row with its gap
local VIEW_ROWS = 8                        -- a list's window, in rows; a longer list scrolls in it
local LIST_H = VIEW_ROWS * ROW
local ARROW_H, ARROW_GAP = 52, 8           -- the list arrows above and below a window
local P2X, P2W = 2160, 640
local SW = P2X + P2W - P1X                 -- the settings panel spans both list panels
local PAD = 14                             -- rows inside a panel's glass
local TOAST_Y = 1480

-- Pointers, metres.
local HOVER, PRESS, RELEASE, THROUGH = 0.10, 0.010, 0.025, -0.05
local TRIGGER_ON, TRIGGER_OFF = 0.70, 0.40
local WALK_AWAY = 1.1
-- A press in a list stays a tap until it has moved this far (canvas px); then it scrolls the list, or
-- slides the slider it landed on when it went sideways. A FINGER'S OWN PUSH DOES NOT COUNT: pushed into
-- a screen it meets at an angle, it slides the touch point along the screen by itself (measured: 1.3 px
-- per px of depth), and pulling back out slides it back. So the push's line -- from where it touched to
-- the deepest it went -- is taken off the tip's position, for the tap test and for what it drags after
-- (a slider too). The first LINE_START of depth draws the line; a deeper point after that refines it
-- only when it lies on it (a finger that slides while it presses deeper is sliding), and the line is
-- settled once the tip has moved a TAP off it.
-- A POINTER is let settle first: pulling the trigger moves the hand that aims it (measured: the ray went
-- 30 px across the screen with the trigger alone), so the press point is where it points SETTLE seconds
-- after the pull.
local TAP = { finger = 32, trigger = 40 }
local SLOPE_MAX = 2.5                      -- px along the screen per px of depth, at most
local SETTLE = 0.12
local LINE_START = 0.008

-- Only an accepted click/slider press vibrates. Hover, dragging and scrolling stay silent.
local HAPTIC_CLICK = 20

local TEXT = {
  en = {
    lang = "en",
    contacts = "CONTACTS", vehicles = "VEHICLES", radio = "RADIO", settings = "VR SETTINGS",
    call = "CALL", messages = "MESSAGES", on = "ON", off = "OFF", volume = "VOLUME",
    playing = "NOW PLAYING", empty = "Nothing here yet", blocked = "Not available right now",
    calling = "Calling…", summoned = "On its way", mounted = "You are driving already",
    repair = "Being repaired", noMessages = "No messages",
    game = "GAME", save = "Save Game", load = "Load Game", options = "Settings", credits = "Credits",
    character = "CHARACTER",
    messagesTab = "MESSAGES", contactsTab = "CONTACTS", allMessages = "All messages",
    showRead = "Show read", hideRead = "Hide read", summon = "CALL VEHICLE", ready = "Ready",
    exit = "Exit to Main Menu", confirmExit = "Press again: main menu", quit = "Exit Game",
    confirmQuit = "Press again: quit the game", noMenu = "The game menu is not ready yet",
    hint = "Point at a setting to read what it does", loading = "Reading the settings…",
  },
  ru = {
    lang = "ru",
    contacts = "КОНТАКТЫ", vehicles = "ТРАНСПОРТ", radio = "РАДИО", settings = "НАСТРОЙКИ VR",
    call = "ПОЗВОНИТЬ", messages = "СООБЩЕНИЯ", on = "ВКЛ", off = "ВЫКЛ", volume = "ГРОМКОСТЬ",
    playing = "СЕЙЧАС ИГРАЕТ", empty = "Пока пусто", blocked = "Сейчас недоступно",
    calling = "Вызов…", summoned = "Уже едет", mounted = "Вы уже за рулём",
    repair = "В ремонте", noMessages = "Нет сообщений",
    game = "ИГРА", save = "Сохранить игру", load = "Загрузить игру", options = "Настройки", credits = "Титры",
    character = "ПЕРСОНАЖ",
    messagesTab = "СООБЩЕНИЯ", contactsTab = "КОНТАКТЫ", allMessages = "Все сообщения",
    showRead = "Показать прочитанные", hideRead = "Скрыть прочитанные", summon = "ВЫЗВАТЬ", ready = "Готов",
    exit = "Выйти в главное меню", confirmExit = "Ещё раз — в главное меню", quit = "Выйти из игры",
    confirmQuit = "Ещё раз — выход из игры", noMenu = "Меню игры ещё не готово",
    hint = "Наведите на параметр — здесь будет подсказка", loading = "Чтение настроек…",
  },
}
local T = TEXT.en

local S = {
  time = 0.0,
  open = false,
  anchor = nil,          -- { C, R, U } in the world, or in the vehicle's frame when .vehicle
  menu = nil,            -- { id, entity, comp, root, built }
  beams = { {}, {} },
  section = nil,
  items = nil,
  contact = nil,
  hands = { {}, {} },
  buttons = {},
  views = {},            -- the scroll views on the screen, by name
  hovered = {},
  anims = {},
  toast = nil,
  closeAt = nil,
  refreshAt = nil,
  capOn = false,
  stats = {},
  glass = {},            -- the glass planes behind the panels (entity handles, reused)
  glassRects = {},       -- this drawing's panels, canvas px: { x, y, w, h }
  glassOn = true,
  glassAfter = 0,        -- new glass waits until the first widget of the latest slide-in is half in
  slideAt = nil,         -- the tick (S.time) of the latest slide-in
  haptics = {},          -- vibrations sent, by kind: for the harness
  clicks = {},           -- the last clicks: for the harness
  columnOffset = COL_HOME,
  columnTarget = COL_HOME,
  panelInteractiveAt = 0,
}

---------------------------------------------------------------- vectors

local add, sub, mul, dot, cross = Space.add, Space.sub, Space.mul, Space.dot, Space.cross
local norm, clamp, qrot, qconj = Space.norm, Space.clamp, Space.qrot, Space.qconj
local transformFor = Space.transformFor
local camera = Space.view

---------------------------------------------------------------- the game's state

local slotWeaponRight = nil
local function armed()
  local pl = Game.GetPlayer()
  if pl == nil then return false end
  if slotWeaponRight == nil then
    pcall(function() slotWeaponRight = TweakDBID.new("AttachmentSlots.WeaponRight") end)
    if slotWeaponRight == nil then return false end
  end
  local out = false
  pcall(function() out = Game.GetTransactionSystem():GetItemInSlot(pl, slotWeaponRight) ~= nil end)
  return out
end

-- THE BOARD OF THE PLAYER THERE IS NOW. It was taken once, from the first player the menu saw. The VR training
-- swaps the player (V, its own puppet, V again), and a board left over from the puppet that is gone kept its last
-- scene tier: the chord asked a cinematic tier that no longer existed and the menu would not open.
local bbDefs, psm, psmFor = nil, nil, nil
local function sceneTier()
  local pl = Game.GetPlayer()
  local tier = 0
  if pl == nil then return tier end
  pcall(function()
    if bbDefs == nil then bbDefs = Game.GetAllBlackboardDefs() end
    local who = tostring(pl:GetEntityID().hash)
    if psm == nil or psmFor ~= who then
      psm = Game.GetBlackboardSystem():GetLocalInstanced(pl:GetEntityID(), bbDefs.PlayerStateMachine)
      psmFor = who
    end
    tier = psm:GetInt(bbDefs.PlayerStateMachine.SceneTier)
  end)
  return tier
end

-- Every weapon put away, the way the weapon wheel's holster wedge does it (modules/wheel.lua).
local function holsterAll()
  local pl = Game.GetPlayer()
  if pl == nil then return end
  pcall(function()
    local r = NewObject("EquipmentSystemWeaponManipulationRequest")
    r.owner = pl
    r.requestType = EquipmentManipulationAction.UnequipAll
    r.equipAnimType = gameEquipAnimationType.Default
    Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem")):QueueRequest(r)
  end)
end

local function paused()
  local p = false
  pcall(function() p = Game.GetSystemRequestsHandler():IsGamePaused() end)
  return p
end

-- The main menu and the lobby in it: no game to have a menu of, and the lobby has its own.
local function preGame()
  local p = false
  pcall(function() p = Game.GetSystemRequestsHandler():IsPreGame() end)
  return p
end

local function sound(name)
  pcall(function() Game.GetAudioSystem():Play(CName.new(name)) end)
end

-- THE SOUNDS, picked from among the game's UI sounds (build/vr-menu-20261021/sound_folder.py lays
-- the candidates out to listen to). Two of them are one variant of an event that plays a random one each
-- time, so all four ship as WAVs (sounds\) played by the port (VRMenuPlayWav) at the game's effects
-- volume; the event is the fallback when the port cannot play them.
local SOUND_DIR = "CyberpunkVRPort_Menu/sounds/"
local SOUNDS = {
  open = { "open.wav", "ui_messenger_recieved" },     -- its first variant
  close = { "close.wav", "ui_gui_close" },
  click = { "click.wav", "ui_terminal_positive" },
  notify = { "notify.wav", "ui_messenger_select" },  -- its third variant: a toast, the exit confirmation
}

local function play(kind)
  local s = SOUNDS[kind]
  if s == nil then return end
  if S.inClick then S.soundInClick = true end
  local ok, played = false, false
  if type(VRMenuPlayWav) == "function" then
    ok, played = pcall(VRMenuPlayWav, SOUND_DIR .. s[1], S.volume or 1.0)
  end
  if not (ok and played) then sound(s[2]) end
end

local function haptic(hand, kind)
  if hand == nil or type(VRHapticEvent) ~= "function" then return end
  kind = kind or HAPTIC_CLICK
  S.haptics[kind] = (S.haptics[kind] or 0) + 1
  pcall(VRHapticEvent, kind, hand == 1 and 1 or 2)
end

---------------------------------------------------------------- entities

local function spawn(path, tr)
  local id = nil
  pcall(function() id = exEntitySpawner.Spawn(path, tr, "") end)
  return { id = id, at = S.time }
end

local function retireEntity(id)
  local ok, done = pcall(function()
    local e = Game.FindEntityByID(id)
    if e == nil then return false end
    exEntitySpawner.Despawn(e)
    return true
  end)
  return ok and done
end

local function retirePending()
  for i = #retiring, 1, -1 do
    if retireEntity(retiring[i]) then table.remove(retiring, i) end
  end
end

local function despawn(h)
  if h == nil or h.id == nil then return end
  if not retireEntity(h.id) then retiring[#retiring + 1] = h.id end
  h.id, h.entity, h.comp, h.root = nil, nil, nil, nil
end

-- The entity, its widget component and the widget root, once the spawn has landed.
local function resolve(h)
  if h == nil or h.id == nil then return false end
  if h.root ~= nil then return true end
  pcall(function()
    h.entity = Game.FindEntityByID(h.id)
    if h.entity ~= nil then h.comp = h.entity:FindComponentByName(CName.new(UI)) end
    if h.comp ~= nil then h.root = h.comp:GetWidget() end
  end)
  if h.root ~= nil then
    h.readyIn = S.time - h.at
    return true
  end
  return false
end

local function unglitch(h)
  if h.comp == nil then return end
  pcall(function() if h.comp.glitchValue ~= 0 then h.comp.glitchValue = 0.0 end end)
end

local function place(h, tr)
  if h.entity == nil then return end
  pcall(function() Game.VREntitySetWorldTransform(h.entity, tr) end)
end

-- The native ink target is independent of atlas coverage, so it can paint with the display
-- disabled. Reveal only after the GPU finished a write made after this canvas was built.
-- No geometry is parked under the floor and no guessed number of update ticks is involved.
local function ready(h)
  if h.ready then return true end
  if h.comp == nil or h.root == nil or type(VRMenuSurfaceReady) ~= "function" then return false end
  local ok, done = pcall(VRMenuSurfaceReady, h.comp, h.root)
  if not ok or not done then return false end
  local display = h.entity:FindComponentByName(CName.new("display"))
  if display == nil then return false end
  local shown = pcall(function() display:Toggle(true) end)
  h.ready = shown
  return h.ready
end

---------------------------------------------------------------- the glass

-- THE GLASS behind the panels: the engine's frosted glass (vr_menu_glass.ent) on planes of their own,
-- GLASS_BACK behind the screen, each scaled to its panel. The screen is additive and cannot darken what is
-- behind it; the glass does, and blurs it, so the light drawn on the screen reads over a bright street.
-- One plane per panel, reused as the panels change; one with no panel is switched off. A plane starts
-- disabled, and none comes on before the screen's native target is ready, nor before the first widget of a
-- slide-in is half in (S.glassAfter): dark glass with no light on it is the black screen again.
local GLASS_BACK = 0.006

local function glassMesh(h)
  if h.comp ~= nil then return h.comp end
  pcall(function()
    h.entity = Game.FindEntityByID(h.id)
    if h.entity ~= nil then h.comp = h.entity:FindComponentByName(CName.new("display")) end
  end)
  return h.comp
end

local function despawnGlass()
  for i = 1, #S.glass do despawn(S.glass[i]) end
  S.glass = {}
end

local function syncGlass(C, R, U, moving)
  local want = {}
  for _,b in ipairs(S.icons or {}) do
    local alpha=b.canvas:GetOpacity();local margin=b.canvas:GetMargin()
    local size=b.w-8
    want[#want+1]={x=margin.left+S.columnOffset+(b.w-size)/2,y=margin.top+(b.h-size)/2,
      w=size,h=size,icon=true,visible=alpha>.05}
  end
  if S.summaryRect then
    S.summaryRect.visible=S.layerSummary and S.layerSummary:GetOpacity()>.2
    want[#want+1]=S.summaryRect
  end
  if S.resourceRect then want[#want+1]=S.resourceRect end
  if S.glassOn then for _,r in ipairs(S.glassRects) do
    r.visible=S.layerPanel and S.layerPanel:GetOpacity()>.2
    want[#want+1]=r
  end end
  local N = cross(R, U)
  for i = 1, math.max(#want, #S.glass) do
    local r, h = want[i], S.glass[i]
    local asset=r and (r.icon and ICON_ENT or r.opaque and OPAQUE_ENT or GLASS_ENT)
    if h and r and h.asset~=asset then despawn(h);S.glass[i]=nil;h=nil end
    if r ~= nil and h == nil then
      h = spawn(asset, transformFor(C, R, U));h.asset=asset
      S.glass[i] = h
    end
    if h ~= nil and glassMesh(h) ~= nil then
      if r == nil or r.visible==false then
        if h.shown ~= false then
          pcall(function() h.comp:Toggle(false) end)
          h.shown = false
        end
      else
        local key = string.format("%d,%d,%d,%d", r.x, r.y, r.w, r.h)
        if h.key ~= key then
          pcall(function()
            h.comp.visualScale = Vector3.new(r.w / PX, r.h / PX, 1.0)
            h.comp:Toggle(false)
          end)
          h.shown = false
        end
        if h.key ~= key or moving or h.anchor ~= S.anchor then
          local cx, cy = (r.x + r.w / 2 - CW / 2) / PX, (CH / 2 - (r.y + r.h / 2)) / PX
          local depth=r.depth or (r.icon and .0006 or GLASS_BACK)
          place(h, transformFor(add(add(add(C, mul(R, cx)), mul(U, cy)), mul(N, -depth)), R, U))
          h.key, h.anchor = key, S.anchor
        end
        if h.shown ~= true and S.menu ~= nil and S.menu.placed and (r.icon or S.time >= S.glassAfter) then
          pcall(function() h.comp:Toggle(true) end)
          h.shown = true
        end
      end
    end
  end
end

---------------------------------------------------------------- the anchor

-- The plane's frame this frame: fixed in the world on foot, carried in the car's frame when mounted.
local function frame()
  local a = S.anchor
  if a == nil then return nil end
  if a.vehicle == nil then return a.C, a.R, a.U end
  local v = D.mounted()
  if v == nil then return nil end
  local p, q
  pcall(function()
    local wp = v:GetWorldPosition()
    p, q = { wp.x, wp.y, wp.z }, v:GetWorldOrientation()
  end)
  if p == nil then return nil end
  return add(p, qrot(q, a.C)), qrot(q, a.R), qrot(q, a.U)
end

-- WHERE THE MENU GOES, however it was asked for: straight ahead of the head, square to its heading -- the
-- middle of the screen (the column with the list beside it) on the line of sight, a little low, at the
-- reach of a finger, leaning back by the angle it is seen at so it faces the eyes.
local REACH, DROP = 0.50, -0.18
local function anchorFor(cam)
  local up = { 0, 0, 1 }
  local f, R = cam.f, cam.r
  local lean = clamp(math.atan2(-DROP, REACH), 0.0, math.rad(25))
  local U = add(mul(up, math.cos(lean)), mul(f, math.sin(lean)))
  local C = add(add(cam.p, mul(f, REACH)), mul(up, DROP))
  local a = { C = C, R = R, U = U }
  local v = D.mounted()
  if v ~= nil then
    pcall(function()
      local wp = v:GetWorldPosition()
      local qi = qconj(v:GetWorldOrientation())
      a = { vehicle = true, C = qrot(qi, sub(C, { wp.x, wp.y, wp.z })), R = qrot(qi, R), U = qrot(qi, U) }
    end)
  end
  return a
end

---------------------------------------------------------------- buttons

-- WHERE A BUTTON IS. Widgets are built in canvases of their own, so each one is told where its canvas's
-- top-left is on the screen: (ox, oy) in screen canvas pixels -- or, inside a scrolling list, in the
-- list's content, which slides under the list's window (the button is hit where it is drawn, and only
-- inside the window).
local function at(ox, oy, view) return { ox = ox, oy = oy, view = view } end
local ORIGIN = at(0, 0)

local function clearButtons(group)
  for i = #S.buttons, 1, -1 do
    if S.buttons[i].group == group then table.remove(S.buttons, i) end
  end
end

-- Every button of a list that is going away.
local function dropView(view)
  for i = #S.buttons, 1, -1 do
    if S.buttons[i].view == view then table.remove(S.buttons, i) end
  end
end

local function button(group, H, x, y, w, h, click, paint)
  local b = { group = group, x = H.ox + x, y = H.oy + y, w = w, h = h, click = click, paint = paint, view = H.view }
  S.buttons[#S.buttons + 1] = b
  if paint then paint(false) end
  return b
end

local function buttonAt(x, y)
  for i = #S.buttons, 1, -1 do
    local b = S.buttons[i]
    local by = b.y
    if b.view ~= nil then by = b.view.y + b.y - b.view.offset end
    local visible = b.visible ~= false and (b.group == "col" or S.time >= S.panelInteractiveAt)
      and (S.modalGroup==nil or b.group==S.modalGroup or b.group=="col")
    if visible and x >= b.x and x <= b.x + b.w and y >= by and y <= by + b.h
      and (b.view == nil or b.view:visibleAt(x, y)) then
      if b.blocker then return nil end
      local dx, dy = (x - b.x) / b.w - 0.5, (y - by) / b.h - 0.5
      if (not b.circle or dx * dx + dy * dy <= 0.25)
        and (not b.contains or b.contains(x-b.x,y-by)) then return b end
    end
  end
  return nil
end

local function toast(msg)
  S.toast = { text = msg, until_ = S.time + 2.5 }
  play("notify")
  if S.toastWidget ~= nil then
    pcall(function()
      S.toastWidget:SetText(msg or "")
      S.toastWidget:SetVisible(true)
      S.toastBg:SetVisible(true)
    end)
  end
end

---------------------------------------------------------------- animation

-- SAO's drop: the column falls into place icon by icon, a list unrolls row by row, the detail panel
-- slides out of the list. A widget goes from (x - dx, y - dy) and transparent to (x, y) and opaque.
local ANIM = 0.16
local COLUMN_SHIFT = 0.26

local function moveColumn(target)
  if S.columnTarget == target then return end
  S.columnTarget = target
  S.columnMove = { from = S.columnOffset, to = target, t0 = S.time }
end

local function stepColumn()
  local a = S.columnMove
  if a ~= nil then
    local t = clamp((S.time - a.t0) / COLUMN_SHIFT, 0, 1)
    local e = t * t * (3 - 2 * t)
    S.columnOffset = a.from + (a.to - a.from) * e
    if t >= 1 then S.columnOffset, S.columnMove = a.to, nil end
  end
  if S.layerCol ~= nil then K.move(S.layerCol, S.columnOffset, 0) end
  if S.resourceRect then S.resourceRect.x=COLX+S.columnOffset+74+RES_INSET end
  for _, b in ipairs(S.icons or {}) do b.x = b.x0 + S.columnOffset end
end

local function slideIn(group, w, x, y, delay, dx, dy, hitButton)
  if group ~= "col" then delay = delay + math.max(0, (S.panelStart or S.time) - S.time) end
  w:SetOpacity(0.0)
  K.move(w, x - dx, y - dy)
  S.anims[#S.anims + 1] = { group = group, w = w, x = x, y = y, dx = dx, dy = dy,
    t0 = S.time + delay, button = hitButton }
  if hitButton ~= nil then hitButton.visible = false end
  if group ~= "col" then S.panelInteractiveAt = math.max(S.panelInteractiveAt, S.time + delay + ANIM) end
  -- one drawing's slide-ins all start in one tick: the glass waits for the earliest of them
  local lit = S.time + delay + ANIM / 2
  if S.slideAt ~= S.time then
    S.slideAt, S.glassAfter = S.time, lit
  else
    S.glassAfter = math.min(S.glassAfter, lit)
  end
end

local function dropAnims(group)
  local keep = {}
  for _, a in ipairs(S.anims) do
    if a.group ~= group then keep[#keep + 1] = a end
  end
  S.anims = keep
end

local function stepAnims()
  if #S.anims == 0 then return end
  local keep = {}
  for _, a in ipairs(S.anims) do
    local t = clamp((S.time - a.t0) / ANIM, 0, 1)
    local e = 1 - (1 - t) * (1 - t)
    pcall(function()
      a.w:SetOpacity(e)
      K.move(a.w, a.x - a.dx * (1 - e), a.y - a.dy * (1 - e))
    end)
    if a.button ~= nil then
      a.button.x = a.x - a.dx * (1 - e) + S.columnOffset
      a.button.y = a.y - a.dy * (1 - e)
      a.button.visible = e > 0.05
    end
    if t < 1 then keep[#keep + 1] = a end
  end
  S.anims = keep
end

---------------------------------------------------------------- drawing

local build = {}

-- GLYPHS ARE SIZED BY WHAT IS DRAWN IN THEM, not by their atlas cell: the mappin icons share a 64 x 64
-- cell but the cog fills 21 px of it and the car 62 x 20, and the phone glyphs are cut tight. Each entry
-- carries its cell and the opaque box inside it (measured off the textures, alpha > 40), and glyph()
-- scales that box to one visual area -- capped in its long side, so the flat car does not run wide.
-- { atlas, part, cell w, cell h, box left, top, right, bottom [, scale for a glyph that reads light] }
local GLYPH = {
  call = { K.ATLAS.phone, "phone_icon", 47, 44, 0, 0, 47, 44 },
  message = { K.ATLAS.mappin, "talk", 64, 64, 5, 7, 59, 58 },
  car = { K.ATLAS.mappin, "car", 64, 64, 1, 22, 63, 42 },
  motorcycle = { K.ATLAS.mappin, "motorcycle", 64, 64, 1, 17, 63, 45 },
  weaponized = { K.ATLAS.mappin, "vehicle_weaponized", 64, 64, 0, 11, 64, 54 },
  cog = { K.ATLAS.mappin, "cog", 64, 64, 21, 18, 42, 39, 1.12 },
  radio = { K.ATLAS.phone, "radio_icon", 57, 51, 1, 0, 57, 51 },
  save = { K.ATLAS.mappin, "download", 64, 64, 12, 19, 52, 42 },
  load = { K.ATLAS.mappin, "journal_all", 64, 64, 8, 10, 56, 52 },
  credits = { K.ATLAS.mappin, "codex_glossary", 64, 64, 13, 7, 53, 56 },
  exit = { K.ATLAS.mappin, "apartment", 64, 64, 1, 3, 63, 61, 0.85 },    -- home: the main menu
  quit = { K.ATLAS.mappin, "Off", 64, 64, 11, 9, 54, 56 },             -- power: the desktop
  person = { K.ATLAS.mappin, "codex_character", 64, 64, 13, 10, 51, 54 },
  map = { "base\\gameplay\\gui\\fullscreen\\hub_menu\\hub_atlas.inkatlas", "ico_map", 57, 57, 0, 0, 57, 57, 1.12 },
}

-- The gamepad has no glyph in the game's atlases, so it is drawn: a body and two grips in the colour,
-- the d-pad and two buttons punched out of it in black (black is nothing on this screen). Returns the
-- coloured parts, the ones a hover tints.
local function gamepadGlyph(c, cx, cy, S, col)
  local parts = {
    K.rect(c, cx - 0.25 * S, cy - 0.12 * S, 0.50 * S, 0.20 * S, col, 1.0),
    K.image(c, cx - 0.33 * S, cy - 0.10 * S, 0.24 * S, 0.24 * S, K.ATLAS.phone, "circle", col, 1.0),
    K.image(c, cx + 0.09 * S, cy - 0.10 * S, 0.24 * S, 0.24 * S, K.ATLAS.phone, "circle", col, 1.0),
  }
  local k, dx, dy = 0.035 * S, cx - 0.20 * S, cy - 0.02 * S
  K.rect(c, dx - 0.06 * S, dy - k / 2, 0.12 * S, k, K.BLACK, 1.0)
  K.rect(c, dx - k / 2, dy - 0.06 * S, k, 0.12 * S, K.BLACK, 1.0)
  local b = 0.06 * S
  K.image(c, cx + 0.14 * S, cy - 0.09 * S, b, b, K.ATLAS.phone, "circle", K.BLACK, 1.0)
  K.image(c, cx + 0.21 * S, cy - 0.02 * S, b, b, K.ATLAS.phone, "circle", K.BLACK, 1.0)
  return parts
end

-- The station logos (radiostations_icons.inkatlas, 1008 x 1184), each its own shape.
local LOGO = {
  ["30_principales"] = { 292, 260 }, body_heat = { 352, 192 }, dark_star = { 350, 153 },
  growl_fm = { 244, 323 }, impulse_999 = { 343, 308 }, morro_rock = { 400, 192 }, night_fm = { 344, 146 },
  no_station = { 230, 214 }, pacific_dreams = { 267, 253 }, radio_pebkac = { 355, 211 },
  radio_samizdat = { 239, 292 }, ritual = { 344, 328 }, royal_blue = { 362, 199 }, the_dirge = { 335, 178 },
  vexElsTrom = { 382, 129 },
}

local function fit(parent, cx, cy, boxW, boxH, atlas, part, pw, ph, col, op)
  local s = math.min(boxW / pw, boxH / ph)
  local w, h = pw * s, ph * s
  return K.image(parent, cx - w / 2, cy - h / 2, w, h, atlas, part, col, op)
end

-- `size` is the icon's cell: the drawn part gets the area of a square 0.42 of it, at most 0.62 of it long.
local function glyph(parent, cx, cy, size, g, col, op)
  local bw, bh = g[7] - g[5], g[8] - g[6]
  local s = math.min(size * 0.42 / math.sqrt(bw * bh), size * 0.62 / math.max(bw, bh)) * (g[9] or 1.0)
  local w, h = g[3] * s, g[4] * s
  local ox = ((g[5] + g[7]) / 2 - g[3] / 2) * s
  local oy = ((g[6] + g[8]) / 2 - g[4] / 2) * s
  return K.image(parent, cx - w / 2 - ox, cy - h / 2 - oy, w, h, g[1], g[2], col, op)
end

-- A chevron of two bars turned 45 degrees, pointing `dir` (up, down, left, right), centred on (cx, cy):
-- 2a across and a deep. Returns the bars.
local function chevron(parent, cx, cy, a, t, dir, col, op)
  local L, h = a * math.sqrt(2) + t * 0.5, a / 2
  local arms
  if dir == "up" then arms = { { cx - h, cy, -45 }, { cx + h, cy, 45 } }
  elseif dir == "down" then arms = { { cx - h, cy, 45 }, { cx + h, cy, -45 } }
  elseif dir == "right" then arms = { { cx, cy - h, 45 }, { cx, cy + h, -45 } }
  else arms = { { cx, cy - h, -45 }, { cx, cy + h, 45 } } end
  local out = {}
  for i, a2 in ipairs(arms) do
    local r = K.rect(parent, a2[1] - L / 2, a2[2] - t / 2, L, t, col, op or 1.0)
    pcall(function() r:SetRotation(a2[3]) end)
    out[i] = r
  end
  return out
end

-- Frosted world glass with a restrained blue holographic frame. Focus and selection glow amber.
local R_PANEL, R_CELL = 30, 22

-- A panel's pane. Given the panel's place on the screen (sx, sy), it also gets the real glass behind
-- it -- inset so the square plane stays inside the rounded rim.
local function pane(parent, w, h, r)
  K.holoPanel(parent,w,h)
end

local function glassRect(sx, sy, w, h, r)
  -- A full panel's inset consumed almost half the height of a 64px tab.
  -- Keep compact panes close to their ink border, including their depth offset.
  local compact=h<100
  local i = compact and 2 or math.floor(r * 0.5)
  return { x = sx + i, y = sy + i, w = w - 2 * i, h = h - 2 * i, depth=compact and .0006 or nil }
end

local function glass(parent, w, h, sx, sy)
  if sx ~= nil then S.glassRects[#S.glassRects + 1] = glassRect(sx, sy, w, h, R_PANEL) end
  pane(parent, w, h, R_PANEL)
end

-- A list row, as an item of a scroll view: a rounded glass cell (its rim lights only under a pointer or
-- when chosen), an optional icon cell on the left, the label, an optional note on the right. Returns the
-- view item { h, build }.
local function listRow(group, label, click, opts)
  opts = opts or {}
  return { h = ROW, build = function(itc, it, v)
    local w, h = v.cw, ROW - 14
    local c = K.canvas(itc, 0, 0, w, h)
    local bg = K.roundFill(c, 0, 0, w, h, K.NAVY, 0.18, 4)
    local edge = K.cutFrame(c, 1, 1, w-2, h-2, K.CYAN, 0.22, 10, 1)
    local tx = 36
    local icon = nil
    if opts.icon ~= nil then
      local iw = opts.iconW or 84
      icon = opts.icon(c, 30, 0, iw, h)
      tx = 30 + iw + 24
    end
    local txt = K.text(c, tx, 0, w - tx - 34 - (opts.rightW or 0), h, K.clip(label, opts.clip or 24), 50,
      opts.disabled and K.GREY or K.TEXT)
    if opts.eq then
      for i, hh in ipairs({ 34, 56, 42 }) do
        K.rect(c, w - 110 + (i - 1) * 22, (h - hh) / 2, 12, hh, K.YELLOW, 1.0)
      end
    end
    if opts.right ~= nil then
      K.text(c, w - 30 - (opts.rightW or 160), 0, opts.rightW or 160, h, opts.right, 40, opts.rightCol or K.YELLOW, "right")
    end
    local b
    b = button(group, at(v.x, it.y, v), 0, 0, w, h, click, function(hover)
      local sel = b ~= nil and b.selected
      if opts.disabled then
        bg:SetOpacity(0.02)
        K.tint(edge, K.GREY, 0.0)
        return
      end
      local lit = hover and K.YELLOW or (sel and K.YELLOW or nil)
      bg:SetTintColor(K.color(lit or K.NAVY))
      bg:SetOpacity(hover and 0.16 or (sel and 0.12 or 0.18))
      K.tint(edge, lit or K.CYAN, lit and 1.0 or 0.22)
      txt:SetTintColor(K.color(hover and K.WHITE or (sel and K.YELLOW or K.TEXT)))
      if icon ~= nil and opts.tintIcon then icon:SetTintColor(K.color(hover and K.WHITE or (sel and K.YELLOW or K.CYAN))) end
    end)
    b.selected = opts.selected
    b.paint(false)
    if S.anim1 and v.first then slideIn(group, c, 0, 0, 0.03 + it.index * 0.035, 0, 36) end
  end }
end

-- A panel's header, drawn in its own canvas at the panel's top: the title in white, a hairline under it.
local function header(parent, w, title, col)
  local c = K.canvas(parent, 0, 0, w, HEAD)
  K.text(c, PAD + 18, 12, w - 340, HEAD - 26, title, 52, K.WHITE)
  K.rect(c, PAD + 16, HEAD - 8, w - 2 * PAD - 32, 2, K.ICE, 0.16)
  return c
end

-- A number in a pill, right-aligned at x1 in the header (left of the frame's corner mark, inside the glass).
local function countPill(c, x1, n)
  local s = tostring(n)
  local w = 52 + 22 * #s
  K.roundFill(c, x1 - w, 26, w, HEAD - 52, K.YELLOW, 0.10, 22)
  K.roundEdge(c, x1 - w, 26, w, HEAD - 52, K.YELLOW, 0.85, 22)
  K.text(c, x1 - w, 26, w, HEAD - 52, s, 36, K.YELLOW, "center")
end

-- A big push button in a panel canvas, with an optional glyph on its left.
local function action(parent, group, H, x, y, w, h, label, enabled, fn, colOn, g)
  local base = colOn or K.CYAN
  local bg = K.roundFill(parent, x, y, w, h, K.NAVY, enabled and 0.18 or 0.04, 4)
  local edge = K.cutFrame(parent, x, y, w, h, enabled and base or K.GREY, enabled and 0.5 or 0.2, 10, 1.5)
  local icon = nil
  if g ~= nil then icon = glyph(parent, x + h * 0.62, y + h / 2, h * 1.05, g, enabled and base or K.GREY, 1.0) end
  local tx = g ~= nil and h * 0.9 or 0
  local txt = K.text(parent, x + tx, y, w - tx, h, label, 54, enabled and K.TEXT or K.GREY, "center")
  return button(group, H, x, y, w, h, function()
    if enabled then fn() else toast(T.blocked) end
  end, function(hover)
    if not enabled then return end
    bg:SetTintColor(K.color(hover and K.YELLOW or K.NAVY))
    bg:SetOpacity(hover and 0.16 or 0.18)
    K.tint(edge, hover and K.YELLOW or base, hover and 1.0 or 0.5)
    txt:SetTintColor(K.color(hover and K.YELLOW or K.TEXT))
    if icon ~= nil then icon:SetTintColor(K.color(hover and K.WHITE or base)) end
  end)
end

-- A slider: the track, the lit part up to the value, the knob. Pushed with a finger and slid along
-- the screen, or grabbed with the trigger and swept, it calls set(value) with each `step` (1 unless
-- given) it crosses, and ticks the hand that holds it; `value` is read back from what set() returns.
-- onPhase("begin" / "end") hears it taken and let go.
local function slider(parent, group, H, x, y, w, h, lo, hi, value, set, step, onPhase)
  step = step or 1
  local KNOB = 64
  local ty = y + h / 2
  local track = K.roundFill(parent, x, ty - 6, w, 12, K.CYAN, 0.35, 6)
  local fill = K.roundFill(parent, x, ty - 6, 0, 12, K.CYAN, 1.0, 6)
  local knobC = K.canvas(parent, x, ty - KNOB / 2, KNOB, KNOB)
  local ring = K.image(knobC, 0, 0, KNOB, KNOB, K.ATLAS.phone, "circle", K.CYAN, 1.0)
  K.image(knobC, 7, 7, KNOB - 14, KNOB - 14, K.ATLAS.phone, "circle", K.BLACK, 1.0)
  local dot = K.image(knobC, KNOB / 2 - 10, KNOB / 2 - 10, 20, 20, K.ATLAS.phone, "circle", K.CYAN, 1.0)
  local held, hovered = false, false
  local function show(v)
    local t = (hi > lo) and clamp((v - lo) / (hi - lo), 0, 1) or 0
    K.roundSize(fill, w * t, 12, 6)
    K.move(knobC, x + w * t - KNOB / 2, ty - KNOB / 2)
  end
  local function paint()
    local col = held and K.YELLOW or (hovered and K.YELLOW or K.CYAN)
    fill:SetTintColor(K.color(col))
    ring:SetTintColor(K.color(col))
    dot:SetTintColor(K.color(col))
    track:SetOpacity((held or hovered) and 0.5 or 0.35)
  end
  show(value or lo)
  local b = button(group, H, x - KNOB / 2, y, w + KNOB, h, nil, function(hover)
    hovered = hover
    paint()
  end)
  local x0 = b.x + KNOB / 2           -- the track's left end on the screen
  -- A press on the knob takes hold of it where it is (no jump to the finger); a press on the track
  -- moves the knob there.
  local grab = 0
  b.drag = function(px, phase, hand)
    if phase == "begin" then
      held = true
      local t = (hi > lo) and clamp(((b.value or lo) - lo) / (hi - lo), 0, 1) or 0
      local knobX = x0 + w * t
      grab = math.abs(px - knobX) <= KNOB / 2 and (knobX - px) or 0
      if onPhase then onPhase("begin") end
    elseif phase == "end" then
      held = false
    end
    if phase ~= "end" and px ~= nil then
      local t = clamp((px + grab - x0) / w, 0, 1)
      local v = clamp(lo + math.floor(t * (hi - lo) / step + 0.5) * step, math.min(lo, hi), math.max(lo, hi))
      if b.value == nil or math.abs(v - b.value) > step * 0.001 then
        local got = set(v)
        if got ~= nil then
          b.value = got
          show(got)
        end
      end
    end
    paint()
    if phase == "end" and onPhase then onPhase("end") end
  end
  b.value = value
  b.show = function(v)
    b.value = v
    show(v)
  end
  return b
end

-- A square button with a mark: false / true draw - / +, a direction draws a chevron.
local function stepButton(parent, group, H, x, y, w, h, mark, enabled, fn)
  local bg = K.roundFill(parent, x, y, w, h, K.CYAN, enabled and 0.14 or 0.04, R_CELL)
  local edge = K.roundEdge(parent, x, y, w, h, enabled and K.CYAN or K.GREY, enabled and 0.9 or 0.3, R_CELL)
  local col = enabled and K.CYAN or K.GREY
  local bars
  if type(mark) == "string" then
    bars = chevron(parent, x + w / 2, y + h / 2, math.min(w, h) * 0.20, 7, mark, col, 1.0)
  else
    local L, T = math.min(w, h) * 0.42, 8
    bars = { K.rect(parent, x + (w - L) / 2, y + (h - T) / 2, L, T, col, 1.0) }
    if mark then bars[2] = K.rect(parent, x + (w - T) / 2, y + (h - L) / 2, T, L, col, 1.0) end
  end
  return button(group, H, x, y, w, h, function()
    if enabled then fn() end
  end, function(hover)
    if not enabled then return end
    bg:SetTintColor(K.color(hover and K.YELLOW or K.CYAN))
    bg:SetOpacity(hover and 0.30 or 0.14)
    K.tint(edge, hover and K.YELLOW or K.CYAN, hover and 1.0 or 0.9)
    K.tint(bars, hover and K.WHITE or K.CYAN)
  end)
end

-- SAO's list arrows: a wide bar above and below a list that does not fit, a chevron in it. A press
-- scrolls the list by most of its window; the bar dims at its own end of the list. `view` is the list,
-- or a function giving it (the settings swap theirs as the page changes).
local function arrowBar(parent, group, H, x, y, w, h, up, view)
  local bg = K.roundFill(parent, x, y, w, h, K.ICE, 0.04, h / 2)
  local edge = K.roundEdge(parent, x, y, w, h, K.ICE, 0.0, h / 2)
  local chev = chevron(parent, x + w / 2, y + h / 2, 19, 7, up and "up" or "down", K.CYAN, 1.0)
  local function current()
    if type(view) == "function" then return view() end
    return view
  end
  local function live()
    local v = current()
    if v == nil or not v:scrollable() then return false end
    if up then return not v:atTop() end
    return not v:atBottom()
  end
  return button(group, H, x, y, w, h, function()
    local v = current()
    if live() then v:scrollBy((up and -1 or 1) * v.h * 0.8) end
  end, function(hover)
    local on = live()
    local lit = (hover and on) and K.YELLOW or nil
    bg:SetTintColor(K.color(lit or K.ICE))
    bg:SetOpacity(on and (hover and 0.24 or 0.04) or 0.02)
    K.tint(edge, lit or K.ICE, (on and hover) and 1.0 or 0.0)
    K.tint(chev, lit and K.WHITE or K.CYAN, on and 1.0 or 0.25)
  end)
end

-- How tall a list of view items stands in its panel, arrows included, and whether it scrolls. The
-- arrows keep ARROW_FOOT clear under them, off the panel's bottom rim. A scrolling list's window is
-- ARROW_LIFT shorter than maxH: that lifts its arrows clear of the rim and the frame's corner mark while
-- the panel keeps its height.
local ARROW_FOOT, ARROW_LIFT = 26, 36
local function listSize(rows, maxH)
  local total = 0
  for _, r in ipairs(rows) do total = total + r.h end
  if total <= maxH then return total, false end
  return maxH + ARROW_GAP + ARROW_H + ARROW_FOOT, true
end

-- A list in panel p (its top-left at sx, sy on the screen), from `top` down, `w` wide: the rows in a
-- scroll view, with the arrows above and below its window when they do not all fit in maxH. `keep` is
-- the scroll to start from (a pick or a refresh redraws the list where it was). Returns the view.
local function list(p, group, key, sx, sy, top, w, rows, maxH, keep)
  local total = 0
  for _, r in ipairs(rows) do total = total + r.h end
  local scrolls = total > maxH
  local y = top
  local viewH = scrolls and maxH - ARROW_LIFT or math.max(total, 1)
  local v = V.new(p, PAD, y, sx + PAD, sy + y, w, viewH)
  v.section = S.section
  for _, r in ipairs(rows) do v:add(r.h, r.build) end
  local arrows = {}
  if scrolls then
    local aw = (w - 14) / 2
    arrows[1] = arrowBar(p, group, at(sx, sy), PAD, y + viewH + ARROW_GAP, aw, ARROW_H, true, v)
    arrows[2] = arrowBar(p, group, at(sx, sy), PAD + aw + 14, y + viewH + ARROW_GAP, aw, ARROW_H, false, v)
  end
  v.onScroll = function()
    for _, b in ipairs(arrows) do b.paint(S.hovered[b] == true) end
  end
  if keep ~= nil then v:scrollTo(keep) end
  S.views[key] = v
  v:update(0, 0)
  return v
end

-- Window-tinted glass, a plain outer ring and an animated segmented inner ring (the close cross has
-- neither, only its glass). The open section's icon points at its panel with a chevron, SAO's way -- to
-- the left for the panel on the left.
local function iconButton(key, g, y, click, size, left)
  size = size or ICON
  local x0, y0 = COLX - size / 2, y - size / 2
  local c = K.canvas(S.layerCol, x0, y0, size, size)
  local ringHost=K.canvas(c,0,0,size,size)
  ringHost:SetRenderTransformPivot(Vector2.new({X=.5,Y=.5}))
  local bare = key == "close"
  local ring = bare and {} or K.ring(ringHost,size/2,size/2,size*.39,1.5,K.CYAN,.68,32,4)
  local outer = bare and {} or K.ring(c,size/2,size/2,size/2-3,1.5,K.CYAN,.38,48)
  local visual={color=K.CYAN,outer=.68,inner=.38}
  local marks = {}
  if type(g) == "function" then
      marks = g(c, size / 2, size / 2, size * 0.78, K.WHITE)
  elseif g ~= nil then
    marks[1] = glyph(c, size / 2, size / 2, size, g, K.WHITE, 1.0)
  else
    marks[1] = K.slash(c, size / 2, size / 2, size * 0.40, 7, K.CYAN, 1.0)
    marks[2] = K.slash(c, size / 2, size / 2, size * 0.40, 7, K.CYAN, 1.0)
    pcall(function() marks[2]:SetRotation(45.0) end)
  end
  local pointer = left and chevron(c, -42, size / 2, 15, 7, "left", K.YELLOW, 0.0)
    or chevron(c, size + 42, size / 2, 15, 7, "right", K.YELLOW, 0.0)
  local summaryPointer=key=="person" and chevron(c,-42,size/2,15,7,"left",K.YELLOW,0) or nil
  local b = button("col", ORIGIN, x0, y0, size, size, click, function(hover)
    local sel = S.section == key
      or (key=="person" and (S.section=="inventory" or S.section=="character" or S.section=="cyberware" or S.section=="crafting" or S.section=="trade" or S.section=="stash"))
      or (key=="game" and S.section=="load")
      or (key=="quests" and (S.section=="journal" or S.section=="codex" or S.section=="shards" or S.section=="tarot"))
    local lit = hover and K.YELLOW or (sel and K.YELLOW or nil)
    local col = lit or K.CYAN
    visual.color=col;visual.outer=lit and 1 or .68;visual.inner=lit and .9 or .38
    K.tint(ring,col,lit and 1 or .68)
    K.tint(outer,col,lit and .9 or .38)
    K.tint(marks,lit and K.YELLOW or K.WHITE,1)
    K.tint(pointer, K.YELLOW, sel and 1.0 or 0.0)
    if summaryPointer then K.tint(summaryPointer,K.YELLOW,sel and 1 or 0) end
  end)
  b.key, b.canvas, b.x0, b.y0 = key, c, x0, y0
  b.ringHost,b.ring,b.outer,b.visual=ringHost,ring,outer,visual
  b.x, b.circle = x0 + S.columnOffset, true
  return b
end

-- What the settings module draws with.
local kit = {
  S = S, T = T, PAD = PAD, HEAD = HEAD, ROW = ROW, ARROW_H = ARROW_H, LIST_H = LIST_H, R_CELL = R_CELL,
  P1X = P1X, P1Y = P1Y, P1W = P1W, P2X = P2X, P2W = P2W, GLYPH = GLYPH,
  at = at, button = button, clearButtons = clearButtons, dropView = dropView, slider = slider, stepButton = stepButton,
  arrowBar = arrowBar, chevron = chevron, glass = glass, header = header, slideIn = slideIn,
  glyph = glyph,
  panelRect = function(x,y,w,h) return glassRect(x,y,w,h,R_PANEL) end,
  sound = sound, play = play, toast = toast, action = action, list = list, listRow = listRow,
  listSize = listSize,
  rebuild = function(anim1, anim2) build.panels(anim1, anim2) end,
}

local function groupOf(key)
  if key=="inventory" or key=="character" or key=="cyberware" or key=="crafting" or key=="trade" or key=="stash" then return "person" end
  if key=="journal" or key=="codex" or key=="shards" or key=="tarot" then return "quests" end
  if key=="load" then return "game" end
  return key
end

local function openSection(key,keepOpen)
  if not S.open or S.menu == nil or not S.menu.built then return end
  if S.section == "contacts" then Msg.leave(kit) end
  Gameplay.leave(kit)
  if S.section == key and not keepOpen then
    S.section = nil
  else
    S.section = key
  end
  S.items, S.vehicle = nil, nil
  moveColumn(S.section ~= nil and 0 or COL_HOME)
  S.panelStart = S.time + ((S.section ~= nil and S.columnMove ~= nil) and COLUMN_SHIFT or 0)
  build.panels(true, false)
  for _, b in ipairs(S.buttons) do
    if b.group == "col" and b.paint then b.paint(false) end
  end
end

local function openGroup(group)
  if groupOf(S.section)==group then openSection(nil,true);return end
  local visit=VendorVisit.current(M.shared)
  local key=group=="person" and (visit and (visit.ripperdoc==false and "trade" or "cyberware") or "inventory") or "journal"
  if group=="person" and StashVisit.current(M.shared) then key="stash" end
  openSection(key,true)
end

function build.column()
  clearButtons("col")
  dropAnims("col")
  S.icons = {
    iconButton("close", nil, COL.close, function() M.close() end, CLOSE_ICON),
    iconButton("person", GLYPH.person, COL.person, function() openGroup("person") end),
    iconButton("map", GLYPH.map, COL.map, function() M.openMap() end),
    iconButton("quests", function(p,cx,cy,size,col)
      return { K.roundFill(p,cx-4,cy-size*.26,8,size*.34,col,1,4),
        K.roundFill(p,cx-4,cy+size*.16,8,8,col,1,4) }
    end, COL.quests, function() openGroup("quests") end),
    iconButton("contacts", GLYPH.call, COL.contacts, function() openSection("contacts") end),
    iconButton("vehicles", GLYPH.car, COL.vehicles, function() openSection("vehicles") end),
    iconButton("radio", GLYPH.radio, COL.radio, function() openSection("radio") end),
    iconButton("timeskip", function(parent, cx, cy, size, col, op)
      op = op or 1.0
      local r = size * 0.48
      local c = K.canvas(parent, cx-r/2, cy-r/2, r, r)
      local ring = K.image(c, 0, 0, r, r, K.ATLAS.phone, "circle", col, op)
      K.image(c, 5, 5, r-10, r-10, K.ATLAS.phone, "circle", K.BLACK, op)
      local hour = K.rect(c, r/2-2, r*.2, 4, r*.32, col, op)
      local minute = K.rect(c, r/2, r/2-2, r*.25, 4, col, op)
      return { ring, hour, minute }
    end, COL.timeskip, function() openSection("timeskip") end),
    iconButton("game", gamepadGlyph, COL.game, function() openSection("game") end),
    iconButton("settings", GLYPH.cog, COL.settings, function() openSection("settings") end),
  }
  for i, b in ipairs(S.icons) do
    slideIn("col", b.canvas, b.x0, b.y0, (i - 1) * 0.05, 0, 70, b)
  end
  -- the rim encloses the strip's glass: the plane is inset inside it and lies on the screen itself
  local top=COL.close-RES_H/2
  local status=K.canvas(S.layerCol,COLX+90,top,650,RES_H)
  S.resourceBg=K.roundFill(status,-16,0,650,RES_H,K.NAVY,.14,8)
  S.resourceEdge=K.roundEdge(status,-16,0,650,RES_H,K.CYAN,.4,8)
  S.resourceRect={x=COLX+S.columnOffset+74+RES_INSET,y=top+RES_INSET,w=650-2*RES_INSET,h=RES_H-2*RES_INSET,
    opaque=false,depth=.0006}
  S.moneyWidget=K.text(status,0,0,410,RES_H,"",34,K.YELLOW)
  S.moneyWidget:SetOpacity(.86)
  S.weightIcon=K.canvas(status,250,0,30,RES_H)
  K.roundEdge(S.weightIcon,0,20,24,24,K.CYAN,.85,3)
  K.roundEdge(S.weightIcon,6,11,12,12,K.CYAN,.85,3)
  S.weightWidget=K.text(status,291,0,300,RES_H,"",32,K.CYAN)
  S.weightWidget:SetOpacity(.86)
  S.resourceAt,S.resourceText,S.resourceLayout,S.resourceTarget=nil,nil,nil,nil
  slideIn("col",status,COLX+90,top,.04,0,24)
end

local function groupedNumber(n)
  local text=tostring(math.floor(n))
  return text:reverse():gsub("(%d%d%d)","%1 "):reverse():gsub("^ ","")
end

local function updateResources(dt)
  if not S.moneyWidget then return end
  if not S.resourceAt or S.time-S.resourceAt>=1 then
    S.resourceAt=S.time
    local data=InventoryData.resources()
    if data then
      S.resourceValues=data
      local money="€$ "..groupedNumber(data.money)
      local weight=tostring(data.weight).."/"..tostring(data.capacity)
      if S.resourceText~=money..":"..weight then
        S.resourceText=money..":"..weight;S.resourceMeasure=2
        S.moneyWidget:SetText(money);S.weightWidget:SetText(weight)
        S.weightWidget:SetTintColor(K.color(data.weight>data.capacity and K.RED or K.CYAN))
      end
    end
  end
  if S.resourceMeasure then
    S.resourceMeasure=S.resourceMeasure-1
    if S.resourceMeasure<=0 then
      local mw,ww=S.moneyWidget:GetDesiredSize().X,S.weightWidget:GetDesiredSize().X
      if mw>1 and ww>1 then
        S.resourceTarget={icon=mw+18,weight=mw+55,width=mw+ww+86}
        S.resourceMeasure=nil
      else S.resourceMeasure=1 end
    end
  end
  local target=S.resourceTarget
  if target then
    local a=S.resourceLayout or {icon=target.icon,weight=target.weight,width=target.width};S.resourceLayout=a
    for _,k in ipairs({"icon","weight","width"}) do a[k]=a[k]+(target[k]-a[k])*math.min(1,(dt or 0)*12);if math.abs(target[k]-a[k])<.5 then a[k]=target[k] end end
    K.move(S.weightIcon,a.icon,0);K.move(S.weightWidget,a.weight,RES_H/2)
    K.roundSize(S.resourceBg,a.width,RES_H,8);K.roundSize(S.resourceEdge[1],a.width,RES_H,8)
    S.resourceRect.w=a.width-2*RES_INSET
  end
end

-- A list panel: the glass, the header (with the count when the list scrolls), the rows.
local function listPanel(p, title, rows)
  local h, scrolls = listSize(rows, LIST_H)
  glass(p, P1W, HEAD + math.max(h, #rows == 0 and ROW or 0) + 14, P1X, P1Y)
  local hc = header(p, P1W, title, K.CYAN)
  if S.anim1 then slideIn("p1", hc, 0, 0, 0.0, 0, 20) end
  if scrolls then countPill(hc, P1W - 72, #rows) end
  if #rows == 0 then
    K.text(p, 34, HEAD, P1W - 68, ROW, T.empty, 44, K.GREY)
    return nil
  end
  return list(p, "p1", "p1", P1X, P1Y, HEAD, P1W - 2 * PAD, rows, LIST_H, S.keepScroll)
end

function build.vehicles(p)
  S.items = S.items or D.vehicles()
  local rows = {}
  for _, v in ipairs(S.items) do
    local g = GLYPH[v.kind] or GLYPH.car
    local o = {
      iconW = 84, tintIcon = true,
      icon = function(rc, x, y, w, h) return glyph(rc, x + w / 2, h / 2, h * 1.1, g, K.CYAN, 1.0) end,
    }
    if v.repair > 0.0 then
      o.right = string.format("%d:%02d", math.floor(v.repair / 60), math.floor(v.repair % 60))
      o.rightW, o.rightCol, o.disabled = 140, K.GREY, true
    end
    o.selected = S.vehicle ~= nil and S.vehicle.name == v.name and S.vehicle.picture == v.picture
    rows[#rows + 1] = listRow("p1", v.name, function()
      S.vehicle = o.selected and nil or v
      build.panels(false, S.vehicle ~= nil)
    end, o)
  end
  listPanel(p, T.vehicles, rows)
end

-- The picked vehicle, as the game's own vehicle list shows it before the call: its picture, whether it
-- is being repaired, and the button that calls it.
function build.vehicleDetail(p)
  local v = S.vehicle
  if v == nil then return end
  local H2 = at(P2X, P1Y)
  local PIC_H = 240
  glass(p, P2W, HEAD + PIC_H + 30 + 70 + ROW + 30, P2X, P1Y)
  header(p, P2W, K.clip(K.upper(v.name), 18), K.YELLOW)
  local y = HEAD + 10
  local repairing = v.repair > 0.0
  if v.picture ~= nil then
    local sz = PICS[v.picture.name or ""] or { 470, 156 }
    fit(p, P2W / 2, y + PIC_H / 2, P2W - 2 * PAD - 40, PIC_H, v.picture.atlas, v.picture.part, sz[1], sz[2], K.WHITE,
      repairing and 0.25 or 1.0)
  else
    glyph(p, P2W / 2, y + PIC_H / 2, PIC_H, GLYPH[v.kind] or GLYPH.car, K.CYAN, 1.0)
  end
  y = y + PIC_H + 20
  local status = repairing and string.format("%s  %d:%02d", T.repair, math.floor(v.repair / 60), math.floor(v.repair % 60))
    or (D.mounted() ~= nil and T.mounted or T.ready)
  K.text(p, 0, y, P2W, 60, status, 38, repairing and K.GREY or K.CYAN, "center")
  y = y + 80
  action(p, "p2", H2, PAD, y, P2W - 2 * PAD, ROW + 10, T.summon, not repairing and D.mounted() == nil, function()
    local ok, why = D.summon(v)
    if ok then
      toast(T.summoned)
      S.closeAt = S.time + 0.9
    else
      toast(T[why] or T.blocked)
    end
  end, K.CYAN, GLYPH[v.kind] or GLYPH.car)
end

function build.radio(p, p2)
  S.items = S.items or D.stations()
  local st = D.radio()
  -- what was just asked of the receiver shows at once; the game's own state takes over as soon as it
  -- agrees, or after a moment if it never does (a refresh keeps looking until then)
  local want = S.radioWant
  if want ~= nil then
    if (st.active == want.active and (not want.active or want.index < 0 or st.index == want.index))
        or S.time - want.at > 2.0 then
      S.radioWant = nil
    else
      st.active = want.active
      if want.active and want.index >= 0 then
        st.index = want.index
        for _, s in ipairs(S.items) do
          if s.index == want.index then st.station = s.name end
        end
        st.track = ""
      end
      S.refreshAt = S.time + 0.15
    end
  end
  S.radioState = st
  local rows = {}
  for _, s in ipairs(S.items) do
    local o = {
      iconW = 150,
      icon = function(rc, x, y, w, h)
        if s.logo == nil then return nil end
        local sz = LOGO[s.logo.name] or { 300, 180 }
        return fit(rc, x + w / 2, h / 2, w, h - 16, s.logo.atlas, s.logo.part, sz[1], sz[2], K.WHITE, 1.0)
      end,
    }
    if st.active and s.index == st.index then o.selected, o.eq, o.rightW = true, true, 90 end
    rows[#rows + 1] = listRow("p1", s.name, function()
      D.tune(s.index)
      S.radioWant = { active = true, index = s.index, at = S.time }
      build.panels(false, false)
    end, o)
  end
  -- One shared window: station list on the left, receiver controls on the right.
  glass(p, SW, 1120, P1X, P1Y)
  header(p, SW, T.radio, K.CYAN)
  list(p, "p1", "p1", P1X, P1Y, HEAD + 20, P1W - 2 * PAD, rows, 860, S.keepScroll):enableMask()
  K.rect(p, P2X - P1X - 30, HEAD + 20, 2, 980, K.CYAN, 0.2)
  -- the receiver sits in the middle of the window's right part: between the divider and the inner rim
  local RX, RW = P2X - P1X - 14, 628
  p2 = K.canvas(p, RX, 0, RW, 1120)

  -- the receiver: power, volume, what plays
  local H2 = at(P1X + RX, P1Y)
  local y = HEAD + 20
  K.text(p2, PAD, y, RW - 2 * PAD, 60, K.clip(K.upper(st.active and st.station ~= "" and st.station or T.radio), 23), 36, K.YELLOW, "center")
  y = y + 76
  local on = st.active
  action(p2, "p2", H2, PAD, y, RW - 2 * PAD, ROW + 10, T.radio .. ": " .. (on and T.on or T.off), true, function()
    if on then
      D.radioOff()
      S.radioWant = { active = false, index = -1, at = S.time }
    else
      local list = D.stations()
      local index = S.lastStation or (list[1] and list[1].index) or 0
      D.tune(index)
      S.radioWant = { active = true, index = index, at = S.time }
    end
    build.panels(false, false)
  end, on and K.CYAN or K.CYAN, GLYPH.radio)
  if on and st.index >= 0 then S.lastStation = st.index end
  y = y + ROW + 30
  local vol, lo, hi = D.volume()
  local volLabel = K.text(p2, PAD, y, RW - 2 * PAD, 60, T.volume .. "   " .. (vol and tostring(vol) or "--"), 40,
    K.TEXT, "center")
  y = y + 64
  local sl = nil
  local function apply(n)
    local got = D.setVolume(n)
    if got ~= nil then
      pcall(function() volLabel:SetText(T.volume .. "   " .. tostring(got)) end)
      if sl ~= nil then sl.show(got) end
    end
    return got
  end
  local SB = 104
  stepButton(p2, "p2", H2, PAD, y, SB, ROW - 14, false, vol ~= nil, function()
    apply((sl and sl.value or vol) - 1)
  end)
  stepButton(p2, "p2", H2, RW - PAD - SB, y, SB, ROW - 14, true, vol ~= nil, function()
    apply((sl and sl.value or vol) + 1)
  end)
  if vol ~= nil then
    local sx = PAD + SB + 44
    sl = slider(p2, "p2", H2, sx, y, RW - 2 * sx, ROW - 14, lo, hi, vol, apply)
  end
  y = y + ROW + 16
  if on and st.track ~= "" then
    K.text(p2, 0, y, RW, 56, T.playing, 36, K.CYAN, "center")
    K.text(p2, 0, y + 56, RW, 70, K.clip(st.track, 28), 44, K.CYAN, "center", true)
  end
  if on then
    for _, station in ipairs(S.items) do
      if station.index == st.index and station.logo then
        local sz = LOGO[station.logo.name] or {300,180}
        fit(p2, RW / 2, 820, RW - 120, 230, station.logo.atlas, station.logo.part, sz[1], sz[2], K.WHITE, 1)
        break
      end
    end
  end
end

-- The pause menu's own entries, Resume left out: each opens the pause menu and then its page in it,
-- the way the pause menu's buttons do (MenuScenario_PauseMenu's events); the exit asks twice.
local function pausePage(event)
  local ctrl = M.shared and M.shared.menuController
  if ctrl == nil then
    toast(T.noMenu)
    return
  end
  M.close()
  if D.menuEvent(ctrl, "OnOpenPauseMenu") and event ~= nil then
    S.pending = { at = S.time + 0.3, event = event }
  end
end

function build.game(p)
  local items = {
    { key = "save", label = D.label("UI-ResourceExports-SaveGame", T.save), event = "OnSwitchToSaveGame" },
  }
  if D.savesCount() > 0 then
    items[#items + 1] = { key = "load", label = D.label("UI-ScriptExports-LoadGame0", T.load), section = "load" }
  end
  items[#items + 1] = { key = "cog", label = D.label("UI-Labels-Settings", T.options), event = "OnSwitchToSettings" }
  items[#items + 1] = { key = "credits", label = D.label("UI-Labels-Credits", T.credits), event = "OnCreditsPicker" }
  -- the two ways out ask for a second press within 3 s; the row says what that press will do
  items[#items + 1] = { key = "exit", label = D.label("UI-Labels-ExitToMenu", T.exit), confirm = T.confirmExit,
    go = D.exitToMainMenu }
  items[#items + 1] = { key = "quit", label = D.label("UI-Labels-CloseGame", T.quit), confirm = T.confirmQuit,
    go = D.exitGame }
  local armed = (S.armed ~= nil and S.time < S.armed.until_) and S.armed.key or nil
  local rows = {}
  for _, it in ipairs(items) do
    local o = {
      iconW = 84, tintIcon = true,
      icon = function(rc, x, y, w, h) return glyph(rc, x + w / 2, h / 2, h * 1.1, GLYPH[it.key], K.CYAN, 1.0) end,
      selected = armed ~= nil and armed == it.key,
    }
    local label = (armed ~= nil and armed == it.key) and it.confirm or it.label
    rows[#rows + 1] = listRow("p1", label, function()
      if it.section then
        openSection(it.section,true)
      elseif it.go == nil then
        pausePage(it.event)
      elseif S.armed ~= nil and S.armed.key == it.key and S.time < S.armed.until_ then
        S.armed = nil
        M.close()
        it.go()
      else
        S.armed = { key = it.key, until_ = S.time + 3.0 }
        S.refreshAt = S.time + 3.05
        build.panels(false, false)
        play("notify")
      end
    end, o)
  end
  listPanel(p, T.game, rows)
end

-- A press, a drag or a scroll that is still going on, of either hand, forgotten: the panel under it
-- is about to be drawn again.
local function dropGestures()
  for _, P in ipairs(S.hands) do
    Stick.cancel(P)
    P.drag, P.dragBy, P.pend, P.scroll, P.touch = nil, nil, nil, nil, nil
  end
end

-- Tabs the game names itself: their label is the game's own text, in the game's language.
local TAB_LOC={crafting="UI-PanelNames-CRAFTING"}
-- anim1: the list unrolls (a new section); anim2: the detail panel slides out of it.
function build.tabs()
  local group=groupOf(S.section);local tabs
  if group=="person" then
    tabs={{"inventory","СНАРЯЖЕНИЕ","EQUIPMENT"},{"character","ПЕРСОНАЖ","CHARACTER"},{"cyberware","КИБЕРИМПЛАНТЫ","CYBERWARE"},
      {"crafting","СОЗДАНИЕ","CRAFTING"}}
    if VendorVisit.current(M.shared) then tabs[#tabs+1]={"trade","ТОРГОВЛЯ","TRADE"} end
    if StashVisit.current(M.shared) then tabs[#tabs+1]={"stash","ТАЙНИК","STASH"} end
  elseif group=="quests" then
    -- the shards are a page of the database
    tabs={{"journal","КВЕСТЫ","QUESTS"},{"codex","БАЗА ДАННЫХ","DATABASE",shards=true},{"tarot","КАРТЫ ТАРО","TAROT CARDS"}}
  else return end
  local width=1780;local y=174;local host=K.canvas(S.layerPanel,P1X,y,width,64)
  local gap=20;local tw=(width-24-gap*(#tabs-1))/#tabs
  for i,t in ipairs(tabs) do
    local x=12+(i-1)*(tw+gap);local selected=S.section==t[1] or (t.shards and S.section=="shards")
    local tab=K.canvas(host,x,0,tw,64);glass(tab,tw,64,P1X+x,y)
    local bg=K.roundFill(tab,6,5,tw-12,54,selected and K.YELLOW or K.NAVY,.12,3)
    -- the game's own word where it has one
    local key=TAB_LOC[t[1]];local text=key and GetLocalizedText(key) or ""
    if text=="" or text==key then text=L.tr(t[2],t[3]) end
    local label=K.text(tab,8,4,tw-16,56,K.upper(text),30,selected and K.YELLOW or K.TEXT,"center")
    if selected then K.rect(tab,14,59,tw-28,2,K.YELLOW,1) end
    local b=button("tabs",at(P1X,y),x,4,tw,56,function()
      if t[1]=="trade" then M.openTrade() elseif S.section~=t[1] then openSection(t[1],true) end
    end,function(h)
      bg:SetTintColor(K.color((h or selected) and K.YELLOW or K.NAVY));bg:SetOpacity(h and .18 or .12)
      label:SetTintColor(K.color((h or selected) and K.YELLOW or K.TEXT))
    end);b.key="tab:"..t[1]
  end
end

function build.summary()
  local show=groupOf(S.section)=="person"
  if show==S.summaryVisible then return end
  S.summaryVisible=show
  dropAnims("summary")
  if show then
    S.layerSummary:SetVisible(true);Gameplay.summary(kit,S.layerSummary)
    slideIn("summary",S.layerSummary,0,0,0,-28,0)
  else
    clearButtons("summary");S.layerSummary:RemoveAllChildren();S.layerSummary:SetVisible(false);S.summaryRect=nil
  end
end

function build.panels(anim1, anim2)
  Gameplay.beforeBuild(kit)
  local L = S.layerPanel
  local revealing = S.section ~= nil and S.time < (S.panelRevealEnd or 0)
  local old = S.views.p1
  S.keepScroll = (old ~= nil and old.section == S.section) and old.offset or nil
  L:RemoveAllChildren()
  clearButtons("p1")
  clearButtons("p2")
  clearButtons("set")
  clearButtons("sethead")
  clearButtons("tabs")
  clearButtons("sort")
  S.modalGroup=nil
  dropAnims("p1")
  dropAnims("p2")
  dropGestures()
  Set.leave()
  S.views = {}
  S.glassRects = {}
  S.hovered = {}
  S.anim1 = anim1
  build.summary()
  -- everything below lives in the panel layer, emptied above: its static ornaments may be kept ones
  K.poolBegin()
  S.panelInteractiveAt = math.max(S.time, S.columnMove and S.columnMove.t0 + COLUMN_SHIFT or 0,
    revealing and S.panelRevealEnd or 0)
  if anim1 and S.section ~= nil then
    dropAnims("panelRoot")
    S.panelRevealEnd = math.max(S.time, S.panelStart or S.time) + ANIM
    slideIn("panelRoot", L, 0, 0, 0, 30, 0)
  elseif not revealing then
    dropAnims("panelRoot")
    S.panelRevealEnd = 0
    L:SetOpacity(1)
    K.move(L, 0, 0)
  end
  if S.section == "settings" then
    local ps = K.canvas(L, P1X, P1Y, SW, CH - P1Y)
    kit.T = T
    Set.build(kit, ps, P1X, P1Y, SW)
    if anim1 then slideIn("p1", ps, P1X, P1Y, 0.0, 0, 30) end
  else
    local p1 = K.canvas(L, P1X, P1Y, P1W, CH - P1Y)
    local p2 = K.canvas(L, P2X, P1Y, Msg.THREAD_W, CH - P1Y)
    if S.section == "load" or S.section == "stash" or S.section == "trade" or S.section == "cyberware" or S.section == "crafting" or S.section == "inventory" or S.section == "character" or S.section == "journal" or S.section == "codex" or S.section == "shards" or S.section == "tarot" or S.section == "timeskip" then
      kit.T = T
      Gameplay.build(kit, p1, p2)
    elseif S.section == "contacts" then
      kit.T = T
      Msg.build(kit, p1, p2)
    elseif S.section == "vehicles" then
      build.vehicles(p1)
      build.vehicleDetail(p2)
    elseif S.section == "radio" then
      build.radio(p1, p2)
    elseif S.section == "game" then
      build.game(p1)
    end
    if anim2 or (anim1 and S.section == "radio") then slideIn("p2", p2, P2X, P1Y, anim1 and 0.12 or 0.0, 50, 0) end
  end
  build.tabs()
  S.toastBg = K.roundFill(L, P1X, TOAST_Y, P1W, 70, K.ICE, 0.08, 35)
  S.toastWidget = K.text(L, P1X + 30, TOAST_Y, P1W - 60, 70, "", 44, K.YELLOW)
  S.toastWidget:SetVisible(S.toast ~= nil and S.time < S.toast.until_)
  S.toastBg:SetVisible(S.toast ~= nil and S.time < S.toast.until_)
  if S.toast ~= nil and S.time < S.toast.until_ then S.toastWidget:SetText(S.toast.text) end
  K.poolEnd()
end

function build.all()
  local root = S.menu.root
  root:RemoveAllChildren()
  K.poolFlush()
  S.buttons, S.hovered, S.anims, S.views = {}, {}, {}, {}
  -- With the summary closed, every opening widget initially has zero animated opacity. Submit a
  -- black pixel so the private surface gets its first GPU write before revealing those animations.
  S.surfaceWarmup=K.rect(root,0,0,1,1,K.BLACK,1)
  -- three layers, so a rebuilt panel never covers the cursors
  S.layerCol = K.canvas(root, S.columnOffset, 0, CW, CH)
  S.layerSummary = K.canvas(root, 0, 0, 1000, CH)
  S.summaryVisible=false;S.layerSummary:SetVisible(false)
  S.layerPanel = K.canvas(root, 0, 0, CW, CH)
  local top = K.canvas(root, 0, 0, CW, CH)
  S.cursors={};S.pressFX={}
  for i=1,2 do
    local host=K.canvas(top,0,0,64,64);host:SetRenderTransformPivot(Vector2.new({X=0,Y=0}));host:SetOpacity(0)
    S.cursors[i]={host=host,inner=K.ring(host,32,32,17,2,K.CYAN,.9,28),outer=K.ring(host,32,32,26,1.5,K.CYAN,.4,32,4),
      dot=K.image(host,29,29,6,6,K.ATLAS.phone,"circle",K.WHITE,1)}
    local fx=K.canvas(top,0,0,96,96);fx:SetRenderTransformPivot(Vector2.new({X=.5,Y=.5}));fx:SetOpacity(0)
    K.ring(fx,48,48,22,3,K.YELLOW,.95,32);K.ring(fx,48,48,32,2,K.YELLOW,.65,36);K.ring(fx,48,48,42,1,K.YELLOW,.3,40)
    S.pressFX[i]={host=fx}
  end
  build.column()
  build.panels(S.section~=nil, false)
  S.openAnimAt = S.time
  S.menu.built = true
end

local function buildBeam(h)
  local root = h.root
  root:RemoveAllChildren()
  h.strip = K.beamBuild(root, K.CYAN, 0.25, K.WHITE, 0.9)
  h.built = true
end

---------------------------------------------------------------- open / close

local function capture(on)
  if type(VRMenuCapture) == "function" then pcall(VRMenuCapture, on) end
end

function M.open(cam,initialSection)
  if S.waitTransition or preGame() then return false end
  -- Not while full-body tracking calibrates: the steps move the hands (an accidental grip swipe opened it)
  if S.fbtCalibrating then return false end
  if type(VRMenuSurfaceReady) ~= "function" then return false end
  cam = cam or camera()
  if cam == nil then return false end
  if S.open then M.close() end
  local weightOK,weightError=InventoryData.refreshWeight()
  if not weightOK then print("[VRMenu] weight refresh failed: "..tostring(weightError)) end
  if not S.capOn and type(SetVRDiagCapture) == "function" then
    if pcall(SetVRDiagCapture, 1) then S.capOn = true end
  end
  S.anchor = anchorFor(cam)
  S.open, S.section, S.items, S.vehicle, S.toast, S.closeAt = true, initialSection, nil, nil, nil, nil
  local column=initialSection and 0 or COL_HOME
  S.columnOffset, S.columnTarget, S.columnMove = column, column, nil
  S.panelStart, S.panelInteractiveAt, S.openAnimAt = S.time, S.time, nil
  S.panelRevealEnd = 0
  Msg.leave(kit)
  Gameplay.leave(kit)
  S.lang = S.langOverride or D.language()
  L.set(S.lang)
  S.volume = D.effectsVolume()
  T = L.labels(TEXT.en, TEXT.ru)
  kit.T = T
  local C, R, U = frame()
  if C == nil then return false end
  if S.menu == nil or S.menu.id == nil then
    S.menu = spawn(ENT, transformFor(C, R, U))
  end
  for i = 1, 2 do
    if S.beams[i].id == nil then
      S.beams[i] = spawn(BEAM_ENT, transformFor(C, R, U))
    end
  end
  capture(true)
  play("open")
  Gameplay.prefetch()
  return true
end

function M.close()
  S.columnAction=nil
  if not S.open then return end
  local visit=M.shared and M.shared.ripperSession
  if visit and visit.active then VendorVisit.closeSession(visit);M.shared.ripperSession=nil end
  local stash=M.shared and M.shared.stashSession
  if stash and stash.active then M.shared.stashSession=nil end
  S.open = false
  S.summaryRect = nil
  S.summaryVisible=false
  S.resourceRect = nil
  S.section, S.items, S.vehicle = nil, nil, nil
  Msg.leave(kit)
  Gameplay.leave(kit)
  Set.leave()
  despawn(S.menu)
  S.menu = nil
  for i = 1, 2 do
    despawn(S.beams[i])
    S.beams[i] = {}
  end
  S.buttons, S.hovered, S.anims, S.views = {}, {}, {}, {}
  S.icons, S.columnMove, S.openAnimAt = {}, nil, nil
  S.moneyWidget,S.weightWidget,S.resourceValues=nil,nil,nil
  dropGestures()
  despawnGlass()
  capture(false)
  play("close")
end

function M.openTrade()
  local visit=VendorVisit.current(M.shared)
  if not visit then toast(T.blocked);return false end
  if visit.purchase then toast(L.tr("Покупка ещё выполняется","Purchase is still in progress"));return false end
  openSection("trade",true)
  return true
end

function M.openMap()
  if not D.openMap(M.shared and M.shared.menuController) then toast(T.blocked);return false end
  M.close()
  return true
end

---------------------------------------------------------------- the pointers

local function setCursor(i, x, y, size, col, op)
  local c = S.cursors and S.cursors[i]
  if c == nil then return end
  if op <= 0.0 then
    c.host:SetOpacity(0.0)
    return
  end
  local span=math.max(52,size*1.5)
  c.x,c.y=x,y
  K.move(c.host,x-span/2,y-span/2);c.host:SetScale(Vector2.new({X=span/64,Y=span/64}))
  if c.color~=col then
    K.tint(c.inner,col,.95);K.tint(c.outer,col,.45);c.dot:SetTintColor(K.color(col));c.color=col
  end
  c.host:SetOpacity(op)
end

local function invokeClick(b)
  S.inClick, S.soundInClick = true, false
  local ok, err = pcall(b.click)
  S.inClick = false
  if not ok then print("[VRMenu] click failed: " .. tostring(err)) end
  if not b.mute and not S.soundInClick then play("click") end
end

local function click(b, hand, x, y)
  if b == nil or b.click == nil then return end
  if S.columnAction then return end
  if S.time - (S.lastClick or -1) < 0.15 then return end
  S.lastClick = S.time
  S.clicks[#S.clicks + 1] = { t = S.time, group = b.group, key = b.key, x = b.x, y = b.y, view = b.view ~= nil }
  if #S.clicks > 8 then table.remove(S.clicks, 1) end
  haptic(hand, HAPTIC_CLICK)
  if b.group=="col" then b.pulseAt=S.time end
  local fx=S.pressFX and S.pressFX[hand];local cursor=S.cursors and S.cursors[hand]
  if fx and (x or cursor and cursor.x) then
    fx.x,fx.y,fx.at=x or cursor.x,y or cursor.y,S.time
  end
  -- the click's own sound, unless what it did made one (a toast, the menu closing): never two at once
  if b.group=="col" and (b.key=="close" or b.key=="map") then
    S.columnAction={button=b,at=S.time+.10}
  else invokeClick(b) end
end

local function updatePressFX()
  for _,b in ipairs(S.icons or {}) do if b.pulseAt then
    local t=(S.time-b.pulseAt)/.26
    if t>=1 then
      b.pulseAt=nil;b.ringHost:SetRotation(0);b.ringHost:SetScale(Vector2.new({X=1,Y=1}))
      K.tint(b.ring,b.visual.color,b.visual.outer);K.tint(b.outer,b.visual.color,b.visual.inner)
    else
      local scale=1+.07*math.sin(math.pi*t);b.ringHost:SetScale(Vector2.new({X=scale,Y=scale}));b.ringHost:SetRotation(45*(1-(1-t)^3))
      for i,segment in ipairs(b.ring) do
        local distance=((i-1)/#b.ring-t*1.5)%1
        segment:SetTintColor(K.color(K.YELLOW));segment:SetOpacity(.35+.65*math.max(0,1-distance/.3))
      end
      K.tint(b.outer,K.YELLOW,.95)
    end
  end end
  for _,fx in ipairs(S.pressFX or {}) do if fx.at then
    local t=(S.time-fx.at)/.24
    if t>=1 or not buttonAt(fx.x,fx.y) then fx.at=nil;fx.host:SetOpacity(0)
    else
      K.move(fx.host,fx.x-48,fx.y-48);local scale=.65+.6*t
      fx.host:SetScale(Vector2.new({X=scale,Y=scale}));fx.host:SetOpacity((1-t)^2)
    end
  end end
end

local function beam(i, show, origin, hit, length, cam)
  local h = S.beams[i]
  if h == nil or not resolve(h) then return end
  if not h.built then buildBeam(h) end
  if not ready(h) then return end
  unglitch(h)
  if not show then
    if h.shown ~= false then
      K.beamShow(h.strip, false)
      h.shown = false
    end
    return
  end
  local dir = norm(sub(hit, origin))
  local toCam = sub(cam.p, origin)
  local n = norm(sub(toCam, mul(dir, dot(toCam, dir))))
  if n[1] == 0 and n[2] == 0 and n[3] == 0 then return end
  local R = cross(n, dir)
  place(h, transformFor(add(origin, mul(dir, BEAM_LEN / 2)), R, mul(dir, -1)))
  K.beamLength(h.strip, length)
  if h.shown ~= true then
    K.beamShow(h.strip, true)
    h.shown = true
  end
end

-- THE GESTURES, the same for a fingertip ("finger") and a pointer held on its trigger ("trigger").
-- A press outside a list clicks at once, or takes hold of the slider it lands on. A press inside a
-- list waits to see what it is: let go where it was, it is a tap and clicks; moved, it scrolls the
-- list -- or, sideways on a slider, moves the slider.
local function takeSlider(P, hand, b, by, x, y)
  P.drag, P.dragBy = b, by
  haptic(hand, HAPTIC_CLICK)
  pcall(b.drag, x, "begin", hand, y)
end

-- The push's line: canvas px per metre of depth, from the touch to the deepest point so far.
local function pushSlope(t)
  local dd = t.dd - t.d0
  if math.abs(dd) < 0.004 then return 0, 0 end
  local kx, ky = (t.xd - t.x0) / dd, (t.yd - t.y0) / dd
  local m, cap = math.sqrt(kx * kx + ky * ky), SLOPE_MAX * PX
  if m > cap then kx, ky = kx * cap / m, ky * cap / m end
  return kx, ky
end

local function press(b, hand, P, x, y, by, d)
  P.touch = d ~= nil and { by = by, x0 = x, y0 = y, d0 = d, dd = d, xd = x, yd = y, kx = 0, ky = 0 } or nil
  local view = b and b.view or nil
  if view ~= nil then
    P.pend = { b = b, view = view, x0 = x, y0 = y, by = by, t0 = S.time }
  elseif b ~= nil and b.drag ~= nil then
    takeSlider(P, hand, b, by, x, y)
  else
    click(b, hand, x, y)
  end
end

local function move(P, hand, x, y, by, d)
  -- where the tip is along the screen, its own push taken off
  local t = P.touch
  if t ~= nil and t.by == by and d ~= nil then
    if not t.settled and d < t.dd then
      local rx = x - t.kx * (d - t.d0) - t.x0
      local ry = y - t.ky * (d - t.d0) - t.y0
      if t.d0 - d < LINE_START or math.max(math.abs(rx), math.abs(ry)) < TAP[by] * 0.75 then
        t.dd, t.xd, t.yd = d, x, y
        t.kx, t.ky = pushSlope(t)
      end
    end
    x, y = x - t.kx * (d - t.d0), y - t.ky * (d - t.d0)
    if not t.settled and math.max(math.abs(x - t.x0), math.abs(y - t.y0)) > TAP[by] then t.settled = true end
  end
  if P.dragBy == by and P.drag ~= nil then pcall(P.drag.drag, x, "move", hand, y) end
  local g = P.pend
  if g ~= nil and g.by == by and by == "trigger" and S.time - g.t0 < SETTLE then g.x0, g.y0 = x, y end
  if g ~= nil and g.by == by then
    local dx, dy = x - g.x0, y - g.y0
    local along = math.max(math.abs(dx), math.abs(dy))
    if along > TAP[by] then
      P.pend = nil
      S.lastTurn = { by = by, along = along, x0 = g.x0, y0 = g.y0, x = x, y = y, d = d,
                     kx = t and t.kx, ky = t and t.ky }
      if g.b ~= nil and g.b.drag ~= nil and math.abs(dx) > math.abs(dy) then
        takeSlider(P, hand, g.b, by, g.x0, g.y0)
        pcall(g.b.drag, x, "move", hand, y)
      elseif g.view:scrollable() then
        g.view:beginDrag(g.y0)
        P.scroll = { view = g.view, by = by }
      end
    end
  end
  local s = P.scroll
  if s ~= nil and s.by == by then
    s.view:dragTo(y, S.time)
  end
end

-- The press is over: let go (cancel: the hand was lost, nothing clicks).
local function finish(P, hand, by, cancel)
  if P.touch ~= nil and P.touch.by == by then P.touch = nil end
  if P.dragBy == by and P.drag ~= nil then
    local b = P.drag
    P.drag, P.dragBy = nil, nil
    pcall(b.drag, nil, "end", hand)
  end
  local g = P.pend
  if g ~= nil and g.by == by then
    P.pend = nil
    if not cancel and g.b ~= nil then
      if g.b.drag ~= nil then
        -- a tap on a slider: the knob goes where it was tapped
        takeSlider(P, hand, g.b, by, g.x0, g.y0)
        P.drag, P.dragBy = nil, nil
        pcall(g.b.drag, nil, "end", hand)
      else
        click(g.b, hand, g.x0, g.y0)
      end
    end
  end
  local s = P.scroll
  if s ~= nil and s.by == by then
    P.scroll = nil
    s.view:endDrag()
  end
end

local function busyWith(P, by)
  return P.dragBy == by or (P.pend ~= nil and P.pend.by == by) or (P.scroll ~= nil and P.scroll.by == by)
end

local function pointers(cam, C, R, U)
  if type(VRMenuPointer) ~= "function" then return end
  local out = nil
  pcall(function()
    out = VRMenuPointer(Vector4.new(C[1], C[2], C[3], 1), Vector4.new(R[1], R[2], R[3], 0),
      Vector4.new(U[1], U[2], U[3], 0), Vector4.new(W, H, 0, 0))
  end)
  if out == nil or #out < 8 then
    for hand = 1, 2 do
      local P=S.hands[hand]
      Stick.cancel(P);finish(P,hand,"finger",true);finish(P,hand,"trigger",true)
      setCursor(hand, 0, 0, 40, K.CYAN, 0)
      beam(hand, false, {}, {}, 0, cam)
    end
    S.stats = {}
    return
  end
  local hover = {}
  -- The native XInput bridge publishes the physical R3 button before the game consumes it.
  local rightStick=type(GetVRSharedSlot)=="function" and GetVRSharedSlot(159) or 0
  for hand = 1, 2 do
    local P = S.hands[hand]
    local base = (hand - 1) * 4
    local tip, ray, origin, hit = out[base + 1], out[base + 2], out[base + 3], out[base + 4]
    local poking, cursor = false, false
    P.pokeButton = nil
    local tipX, tipY = nil, nil

    local stick=hand==2 and rightStick or 0
    local stickHeld=Stick.held(P,stick)
    if stickHeld or P.stickScroll then
      finish(P,hand,"finger",true);finish(P,hand,"trigger",true)
      P.state="idle";P.lastD=nil
      -- A trigger held during an R3 grab must be released before it can start another click.
      P.trig=origin.w>TRIGGER_OFF
    end
    local stickButton,stickMode
    if tip.w>.5 and tip.z<HOVER and tip.z>THROUGH then
      stickButton=buttonAt(tip.x*CW,tip.y*CH);stickMode="tip"
    end
    if not stickButton and ray.w>.5 then stickButton=buttonAt(ray.x*CW,ray.y*CH);stickMode="ray" end
    Stick.update(P,stick,tip.w>.5 or ray.w>.5,stickButton and stickButton.view,stickMode,
      ray.w>.5 and ray.y*CH or nil,tip.w>.5 and tip.y*CH or nil,S.time)
    stickHeld=stickHeld or P.stickDown==true

    -- the fingertip: it presses what it pushes into from the front, and what it pressed follows it
    -- along the screen until it comes back out
    if tip.w > 0.5 then
      local d = tip.z
      local inside = tip.x >= 0 and tip.x <= 1 and tip.y >= 0 and tip.y <= 1
      local x, y = tip.x * CW, tip.y * CH
      tipX, tipY = x, y
      local b = inside and buttonAt(x, y) or nil
      P.state = P.state or "idle"
      if not stickHeld and P.state == "idle" then
        if b ~= nil and d <= PRESS and d > THROUGH and P.lastD ~= nil and P.lastD > PRESS then
          press(b, hand, P, x, y, "finger", d)
          P.state = "pressed"
        elseif d <= THROUGH then
          P.state = "through"
        end
      elseif not stickHeld and P.state == "pressed" and d > RELEASE then
        P.state = "idle"
        finish(P, hand, "finger")
      elseif not stickHeld and P.state == "through" and d > RELEASE then
        P.state = "idle"
      end
      if not stickHeld and P.state == "pressed" then move(P, hand, x, y, "finger", d) end
      P.lastD = not stickHeld and d or nil
      P.pokeButton = nil
      if b ~= nil and d < HOVER and (d > THROUGH or busyWith(P, "finger")) then
        poking = true
        P.pokeButton = b
        if b ~= nil then hover[b] = true end
        local ring = 20 + math.max(d, 0) * 380
        setCursor(hand, x, y, ring, K.YELLOW, P.state == "pressed" and 1 or .9)
        cursor = true
      end
    else
      P.lastD = nil
      if P.state == "pressed" then
        P.state = "idle"
        finish(P, hand, "finger", true)
      end
    end

    -- the pointer, while the finger is not touching
    local trigger = origin.w
    local rayInside = not poking and not busyWith(P, "finger") and ray.w > 0.5
      and ray.x >= 0 and ray.x <= 1 and ray.y >= 0 and ray.y <= 1
    local rb, rayX, rayY = nil, nil, nil
    if rayInside then
      rayX, rayY = ray.x * CW, ray.y * CH
      rb = buttonAt(rayX, rayY)
    end
    local rayOn = rb ~= nil
    if rayOn then
      hover[rb] = true
      setCursor(hand, rayX, rayY, (P.trig or P.stickScroll) and 16 or 22, K.YELLOW, 1.0)
      cursor = true
    end
    if not stickHeld and trigger > TRIGGER_ON and not P.trig then
      P.trig = true
      -- the trigger presses what the pointer is on, or what the finger hovers when it is that close
      if rayOn then
        press(rb, hand, P, rayX, rayY, "trigger")
      elseif poking then
        press(P.pokeButton, hand, P, tipX, tipY, "trigger")
      end
    elseif not stickHeld and trigger < TRIGGER_OFF and P.trig then
      P.trig = false
      finish(P, hand, "trigger")
    end
    if not stickHeld and P.trig then
      local x, y = rayX, rayY
      if x == nil and poking then x, y = tipX, tipY end
      if x ~= nil then move(P, hand, x, y, "trigger") end
    end
    if P.drag ~= nil then hover[P.drag] = true end

    if not cursor then setCursor(hand, 0, 0, 40, K.CYAN, 0.0) end
    beam(hand, rayOn, { origin.x, origin.y, origin.z }, { hit.x, hit.y, hit.z }, ray.z, cam)
    S.stats[hand] = { tip = { tip.x, tip.y, tip.z, tip.w }, ray = { ray.x, ray.y, ray.z, ray.w }, trigger = trigger, stick = stick,
                      cursor = cursor, beam = rayOn, hover = (P.pokeButton or rb) and (P.pokeButton or rb).key,
                      state = P.state, drag = P.dragBy, pend = P.pend ~= nil, scroll = P.scroll ~= nil or P.stickScroll ~= nil,
                      stickScroll = P.stickScroll~=nil,
                      touch = P.touch and { kx = P.touch.kx, ky = P.touch.ky, d0 = P.touch.d0, dd = P.touch.dd,
                                            settled = P.touch.settled == true } or nil }
  end
  -- repaint only what changed
  for b, _ in pairs(S.hovered) do
    if not hover[b] and b.paint then b.paint(false) end
  end
  for b, _ in pairs(hover) do
    if not S.hovered[b] and b.paint then b.paint(true) end
  end
  S.hovered = hover
  if S.section == "settings" then Set.hover(hover) end
end

---------------------------------------------------------------- the frame

local function tick(dt)
  -- a panel build that threw must not leave widgets outside the panel layer poolable
  K.poolEnd()
  retirePending()
  if S.columnAction and S.time>=S.columnAction.at then
    local b=S.columnAction.button;S.columnAction=nil;invokeClick(b)
  end
  if Gameplay.transition(kit, dt) then return end
  -- the swipe is read every frame, open or not: the history lives in the plugin
  local g = nil
  if type(VRMenuGesture) == "function" then pcall(function() g = VRMenuGesture() end) end
  -- L3 + R3 held, or the gripped swipe down: open or close, the menu straight ahead either way
  if g ~= nil and #g >= 1 and (g[1].x > 1.5 or #g >= 3) then
    if S.open then
      -- A physical grip swipe must not interrupt a list grabbed with R3. The explicit
      -- L3 + R3 chord still closes it; discard any incidental swipe during the grab.
      if g[1].x>1.5 or not Stick.blocksSwipe(S.hands[2],S.time) then M.close() end
    elseif sceneTier() < 3 and not paused() then
      if not armed() then
        M.open(camera())
      elseif g[1].x > 1.5 then
        -- A weapon in the hand stops only the swipe: a grip pulled down while holding a gun is easily an
        -- accident. The L3 + R3 chord is held on purpose, and with a gun out it did nothing at all -- the one
        -- way back into the menu (the menu closes itself while a weapon is drawn). So the chord puts the
        -- weapons away, as the wheel's holster does, and the menu opens once the hands are empty.
        holsterAll()
        S.openWhenUnarmed = S.time + 2.0
      end
    end
  end
  if S.openWhenUnarmed ~= nil and not S.open then
    if not armed() then
      S.openWhenUnarmed = nil
      if sceneTier() < 3 and not paused() then M.open(camera()) end
    elseif S.time > S.openWhenUnarmed then
      S.openWhenUnarmed = nil
    end
  end
  if not S.open then return end
  Gameplay.step()
  Gameplay.updateSummary(kit)

  if S.closeAt ~= nil and S.time >= S.closeAt then
    M.close()
    return
  end
  S.slow = (S.slow or 0) + dt
  if S.slow > 0.25 then
    S.slow = 0
    local visit=M.shared and M.shared.ripperSession
    if armed() or sceneTier() >= 3 or (S.anchor.vehicle == true) ~= (D.mounted() ~= nil) then
      M.close()
      return
    end
  end

  local C, R, U = frame()
  local cam = camera()
  if C == nil or cam == nil then
    M.close()
    return
  end
  if not S.anchor.vehicle then
    local col = add(add(C, mul(R, (COLX + S.columnOffset - CW / 2) / PX)), mul(U, (CH / 2 - COLY) / PX))
    local d = sub(col, cam.p)
    if math.sqrt(d[1] * d[1] + d[2] * d[2]) > WALK_AWAY or dot(sub(cam.p, C), cross(R, U)) < 0 then
      M.close()
      return
    end
  end

  if not resolve(S.menu) then return end
  unglitch(S.menu)
  if not S.menu.built then
    build.all()
    print(string.format("[VRMenu] screen ready %.2f s after the spawn", S.menu.readyIn or -1))
  end
  updateResources(dt)
  if ready(S.menu) and (S.anchor.vehicle or not S.menu.placed) then
    place(S.menu, transformFor(C, R, U))
    S.menu.placed = true
  end
  if not S.menu.placed then return end
  if S.surfaceWarmup then S.menu.root:RemoveChild(S.surfaceWarmup);S.surfaceWarmup=nil end
  -- The opening drop begins when the prepared display is revealed, so GPU preparation
  -- cannot consume the animation while the screen is still invisible.
  if S.openAnimAt ~= nil then
    local wait = S.time - S.openAnimAt
    for _, a in ipairs(S.anims) do a.t0 = a.t0 + wait end
    S.glassAfter=S.glassAfter+wait
    S.panelStart=S.panelStart+wait
    S.panelInteractiveAt=S.panelInteractiveAt+wait
    S.panelRevealEnd=S.panelRevealEnd+wait
    S.openAnimAt = nil
  end
  stepColumn()
  stepAnims()
  syncGlass(C, R, U, S.anchor.vehicle == true)
  if S.refreshAt ~= nil and S.time >= S.refreshAt then
    S.refreshAt = nil
    build.panels(false, false)
  end
  if S.toast ~= nil and S.time >= S.toast.until_ and S.toastWidget ~= nil then
    S.toast = nil
    pcall(function()
      S.toastWidget:SetVisible(false)
      S.toastBg:SetVisible(false)
    end)
  end
  if S.section == "settings" then Set.update(dt) end
  if S.section == "contacts" then Msg.update(kit, dt) end
  if S.section == "load" or S.section == "stash" or S.section == "trade" or S.section == "cyberware" or S.section == "crafting" or S.section == "inventory" or S.section == "character" or S.section == "journal" or S.section == "codex" or S.section == "shards" or S.section == "tarot" or S.section == "timeskip" then
    Gameplay.update(kit)
  end
  pointers(cam, C, R, U)
  updatePressFX()
  for _, v in pairs(S.views) do v:update(dt, 3) end
end

---------------------------------------------------------------- driven by init.lua

-- `shared` belongs to init.lua and outlives a hot reload: the in-game menu controller it observes.
function M.init(shared)
  M.shared = shared
  shared.vendorClock=S.time
  VendorVisit.bind(shared)
  StashVisit.bind(shared)
  LoadData.bind(shared)
  kit.menuController = function() return shared.menuController end
  kit.ripperSession = function() return shared.ripperSession end
  kit.vendorSession = function() return VendorVisit.current(shared) end
  kit.stashSession = function() return StashVisit.current(shared) end
  kit.close = M.close
  kit.loadSave = function(handler,index)
    M.close()
    handler:LoadSavedGame(index)
  end
  kit.openSection = openSection
  kit.clearSummaryButtons = function() clearButtons("summary") end
  -- The next hot-loaded module must finish removals queued by the previous one.
  shared.menuRetiring = shared.menuRetiring or retiring
  retiring = shared.menuRetiring
end

function M.update(dt)
  S.time = S.time + (dt or 0)
  -- Full-body tracking calibration started from the settings: the menu gets out of the way (VRFbtCalibrating).
  if type(VRFbtCalibrating) == "function" then
    local ok, on = pcall(VRFbtCalibrating)
    on = ok and on == 1
    if on and S.open then M.close() end   -- at the start, and whatever opens it during the calibration
    -- The step's instruction is drawn in the view (the menu is closed): hand the plugin the translated texts once
    -- per calibration.
    if on and not S.fbtCalibrating and type(VRFbtSetCalibrationText) == "function" then
      local steps = {"Step 1: look ahead at the horizon line, then press both triggers (press to cancel)",
                     "Step 2: arms down, look down, then press both triggers (press to cancel)",
                     "Step 3: face the cross: head on its top, arms straight out along its arms, until the countdown ends (press to keep V's arms)",
                     "",
                     -- 5..7: why a pull was not taken (Fbt/Game/Calibration.hpp kRefuse*), drawn in red under the step
                     "Not taken: look straight at the horizon line until it turns green, then press again",
                     "Not taken: stretch both arms straight out along the cross, then hold still again",
                     "Not taken: stand straight with the headset, trackers and controllers tracked, then press again"}
      for i, en in ipairs(steps) do
        -- the menu's own hint in brackets goes too (ASCII or full-width brackets)
        local text = L.setting(en):gsub("^%*%s*", ""):gsub("%s*%b()%s*$", ""):gsub("%s*（.-）%s*$", "")
        pcall(VRFbtSetCalibrationText, i, text)
      end
    end
    S.fbtCalibrating = on
  end
  Hud.update(S.open and S.menu~=nil and S.menu.placed==true,dt or 0)
  M.shared.vendorClock=S.time
  VendorVisit.pollNative(M.shared)
  local stash=M.shared and M.shared.stashSession
  if stash and stash.active and not StashVisit.current(M.shared) then
    M.shared.stashSession=nil
    if S.open then openSection("inventory",true) end
  elseif stash and stash.pending and Game.GetPlayer() then
    if not stash.transferring then
      if S.open then M.close() end
      M.shared.ripperSession=nil
      stash.transferring=true
      if stash.closeNative then D.menuEvent(M.shared.menuController,"OnCloseHubMenuRequest");stash.closeNative=false end
      return
    end
    if not paused() then
      if not S.open then M.openCentred() end
      if S.menu and S.menu.built then
        stash.active=true;stash.pending=false
        openSection("stash",true)
      end
    end
  end
  local visit=M.shared and M.shared.ripperSession
  if visit and visit.pending and Game.GetPlayer() then
    if not visit.transferring then
      if S.open then M.close() end
      visit.transferring=true
      return
    end
    if VendorVisit.advance(M.shared,visit,S.time) and not paused() then
      if not S.open then
        -- Build the requested shop before the surface's first GPU-ready check.
        -- Opening the empty column first exposed glass before its panel pixels.
        visit.active=true
        if M.openCentred(visit.ripperdoc==false and "trade" or "cyberware") then
          VendorVisit.openSession(visit)
          visit.pending=false;visit.controller=nil
        else visit.active=false end
      end
    end
  end
  if S.pending ~= nil and S.time >= S.pending.at then
    local ev = S.pending.event
    S.pending = nil
    D.menuEvent(M.shared and M.shared.menuController, ev)
  end
  if Game.GetPlayer() == nil then
    Hud.restore()
    VendorVisit.cancelHandoff(M.shared)
    M.shared.ripperSession=nil
    M.shared.stashSession=nil
    Gameplay.cancelTransition(kit)
    if S.open then
      pcall(Gameplay.leave,kit)
      S.open = false
      capture(false)
    end
    S.menu, S.beams, S.anchor, S.glass = nil, { {}, {} }, nil, {}
    psm = nil
    return
  end
  local ok, err = pcall(tick, dt or 0)
  if not ok then
    S.lastError = tostring(err)
    print("[VRMenu] " .. tostring(err))
    Gameplay.cancelTransition(kit)
    pcall(M.close)
  end
end

-- Everything this module holds in the world goes: the screen, the beams, the held-back buttons.
function M.release()
  if type(VRMenuVendorPoll)=="function" then pcall(VRMenuVendorPoll,false) end
  Hud.restore()
  VendorVisit.restorePresentation(M.shared)
  pcall(M.close)
  Gameplay.cancelTransition(kit)
  capture(false)
end

-- For the harness and the console: GetMod("CyberpunkVRPort_Menu").state()
function M.state()
  local views = {}
  for k, v in pairs(S.views) do
    views[k] = { offset = v.offset, max = v:maxOffset(), items = #v.items, h = v.h }
  end
  return {
    open = S.open, section = S.section, built = S.menu ~= nil and S.menu.built or false,
    placed = S.menu ~= nil and S.menu.placed or false,
    readyIn = S.menu ~= nil and S.menu.readyIn or nil, buttons = #S.buttons, lang = S.lang,
    hands = S.stats, anchor = S.anchor, views = views, settings = Set.state(), haptics = S.haptics,
    lastTurn = S.lastTurn, clicks = S.clicks, time = S.time,
    columnX = COLX + S.columnOffset, columnMoving = S.columnMove ~= nil,
    pendingRetire = #retiring,
    volume = S.section == "radio" and D.volume() or nil,
  }
end

function M.isOpen() return S.open end
function M.section(key) openSection(key) end
function M.debugRoot() return S.menu and S.menu.root end
function M.debugComp() return S.menu and S.menu.comp end
function M.debugState() return S end
function M.rebuild() if S.open and S.menu and S.menu.built then build.panels(false, false) end end
-- For the harness: n rebuilds of the open panel with every ink helper counted and timed (ms per call
-- kind, summed over the n), plus the whole and its main phases.
function M.profile(n,modules)
  if not (S.open and S.menu and S.menu.built) then return "closed" end
  local clock,stats,saved=os.clock,{},{}
  local targets={K=K}
  for _,name in ipairs(modules or {}) do targets[name]=require("modules/"..name) end
  for prefix,mod in pairs(targets) do
    for name,fn in pairs(mod) do
      if type(fn)=="function" and name~="color" and name~="clip" and name~="upper" and name~="loc" and name~="key" then
        local label=prefix=="K" and name or prefix.."."..name
        saved[#saved+1]={mod,name,fn}
        mod[name]=function(...)
          local t=clock();local a,b,c=fn(...)
          local e=stats[label] or {n=0,t=0};e.n=e.n+1;e.t=e.t+(clock()-t);stats[label]=e
          return a,b,c
        end
      end
    end
  end
  local phases={}
  local function timed(name,fn) local t=clock();fn();phases[name]=(phases[name] or 0)+(clock()-t) end
  local t0=clock()
  for _=1,n or 10 do
    timed("panels",function() build.panels(false,false) end)
    timed("views",function() for _,v in pairs(S.views) do v:update(0,3) end end)
  end
  local total=clock()-t0
  for _,s in ipairs(saved) do s[1][s[2]]=s[3] end
  local out={string.format("total %.1f ms/rebuild",total*1000/(n or 10))}
  for name,v in pairs(phases) do out[#out+1]=string.format("%s %.1f",name,v*1000/(n or 10)) end
  local list={}
  for name,e in pairs(stats) do list[#list+1]={name=name,n=e.n,t=e.t} end
  table.sort(list,function(a,b) return a.t>b.t end)
  for i=1,math.min(#list,20) do
    local e=list[i];out[#out+1]=string.format("%s x%d %.1f",e.name,e.n/(n or 10),e.t*1000/(n or 10))
  end
  return table.concat(out,"  ")
end
function M.click(x, y) click(buttonAt(x, y), 2, x, y) end
function M.hit(x, y)
  local b = buttonAt(x, y)
  return b and { key = b.key, group = b.group, x = b.x, y = b.y, w = b.w, h = b.h } or nil
end
function M.scroll(key, dy)
  local v = S.views[key or "p1"]
  if v ~= nil then v:scrollBy(dy) end
end
function M.settingsPage(page) Set.page(page) end
function M.settingsRows() return Set.rows() end
-- Checking a translation without changing the game's language: the menu opens in `code` (a game language code,
-- nil = the game's own again) from the next open on. L.missing() then lists what that language still lacks.
function M.language(code) S.langOverride = code end
function M.missing() return L.missing() end
function M.openCentred(initialSection) return M.open(camera(),initialSection) end
function M.glass(on)
  S.glassOn = on ~= false
  return #S.glass
end

return M
