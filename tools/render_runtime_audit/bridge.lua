-- Attach-only fixture. No save, load, runtime switch, or game lifecycle calls.
RenderRuntimeAudit = RenderRuntimeAudit or {}
local A = RenderRuntimeAudit
local function xyz(p) return {p.x, p.y, p.z} end
function A.protection()
  local root = GoalSaveGuard
  local verified = root and root.verified == true and root.removed ~= true and type(root.name) == "string"
    and root.name:match("^cpvr_goal_root_[a-f0-9]+$") ~= nil
    and (root.verification == "causal-remove-readd" or root.verification == "observer-and-engine")
  local manager = Game.GetScriptableSystemsContainer():Get(CName.new("SaveLocksManager"))
  local locked = Game.IsSavingLocked()
  return {protected = verified == true and locked == true and manager:IsSavingLocked() == true,
    rootVerified = verified == true, rootName = verified and root.name or nil, engineLocked = locked == true}
end
function A.requireProtection()
  local state = A.protection()
  assert(state.protected, "a verified retained GoalSaveGuard and active native saving locks are required")
  if A.session then assert(A.session.rootName == state.rootName, "fixture's root save guard changed") end
  return state
end
function A.state()
  local p = assert(Game.GetPlayer(), "player not spawned")
  local vehicle = p:GetMountedVehicle()
  return {position = xyz(p:GetWorldPosition()), forward = xyz(p:GetWorldForward()),
    player = tostring(p:GetEntityID().hash), mounted = vehicle ~= nil,
    vehicle = vehicle and tostring(vehicle:GetEntityID().hash) or nil,
    vehicleRecord = vehicle and tostring(vehicle:GetRecordID()) or nil,
    vehiclePosition = vehicle and xyz(vehicle:GetWorldPosition()) or nil,
    inMenu = Game.GetBlackboardSystem():Get(Game.GetAllBlackboardDefs().UI_System):GetBool(Game.GetAllBlackboardDefs().UI_System.IsInMenu)}
end
function A.begin(token)
  assert(not A.session, "previous render-audit fixture needs cleanup first")
  local protection = A.requireProtection()
  Game.GetAutoSaveSystem():InvalidateAutoSaveRequests()
  A.session = {token = token, markers = {}, tag = CName.new("cpvr_render_audit_" .. token), rootName = protection.rootName}
  return A.state()
end
local function owned(token)
  local s = assert(A.session, "no owned render fixture")
  assert(s.token == token, "fixture belongs to another run")
  return s
end
function A.markers(token, points)
  local s = owned(token)
  A.requireProtection()
  assert(#s.markers == 0, "test markers already exist")
  assert(TweakDB:GetRecord(TweakDBID.new("Mappins.DefaultStaticMappin")), "missing mappin record")
  local result = {}
  for _, point in ipairs(points) do
    local d = MappinData.new()
    d.mappinType = TweakDBID.new("Mappins.DefaultStaticMappin")
    d.variant = gamedataMappinVariant.CustomPositionVariant
    d.active = true; d.visibleThroughWalls = true
    local id = Game.GetMappinSystem():RegisterMappin(d, Vector4.new(point[1], point[2], point[3], 1))
    s.markers[#s.markers + 1] = id
    result[#result + 1] = {id = tostring(id), position = point}
  end
  return result
end
function A.spawnVehicle(token, record, position)
  local s = owned(token)
  A.requireProtection()
  assert(not s.vehicleCreated, "owned test vehicle already exists")
  assert(TweakDB:GetRecord(TweakDBID.new(record)), "vehicle record does not exist")
  local spec = DynamicEntitySpec.new()
  spec.recordID = TweakDBID.new(record)
  spec.position = Vector4.new(position[1], position[2], position[3], 1)
  spec.orientation = Game.GetPlayer():GetWorldOrientation()
  spec.persistState = false; spec.persistSpawn = false; spec.alwaysSpawned = true; spec.tags = {s.tag}
  local id = Game.GetDynamicEntitySystem():CreateEntity(spec)
  s.vehicleCreated = true; s.vehicleId = id; s.vehicleObserved = false; s.vehicleDeleteAccepted = false
  return {id = tostring(id.hash), tag = tostring(s.tag), record = record, position = position}
end
function A.vehicle(token)
  local s = owned(token)
  assert(s.vehicleCreated and s.vehicleId, "no owned vehicle request")
  local system = Game.GetDynamicEntitySystem()
  local entity = system:GetEntity(s.vehicleId)
  local spawned = system:IsSpawned(s.vehicleId) == true
  if entity or spawned then s.vehicleObserved = true end
  return {id = tostring(s.vehicleId.hash), managed = system:IsManaged(s.vehicleId) == true,
    spawning = system:IsSpawning(s.vehicleId) == true, spawned = spawned,
    materialized = entity ~= nil, observed = s.vehicleObserved, deleteAccepted = s.vehicleDeleteAccepted}
end
function A.cleanup(token)
  local s = owned(token)
  A.requireProtection()
  local errors, remaining = {}, {}
  for _, id in ipairs(s.markers) do
    local ok, err = pcall(function() Game.GetMappinSystem():UnregisterMappin(id) end)
    if not ok then remaining[#remaining + 1] = id; errors[#errors + 1] = tostring(err) end
  end
  s.markers = remaining
  if s.vehicleCreated then
    local p = Game.GetPlayer(); local mounted = p and p:GetMountedVehicle()
    -- CET returns a fresh userdata wrapper for each EntityID read. Compare its
    -- exact Uint64 hash; userdata identity can differ for the same vehicle.
    assert(not mounted or tostring(mounted:GetEntityID().hash) ~= tostring(s.vehicleId.hash), "dismount the owned test vehicle before cleanup")
    -- CreateEntity is asynchronous. Empty GetTagged before materialization says
    -- nothing about cancellation. Delete only the exact ID returned to this run.
    local system = Game.GetDynamicEntitySystem()
    if system:DeleteEntity(s.vehicleId) == true then s.vehicleDeleteAccepted = true end
    local status = A.vehicle(token)
    if not s.vehicleDeleteAccepted or status.managed or status.spawning or status.spawned or status.materialized then
      return {requested = true, complete = false, vehiclePending = true, vehicle = status,
        markerFailures = errors, remainingMarkers = #remaining}
    end
    s.vehicleCreated = false
  end
  if #errors == 0 then Game.GetAutoSaveSystem():InvalidateAutoSaveRequests(); A.session = nil end
  return {requested = true, complete = #errors == 0, markerFailures = errors, remainingMarkers = #remaining}
end
