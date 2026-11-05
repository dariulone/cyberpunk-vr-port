-- The engrams on the table: one shard for each of the player's playthroughs (a nomad's, a corpo's -- each branch of
-- saves), read from that playthrough's newest save. No saves, no shards.
--
-- WHAT ONE IS. The game's own data shard, the one on desks all over Night City, in one of its four looks
-- (tools/vr_lobby_assets/make_engrams.py: vrport\engram\vr_engram_<look>.ent, a dynamic body of its size): a thing
-- on the table like the rest (modules/items.lua), where the scene's shards lay (the first marks where the row starts,
-- the second which way it runs; a fourth starts a row behind), held the way a shard is held (modules/grips.lua).
-- Put to the neck -- the personal link, under the ear -- it goes in: it is taken from the hand and the player is
-- that V: the construct takes its hologram off (modules/construct.lua M.ownBody), the save's look goes on the
-- player's own body (modules/creation.lua M.wear) and its clothes on top (modules/clothes.lua). CONTINUE goes on with that playthrough (modules/mainmenu.lua asks
-- M.inserted()). Taken out -- a grip closed at the neck -- it is in that hand again and the hologram is back. One
-- engram is in at a time: another put in sends the one that was back to the table.
--
-- WHERE THE LOOKS COME FROM. The plugin reads every save (VRLobbySaveLooks, src/Lobby/SaveLooks.cpp): a "look" line
-- per playthrough -- its id, the save folder, life path, body and brain, level, time -- and under it the save's
-- "app" lines (group, option, appearance) and "morph" lines (group, region, morph), then what V wears: "wear" lines
-- (equipment area, item) or the EquipmentEx outfit's "outfit" lines (slot, item).
local M = {}
local Items = require("modules/items")
local Hands = require("modules/hands")
local Construct = require("modules/construct")
local Clothes = require("modules/clothes")
local V = require("modules/vec")

local SHARD = "^item shard"
local ENT = "vrport\\engram\\vr_engram_%s.ent"
local LOOKS = { "default", "b", "c", "d" }
local RADIUS, MASS = 0.023, 0.02         -- make_engrams.py: the shard's half length, its mass
local SHARD_HALF = { 0.011, 0.0232, 0.002 } -- the game shard's half sizes (shard_a.mesh bounds), for the grasp
local SPACING = 0.09                     -- between two engrams in a row (m)
local PER_ROW = 3
local NECK_DROP = 0.11                   -- the neck under the eyes (m)
local NECK_BACK = 0.09                   -- and behind them, under the ears: straight under the eyes is the mouth (m)
local INSERT = 0.08                      -- how near the neck a held engram goes in: the mouth stays out of it (m)
local TAKE_OUT = 0.14                    -- how near the neck a closing grip takes it out (m)
local AWAY = 0.25                        -- how far from the neck one taken out goes before it can go in again (m)
local SOUND_DIR = "CyberpunkVRPort_Menu/sounds/"

local S = { entries = {}, inserted = nil, wearing = false, error = nil, out = nil }
local Creation = nil

---------------------------------------------------------------- the playthroughs

