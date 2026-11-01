-- The construct: while the lobby is up the player is a gold hologram, a construct waiting for a body.
--
-- The body meshes the main menu's puppet wears carry one more appearance, the construct (the plugin patches it in from
-- mods/archive/cyberpunkvrport.archive.xl, tools/vr_lobby_assets/make_construct.py). Every component holding one of
-- those meshes is switched to it (Game.VRComponentChangeAppearance). The arms cannot carry a patched
-- appearance (their meshes preload their materials), so the construct has its own copies of them: their components
-- are pointed at the copy (Game.VRComponentChangeResource). Every other skinned mesh the puppet shows -- clothes, their
-- shadow meshes, cyberware -- is switched off, and so is the arms' personal link (its plug in the left palm: the
-- copy's chunk mask leaves it out). All of it is held (the game may put clothes back on) and undone when the lobby
-- ends. The port's own components (vrp_*) are left alone, all but the wrist screen. The clothes are switched off, not
-- taken off: the body takes its shape from them (modules/clothes.lua puts them back after an engram's).
--
-- AN ENGRAM'S BODY (modules/engrams.lua) GOES ON AT ONCE. While the character creation system puts the engram's look
-- on the puppet -- piece by piece, over seconds -- and its clothes come in one by one, all of it is held out of sight
-- as it comes (M.stage: every frame, whatever skinned mesh shows that is not the construct's is switched off, and a
-- construct mesh the system gives a look of its own is given the construct's back), the construct still up; once it
-- is all on, it is shown in one frame (M.reveal) and the construct is off. Taken out (M.ownBody false), what the
-- engrams brought goes off, what their bodies covered of the construct comes back, and the construct is on again.
-- What the engrams brought is every component that came while one went on, told apart by its address
-- (Game.VRComponentKey): names repeat, and the system's copies of a component carry the puppet's own one's id.
--
-- AN ENGRAM OF THE OTHER BODY. The puppet is the body the main menu spawned (a man's, mods/tweaks/vrport/
-- VRPort.LobbyBody.yaml) and a woman's V goes on it as a woman: the game takes the puppet for her body while the engram
-- is in (M.resolveAs: the items given to it come in that body's variant), the system puts that body's meshes on it
-- and the meshes the puppet was spawned with are switched off (M.dressBody). The bones stay the spawned body's.
local M = {}

local LIST = require("modules/construct_list")

-- The port's wrist screen (mods/cet/CyberpunkVRPort_WristHud) is not a construct's: off while the lobby is up, and
-- left off -- a toggle on that mesh loses its screen's picture for good, and the load out of the lobby builds the
-- player anew.
local WRIST = "vrp_latd_screen"
-- Character.Player_Puppet_Base's gender entities, a man's and a woman's: the one marked initial is the body the game
-- takes the main menu's puppet for (GetResolvedGenderName reads the flag as it is asked).
local GENDERS = { [true] = "Character.Player_Puppet_Base_inline1", [false] = "Character.Player_Puppet_Base_inline0" }
-- An engram out, how long the puppet is held every frame: the main menu's clothes come back on it then (s).
local SETTLE = 2.0

-- Per component name: what it wore before ({ app, entry, mask }), or that it was switched off; the gender entities'
-- own flags. By component key: those the engrams brought (engram, kept for the lobby: the system takes some of them
-- up again), the construct's an engram's body switched off (covered), the puppet's before the look goes on and whether
-- each was on (before), those held out of sight while it goes on (staged), and the look the system gave a construct
-- mesh meanwhile (restyled).
local S = { worn = {}, hidden = {}, clock = 0, settle = 0, enabled = true, ownBody = false, initial = nil,
            engram = {}, covered = {}, staging = false, before = {}, staged = {}, restyled = {} }

local function resourceOf(c)
  local h = nil
  pcall(function() h = c.mesh.hash end)
  if h == nil then pcall(function() h = c.morphResource.hash end) end
  if h == nil then return nil end
  return (tostring(h):gsub("ULL$", ""))
end

local function keyOf(c) return tostring(Game.VRComponentKey(c)) end

-- The puppet's skinned meshes: the ones the construct is made of and covers.
local function meshes(puppet) return Game.VREntitySkinnedMeshes(puppet) end

local function toConstruct(c, entry)
  if entry.own ~= nil then
    -- One load: the appearance and the chunks shown are set first, then the resource they are loaded from.
    if entry.mask ~= nil then c.chunkMask = entry.mask end
    c.meshAppearance = CName.new(LIST.appearance)
    Game.VRComponentChangeResource(c, entry.own, false)
  elseif c.meshAppearance.value ~= LIST.appearance then
    Game.VRComponentChangeAppearance(c, CName.new(LIST.appearance), false)
  end
end

local function apply(puppet)
  for _, c in ipairs(meshes(puppet)) do
    local name = c:GetName().value
    if name == WRIST then
      if c:IsEnabled() then c:Toggle(false) end
    elseif string.sub(name, 1, 4) ~= "vrp_" then
      local key = resourceOf(c) or ""
      local entry = LIST.worn[key]
      if entry ~= nil and not S.engram[keyOf(c)] then
        if S.worn[name] == nil then S.worn[name] = { app = c.meshAppearance.value, entry = entry, mask = c.chunkMask } end
        toConstruct(c, entry)
      elseif LIST.own[key] == nil and c.meshAppearance.value ~= LIST.appearance and c:IsEnabled() then
        S.hidden[name] = true
        c:Toggle(false)
      end
    end
  end
end

-- What has come on the puppet while an engram's look goes on, switched off as it comes, and the construct kept as it
-- was: its meshes the construct's, and the puppet's own the system switches on (a second pair of arms) off again. The
-- puppet's skinned meshes looked through every frame (~0.2 ms).
local function holdNew(puppet)
  for _, c in ipairs(meshes(puppet)) do
    local key = keyOf(c)
    local wasOn = S.before[key]
    if wasOn == nil then S.engram[key] = true end
    local name = c:IsEnabled() and c:GetName().value or nil
    if name == nil or string.sub(name, 1, 4) == "vrp_" then
      -- off, or the port's own
    elseif S.engram[key] then
      S.staged[key] = true
      c:Toggle(false)
    elseif c.meshAppearance.value ~= LIST.appearance then
      local was = S.worn[name]
      if was ~= nil then
        S.restyled[key] = c.meshAppearance.value
        toConstruct(c, was.entry)
      else
        S.staged[key] = true
        c:Toggle(false)
      end
    elseif not wasOn then
      c:Toggle(false)
    end
  end
end

-- Held while the lobby is up: once a second is soon enough for anything the game puts back on, but for the clothes
-- that come back as an engram is out.
function M.update(dt)
  if S.staging then
    local puppet = Game.GetPlayer()
    if puppet ~= nil then holdNew(puppet) end
    return
  end
  -- The puppet's own body: what the system puts on it (its own body parts, the nails, the cyberware) stays; the
  -- clothes taken off before stay off.
  if not S.enabled or S.ownBody then return end
  S.clock, S.settle = S.clock - (dt or 0), S.settle - (dt or 0)
  if S.clock > 0 and S.settle <= 0 then return end
  S.clock = 1.0
  local puppet = Game.GetPlayer()
  if puppet ~= nil then apply(puppet) end
end

-- The body back as it was (and, unless only the body is wanted, the clothes back on). A look put on since (the
-- system's, an engram's) is kept. The engrams' components are left as they are.
local function undo(bodyOnly)
  local puppet = Game.GetPlayer()
  if puppet ~= nil then
    pcall(function()
      for _, c in ipairs(meshes(puppet)) do
        local name = c:GetName().value
        local key = keyOf(c)
        local was = S.worn[name]
        if not S.engram[key] then
          if was ~= nil then
            local now = c.meshAppearance.value
            local app = S.restyled[key] or (now ~= LIST.appearance and now or was.app)
            if was.entry.own ~= nil then
              if was.entry.mask ~= nil then c.chunkMask = was.mask end
              c.meshAppearance = CName.new(app)
              Game.VRComponentChangeResource(c, was.entry.path, false)
            elseif now ~= app then
              Game.VRComponentChangeAppearance(c, CName.new(app), false)
            end
          end
          if S.hidden[name] and not bodyOnly then c:Toggle(true) end
        end
      end
    end)
  end
  S.worn, S.clock, S.restyled = {}, 0, {}
  if not bodyOnly then S.hidden = {} end
end

-- The game takes the puppet for a man's body (true) or a woman's (false); nil: the one it was spawned as again.
function M.resolveAs(male)
  if male == nil and S.initial == nil then return end
  if S.initial == nil then
    S.initial = {}
    for g, record in pairs(GENDERS) do S.initial[g] = TweakDB:GetFlat(record .. ".initial") == true end
  end
  for g, record in pairs(GENDERS) do
    local on = S.initial[g]
    if male ~= nil then on = g == male end
    TweakDB:SetFlat(record .. ".initial", on)
  end
end

-- What the engrams brought off the puppet and what their bodies covered of the construct back on.
local function unwear(puppet)
  for _, c in ipairs(meshes(puppet)) do
    local key = keyOf(c)
    if S.engram[key] then
      if c:IsEnabled() then c:Toggle(false) end
    elseif S.covered[key] then
      c:Toggle(true)
      S.hidden[c:GetName().value] = nil
    end
  end
  S.covered, S.staged, S.staging, S.restyled = {}, {}, false, {}
end

function M.restore()
  S.ownBody, S.staging = false, false
  pcall(M.resolveAs, nil)
  S.covered, S.staged, S.before, S.restyled = {}, {}, {}, {}
  if next(S.worn) ~= nil or next(S.hidden) ~= nil then undo(false) end
  S.engram = {}
end

-- The parts of a head the system puts on a puppet (the head, its hair, beard, teeth, eyes, make-up, scars,
-- earrings): the game shows none of them to the player in the first person.
local function headPart(name, app)
  if name:find("^h%l*_") or name:find("^beard") or name:find("^i1_") then return true end
  return name:find("^MorphTargetSkinnedMesh") ~= nil and not app:find("personal_link") and not app:find("nails")
end

-- What the lobby's puppet wears under its clothes (switched on again with its own body).
local function underwear(name) return name:find("boxers") or name:find("underwear") or name:find("panties") end

-- A mesh of a man's body (true) or a woman's (false), by the name the game gives it.
local function ofBody(name, male)
  if male then return name:find("pma") ~= nil or name:find("_ma_") ~= nil end
  return name:find("pwa") ~= nil or name:find("_wa_") ~= nil
end

-- The body the puppet was spawned with: a man's if its own torso is (the spawned one is a garment component, the
-- system's are morph target ones); nil if it has none.
local function spawnedMale(puppet)
  for _, c in ipairs(meshes(puppet)) do
    if c:GetClassName().value == "entGarmentSkinnedMeshComponent" then
      local name = c:GetName().value
      if name == "t0_000_pma_base__full" then return true end
      if name == "t0_000_pwa_base__full" then return false end
    end
  end
  return nil
end

-- A component switched off for the engram's V: one of the construct's comes back when the engram is out.
local function cover(c)
  S.hidden[c:GetName().value] = true
  local key = keyOf(c)
  if not S.engram[key] and LIST.worn[resourceOf(c) or ""] ~= nil then S.covered[key] = true end
  c:Toggle(false)
end

-- The character creation system dresses the puppet in components of its own: the body meshes the puppet was
-- spawned with stay under them, in their old look, and show through. Of the components holding the same mesh and
-- showing the same chunks of it the one added last is kept (the system's torso is two components, each showing chunks
-- of it); the others are switched off. The head it puts on goes too, the neck the puppet was spawned with takes the
-- skin of the system's body (and so does the system's own patch between the legs: it is left in the skin the system
-- started with now and then), and the underwear is back on. `male`: the body the system put on, a man's or a woman's;
-- the other one than the puppet was spawned with, the spawned body's meshes go (its neck stays: the system's body has
-- no first person neck), its underwear and shoes with them.
function M.dressBody(male)
  local puppet = Game.GetPlayer()
  if puppet == nil then return 0 end
  local own = spawnedMale(puppet)
  local cross = male ~= nil and own ~= nil and male ~= own
  local last, all = {}, {}
  for _, c in ipairs(meshes(puppet)) do
    local key = c:IsEnabled() and resourceOf(c) or nil
    if key ~= nil and key ~= "0" then
      key = key .. ":" .. tostring(c.chunkMask)
      all[#all + 1] = { c = c, key = key }
      last[key] = c
    end
  end
  local n, skin = 0, nil
  for _, e in ipairs(all) do
    local name, app = e.c:GetName().value, e.c.meshAppearance.value
    if last[e.key] ~= e.c or headPart(name, app) or (cross and ofBody(name, own) and not name:find("^n0_")) then
      cover(e.c)
      n = n + 1
    elseif name:find("^t0_000_p%la_base__full$") then
      skin = app
    end
  end
  for _, c in ipairs(meshes(puppet)) do
    local name = c:GetName().value
    local skinned = name:find("^n0_") or name:find("^i0_000_p%la_base__genitals_none")
    if skin ~= nil and skinned and c.meshAppearance.value ~= skin then Game.VRComponentChangeAppearance(c, CName.new(skin), false) end
    if underwear(name) and not c:IsEnabled() and not (cross and ofBody(name, own)) then c:Toggle(true) end
  end
  return n
end

-- The lobby's puppet stands in the main menu's shoes: off while the engram's V has shoes of its own.
function M.hideShoes()
  local puppet = Game.GetPlayer()
  if puppet == nil then return end
  for _, c in ipairs(meshes(puppet)) do
    if c:IsEnabled() and c:GetName().value:find("^s1_") and not S.engram[keyOf(c)] then cover(c) end
  end
end

-- The puppet's own body (an engram's V on it) or the construct's, in one frame.
function M.ownBody(on)
  if S.ownBody == on then return end
  S.ownBody = on
  if on then
    undo(true)
  else
    pcall(M.resolveAs, nil)
    local puppet = Game.GetPlayer()
    if puppet ~= nil then
      unwear(puppet)
      if S.enabled then apply(puppet) end
    end
    S.clock, S.settle = 1.0, SETTLE
  end
end

-- An engram's look about to go on: the construct up (an engram's V in before goes), and whatever comes on the puppet
-- from now held out of sight until M.reveal.
function M.stage()
  local puppet = Game.GetPlayer()
  if puppet == nil then return end
  M.ownBody(false)
  if S.enabled then apply(puppet) end
  S.before, S.staged, S.restyled = {}, {}, {}
  local engram = {}
  for _, c in ipairs(meshes(puppet)) do
    local key = keyOf(c)
    S.before[key] = c:IsEnabled()
    engram[key] = S.engram[key]
  end
  -- Those gone off the puppet forgotten.
  S.engram = engram
  S.staging = true
end

-- The engram's look is on: the V shown at once -- the construct off, what was held out of sight on, and the body as
-- M.dressBody leaves it (`shoes`: the V has shoes of its own). Under the shut eyes of the way back to the main menu.
function M.reveal(male, shoes)
  local puppet = Game.GetPlayer()
  if puppet == nil then return end
  holdNew(puppet)
  S.staging = false
  M.ownBody(true)
  for _, c in ipairs(meshes(puppet)) do
    if S.staged[keyOf(c)] then c:Toggle(true) end
  end
  S.staged = {}
  M.dressBody(male)
  if shoes then M.hideShoes() end
end

-- The look did not go on: what came of it off, the construct up.
function M.unstage()
  S.staging = false
  S.ownBody = true
  M.ownBody(false)
end

-- The construct on or off (off puts the puppet's own body and clothes back).
function M.enable(on)
  S.enabled = on
  if not on then M.restore() end
end

function M.state()
  local worn, n = {}, 0
  for name, was in pairs(S.worn) do worn[name] = was.app end
  for _ in pairs(S.engram) do n = n + 1 end
  return { enabled = S.enabled, worn = worn, hidden = S.hidden, ownBody = S.ownBody, staging = S.staging, engram = n }
end

return M
