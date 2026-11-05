-- Authoring adapter for the installed WorldBuilder (keanuWheeze). Uses its public Lua objects; no copied loader.
-- CETBridge/MCP entry: GetMod('CyberpunkVRPort_Lobby').editor({op='list'})
-- Only our own group is edited. Positions are in the lobby frame, metres, rotation in degrees.
local M = {}
local group, anchor
local specs = {}
local allowed = { ['mesh/mesh']=true, ['visual/fog']=true, ['light/light']=true, ['entity/entity']=true }
local GROUP = 'CyberpunkVR Lobby Details'

local function clone(v)
  if type(v) ~= 'table' then return v end
  local t = {}; for k,x in pairs(v) do t[k]=clone(x) end; return t
end

local function ui()
  local mod=GetMod('entSpawner')
  assert(mod and mod.baseUI and mod.baseUI.spawnedUI, 'WorldBuilder is not loaded')
  return mod.baseUI.spawnedUI
end

local function ensure()
  if group then return end
  assert(anchor, 'Lobby is not active')
  local u=ui()
  -- Reuse the installed group constructor so its own loader resolves modules inside its CET sandbox.
  group=getmetatable(u.multiSelectGroup).__index:new(u)
  group:load({name=GROUP,modulePath='modules/classes/editor/positionableGroup',childs={}})
  u.addRootElement(group)
end

local function find(name)
  if not group then return end
  for _,child in ipairs(group.childs) do if child.name==name then return child end end
end

local function data(spec)
  local s=clone(spec)
  local p=s.position or {x=0,y=0,z=0}
  local a=math.rad(anchor.yaw); local c,sn=math.cos(a),math.sin(a)
  s.position={x=anchor.pos.x+c*p.x-sn*p.y,y=anchor.pos.y+sn*p.x+c*p.y,z=anchor.pos.z+p.z,w=1}
  s.rotation=s.rotation or {roll=0,pitch=0,yaw=0}
  s.rotation.yaw=(s.rotation.yaw or 0)+anchor.yaw
  s.previewed=false
  return {name=s.name,modulePath='modules/classes/editor/spawnableElement',spawnable=s,visible=true,selected=false}
end

function M.upsert(spec)
  assert(type(spec)=='table' and type(spec.name)=='string' and #spec.name>0, 'Named object required')
  assert(allowed[spec.modulePath], 'Unsupported WorldBuilder object type')
  ensure()
  local existing=find(spec.name)
  if existing then
    existing:load(data(spec))
  else
    -- A temporary group invokes WorldBuilder's normal serialized-object factory without touching other roots.
    local u=ui(); local holder=getmetatable(u.multiSelectGroup).__index:new(u)
    holder:load({name='Import',modulePath='modules/classes/editor/positionableGroup',childs={data(spec)}})
    holder.childs[1]:setParent(group)
  end
  specs[spec.name]=clone(spec)
  return {name=spec.name,queued=true}
end

function M.list()
  local out={}
  for name,s in pairs(specs) do
    local child=find(name)
    out[#out+1]={name=name,type=s.modulePath,position=clone(s.position),spawned=child and child.spawnable:isSpawned() or false}
  end
  table.sort(out,function(a,b) return a.name<b.name end)
  return out
end

function M.close()
  if group then group:remove() end
  group,anchor=nil,nil
end

function M.open(at)
  M.close(); anchor=at
  local f=io.open('data/details.json','r')
  if f then
    local source=f:read('*a'); f:close()
    local records=json.decode(source)
    specs={}
    for _,s in ipairs(records.objects or {}) do M.upsert(s) end
  end
end

function M.command(req)
  assert(type(req)=='table','Object request required')
  if req.op=='list' then return M.list() end
  if req.op=='upsert' then return M.upsert(req.object) end
  if req.op=='remove' then
    local child=find(req.name); if child then child:remove() end
    specs[req.name]=nil; return true
  end
  if req.op=='get' then return clone(specs[req.name]) end
  if req.op=='save' then
    local out={version=1,objects={}}
    for _,item in ipairs(M.list()) do out.objects[#out.objects+1]=specs[item.name] end
    local f=assert(io.open('data/details.json','w')); f:write(json.encode(out)); f:close()
    return {saved=#out.objects,path='data/details.json'}
  end
  error('Expected list, get, upsert, remove or save')
end

return M
