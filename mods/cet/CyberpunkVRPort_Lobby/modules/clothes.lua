-- What an engram's V wears, on the player while that engram is in the neck (modules/engrams.lua): the clothes of
-- the playthrough's newest save, every piece it shows -- the game's own items, given to the lobby's puppet and put
-- in their slots (the transaction system, as the game dresses the player), taken off and away again when the
-- engram comes out or the lobby ends, and what those slots held put back: the main menu's suit, which the construct
-- switches off but keeps on (modules/construct.lua) -- the body takes its shape from it, and without it the
-- hologram's belly faces the eyes and its edge-lit look shows nothing of it. Another engram's look goes on over the
-- one before: the suit is not put back between the two (it would come on the puppet while the next V goes on).
--
-- The save says what (VRLobbySaveLooks: a wear line per clothing area that shows an item -- hidden ones and the
-- wardrobe's replacements already worked out -- or, while the EquipmentEx mod's outfit is on, an outfit line per
-- part with the slot it goes in). An item is put on as its record: the clothes' looks come from it (the rng seed
-- is the stats'), and the body's own variant of the item (the puppet's man's) is the game's choice.
local M = {}

-- The slot each clothing area's item goes in (the equipment system's clothing slots).
local SLOTS = { Head = "AttachmentSlots.Head", Face = "AttachmentSlots.Eyes", OuterChest = "AttachmentSlots.Torso",
                InnerChest = "AttachmentSlots.Chest", Legs = "AttachmentSlots.Legs", Feet = "AttachmentSlots.Feet",
                Outfit = "AttachmentSlots.Outfit", UnderwearTop = "AttachmentSlots.UnderwearTop",
                UnderwearBottom = "AttachmentSlots.UnderwearBottom" }
local FEET, OUTFIT = "AttachmentSlots.Feet", "AttachmentSlots.Outfit"
-- The look an outfit piece (the Outfit slot) gives every other clothing piece: none. Taken off, it does not give them
-- theirs back.
local EMPTY = "empty_appearance_default"

-- What is worn of the look; what the slots held before it.
local S = { worn = {}, before = {}, error = nil }

-- A TweakDBID from its 16 hex digits (the length byte, then the hash).
local function tweak(hex)
  if hex == nil or #hex ~= 16 then return nil end
  local len, hash = tonumber(hex:sub(7, 8), 16), tonumber(hex:sub(9, 16), 16)
  if len == nil or hash == nil then return nil end
  return TweakDBID.new(hash, len)
end

-- The pieces a look shows: { slot, item } per piece.
local function pieces(look)
  local out = {}
  if look.outfit ~= nil and #look.outfit > 0 then
    for _, o in ipairs(look.outfit) do out[#out + 1] = { slot = tweak(o.slot), item = tweak(o.item), feet = false } end
  else
    for _, w in ipairs(look.wear or {}) do
      local slot = SLOTS[w.area]
      if slot ~= nil then
        out[#out + 1] = { slot = TweakDBID.new(slot), item = tweak(w.item), feet = slot == FEET or slot == OUTFIT }
      end
    end
  end
  return out
end

-- The look's clothes off the player, what the slots held before them not put back.
local function takeOff(player, ts)
  for _, w in ipairs(S.worn) do
    pcall(function()
      ts:RemoveItemFromSlot(player, w.slot, true)
      ts:RemoveItem(player, w.item, 1)
    end)
  end
  S.worn = {}
end

-- The look's clothes on the player (over the look before, if any); whether it has shoes of its own (an outfit piece
-- has).
function M.dress(look)
  local player, ts = Game.GetPlayer(), Game.GetTransactionSystem()
  if player == nil or ts == nil then return false end
  takeOff(player, ts)
  local feet = false
  for _, p in ipairs(pieces(look)) do
    if p.slot ~= nil and p.item ~= nil then
      local ok, err = pcall(function()
        if TweakDB:GetRecord(p.item) == nil then return end
        local id = ItemID.FromTDBID(p.item)
        local key = tostring(p.slot)
        local held = ts:GetItemInSlot(player, p.slot)
        if held ~= nil and S.before[key] == nil then S.before[key] = { slot = p.slot, item = held:GetItemID() } end
        ts:GiveItem(player, id, 1)
        ts:AddItemToSlot(player, p.slot, id, true)
        S.worn[#S.worn + 1] = { slot = p.slot, item = id }
        feet = feet or p.feet
      end)
      if not ok then S.error = tostring(err) end
    end
  end
  return feet
end

function M.undress()
  local player, ts = Game.GetPlayer(), Game.GetTransactionSystem()
  if player ~= nil and ts ~= nil then
    takeOff(player, ts)
    for _, b in pairs(S.before) do pcall(function() ts:AddItemToSlot(player, b.slot, b.item, true) end) end
    -- The pieces an outfit piece left without a look get theirs again.
    for _, slot in pairs(SLOTS) do
      pcall(function()
        local held = ts:GetItemInSlot(player, TweakDBID.new(slot))
        if held == nil then return end
        local id = held:GetItemID()
        if ts:GetItemAppearance(player, id).value == EMPTY then ts:ResetItemAppearance(player, id) end
      end)
    end
  end
  S.worn, S.before = {}, {}
end

function M.state() return { worn = #S.worn, error = S.error } end

return M
