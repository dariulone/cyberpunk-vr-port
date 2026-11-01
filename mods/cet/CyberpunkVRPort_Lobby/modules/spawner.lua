-- Everything the lobby puts into the world -- the place, the terminal's screen, the things on the table -- is spawned
-- through the plugin's spawn system (Game.VRSpawn*, src/Natives/SpawnSystem.cpp) under the lobby's tag and one tag of
-- its own kind. Nothing of it goes into a save, and nothing of it streams out with distance.
--
-- The system owns the entities, not the ids a module remembers: one asked away while still loading is still deleted
-- (the legacy spawner can only despawn an entity it can already find, so a respawn in the first frames left the
-- previous one standing), and whatever an earlier instance of the mod left in the world -- a hot reload, an error on
-- the way out -- is found by the tag and removed.
local M = {}

M.TAG = "VRLobby"

-- path: the entity template; tr: a WorldTransform; kind: the tag of what it is (with the lobby's own).
-- Nil when the spawn could not be asked for (the error is kept: M.lastError).
function M.spawn(path, tr, kind)
  local id = nil
  local ok, err = pcall(function()
    id = Game.VRSpawnTemplate(path, CName.new(""), WorldPosition.ToVector4(tr.Position), tr:GetOrientation(),
      { CName.new(M.TAG), CName.new(kind) })
    if id.hash == 0 then id = nil end
  end)
  if not ok then M.lastError = tostring(err) end
  return id
end

function M.delete(id)
  if id == nil then return end
  pcall(function() Game.VRSpawnDelete(id) end)
end

-- Everything of one kind, or everything the lobby has spawned.
function M.clear(kind)
  pcall(function() Game.VRSpawnDeleteTagged(CName.new(kind or M.TAG)) end)
end

-- Spawned: in the world, its components attached (the colliders among them).
function M.spawned(id)
  if id == nil then return false end
  local ok, entity = pcall(function() return Game.VRSpawnedEntity(id) end)
  return ok and entity ~= nil
end

-- Still the system's: spawned, or on its way.
function M.alive(id)
  if id == nil then return false end
  local ok, known = pcall(function() return Game.VRSpawnManaged(id) end)
  return ok and known == true
end

function M.count(kind)
  local n = 0
  pcall(function() n = #Game.VRSpawnTaggedIDs(CName.new(kind or M.TAG)) end)
  return n
end

return M
