-- A scrolling list for the VR menu: dragged with a fingertip pushed into the screen, or with a pointer
-- and a held trigger or right thumbstick button (R3), with momentum when let go.
--
-- An item crossing the top or bottom edge fades with how much of it is still inside; an item outside
-- is hidden. Large artwork can also use an engine scroll-area mask to clip at the viewport. Items are
-- built LAZILY: the ones in the window at once, the rest a few each frame ahead of the scroll, so a
-- 50-row list costs no hitch when it opens. The scroll bar beside it shows where the view is and takes
-- no input.

local K = require("modules/ink")

local V = {}
V.__index = V

local FRICTION = 4.5          -- 1/s: how fast a flick dies down
local BAR = 8                 -- px: the scroll bar's width, inside the view's right edge

local function clamp(x, lo, hi) return math.max(lo, math.min(hi, x)) end

-- What an item of a view w wide gets of the width; the scroll bar has the rest.
function V.contentWidth(w) return w - BAR - 10 end

-- parent: the canvas the view sits in, (px, py) its top-left there; (ax, ay) the same point in screen
-- canvas pixels, which is where the pointers report. w x h is the window onto the content. style (optional,
-- another menu's colours): { track, thumb, held } -- the scroll bar's track, its thumb, the thumb while dragged.
function V.new(parent, px, py, ax, ay, w, h, style)
  local v = setmetatable({}, V)
  v.style = style or { track = K.CYAN, thumb = K.CYAN, held = K.YELLOW }
  v.root = K.canvas(parent, px, py, w, h)
  v.content = K.canvas(v.root, 0, 0, w, h)
  v.track = K.rect(v.root, w - BAR + 2, 0, BAR - 4, h, v.style.track, 0.22)
  v.thumb = K.rect(v.root, w - BAR, 0, BAR, h, v.style.thumb, 0.9)
  v.track:SetVisible(false)
  v.thumb:SetVisible(false)
  v.x, v.y, v.w, v.h = ax, ay, w, h
  v.cw = V.contentWidth(w)
  v.items, v.total, v.offset, v.vel = {}, 0, 0, 0
  v.placed = nil
  v.first = true                 -- until the first update is over: what a drop-in animation keys on
  return v
end

-- An item of height h; build(canvas, it, view) fills its canvas when it first comes near the window.
function V:add(h, build)
  local it = { y = self.total, h = h, build = build, index = #self.items + 1 }
  self.items[#self.items + 1] = it
  self.total = self.total + h
  return it
end

-- Large artwork needs an actual viewport in addition to the row's edge fade.
-- Keep our finger/trigger inertia; only let the engine mask pixels outside the window.
function V:enableMask()
  if self.mask then return end
  local mask=inkScrollAreaWidget.new()
  mask:SetAnchor(inkEAnchor.TopLeft)
  mask:SetAnchorPoint(Vector2.new({X=0,Y=0}))
  mask:SetMargin(inkMargin.new({left=0,top=0,right=0,bottom=0}))
  mask:SetSize(Vector2.new({X=self.w,Y=self.h}))
  mask:SetFitToContent(false)
  mask:SetUseInternalMask(true)
  mask.constrainContentPosition=true
  mask.fitToContentDirection=inkFitToContentDirection.None
  mask:Reparent(self.root)
  self.content:Reparent(mask)
  self.mask=mask
  K.move(self.content,0,0)
  mask:ScrollVertical(self.offset/math.max(1,self:maxOffset()))
  -- A restored nonzero offset can arrive before ink has laid out the new scroll area.
  -- Reapply it after layout; otherwise the fade/hit boxes scroll but the pixels stay at the top.
  self.maskSyncFrames=3
  self.placed=nil
end

function V:maxOffset() return math.max(0, self.total - self.h) end
function V:scrollable() return self.total > self.h + 1 end
function V:atTop() return self.offset <= 1 end
function V:atBottom() return self.offset >= self:maxOffset() - 1 end
-- Held, flung or easing to a target: rows are passing under a still pointer.
function V:moving() return self.dragging ~= nil or self.vel ~= 0 or self.target ~= nil end

function V:contains(x, y)
  return x >= self.x and x <= self.x + self.w and y >= self.y and y <= self.y + self.h
end

-- Hit testing follows the clipped, faded rows, not the transparent scroll viewport.
function V:visibleAt(x, y)
  if not self:contains(x, y) then return false end
  local cy = y - self.y + self.offset
  for _, it in ipairs(self.items) do
    if cy < it.y then return false end
    if cy < it.y + it.h then return it.canvas ~= nil and (it.fade or 0) > 0.2 end
  end
  return false
end

-- How much of an item is inside the window, 0..1.
function V:visible(it)
  local a, b = it.y - self.offset, it.y + it.h - self.offset
  local inside = math.min(b, self.h) - math.max(a, 0)
  return clamp(inside / math.max(it.h, 1), 0, 1)
end

-- A point of item content (its y) in screen canvas pixels, and whether the item is enough in view to
-- be pointed at.
function V:screenY(it, localY) return self.y + it.y + localY - self.offset end
function V:pointable(it) return self:visible(it) > 0.5 end

---------------------------------------------------------------- motion

function V:beginDrag(y)
  self.dragging = { y0 = y, off0 = self.offset, lastY = y, lastT = nil }
  self.vel, self.target = 0, nil
end

function V:dragTo(y, now)
  local d = self.dragging
  if d == nil then return end
  self.offset = clamp(d.off0 - (y - d.y0), 0, self:maxOffset())
  if d.lastT ~= nil and now > d.lastT then
    local inst = -(y - d.lastY) / (now - d.lastT)
    self.vel = self.vel * 0.6 + inst * 0.4
  end
  d.lastY, d.lastT = y, now
end

function V:endDrag()
  self.dragging = nil
  if math.abs(self.vel) < 60 then self.vel = 0 end
end

function V:scrollBy(dy)
  self.target = clamp((self.target or self.offset) + dy, 0, self:maxOffset())
  self.vel = 0
end

function V:scrollTo(offset)
  self.offset = clamp(offset or 0, 0, self:maxOffset())
  self.target, self.vel = nil, 0
end

---------------------------------------------------------------- per frame

-- Moves, builds what is in the window and up to `budget` more, then places the content and fades the
-- edges. Returns true while the list is still moving.
function V:update(dt, budget)
  local moving = false
  if self.dragging == nil then
    if self.target ~= nil then
      local d = self.target - self.offset
      if math.abs(d) < 1 then
        self.offset, self.target = self.target, nil
      else
        self.offset = self.offset + d * math.min(1, dt * 12)
        moving = true
      end
    elseif self.vel ~= 0 then
      self.offset = self.offset + self.vel * dt
      self.vel = self.vel * math.exp(-FRICTION * dt)
      if math.abs(self.vel) < 25 or self.offset <= 0 or self.offset >= self:maxOffset() then self.vel = 0 end
      moving = true
    end
  else
    moving = true
  end
  self.offset = clamp(self.offset, 0, self:maxOffset())

  -- the window, all of it, then up to `budget` items of a screen ahead of it in either direction
  local built, ahead = 0, 0
  local lo, hi = self.offset - self.h * 0.5, self.offset + self.h * 1.5
  for pass = 1, 2 do
    for _, it in ipairs(self.items) do
      if pass == 2 and ahead >= (budget or 4) then break end
      local near = it.y + it.h > lo and it.y < hi
      local inView = it.y + it.h > self.offset and it.y < self.offset + self.h
      if it.canvas == nil and ((pass == 1 and inView) or (pass == 2 and near)) then
        it.canvas = K.canvas(self.content, 0, it.y, self.cw, it.h)
        it.canvas:SetVisible(false)
        it.fade = -1
        local ok, err = pcall(it.build, it.canvas, it, self)
        if not ok then print("[VRMenu] list item: " .. tostring(err)) end
        built = built + 1
        if pass == 2 then ahead = ahead + 1 end
      end
    end
  end
  self.first = false

  if self.placed ~= self.offset or built > 0 or (self.maskSyncFrames or 0)>0 then
    self.placed = self.offset
    K.size(self.content, self.w, math.max(self.total, self.h))
    if self.mask then
      K.move(self.content,0,0)
      self.mask:ScrollVertical(self.offset/math.max(1,self:maxOffset()))
      self.maskSyncFrames=math.max(0,(self.maskSyncFrames or 0)-1)
    else
      K.move(self.content, 0, -self.offset)
    end
    for _, it in ipairs(self.items) do
      if it.canvas ~= nil then
        local f = self:visible(it)
        if math.abs(f - it.fade) > 0.02 or (f == 0) ~= (it.fade == 0) then
          it.fade = f
          it.canvas:SetVisible(f > 0)
          if f > 0 then it.canvas:SetOpacity(f * f) end
        end
      end
    end
    self:bar()
    if self.onScroll then pcall(self.onScroll, self) end
  end
  return moving or built > 0
end

function V:bar()
  local show = self:scrollable()
  self.track:SetVisible(show)
  self.thumb:SetVisible(show)
  if not show then return end
  local th = math.max(60, self.h * self.h / self.total)
  local ty = (self.h - th) * (self.offset / math.max(self:maxOffset(), 1))
  K.move(self.thumb, self.w - BAR, ty)
  K.size(self.thumb, BAR, th)
  local col = self.dragging and self.style.held or self.style.thumb
  self.thumb:SetTintColor(K.color(col))
end

return V
