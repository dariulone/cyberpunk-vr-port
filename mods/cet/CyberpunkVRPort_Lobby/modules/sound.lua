-- The lobby's sound, played by the port (src/Audio/PortSound.cpp: VRSoundChannel, VRMenuPlayWav).
--
-- The game would start the main menu's music the moment the main menu comes up -- in the dark, then under the
-- intro's legal notice; while the lobby can be the main menu the plugin holds that back (src/Lobby/MenuLobby.cpp) and
-- the lobby sounds like this instead, the game's own sounds picked by ear (tools/vr_lobby_assets/lobby_sounds.py
-- makes sounds\*.ogg; game_sounds.py exports the rest to choose from, build/sounds/candidates):
--  - the notice: a cyberspace hum, muffled -- the game is still loading behind it -- looped from its middle, so it
--    does not keep starting over; the plugin's (VRIntroHum, src/Lobby/Intro.cpp), as the notice is: it starts under
--    the boot's screen, before any script here runs;
--  - the logo: a whoosh as it appears, the boot splash's glitch where it glitches; the hum fades out under them;
--  - the lobby: a VR game's music, quiet, looped from 30 s to before the fade it ends on -- unless it is switched off
--    (the main page's button: xr_lobby_ambient, kept with the port's settings).
-- Each follows the game's sliders: master times music for the music, master times effects for the rest. A channel
-- told again what it already plays only takes the new volume, so a reloaded mod does not start anything twice.
local M = {}

local DIR = "CyberpunkVRPort_Lobby/sounds/"
-- volume: against its slider. lowpass Hz; loopFrom, loopTo, crossfade s (Audio/PortSound.hpp).
local WHOOSH = { file = DIR .. "logo_whoosh.ogg", volume = 0.8 }
local GLITCH = { file = DIR .. "logo_glitch.ogg", volume = 0.6 }
local AMBIENT = { file = DIR .. "ambient.ogg", volume = 0.3, lowpass = 0, loopFrom = 30.0, loopTo = 172.0,
                  crossfade = 3.0, music = true }
-- The menu's: it comes up (a hologram turning on), a hand comes over a row (the game's menu's own hover), a press, a
-- page opened, a page left (the VR menu's: CyberpunkVRPort_Menu/sounds).
local MENU_DIR = "CyberpunkVRPort_Menu/sounds/"
local MENU = {
  appear = { file = DIR .. "menu_appear.ogg", volume = 0.7 },
  hover = { file = DIR .. "menu_hover.ogg", volume = 1.0 },
  click = { file = MENU_DIR .. "click.wav", volume = 0.8 },
  open = { file = MENU_DIR .. "open.wav", volume = 0.8 },
  close = { file = MENU_DIR .. "close.wav", volume = 0.8 },
}
local HOVER_GAP = 0.06                   -- s: a hand swept across the rows does not rattle
local AMBIENT_IN, AMBIENT_OUT, GLITCH_OUT = 4.0, 2.0, 0.35

local S = { hum = false, glitch = false, ambient = false, sliders = nil, clock = 0 }

-- The game's sliders, 0..1: master, music, effects.
local function slider(name)
  local n = nil
  pcall(function()
    local ss = Game.GetSettingsSystem()
    if ss:HasGroup("/audio/volume") and ss:GetGroup("/audio/volume"):HasVar(CName.new(name)) then
      n = ss:GetVar("/audio/volume", CName.new(name)):GetValue()
    end
  end)
  return (type(n) == "number") and math.max(0, math.min(100, n)) / 100 or 1.0
end

local function readSliders()
  local master = slider("MasterVolume")
  return { music = master * slider("MusicVolume"), effects = master * slider("SfxVolume") }
end

local function volumeOf(s)
  S.sliders = S.sliders or readSliders()
  return s.volume * (s.music and S.sliders.music or S.sliders.effects)
end

local function channel(name, s, fade)
  if type(VRSoundChannel) ~= "function" then return false end
  local ok, played = false, false
  if s == nil then
    ok, played = pcall(VRSoundChannel, name, "", 0.0, 0.0, -1.0, 0.0, 0.0, fade)
  else
    ok, played = pcall(VRSoundChannel, name, s.file, volumeOf(s), s.lowpass or 0, s.loopFrom or -1, s.loopTo or 0,
                       s.crossfade or 0, fade)
  end
  return ok and played
end

local function once(s)
  if type(VRMenuPlayWav) == "function" then pcall(VRMenuPlayWav, s.file, volumeOf(s)) end
end

local function hum(on)
  S.sliders = S.sliders or readSliders()
  if type(VRIntroHum) == "function" then pcall(VRIntroHum, on, S.sliders.effects) end
end

-- The notice's hum, on or off: on takes over the one the boot's notice started.
function M.hum(on)
  if on == S.hum then return end
  S.hum = on
  hum(on)
end

-- The logo appears: its whoosh, and the hum goes.
function M.logo()
  once(WHOOSH)
  M.hum(false)
end

-- The logo glitches (on), and is whole again (off: the glitch's sound fades before its second burst).
function M.glitch(on)
  if on == S.glitch then return end
  S.glitch = on
  channel("lobby_glitch", on and GLITCH or nil, on and 0.0 or GLITCH_OUT)
end

-- The menu's sound for what happened: "appear", "hover", "click", "open", "close".
function M.menu(kind)
  local s = MENU[kind]
  if s == nil then return end
  if kind == "hover" then
    local now = os.clock()
    if S.hoverAt ~= nil and now - S.hoverAt < HOVER_GAP then return end
    S.hoverAt = now
  end
  once(s)
end

-- Whether the lobby's ambience is switched on (the setting), and switching it.
function M.ambientOn()
  local ok, on = pcall(function() return type(VRLobbyAmbient) ~= "function" or VRLobbyAmbient() end)
  return not ok or on ~= false
end

function M.setAmbientOn(on)
  if type(VRSetLobbyAmbient) == "function" then pcall(VRSetLobbyAmbient, on) end
end

-- The lobby is open (wanted) or not: the ambience plays while it is and the setting allows.
function M.ambient(wanted)
  local on = wanted and M.ambientOn()
  if on == S.ambient then return end
  S.ambient = on
  channel("lobby_ambient", on and AMBIENT or nil, on and AMBIENT_IN or AMBIENT_OUT)
end

-- Once a frame: the sliders read again every second, what plays follows them.
function M.update(dt)
  S.clock = S.clock - (dt or 0)
  if S.clock > 0 then return end
  S.clock = 1.0
  local now = readSliders()
  local was = S.sliders
  S.sliders = now
  if was ~= nil and (was.music ~= now.music or was.effects ~= now.effects) then
    if S.hum then hum(true) end
    if S.ambient then channel("lobby_ambient", AMBIENT, 0.3) end
  end
end

function M.state() return { hum = S.hum, ambient = S.ambient, on = M.ambientOn(), sliders = S.sliders } end

return M