local function fields(line)
  local f = {}
  for field in (line .. "\t"):gmatch("([^\t]*)\t") do f[#f + 1] = field end
  return f
end

-- Each playthrough's look, newest first.
function M.looks()
  local out = {}
  local ok, text = pcall(function() return type(VRLobbySaveLooks) == "function" and VRLobbySaveLooks() or "" end)
  if not ok or text == nil then return out end
  local cur = nil
  for line in text:gmatch("[^\n]+") do
    local f = fields(line)
    if f[1] == "look" then
      cur = { playthrough = f[2], folder = f[3], life = f[4], body = f[5], brain = f[6], level = tonumber(f[7]) or 0,
              time = tonumber(f[8]) or 0, apps = {}, morphs = {}, wear = {}, outfit = {} }
      out[#out + 1] = cur
    elseif cur ~= nil and f[1] == "app" then
      cur.apps[#cur.apps + 1] = { group = tonumber(f[2]), option = f[3], appearance = f[4] }
    elseif cur ~= nil and f[1] == "morph" then
      cur.morphs[#cur.morphs + 1] = { group = tonumber(f[2]), region = f[3], morph = f[4] }
    elseif cur ~= nil and f[1] == "wear" then
      cur.wear[#cur.wear + 1] = { area = f[2], item = f[3] }
    elseif cur ~= nil and f[1] == "outfit" then
      cur.outfit[#cur.outfit + 1] = { slot = f[2], item = f[3] }
    end
  end
  table.sort(out, function(a, b) return a.time > b.time end)
  return out
end

---------------------------------------------------------------- on the table

-- The scene's things without its shards, and an engram for each playthrough where the shards lay.
function M.items(list)
  local out, shards = {}, {}
  for _, e in ipairs(list or {}) do
    if tostring(e.name):find(SHARD) then shards[#shards + 1] = e else out[#out + 1] = e end
  end
  S.entries = {}
  local first = shards[1]
  if first == nil then return out end
  local p0 = first.position
  local step = shards[2] and V.sub(shards[2].position, p0) or { 0.0, 1.0, 0.0 }
  step[3] = 0
  step = V.mul(step, SPACING / math.max(V.len(step), 1e-6))
  -- A row behind: the step turned a quarter, away from the table's front edge (the side the player stands at).
  local back = { -step[2], step[1], 0 }
  if V.dot(back, { p0[1], p0[2], 0 }) < 0 then back = V.mul(back, -1) end
  for i, look in ipairs(M.looks()) do
    local col, row = (i - 1) % PER_ROW, math.floor((i - 1) / PER_ROW)
    local p = V.add(V.add(p0, V.mul(step, col)), V.mul(back, row))
    local entry = { name = "engram " .. i, ent = string.format(ENT, LOOKS[(i - 1) % #LOOKS + 1]), position = p,
                    rotation = first.rotation, radius = RADIUS, mass = MASS, shape = "box", half = SHARD_HALF,
                    look = look }
    out[#out + 1] = entry
    S.entries[#S.entries + 1] = entry
  end
  return out
end

---------------------------------------------------------------- the neck

-- The neck in the world: the eyes (the camera in the player's model space, through the player's transform), lower
-- and back along the gaze (level), under the ears.
local function neck()
  local pl = Game.GetPlayer()
  if pl == nil or type(VRCamModelPos) ~= "function" then return nil end
  local c = VRCamModelPos()
  if c == nil or c.w < 0.5 then return nil end
  local head = V.add(V.v3(pl:GetWorldPosition()), V.qrot(V.quat(pl:GetWorldOrientation()), { c.x, c.y, c.z }))
  local back = { 0, 0, 0 }
  pcall(function()
    local f = Game.GetCameraSystem():GetActiveCameraForward()
    local l = math.sqrt(f.x * f.x + f.y * f.y)
    if l > 1e-3 then back = { -f.x / l * NECK_BACK, -f.y / l * NECK_BACK, 0 } end
  end)
  return { head[1] + back[1], head[2] + back[2], head[3] - NECK_DROP }
end

local function feedback(hand)
  if type(VRMenuPlayWav) == "function" then pcall(VRMenuPlayWav, SOUND_DIR .. "open.wav", 0.9) end
  if type(VRHapticEvent) == "function" then pcall(VRHapticEvent, 20, hand == 2 and 2 or 1) end
end

-- The engram's look on the player (Creation.wear), in its V's body (a woman's on the man's the main menu spawned too:
-- modules/construct.lua): under the eyes shut on the way in the construct is up (the V of an engram in before goes
-- there), the game takes the puppet for that body and the save's clothes go on, over the clothes of the one before
-- (the construct's shoes off then if the V has its own: they go as the V's come); the look goes on, all of it out of
-- sight under the construct, and the V is shown at once when it is all on (Construct.reveal), under the eyes shut on
-- the way out. The look not going on, the shard is back on the table. False when the new game's screens are busy.
local function wear(entry)
  local male = entry.look.body ~= "Female"
  local shoes = false
  local ok = Creation.wear(entry.look, {
    stage = function()
      Construct.stage()
      pcall(Construct.resolveAs, male)
      local ok, feet = pcall(Clothes.dress, entry.look)
      shoes = ok and feet
      if shoes then pcall(Construct.hideShoes) end
    end,
    ready = function() Construct.reveal(male, shoes) end,
    done = function(done)
      S.wearing = false
      if done then return end
      S.error = "the look did not go on"
      pcall(Construct.unstage)
      Clothes.undress()
      if S.inserted == entry then
        S.inserted = nil
        Items.restore(entry)
      end
    end,
  })
  if not ok then return false end
  local previous = S.inserted
  S.inserted, S.wearing, S.error = entry, true, nil
  if previous ~= nil then Items.restore(previous) end
  return true
end

-- The shard gone into the neck: out of the hand that brought it, or (no hand: a test) off the table.
local function takeIn(hand, entry)
  if hand == nil then return Items.takeEntry(entry) end
  Items.take(hand)
  feedback(hand)
end

local function insert(hand, entry)
  if Creation == nil or Creation.busy() or Creation.isOpen() then return end
  if not wear(entry) then
    S.error = "busy"
    return
  end
  takeIn(hand, entry)
end

-- The engram in the neck taken out into the hand that closed there: the hologram is back, and the main menu's suit
-- back on (the man's: the puppet is taken for the man it was spawned as again first).
local function takeOut(hand)
  local entry = S.inserted
  if entry == nil or S.wearing then return end
  if not Items.give(entry, hand) then return end
  S.inserted, S.out = nil, entry
  Construct.ownBody(false)
  Clothes.undress()
  feedback(hand)
end

---------------------------------------------------------------- the lobby's calls

function M.init(creation) Creation = creation end

function M.update(dt)
  if #S.entries == 0 then return end
  local n = neck()
  if n == nil then return end
  for hand = 1, 2 do
    local entry, p = Items.held(hand)
    if entry ~= nil and entry.look ~= nil then
      local d = V.len(V.sub(p, n))
      -- The one just taken out goes in again only once the hand has been away from the neck. The hand, not the
      -- shard: a shard just spawned into the hand reports a stale place for its first frames, far from the neck,
      -- and went straight back in.
      if entry == S.out then
        local H = Hands.hands[hand]
        if H.pos ~= nil and V.len(V.sub(H.pos, n)) > AWAY then S.out = nil end
      elseif d < INSERT then
        insert(hand, entry)
      end
    elseif S.inserted ~= nil and Hands.taking(hand) then
      local H = Hands.hands[hand]
      if V.len(V.sub(H.pos, n)) < TAKE_OUT then takeOut(hand) end
    end
  end
end

-- The engram in the neck: its playthrough's newest save (folder, life path, body, level).
function M.inserted() return S.inserted and S.inserted.look or nil end

-- For tests: engram i put in as a hand at the neck puts it, taken off the table.
function M.insert(i)
  local entry = S.entries[i]
  if entry == nil then return false end
  insert(nil, entry)
  return true
end

-- For tests: the engram in the neck taken out as a hand closing there takes it, and back on the table.
function M.takeOut()
  local entry = S.inserted
  if entry == nil or S.wearing then return false end
  S.inserted = nil
  Construct.ownBody(false)
  Clothes.undress()
  Items.restore(entry)
  return true
end

function M.close()
  pcall(Construct.resolveAs, nil)
  Clothes.undress()
  S.entries, S.inserted, S.wearing, S.out = {}, nil, false, nil
end

function M.state()
  local n = neck()
  return { engrams = #S.entries, inserted = S.inserted and S.inserted.look.folder or false, wearing = S.wearing,
           out = S.out and S.out.look.folder or false,
           error = S.error, clothes = Clothes.state(), neck = n and string.format("%.2f %.2f %.2f", n[1], n[2], n[3]) or false }
end

return M
