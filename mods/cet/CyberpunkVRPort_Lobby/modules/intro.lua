-- The intro: the game starts in the dark.
--
-- Its first stage, the game's own legal notice -- its localised bink, the one the boot splash played -- with the
-- port's word below it, is the boot's: the game's engagement screen shows it while the game boots into the main menu
-- (mods/redscript/CyberpunkVRPort_EngineStereo vrport_boot_notice.reds), and the plugin hangs it in the headset's
-- dark, its hum under it, until this takes it over (src/Lobby/Intro.cpp, VRIntroTakeBootNotice). The first time in a
-- launch that the place stands and the player is on it (modules/lobby.lua), the eyes still shut on the world
-- (VRLobbyCurtain), the notice fades out, and the game's animated logo (cp_logo_intro_<language>.bk2) plays where it
-- hung and flies off ahead, out to ten metres, while the curtain opens and the lobby comes up round it; the logo
-- fades as it gets there, and the lobby opens its menu.
--
-- WHAT IT IS. A lobby screen (vrport\ui\vr_lobby.ent; its canvas is drawn into a texture of its own,
-- src/Natives/MenuSurface.cpp) with its display mesh left off: what its canvas shows reaches the eyes only through
-- the plugin, drawn into the frame over the world -- the headset's and the window's alike (VRLobbyIntro:
-- src/Lobby/Intro.cpp, src/Render/IntroOverlay.cpp) -- at the intro's heading (src/Lobby/Intro.hpp Heading: the head
-- looks about within 90 degrees of it), one picture to both eyes, as the notice -- light on clear air, as the lobby's
-- screens are, so the bink's black is nothing, and the logo's dark red backdrop is cut off under a black point. Its
-- distance, height and brightness are given every frame; the curtain is this file's while the intro runs
-- (closure()). The engine draws a screen's
-- canvas only while the screen is in view -- under the floor its canvas was never drawn -- so the screen itself hangs
-- just before the eyes, wherever the player looks. A canvas of these screens is painted only when something on it
-- changes, and a video that starts is not such a change: its start is followed by one (play()), or the bink plays on
-- unseen.
--
-- Its sound is the lobby's (modules/sound.lua): the notice's hum taken over and faded as the logo appears with a
-- whoosh, the boot splash's glitch where the logo glitches.
local M = {}
local Spawner = require("modules/spawner")
local Sound = require("modules/sound")
local Ink = require("modules/ink")
local V = require("modules/vec")

local ENT = "vrport\\ui\\vr_lobby.ent"
local UI = "vrp_lobby_ui"
local TAG = "VRLobbyIntro"
local CW, CH = 2580, 1980                -- the lobby screen's canvas (tools/vr_lobby_assets/lobby_layout.py)
local AHEAD = 1.5                        -- the screen before the eyes, its display off (m)

local LOGO = "base\\movies\\fullscreen\\logo_splashscreen\\cp_logo_intro_%s.bk2"
local LOGO_LANG = { ["kr-kr"] = "kr", ["zh-cn"] = "zhs", ["zh-tw"] = "zht" }

-- The canvas: the bink 16:9 across it, in its middle.
local VIDEO_H = math.floor(CW * 9 / 16)
local VIDEO_Y = math.floor((CH - VIDEO_H) / 2)

-- How it plays (s, m). The notice fades out as the intro takes it over; the logo starts where it hung (as far and as
-- wide: src/Lobby/Intro.hpp kNoticeDistance, kNoticeWidth) and flies out. The logo's backdrop is a dark red no
-- brighter than 0.04 in linear light, its letters 0.5 and more (measured on the headset image). The logo bink comes
-- in glitching: its first streaks 0.27 s into the flight,
-- torn worst at 0.5 s, whole by 1.0 s (headset shots every 0.22 s, 2026-10-30) -- the glitch's sound starts at
-- glitchAt, its first hit 0.25 s into it, and is faded out from glitchEnd, before its second burst.
local NOTICE_OUT = 0.8
local FLIGHT = { from = 3.4, to = 13.0, heightFrom = 2.3, heightTo = 5.0, time = 6.5, openAfter = 0.8, open = 3.0,
                 fadeOut = 1.4, blackPoint = 0.06, glitchAt = 0.15, glitchEnd = 1.15 }

local S = { played = false, phase = nil, clock = 0, closure = 1.0 }

local function language()
  local lang = "en-us"
  pcall(function()
    local ss = Game.GetSettingsSystem()
    if ss:HasGroup("/language") and ss:GetGroup("/language"):HasVar(CName.new("OnScreen")) then
      lang = NameToString(ss:GetVar("/language", CName.new("OnScreen")):GetValue())
    end
  end)
  return lang
end

local function smooth(x)
  x = math.max(0.0, math.min(1.0, x))
  return x * x * (3.0 - 2.0 * x)
end

-- Starts the video, and has the canvas painted again so that the picture has it.
local function play(path)
  S.video:Stop()
  S.video:SetVideoPath(ResRef.FromString(path))
  S.video:Play()
  S.video:SetVisible(false)
  S.video:SetVisible(true)
end

