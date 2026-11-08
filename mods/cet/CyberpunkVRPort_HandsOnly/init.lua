-- CyberpunkVRPort_HandsOnly -- the player's body drawn as its hands alone (xr_body_hands_only = 1; the plugin
-- publishes it to shared slot [173]).
--
-- WHAT GOES. Every mesh component of the player is switched off -- the body, the head the first person never shows,
-- every garment, the shadow proxies -- except:
--   the ARMS, drawn as their HANDS. Each of the player's arm meshes (both bodies', every arm cyberware's) has a copy
--     cut at the wrists and closed there (tools/hands_only/make_hand_meshes.py -> vrport\body\hands, a vrp_hands__*
--     component of the player entities, off by default; data/hand_meshes.lua). The game's arm component stays the
--     game's -- enabled, disabled and masked as the game decides -- but is masked to nothing, and its copy follows it:
--     on while it is on, in its skin tone, with the game's mask ANDed with the copy's own (the game draws the
--     holstered Gorilla Arms as two components of one mesh with complementary masks: both feed the one copy).
--     Arm pieces without a copy (decals, nails, the cyberware's hands and weapons) keep the chunks data/hand_masks.lua
--     (make_hand_masks.py) says are hand, ANDed into the game's mask. An arm mesh neither table knows (a replacer's) is
--     kept whole rather than losing the hands with it;
--   the port's own pieces (`vrp_*`: the tactical belt, the wrist screen, the glow and the grip markers, the cigarette)
--     but the neck cap and the body stand-in, which belong to the body (NeckCap.cpp and ForceFPP keep them off in this
--     mode themselves); the belt's game items (`ucbelt*`); the palm's Smart Link grip; gloves. The wrist screen's mesh
--     must stay on (switched off, its readout's target dies for good), so CyberpunkVRPort_WristHud empties the readout
--     in this mode instead: it sat on the forearm and hung in the air below the hand.
-- The weapons on the body (CyberpunkVRPort_Holster) and in the hands are entities of their own and stay.
--
-- When the mode ends every mask is the game's again, every component switched off here (only one that was on when it
-- went) on again, every copy off. The game turns garments back on when the wardrobe changes; the next pass, a quarter
-- of a second later, turns them off again.
--
-- Nothing is touched once the player detaches: a component of the session that ended is a handle the collector
-- releases later.
--
-- A SCENE GETS THE BODY BACK. While a scene plays the body (the scene tier that suspends VRIK, VRSceneOwnsBody) the
-- mode yields: the scene's authored body is what is meant to be seen, not two hands where its arms are
-- (2026-11-05: "in cutscenes with hands only the body should appear smoothly but quickly"). It comes and goes with the
-- engine's dissolve (VRComponentDissolve, Rendering/DissolveTime 0.5 s), checked every frame, not every scan:
--   in:  every piece the mode had off or masked comes back dissolved out (the dissolve set while it is still off: the
--        proxy it is given keeps it, no frame of it is seen) and fades in once it is attached (VRComponentDissolveReady:
--        asked earlier, the fade only cleared the bit and the piece came in at once); the hand copies stay until the
--        arms have faded in under them;
--   out: every piece the mode will switch off or mask fades out first (the copies already on, so the hands never
--        go), and only then is switched off. Its dissolve stays set while it is off and is cleared when it comes back:
--        cleared while the piece was still attached, it was drawn again for the frames before it went (measured
--        2026-11-05: the bare torso for three frames after the fade).
-- A plain change of the setting fades the same way. Without the natives (an older plugin) the switch is at once.
local MASKS = require("data/hand_masks")
local COPIES = require("data/hand_meshes")

local SCAN_EVERY = 0.25
local COPY_PREFIX = "vrp_hands__"
local KEEP_PREFIX = { "vrp_", "ucbelt", "a0_004__weapon_grip", "g1_" }
-- never kept, whatever the tables say: the personal link's cable and plug hang from the forearm
local NEVER = { "personal_link" }
local DROP = { vrp_neck_cap = true, vrp_body_stand_in = true }

local FADE = 0.55        -- the engine's dissolve (0.45 s measured) and a margin
local SHOW_DELAY = 0.05  -- a piece switched on is given a frame or two before it is asked whether it is attached
local SHOW_LIMIT = 1.5   -- ...and faded in anyway after this long
-- the engine's dissolve modes (measured): 0 fades both ways; 2 fades out the same but comes back in at once
local DISSOLVE_NOW, DISSOLVE_FADE = 2, 0

