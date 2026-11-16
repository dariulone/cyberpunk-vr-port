-- Exercise the production target selection and update, with only game-facing
-- services replaced. No Cyberpunk process, files or runtime are needed.
math.atan2 = math.atan2 or math.atan
local M = assert(loadfile(arg[1]))()
local S = M.debug()
local enabled = true
VRCyberFingerEnabled = function() return enabled end
local function upvalue(fn, name, replacement)
  for i = 1, 100 do
    local key, value = debug.getupvalue(fn, i)
    if key == nil then break end
    if key == name then
      if replacement ~= nil then debug.setupvalue(fn, i, replacement) end
      return value
    end
  end
  error("missing production upvalue: " .. name)
end
local frame = upvalue(M.update, "frame")
local choose = upvalue(frame, "choose")
local H = {pos = {0,0,0}, palm = {o = {0,0,0}, d = {1,0,0}}}
local near = {id = "near", pos = {.1,0,0}}
local far = {id = "far", pos = {.5,0,0}}
S.time = 0
S.things = {far}
assert(choose(H) == nil, "CyberFinger accepted remote target")
enabled = false
assert(choose(H) == far, "disabled profile changed stock remote target")
enabled = true
S.things = {far, near}
assert(choose(H) == near, "nearest palm target lost")
H.busy = true
assert(choose(H) == nil, "occupied hand can select")
H.busy = false
S.things = {{id = "boundary", pos = {.32,0,0}}}
assert(choose(H) == nil, "reach boundary exceeded")

local taken, owner = nil, "near"
Game = {
  GetPlayer = function() return {} end,
  GetTargetingSystem = function()
    return {GetLookAtObject = function()
      return {GetEntityID = function() return {hash = "near"} end}
    end}
  end,
}
VRPalmModelPos = function() end
upvalue(frame, "drawTaken", function() end)
upvalue(frame, "blocked", function() return false end)
upvalue(frame, "refresh", function() end)
upvalue(frame, "where", function(t) return t.pos end)
upvalue(frame, "hands", function() end)
upvalue(frame, "aim", function() end)
upvalue(frame, "offers", function() return owner, false end)
upvalue(frame, "take", function(_, t) taken = t end)
upvalue(M.update, "outlineFrame", function() end)
local marker = {mark = function() end}
local function tick(grip, previous)
  taken = nil
  S.target, S.flight, S.draw = nil, nil, nil
  S.things, S.byId = {near, far}, {near = near, far = far}
  S.snubbed = {}
  S.hands = {H, {busy = true}}
  H.grip, H.gripPrev = grip, previous
  M.update(.01, marker)
  return taken
end
assert(tick(false, false) == nil, "proximity alone grabs")
assert(tick(true, true) == nil, "already closed hand grabs on approach")
assert(tick(true, false) == near, "closing near a target does not take it")
owner = "far"
assert(tick(true, false) == nil, "sticky prompt reaches beyond palm radius")
enabled = false
assert(tick(true, false) == far, "disabled profile changed stock sticky-prompt behavior")
VRCyberFingerEnabled = nil
S.things = {far}
assert(choose(H) == far, "old plugin fallback changed")
print("PASS nearby selection, occupied hand, close edge, remote rejection, sticky prompt and disabled profile")