-- The video the logo's bink plays in. A video with nothing playing draws nothing, and a canvas that draws nothing is
-- never painted into its texture (VRMenuSurfaceReady): a black dot, clear air on these screens, has it painted.
local function build()
  local layer = Ink.canvas(S.root, 0, 0, CW, CH)
  Ink.rect(layer, 0, 0, 4, 4, { 0, 0, 0 }, 1.0)
  S.video = Ink.place(inkVideoWidget.new(), layer, 0, VIDEO_Y, CW, VIDEO_H)
  S.video:SetLoop(false)
  S.logoPath = string.format(LOGO, LOGO_LANG[language()] or "en")
end

local function show(distance, height, opacity, blackPoint)
  if type(VRLobbyIntro) ~= "function" or S.comp == nil then return false end
  local ok, shown = pcall(VRLobbyIntro, S.comp, distance, height, opacity, blackPoint or 0.0)
  return ok and shown
end

local function finish()
  Sound.hum(false)
  Sound.glitch(false)
  if type(VRLobbyIntro) == "function" then pcall(VRLobbyIntro, nil, 0.0, 0.0, 0.0, 0.0) end
  if S.id ~= nil then Spawner.delete(S.id) end
  S.id, S.comp, S.root, S.entity = nil, nil, nil, nil
  S.phase, S.closure = "done", 0.0
end

local function enter(phase)
  S.phase, S.clock = phase, 0
end

-- The view's place in the world: the plugin's, else the active camera's. Position { x, y, z }, Quaternion.
local function view()
  local p, q = nil, nil
  if type(VRViewWorldPos) == "function" and type(VRViewWorldRot) == "function" then
    local wp = VRViewWorldPos()
    if wp ~= nil and wp.w ~= nil and wp.w > 0.5 then p, q = V.v3(wp), VRViewWorldRot() end
  end
  if p == nil then
    local cam = Transform.new()
    Game.GetCameraSystem():GetActiveCameraWorldTransform(cam)
    p, q = V.v3(cam.position), cam.orientation
  end
  return p, q
end

local function beforeEyes()
  local p, q = view()
  local at = V.add(p, V.qrot(V.quat(q), { 0, AHEAD, 0 }))
  local tr = WorldTransform.new()
  tr:SetPosition(Vector4.new(at[1], at[2], at[3], 1.0))
  tr:SetOrientation(q)
  return tr
end

-- The place stands and the player is on it: the intro begins, once a launch.
function M.begin()
  if S.played then return end
  S.played = true
  local ok = pcall(function() S.id = Spawner.spawn(ENT, beforeEyes(), TAG) end)
  if not ok or S.id == nil then
    finish()
    return
  end
  S.closure = 1.0
  enter("spawn")
end

function M.update(dt)
  dt = dt or 0
  if S.phase == nil or S.phase == "done" then return end
  S.clock = S.clock + dt
  if S.entity ~= nil then pcall(function() Game.VREntitySetWorldTransform(S.entity, beforeEyes()) end) end
  if S.phase == "spawn" then
    pcall(function()
      S.entity = Game.FindEntityByID(S.id)
      if S.entity ~= nil then S.comp = S.entity:FindComponentByName(CName.new(UI)) end
      if S.comp ~= nil then S.root = S.comp:GetWidget() end
    end)
    if S.root ~= nil then
      build()
      enter("ready")
    elseif S.clock > 10.0 then
      finish()
    end
  elseif S.phase == "ready" then
    -- Its canvas drawn once into the screen's own texture: the logo can be shown, once the notice has gone. The
    -- notice's hum is taken over to fade under the logo.
    local ok, done = pcall(VRMenuSurfaceReady, S.comp, S.root)
    if ok and done and show(FLIGHT.from, FLIGHT.heightFrom, 0.0) then
      local took = type(VRIntroTakeBootNotice) == "function" and VRIntroTakeBootNotice(NOTICE_OUT)
      Sound.hum(true)
      enter(took and "notice" or "logo")
    elseif S.clock > 10.0 then
      finish()
    end
  elseif S.phase == "notice" then
    if S.clock >= NOTICE_OUT then enter("logo") end
  elseif S.phase == "logo" then
    play(S.logoPath)
    Sound.logo()
    S.glitched = false
    enter("flight")
  elseif S.phase == "flight" then
    local k = smooth(S.clock / FLIGHT.time)
    local distance = FLIGHT.from + (FLIGHT.to - FLIGHT.from) * k
    local height = FLIGHT.heightFrom + (FLIGHT.heightTo - FLIGHT.heightFrom) * k
    S.closure = 1.0 - smooth((S.clock - FLIGHT.openAfter) / FLIGHT.open)
    local a = 1.0 - smooth((S.clock - FLIGHT.time) / FLIGHT.fadeOut)
    show(distance, height, a, FLIGHT.blackPoint)
    if not S.glitched and S.clock >= FLIGHT.glitchAt then
      S.glitched = true
      Sound.glitch(true)
    elseif S.glitched and S.clock >= FLIGHT.glitchEnd then
      Sound.glitch(false)
    end
    if S.clock >= FLIGHT.time + FLIGHT.fadeOut then finish() end
  end
end

-- Running: the curtain is the intro's (closure), the menu waits.
function M.running() return S.phase ~= nil and S.phase ~= "done" end
function M.closure() return S.closure end

-- The lobby went away while it ran (stepped aside, switched off): nothing of it is left.
function M.stop()
  if M.running() then finish() end
end

function M.state() return { played = S.played, phase = S.phase, clock = S.clock, closure = S.closure } end
function M.debug() return S end

-- For tests: play it again on the next begin().
function M.rewind()
  M.stop()
  S.played, S.phase = false, nil
end

return M