local S = {
  t = 0, at = -1, on = false, force = nil, scene = nil,
  hidden = {},     -- component key -> true: switched off here
  masks = {},      -- component key -> { game = the game's mask, ours = the low bits written }
  copies = {},     -- copy name -> true: a copy this mode has switched on
  unknown = {},    -- arm meshes neither table knows, logged once
  pending = {},    -- { at = time, ready = function or nil, limit = time, run = function }: the second half of a fade
  revealing = 0,   -- pieces still to fade in (the copies wait for them)
  revealedAt = -1, -- when the last of them began to
  fading = {},     -- component key -> true: fading out, not to be touched by a pass until it is off
}

local CN = {}
local function cname(s)
  local c = CN[s]
  if c == nil then
    c = CName.new(s)
    CN[s] = c
  end
  return c
end

local function setting()
  if S.force ~= nil then return S.force end
  return type(GetVRSharedSlot) == "function" and (GetVRSharedSlot(173) or 0) > 0.5
end

local function askScene() return Game.VRSceneOwnsBody() end
local function sceneOwnsBody()
  if S.scene ~= nil then return S.scene end
  local ok, v = pcall(askScene) -- every frame while the mode is on: no closure made per call
  return ok and v == true
end

local function wanted()
  return setting() and not sceneOwnsBody()
end

-- The engine's dissolve on one piece: false where the plugin has no such native.
local function dissolve(c, hidden, mode)
  local ok, done = pcall(function() return Game.VRComponentDissolve(c, hidden, mode) end)
  return ok and done == true
end

-- The mode's dissolve cleared at once, where it is set.
local function undissolve(c)
  local ok, set = pcall(function() return Game.VRComponentDissolved(c) end)
  if ok and set == true then dissolve(c, false, DISSOLVE_NOW) end
end

-- Whether a dissolve reaches the piece's proxy now; true where the plugin cannot tell (it fades in late, or at once).
local function dissolveReady(c)
  local ok, ready = pcall(function() return Game.VRComponentDissolveReady(c) end)
  return not ok or ready == true
end

-- `run` after `delay` seconds -- and, with `ready`, once that says so (or `limit` seconds have passed).
local function later(delay, run, ready, limit)
  S.pending[#S.pending + 1] = { at = S.t + delay, run = run, ready = ready, limit = S.t + (limit or delay) }
end

-- Every second half that is due (all of them with `all`: a new switch finishes the last one first).
local function runPending(all)
  if #S.pending == 0 then return end
  local keep = {}
  local due = {}
  for _, p in ipairs(S.pending) do
    local ready = S.t >= p.at
    if ready and p.ready ~= nil and S.t < p.limit then
      local ok, r = pcall(p.ready)
      ready = not ok or r == true
    end
    if all or ready then due[#due + 1] = p else keep[#keep + 1] = p end
  end
  S.pending = keep
  for _, p in ipairs(due) do
    local ok, err = pcall(p.run)
    if not ok then print("[HandsOnly] " .. tostring(err)) end
  end
end

local function startsWith(s, p) return string.sub(s, 1, #p) == p end

local function kept(name)
  if DROP[name] then return false end
  for _, p in ipairs(KEEP_PREFIX) do
    if startsWith(name, p) then return true end
  end
  return string.find(name, "glove", 1, true) ~= nil
end

local function isMesh(c)
  return c:IsA(cname("entMeshComponent")) or c:IsA(cname("entISkinTargetComponent"))
end

-- The arms themselves, by the names the player entities give them: an arm whose mesh neither table knows (a body
-- replacer's) is kept whole. Any other unknown piece (a decal on the forearm, the personal link's plug) goes.
local ARM_NAMES = { "_hq__l", "_hq__r", "_hq__full", "_fpp__full", "left_arm", "right_arm", "strongarms_l",
  "strongarms_r", "strongarms_holstered_l", "strongarms_holstered_r", "mantisblade_left", "mantisblade_right",
  "launcher_holstered", "fists" }
local function armLike(name)
  for _, a in ipairs(ARM_NAMES) do
    if string.find(name, a, 1, true) ~= nil then return true end
  end
  return false
end

local function meshHash(c)
  local ok, h = pcall(function() return c.mesh.hash end)
  if not ok or h == nil then return nil end
  return (string.gsub(tostring(h), "U?LL$", ""))
end

-- The low `bits` bits of a chunk mask (a 64-bit integer the binding hands over as a cdata or a number), exactly.
local function lowBits(v, bits)
  local m = 2 ^ bits
  local s = string.gsub(tostring(v), "U?LL$", "")
  if not string.match(s, "^%d+$") then
    local n = tonumber(v) or 0
    return n % m
  end
  local r = 0
  for i = 1, #s do r = (r * 10 + (string.byte(s, i) - 48)) % m end
  return r
end

local function bit2(a, b, bits, both)
  local r, p = 0, 1
  for _ = 0, bits - 1 do
    local x, y = math.floor(a / p) % 2 == 1, math.floor(b / p) % 2 == 1
    if (both and x and y) or (not both and (x or y)) then r = r + p end
    p = p * 2
  end
  return r
end
local function band(a, b, bits) return bit2(a, b, bits, true) end
local function bor(a, b, bits) return bit2(a, b, bits, false) end

local function nudge(c)
  pcall(function()
    if c:IsEnabled() then
      c:Toggle(false)
      c:Toggle(true)
    end
  end)
end

local function enabled(c)
  local ok, en = pcall(function() return c:IsEnabled() end)
  return ok and en == true
end

-- The game's mask of a component this mode masks: the one it had before the mode wrote its own, or the one the game
-- has written since (a rebuilt or re-masked garment). `ours` is what the mode writes (low bits, a number).
local function gameMask(c, key, ours)
  local okM, cur = pcall(function() return c.chunkMask end)
  if not okM or cur == nil then return nil end
  local rec = S.masks[key]
  if rec ~= nil and lowBits(cur, 53) == rec.ours then return rec.game end
  S.masks[key] = { game = cur, ours = ours }
  return cur
end

local function writeMask(c, key, ours)
  local rec = S.masks[key]
  if rec == nil then return end
  rec.ours = ours
  local okM, cur = pcall(function() return c.chunkMask end)
  if okM and lowBits(cur, 53) == ours then return end
  pcall(function() c.chunkMask = ours end)
  nudge(c)
end

-- One arm piece without a copy: its hand chunks only.
local function cutArm(c, key, row)
  local game = gameMask(c, key, -1)
  if game == nil then return end
  writeMask(c, key, band(lowBits(game, 24), row[1], math.min(row[2], 24)))
end

-- `fade`: the mode is switching (see A SCENE GETS THE BODY BACK above), not one of the periodic passes.
local function walk(pl, on, fade)
  local seen, want = {}, {}
  for _, c in ipairs(pl:GetComponents()) do
    local okMesh, mesh = pcall(isMesh, c)
    if okMesh and mesh then
      local name = NameToString(c:GetName())
      local n = (seen[name] or 0) + 1
      seen[name] = n
      local key = name .. "#" .. n
      if on then
        if S.fading[key] then
          -- fading out: switched off when the fade ends
        elseif not kept(name) then
          local h = meshHash(c)
          local copy = h and COPIES[h]
          local row = h and MASKS[h]
          for _, x in ipairs(NEVER) do
            if string.find(name, x, 1, true) ~= nil then copy, row = nil, nil end
          end
          if copy ~= nil then
            -- the game's arm: drawn as nothing, its copy drawn in its place while the game has it on
            local game = gameMask(c, key, 0)
            if game ~= nil then
              if fade and enabled(c) and lowBits(game, 53) ~= 0 and dissolve(c, true, DISSOLVE_FADE) then
                -- the whole arm fades out over its copy, then is masked to nothing (dissolved still: see above)
                S.fading[key] = true
                later(FADE, function()
                  S.fading[key] = nil
                  writeMask(c, key, 0)
                end)
              else
                writeMask(c, key, 0)
              end
              local w = want[copy[1]] or { on = false, mask = 0, keep = copy[2], tone = nil }
              if enabled(c) then
                w.on = true
                w.mask = bor(w.mask, band(lowBits(game, 24), copy[2], 24), 24)
                pcall(function() w.tone = w.tone or c.meshAppearance end)
              end
              want[copy[1]] = w
            end
          elseif row ~= nil then
            cutArm(c, key, row)
          elseif armLike(name) then
            if not S.unknown[h or name] then
              S.unknown[h or name] = true
              print("[HandsOnly] arm mesh not in the tables, kept whole: " .. name .. " " .. tostring(h))
            end
          elseif enabled(c) then
            S.hidden[key] = true
            if fade and dissolve(c, true, DISSOLVE_FADE) then
              -- faded out, then switched off (dissolved still: see above)
              S.fading[key] = true
              later(FADE, function()
                S.fading[key] = nil
                pcall(function() c:Toggle(false) end)
              end)
            else
              pcall(function() c:Toggle(false) end)
            end
          end
        end
      else
        local rec = S.masks[key]
        local masked = rec ~= nil
        local hidden = S.hidden[key] == true
        -- dissolved out while it is off, so the proxy it is given on comes in unseen
        local veiled = fade and (masked or hidden) and dissolve(c, true, DISSOLVE_NOW)
        if masked then
          pcall(function() c.chunkMask = rec.game end)
          if veiled and enabled(c) then
            pcall(function() c:Toggle(false) end)
            dissolve(c, true, DISSOLVE_NOW)
            pcall(function() c:Toggle(true) end)
          else
            nudge(c)
          end
        end
        if hidden then pcall(function() c:Toggle(true) end) end
        if veiled then
          -- faded in once it is attached; one the game keeps off has only its dissolve cleared
          S.revealing = S.revealing + 1
          later(SHOW_DELAY, function()
            S.revealing = math.max(0, S.revealing - 1)
            S.revealedAt = S.t
            dissolve(c, false, enabled(c) and DISSOLVE_FADE or DISSOLVE_NOW)
          end, function() return not enabled(c) or dissolveReady(c) end, SHOW_LIMIT)
        elseif masked or hidden then
          undissolve(c)
        end
      end
    end
  end
  -- the copies: each as its game arm(s) want it, every other one off
  local names = {}
  for k in pairs(want) do names[k] = true end
  for k in pairs(S.copies) do names[k] = true end
  for copyName in pairs(names) do
    local w = on and want[copyName] or nil
    local comp = pl:FindComponentByName(cname(copyName))
    if comp ~= nil then
      if w ~= nil and w.on and w.mask ~= 0 then
        if w.tone ~= nil then
          local okA, cur = pcall(function() return comp.meshAppearance end)
          if okA and NameToString(cur) ~= NameToString(w.tone) then
            pcall(function() Game.VRComponentChangeAppearance(comp, w.tone, false) end)
          end
        end
        local okM, cur = pcall(function() return comp.chunkMask end)
        if okM and lowBits(cur, 53) ~= w.mask then
          pcall(function() comp.chunkMask = w.mask end)
          nudge(comp)
        end
        if not enabled(comp) then pcall(function() comp:Toggle(true) end) end
        S.copies[copyName] = true
      else
        if enabled(comp) then
          if fade and not on then
            -- the hands stay until the arms have faded in around them
            later(0, function() if enabled(comp) then pcall(function() comp:Toggle(false) end) end end,
              function() return S.revealing == 0 and S.t - S.revealedAt >= FADE end, SHOW_LIMIT + FADE)
          else
            pcall(function() comp:Toggle(false) end)
          end
        end
        S.copies[copyName] = nil
      end
    elseif w ~= nil and not S.unknown[copyName] then
      S.unknown[copyName] = true
      print("[HandsOnly] no " .. copyName .. " on this player entity (an entity of another mod?)")
    end
  end
  if not on then
    S.hidden, S.masks, S.copies = {}, {}, {}
  end
end

registerForEvent("onInit", function()
  pcall(function()
    ObserveBefore("PlayerPuppet", "OnDetach", function()
      -- the session's components go with it: forget them without touching them
      S.hidden, S.masks, S.copies, S.on = {}, {}, {}, false
      S.pending, S.fading, S.revealing = {}, {}, 0
    end)
  end)
end)

registerForEvent("onUpdate", function(dt)
  S.t = S.t + (dt or 0.016)
  runPending(false)
  -- the mode's state every frame (a scene starting is a frame's work, not a scan's); the passes every SCAN_EVERY,
  -- none while a fade is still to finish
  local want = wanted()
  local switch = want ~= S.on
  if not switch and (S.t - S.at < SCAN_EVERY or #S.pending > 0) then return end
  S.at = S.t
  if not want and not S.on then return end
  local pl = Game.GetPlayer()
  if pl == nil then return end
  if switch then runPending(true) end
  local ok, err = pcall(walk, pl, want, switch)
  if not ok then print("[HandsOnly] " .. tostring(err)) end
  if switch then print("[HandsOnly] " .. (want and "on" or "off") .. (sceneOwnsBody() and " (scene)" or "")) end
  S.on = want
end)

registerForEvent("onShutdown", function()
  runPending(true)
  if S.on then
    local pl = Game.GetPlayer()
    if pl ~= nil then pcall(walk, pl, false) end
  end
end)

return {
  -- GetMod("CyberpunkVRPort_HandsOnly").force(true | false | nil): override the setting (nil = follow it)
  force = function(v) S.force = v return wanted() end,
  -- GetMod("CyberpunkVRPort_HandsOnly").scene(true | false | nil): pretend a scene plays the body (nil = the game's)
  scene = function(v) S.scene = v return wanted() end,
  -- the tables read again (after tools/hands_only/make_hand_*.py), the arms re-cut on the next pass
  hot = function()
    local f1, e1 = loadfile("data/hand_masks.lua")
    local f2, e2 = loadfile("data/hand_meshes.lua")
    if not f1 or not f2 then return "hot reload failed: " .. tostring(e1 or e2) end
    local pl = Game.GetPlayer()
    if pl ~= nil and S.on then pcall(walk, pl, false) end
    MASKS, COPIES = f1(), f2()
    S.on = false
    return "ok"
  end,
  state = function()
    local h, m, c = 0, 0, 0
    for _ in pairs(S.hidden) do h = h + 1 end
    for _ in pairs(S.masks) do m = m + 1 end
    for _ in pairs(S.copies) do c = c + 1 end
    return { on = S.on, hidden = h, masked = m, copies = c }
  end,
}
