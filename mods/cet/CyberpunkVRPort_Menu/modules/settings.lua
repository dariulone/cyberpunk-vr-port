-- The port's settings in the VR menu: the pages of the ImGui panel, read out by the plugin (modules/settings_data.lua)
-- and drawn here as the menu's own rows. So this file knows no setting by name: a new control in the panel shows up
-- here by itself, with its range, its tooltip and its disabled state.
--
-- The page is polled while it is on the screen. A recording with the same rows as the one drawn only
-- refreshes values and texts in place; rows coming or going (a section folded, a mode that shows other
-- controls) draw the list again where it was scrolled to, once no hand is in it.

local K = require("modules/ink")
local V = require("modules/scroll")
local D = require("modules/settings_data")

local Set = {}

local PAGES = D.PAGES


-- The panel, in its own canvas pixels: header, page tabs, the list between its arrows, the tooltip box.
local TAB_Y, TAB_H, TAB_GAP = 104, 80, 8
local ARROW_H = 52
local VIEW_Y, VIEW_H = 196, 800
local DOWN_Y = VIEW_Y + VIEW_H + 8
local INFO_Y, INFO_H = DOWN_Y + ARROW_H + 12, 136
local PANEL_H = INFO_Y + INFO_H + 14

-- Rows.
local RH, GAP = 92, 10                 -- a control's row, and the gap under every row
local INDENT = 40
local FS_LABEL, FS_VALUE, FS_TEXT, FS_INFO = 38, 36, 32, 30
local LINE, INFO_LINE = 40, 38
local SOFT = { 1.0, 0.76, 0.72 }       -- running text: the red, lifted further than a label
local R_CELL = 22                       -- the rounded cells' corner, as the menu's (menu.lua R_CELL)

local ST = {
  kit = nil, panel = nil, host = nil, view = nil, viewPage = nil,
  page = 0, str = nil, cur = nil, sig = nil, slots = {},
  open = {},          -- header id -> open: the player's own folding (else the panel's default)
  offsets = {},       -- page -> where its list was scrolled to
  pollAt = 0, fastUntil = 0,
  sent = {},          -- item id -> when its last edit went out
  queued = {},        -- item id -> { f, v }: a slider's value waiting for its turn
  tip = nil, rebuildNow = false,
}

local function clamp(x, lo, hi) return math.max(lo, math.min(hi, x)) end
local has, split, parse = D.has, D.split, D.parse
local stepOf, snap, fmt, ulen, wrap, fetch = D.stepOf, D.snap, D.fmt, D.ulen, D.wrap, D.fetch

---------------------------------------------------------------- the page string

local function headerOpen(f)
  local o = ST.open[f.id]
  if o == nil then o = has(f.flags, 8) end
  return o
end

-- The rows a page shows: a folded section hides everything up to the next section of its level.
local function visible(items) return D.visible(items, headerOpen) end

-- The push buttons a page opens with (Recenter, Save), which sit in the panel's header instead of the list.
local function leading(items)
  local lead, rest = {}, {}
  for _, f in ipairs(items) do
    if #rest == 0 and f.kind == "B" and f.indent == 0 then lead[#lead + 1] = f else rest[#rest + 1] = f end
  end
  return lead, rest
end

-- What makes two recordings the same list of rows; values and texts may differ.
local function signature(vis)
  local parts = {}
  for i, f in ipairs(vis) do
    local extra = ""
    if f.kind == "T" then extra = tostring(#f.lines)
    elseif f.kind == "O" then extra = f.text
    elseif f.kind == "P" then extra = f.text .. "=" .. f.value
    elseif f.kind == "S" or f.kind == "I" then extra = f.min .. ".." .. f.max
    elseif f.kind == "H" then extra = headerOpen(f) and "1" or "0" end
    parts[i] = table.concat({ f.kind, f.id, f.label, f.flags, f.indent, extra }, "\1")
  end
  return table.concat(parts, "\2")
end

local function differs(a, b)
  return a.value ~= b.value or a.text ~= b.text or a.tip ~= b.tip
end

---------------------------------------------------------------- numbers and text

-- The rows are as tall as their lines: the screen's text widget does not wrap by itself (D.wrap).
local function charsFor(width, size) return math.max(10, math.floor(width / (size * 0.52))) end

local function textColor(f, dim)
  if has(f.flags, 4) then
    local v = math.floor(f.value)
    return { math.floor(v / 65536) % 256 / 255, math.floor(v / 256) % 256 / 255, v % 256 / 255 }
  end
  if dim or has(f.flags, 1) then return K.GREY end
  return SOFT
end

---------------------------------------------------------------- talking to the plugin

local function send(f, value)
  D.edit(f, value)
  local now = ST.kit and ST.kit.S.time or 0
  ST.sent[f.id] = now
  ST.fastUntil = now + 1.0
  ST.pollAt = math.min(ST.pollAt, now + 0.05)
end

-- A slider moving sends at most ten values a second, and the one it is let go at.
local function queue(f, v, final)
  if final then
    ST.queued[f.id] = nil
    send(f, v)
  else
    ST.queued[f.id] = { f = f, v = v }
  end
end

local function flush(id)
  local q = ST.queued[id]
  if q == nil then return end
  ST.queued[id] = nil
  send(q.f, q.v)
end

local function flushDue(now)
  for id, q in pairs(ST.queued) do
    if now - (ST.sent[id] or -1) >= 0.1 then
      ST.queued[id] = nil
      send(q.f, q.v)
    end
  end
end

local function flushAll()
  for id, _ in pairs(ST.queued) do flush(id) end
end

---------------------------------------------------------------- the rows

-- Every control of a row lights the row's cell along with itself.
local function joinRow(ctx, b, tip)
  local own = b.paint
  b.paint = function(hover)
    if own then own(hover) end
    ctx.lit(hover)
  end
  b.tip = tip
  return b
end

-- A row's cell, and under its controls a button of its own that does nothing but be pointed at: that
-- is what lights the row and shows its tooltip.
local function backing(c, ctx, f)
  local x, w, h = ctx.x, ctx.w, ctx.h
  local col = ctx.dim and K.GREY or K.NAVY
  local rest, restEdge = ctx.dim and 0.02 or 0.18, 0.15
  local bg = K.roundFill(c, x, 0, w, h, col, rest, 4)
  local edge = K.cutFrame(c, x, 0, w, h, K.CYAN, restEdge, 9, 1)
  ctx.lit = function(hover)
    bg:SetTintColor(K.color(hover and K.YELLOW or col))
    bg:SetOpacity(hover and 0.16 or rest)
    K.tint(edge, hover and K.YELLOW or K.CYAN, hover and 0.8 or restEdge)
  end
  local b = ctx.kit.button("set", ctx.H, x, 0, w, h, nil, ctx.lit)
  b.passive, b.tip = true, f.tip
  return b
end

local function label(c, ctx, f, x, w)
  local n = math.max(6, math.floor(w / (FS_LABEL * 0.47)))
  return K.text(c, x, 0, w, ctx.h, K.clip(f.label, n), FS_LABEL, ctx.dim and K.GREY or K.TEXT)
end

local KIND = {}

-- A section of the page: a lit band with its title and a chevron; it folds what follows.
KIND.H = function(c, f, ctx)
  local kit, x, w = ctx.kit, ctx.x, ctx.w
  local open = headerOpen(f)
  local rest = open and 0.26 or 0.14
  local bg = K.roundFill(c, x, 14, w, 68, K.CYAN, rest, R_CELL)
  K.text(c, x + 28, 14, w - 140, 68, K.upper(f.label), 42, K.WHITE)
  local chev = kit.chevron(c, x + w - 50, 48, 13, 6, open and "down" or "right", K.CYAN, 1.0)
  local b = kit.button("set", ctx.H, x, 14, w, 68, function()
    ST.open[f.id] = not open
    ST.rebuildNow = true
  end, function(hover)
    bg:SetTintColor(K.color(hover and K.YELLOW or K.CYAN))
    bg:SetOpacity(hover and 0.34 or rest)
    K.tint(chev, hover and K.WHITE or K.CYAN)
  end)
  b.tip = f.tip
end

-- Text: lines of the panel's own, in its colour.
KIND.T = function(c, f, ctx)
  local t = K.text(c, ctx.x + 28, 2, ctx.w - 56, #f.lines * LINE, table.concat(f.lines, "\n"), FS_TEXT,
    textColor(f, ctx.dim))
  if f.tip ~= "" then
    local b = ctx.kit.button("set", ctx.H, ctx.x, 0, ctx.w, ctx.h, nil, nil)
    b.passive, b.tip = true, f.tip
  end
  return function(nf)
    f = nf
    t:SetText(table.concat(nf.lines, "\n"))
    t:SetTintColor(K.color(textColor(nf, ctx.dim)))
  end
end

-- A checkbox: the whole row toggles (a finger is not a mouse); the switch on its right says ON / OFF.
KIND.C = function(c, f, ctx)
  local kit, x, w, h = ctx.kit, ctx.x, ctx.w, ctx.h
  backing(c, ctx, f)
  local TW = 150
  local tx = x + w - TW - 16
  label(c, ctx, f, x + 28, tx - x - 48)
  local tbg = K.roundFill(c, tx, 12, TW, h - 24, K.CYAN, 0.1, (h - 24) / 2)
  local tedge = K.roundEdge(c, tx, 12, TW, h - 24, K.CYAN, 0.6, (h - 24) / 2)
  local ttxt = K.text(c, tx, 12, TW, h - 24, "", FS_VALUE, K.CYAN, "center")
  local on, hover = f.value ~= 0, false
  local function paint()
    local base = ctx.dim and K.GREY or (on and K.YELLOW or K.CYAN)
    local col = hover and K.YELLOW or base
    tbg:SetTintColor(K.color(col))
    tbg:SetOpacity(on and 0.40 or (hover and 0.20 or 0.06))
    K.tint(tedge, col, (on or hover) and 1.0 or 0.55)
    ttxt:SetText(on and kit.T.on or kit.T.off)
    ttxt:SetTintColor(K.color((on or hover) and K.WHITE or base))
  end
  paint()
  if not ctx.dim then
    local b = kit.button("set", ctx.H, x, 0, w, h, function()
      on = not on
      paint()
      send(f, on and 1 or 0)
    end, function(hv)
      hover = hv
      paint()
      ctx.lit(hv)
    end)
    b.tip = f.tip
  end
  return function(nf)
    f = nf
    on = nf.value ~= 0
    paint()
  end
end

-- A slider: the label, the value as the panel prints it, then - [track] +.
local function sliderRow(c, f, ctx)
  local kit, x, w, h = ctx.kit, ctx.x, ctx.w, ctx.h
  backing(c, ctx, f)
  local LW, VW, SB = math.floor(w * 0.36), 200, 80
  label(c, ctx, f, x + 28, LW - 28)
  local val = K.text(c, x + LW, 0, VW, h, f.text, FS_VALUE, ctx.dim and K.GREY or K.YELLOW, "right")
  local mx = x + LW + VW + 28
  local px = x + w - SB - 12
  local sx = mx + SB + 46
  local sw = px - 46 - sx
  local lo, hi = math.min(f.min, f.max), math.max(f.min, f.max)
  local step = stepOf(f)
  local cur = f.value
  local sl = nil
  local function set(v, final)
    v = clamp(snap(f, v, lo, step), lo, hi)
    cur = v
    pcall(function() val:SetText(fmt(f, v)) end)
    if sl ~= nil then sl.show(v) end
    queue(f, v, final)
    return v
  end
  if ctx.dim then
    local t = (hi > lo) and clamp((cur - lo) / (hi - lo), 0, 1) or 0
    K.rect(c, sx, h / 2 - 5, sw, 10, K.GREY, 0.25)
    K.rect(c, sx, h / 2 - 5, sw * t, 10, K.GREY, 0.6)
  else
    joinRow(ctx, kit.stepButton(c, "set", ctx.H, mx, 10, SB, h - 20, false, true, function()
      set(cur - step, true)
    end), f.tip)
    joinRow(ctx, kit.stepButton(c, "set", ctx.H, px, 10, SB, h - 20, true, true, function()
      set(cur + step, true)
    end), f.tip)
    sl = kit.slider(c, "set", ctx.H, sx, 0, sw, h, lo, hi, cur, function(v) return set(v, false) end, step,
      function(phase)
        ctx.slot.held = phase == "begin"
        if phase == "end" then flush(f.id) end
      end)
    joinRow(ctx, sl, f.tip)
  end
  return function(nf)
    if ctx.slot.held then return end
    f = nf
    cur = nf.value
    pcall(function() val:SetText(nf.text) end)
    if sl ~= nil then sl.show(cur) end
  end
end
KIND.S, KIND.I = sliderRow, sliderRow

-- A number field: - value +, by the field's own step.
KIND.N = function(c, f, ctx)
  local kit, x, w, h = ctx.kit, ctx.x, ctx.w, ctx.h
  backing(c, ctx, f)
  local SB, VW = 80, 200
  local px = x + w - SB - 12
  local vx = px - 12 - VW
  local mx = vx - 12 - SB
  label(c, ctx, f, x + 28, mx - x - 48)
  local step = (f.min ~= 0) and f.min or 1
  local cur = f.value
  local val = K.text(c, vx, 0, VW, h, f.text, FS_VALUE, ctx.dim and K.GREY or K.YELLOW, "center")
  local function set(v)
    cur = v
    pcall(function() val:SetText(tostring(math.floor(v + 0.5))) end)
    send(f, v)
  end
  if not ctx.dim then
    joinRow(ctx, kit.stepButton(c, "set", ctx.H, mx, 10, SB, h - 20, false, true, function() set(cur - step) end), f.tip)
    joinRow(ctx, kit.stepButton(c, "set", ctx.H, px, 10, SB, h - 20, true, true, function() set(cur + step) end), f.tip)
  end
  return function(nf)
    f = nf
    cur = nf.value
    pcall(function() val:SetText(nf.text) end)
  end
end

-- A combo: < option >, the option itself steps forward too.
KIND.O = function(c, f, ctx)
  local kit, x, w, h = ctx.kit, ctx.x, ctx.w, ctx.h
  backing(c, ctx, f)
  local SB, OW = 80, math.min(460, math.floor(w * 0.34))
  local rx = x + w - SB - 12
  local ox = rx - 10 - OW
  local lx = ox - 10 - SB
  label(c, ctx, f, x + 28, lx - x - 48)
  local opts = split(f.text, "\29")
  local idx = math.floor(f.value + 0.5)
  local col = ctx.dim and K.GREY or K.CYAN
  local obg = K.roundFill(c, ox, 10, OW, h - 20, col, 0.10, R_CELL)
  local oedge = K.roundEdge(c, ox, 10, OW, h - 20, col, 0.5, R_CELL)
  local otxt = K.text(c, ox, 10, OW, h - 20, "", FS_VALUE, ctx.dim and K.GREY or K.YELLOW, "center")
  local function show()
    otxt:SetText(K.clip(opts[idx + 1] or "?", math.max(4, math.floor(OW / (FS_VALUE * 0.5)))))
  end
  show()
  local function pick(d)
    if #opts == 0 then return end
    idx = (idx + d) % #opts
    show()
    send(f, idx)
  end
  if not ctx.dim then
    joinRow(ctx, kit.stepButton(c, "set", ctx.H, lx, 10, SB, h - 20, "left", true, function() pick(-1) end), f.tip)
    joinRow(ctx, kit.button("set", ctx.H, ox, 10, OW, h - 20, function() pick(1) end, function(hv)
      obg:SetTintColor(K.color(hv and K.YELLOW or K.CYAN))
      obg:SetOpacity(hv and 0.28 or 0.10)
      K.tint(oedge, hv and K.YELLOW or K.CYAN, hv and 1.0 or 0.5)
      otxt:SetTintColor(K.color(hv and K.WHITE or K.YELLOW))
    end), f.tip)
    joinRow(ctx, kit.stepButton(c, "set", ctx.H, rx, 10, SB, h - 20, "right", true, function() pick(1) end), f.tip)
  end
  return function(nf)
    f = nf
    opts = split(nf.text, "\29")
    idx = math.floor(nf.value + 0.5)
    show()
  end
end

-- Tabs inside a page: one segment each, the chosen one yellow.
KIND.P = function(c, f, ctx)
  local kit, x, w, h = ctx.kit, ctx.x, ctx.w, ctx.h
  local opts = split(f.text, "\29")
  local n, gap = math.max(1, #opts), 8
  local tw = (w - (n - 1) * gap) / n
  local sel = math.floor(f.value + 0.5)
  for i, name in ipairs(opts) do
    local tx = x + (i - 1) * (tw + gap)
    local on = sel == i - 1
    local base = ctx.dim and K.GREY or (on and K.YELLOW or K.CYAN)
    local bg = K.roundFill(c, tx, 0, tw, h, base, on and 0.30 or 0.08, R_CELL)
    local edge = K.roundEdge(c, tx, 0, tw, h, base, on and 1.0 or 0.45, R_CELL)
    local txt = K.text(c, tx, 0, tw, h, K.clip(name, math.max(4, math.floor(tw / (34 * 0.5)))), 34,
      on and K.WHITE or (ctx.dim and K.GREY or K.TEXT), "center")
    if not ctx.dim and not on then
      local b = kit.button("set", ctx.H, tx, 0, tw, h, function()
        send(f, i - 1)
      end, function(hv)
        bg:SetTintColor(K.color(hv and K.YELLOW or K.CYAN))
        bg:SetOpacity(hv and 0.28 or 0.08)
        K.tint(edge, hv and K.YELLOW or K.CYAN, hv and 1.0 or 0.45)
        txt:SetTintColor(K.color(hv and K.WHITE or K.TEXT))
      end)
      b.tip = f.tip
    end
  end
end

-- A push button.
KIND.B = function(c, f, ctx)
  local kit, x, w, h = ctx.kit, ctx.x, ctx.w, ctx.h
  local bw = math.min(w, 760)
  local col = ctx.dim and K.GREY or K.CYAN
  local bg = K.roundFill(c, x, 0, bw, h, col, ctx.dim and 0.04 or 0.14, R_CELL)
  local edge = K.roundEdge(c, x, 0, bw, h, col, ctx.dim and 0.3 or 0.9, R_CELL)
  local txt = K.text(c, x, 0, bw, h, K.clip(K.upper(f.label), math.floor(bw / (FS_LABEL * 0.55))), FS_LABEL, col,
    "center")
  if not ctx.dim then
    local b = kit.button("set", ctx.H, x, 0, bw, h, function()
      send(f, 1)
    end, function(hv)
      bg:SetTintColor(K.color(hv and K.YELLOW or K.CYAN))
      bg:SetOpacity(hv and 0.30 or 0.14)
      K.tint(edge, hv and K.YELLOW or K.CYAN, hv and 1.0 or 0.9)
      txt:SetTintColor(K.color(hv and K.WHITE or K.CYAN))
    end)
    b.tip = f.tip
  end
end

-- A progress bar.
KIND.G = function(c, f, ctx)
  local x, w = ctx.x + 28, ctx.w - 56
  K.roundFill(c, x, 10, w, 16, K.CYAN, 0.25, 8)
  local fill = K.roundFill(c, x, 10, w * clamp(f.value, 0, 1), 16, K.YELLOW, 0.9, 8)
  return function(nf) K.roundSize(fill, w * clamp(nf.value, 0, 1), 16, 8) end
end

-- A binding: the input in yellow, what it does after it.
KIND.K = function(c, f, ctx)
  local x, w, h = ctx.x, ctx.w, ctx.h
  local IW = math.floor(w * 0.40)
  K.text(c, x + 28, 0, IW - 28, h, K.clip(f.label, math.floor((IW - 28) / (34 * 0.5))), 34, K.YELLOW)
  K.text(c, x + IW, 0, w - IW - 28, h, K.clip(f.text, math.floor((w - IW - 28) / (34 * 0.5))), 34, SOFT)
  K.rect(c, x + 28, h - 2, w - 56, 2, K.CYAN, 0.18)
end

KIND.X = function(c, f, ctx)
  K.rect(c, ctx.x + 20, 6, ctx.w - 40, 2, K.CYAN, 0.35)
end

local function heightOf(f)
  local k = f.kind
  if k == "H" then return 96 end
  if k == "T" then return #f.lines * LINE + 14 end
  if k == "X" then return 26 end
  if k == "G" then return 46 end
  if k == "K" then return 56 end
  return RH
end

local function buildItem(c, it, v)
  local f = it.f
  local x = f.indent * INDENT
  local ctx = {
    kit = ST.kit, H = ST.kit.at(v.x, it.y, v), x = x, w = v.cw - x, slot = it,
    h = it.h - (f.kind == "H" and 0 or GAP), dim = has(f.flags, 16),
  }
  local make = KIND[f.kind]
  if make ~= nil then it.refresh = make(c, f, ctx) end
end

-- The page's leading push buttons, small, right-aligned on the header's line.
local HEAD_BH, HEAD_FS = 48, 28
local function drawHead(lead)
  local kit = ST.kit
  if ST.head == nil then return end
  ST.head:RemoveAllChildren()
  kit.clearButtons("sethead")
  local x1 = ST.w - 76          -- clear of the frame's corner mark
  for i = #lead, 1, -1 do
    local f = lead[i]
    local label = K.upper(f.label)
    local bw = math.floor(ulen(label) * HEAD_FS * 0.55) + 44
    local x = x1 - bw
    local y = math.floor((kit.HEAD - HEAD_BH) / 2) + 2
    local dim = has(f.flags, 16)
    local col = dim and K.GREY or K.CYAN
    local bg = K.roundFill(ST.head, x, y, bw, HEAD_BH, col, dim and 0.04 or 0.14, HEAD_BH / 2)
    local edge = K.roundEdge(ST.head, x, y, bw, HEAD_BH, col, dim and 0.3 or 0.9, HEAD_BH / 2)
    local txt = K.text(ST.head, x, y, bw, HEAD_BH, label, HEAD_FS, col, "center")
    if not dim then
      local b = kit.button("sethead", kit.at(ST.sx, ST.sy), x, y, bw, HEAD_BH, function()
        send(f, 1)
      end, function(hv)
        bg:SetTintColor(K.color(hv and K.YELLOW or K.CYAN))
        bg:SetOpacity(hv and 0.30 or 0.14)
        K.tint(edge, hv and K.YELLOW or K.CYAN, hv and 1.0 or 0.9)
        txt:SetTintColor(K.color(hv and K.WHITE or K.CYAN))
      end)
      b.tip = f.tip
    end
    x1 = x - 14
  end
end

---------------------------------------------------------------- the list

local function repaint(list)
  if list == nil or ST.kit == nil then return end
  local hov = ST.kit.S.hovered
  for _, b in ipairs(list) do
    if b.paint then b.paint(hov[b] == true) end
  end
end

-- A hand pressing, dragging or scrolling in the list: it holds the list's widgets.
local function busy()
  local v = ST.view
  if v == nil then return false end
  for _, P in ipairs(ST.kit.S.hands) do
    if (P.pend and P.pend.view == v) or (P.scroll and P.scroll.view == v) or (P.drag and P.drag.view == v) then
      return true
    end
  end
  return false
end

local function rebuild(vis)
  local kit = ST.kit
  if ST.host == nil then return end
  local old = ST.view
  local offset = (old ~= nil and ST.viewPage == ST.page) and old.offset or (ST.offsets[ST.page] or 0)
  if old ~= nil then kit.dropView(old) end
  ST.host:RemoveAllChildren()
  local PAD = kit.PAD
  local v = V.new(ST.host, PAD, VIEW_Y, ST.sx + PAD, ST.sy + VIEW_Y, ST.w - 2 * PAD, VIEW_H)
  ST.slots = {}
  for i, f in ipairs(vis) do
    local slot = v:add(heightOf(f), buildItem)
    slot.f = f
    ST.slots[i] = slot
  end
  -- before the plugin has recorded the page (a frame) the list is just empty, no reading line
  if #vis == 0 and ST.cur then
    K.text(ST.host, PAD + 34, VIEW_Y, ST.w - 2 * PAD - 68, 100, kit.T.empty, 40, K.GREY)
  end
  v.onScroll = function()
    ST.offsets[ST.page] = v.offset
    repaint(ST.arrows)
  end
  v:scrollTo(offset)
  ST.view, ST.viewPage = v, ST.page
  kit.S.views.set = v
  v:update(0, 0)
  repaint(ST.arrows)
end

-- Draws a recording: in place when it has the rows already drawn, else as a new list. False when the
-- list could not be drawn again yet (a hand is in it).
local function show(pg, force, anyway)
  local cw = V.contentWidth(ST.w - 2 * ST.kit.PAD)
  local lead, rest = leading(pg.items)
  local vis = visible(rest)
  for _, f in ipairs(vis) do
    if f.kind == "T" then f.lines = wrap(f.text, charsFor(cw - f.indent * INDENT - 56, FS_TEXT)) end
  end
  local sig = signature(vis) .. "\3" .. signature(lead)
  if force or ST.view == nil or sig ~= ST.sig then
    if busy() and not anyway then return false end
    ST.cur, ST.sig = pg, sig
    drawHead(lead)
    rebuild(vis)
    return true
  end
  ST.cur = pg
  local now = ST.kit.S.time
  for i, f in ipairs(vis) do
    local slot = ST.slots[i]
    if slot ~= nil and differs(slot.f, f) then
      -- a value just sent, or still being dragged, is the player's until the plugin has caught up
      local quiet = slot.held or ST.queued[f.id] ~= nil or (ST.sent[f.id] ~= nil and now - ST.sent[f.id] < 0.4)
      if not quiet then
        slot.f = f
        if slot.refresh ~= nil then pcall(slot.refresh, f) end
      end
    end
  end
  return true
end

local function setPage(i)
  if i == ST.page or i < 0 or i >= #PAGES then return end
  flushAll()
  if ST.view ~= nil then ST.offsets[ST.page] = ST.view.offset end
  ST.page = i
  ST.str, ST.sig, ST.cur = nil, nil, nil
  repaint(ST.tabs)
  local str = fetch(i)
  local pg = parse(str)
  if pg ~= nil and pg.page == i then
    ST.str = str
    show(pg, true, true)
  else
    rebuild({})
  end
  ST.pollAt = 0
end

---------------------------------------------------------------- driven by menu.lua

-- The panel, in canvas p whose top-left is (sx, sy) on the screen, w wide.
function Set.build(kit, p, sx, sy, w)
  -- ST.panel is set last: a panel half built (an error in it) is no panel to update
  ST.kit, ST.panel, ST.host, ST.sx, ST.sy, ST.w = kit, nil, nil, sx, sy, w
  ST.view, ST.viewPage, ST.sig, ST.str, ST.cur, ST.slots, ST.tip = nil, nil, nil, nil, nil, {}, nil
  ST.head = nil
  local PAD = kit.PAD
  kit.glass(p, w, PANEL_H, sx, sy)
  kit.header(p, w, kit.T.settings, K.CYAN)
  ST.head = K.canvas(p, 0, 0, w, kit.HEAD)

  local n = #PAGES
  local tw = (w - 2 * PAD - (n - 1) * TAB_GAP) / n
  ST.tabs = {}
  for i = 1, n do
    local x = PAD + (i - 1) * (tw + TAB_GAP)
    local bg = K.roundFill(p, x, TAB_Y, tw, TAB_H, K.ICE, 0.04, TAB_H / 2)
    local edge = K.roundEdge(p, x, TAB_Y, tw, TAB_H, K.ICE, 0.0, TAB_H / 2)
    local under = K.rect(p, x + 12, TAB_Y + TAB_H - 10, tw - 24, 4, K.YELLOW, 0.0)
    local txt = K.text(p, x, TAB_Y, tw, TAB_H, D.pageName(i), 34, K.TEXT, "center")
    ST.tabs[i] = kit.button("set", kit.at(sx, sy), x, TAB_Y, tw, TAB_H, function()
      setPage(i - 1)
    end, function(hover)
      local sel = ST.page == i - 1
      local lit = hover and K.YELLOW or (sel and K.YELLOW or nil)
      bg:SetTintColor(K.color(lit or K.ICE))
      bg:SetOpacity(hover and 0.30 or (sel and 0.22 or 0.04))
      K.tint(edge, lit or K.ICE, lit and 1.0 or 0.0)
      under:SetOpacity(sel and 1.0 or 0.0)
      txt:SetTintColor(K.color(hover and K.WHITE or (sel and K.YELLOW or K.TEXT)))
    end)
  end

  local function current() return ST.view end
  ST.arrows = {
    kit.arrowBar(p, "set", kit.at(sx, sy), PAD, DOWN_Y, (w - 2 * PAD - 14) / 2, ARROW_H, true, current),
    kit.arrowBar(p, "set", kit.at(sx, sy), (w + 14) / 2, DOWN_Y, (w - 2 * PAD - 14) / 2, ARROW_H, false, current),
  }
  ST.host = K.canvas(p, 0, 0, w, PANEL_H)
  K.roundFill(p, PAD, INFO_Y, w - 2 * PAD, INFO_H, K.ICE, 0.04, R_CELL)
  ST.info = K.text(p, PAD + 28, INFO_Y + 8, w - 2 * PAD - 56, INFO_H - 16, kit.T.hint, FS_INFO, K.GREY)
  ST.panel = p

  -- what the plugin recorded of the page last time, at once; the poll brings the fresh one
  ST.pollAt = 0
  local str = fetch(ST.page)
  local pg = parse(str)
  if pg ~= nil and pg.page == ST.page then
    ST.str = str
    show(pg, true, true)
  else
    rebuild({})
  end
end

-- The panel is going (another section, a redraw, the menu closing): what a slider still holds goes out.
function Set.leave()
  flushAll()
  if ST.view ~= nil then ST.offsets[ST.page] = ST.view.offset end
  ST.panel, ST.host, ST.view, ST.info, ST.tabs, ST.arrows, ST.slots = nil, nil, nil, nil, nil, nil, {}
  ST.head = nil
  ST.rebuildNow = false
end

function Set.update(dt)
  local kit = ST.kit
  if kit == nil or ST.panel == nil then return end
  local now = kit.S.time
  flushDue(now)
  if ST.rebuildNow and ST.cur ~= nil and show(ST.cur, true) then ST.rebuildNow = false end
  if now < ST.pollAt then return end
  ST.pollAt = now + ((now < ST.fastUntil or ST.cur == nil) and 0.08 or 0.30)
  local str = fetch(ST.page)
  if str == nil or str == "" or str == ST.str then return end
  local pg = parse(str)
  if pg == nil or pg.page ~= ST.page then return end
  if show(pg, false) then ST.str = str end
end

-- The tooltip of what a pointer is on, in the box under the list.
function Set.hover(hover)
  if ST.info == nil then return end
  local tip = nil
  for b, _ in pairs(hover) do
    if b.tip ~= nil and b.tip ~= "" then
      tip = b.tip
      if not b.passive then break end
    end
  end
  if tip == ST.tip then return end
  ST.tip = tip
  local lines = { ST.kit.T.hint }
  if tip ~= nil then
    -- the panel's tooltips are broken into short lines for a mouse tooltip; a blank line is a paragraph
    local flat = tip:gsub("\n%s*\n", "\1"):gsub("%s*\n%s*", " "):gsub("\1", "\n")
    lines = wrap(flat, charsFor(ST.w - 2 * ST.kit.PAD - 56, FS_INFO))
    if #lines > 3 then lines = { lines[1], lines[2], lines[3] .. " …" } end
  end
  pcall(function()
    ST.info:SetText(table.concat(lines, "\n"))
    ST.info:SetTintColor(K.color(tip ~= nil and SOFT or K.GREY))
  end)
end

function Set.page(i)
  if ST.panel ~= nil then setPage(i) else ST.page = clamp(i, 0, #PAGES - 1) end
end

function Set.state()
  return {
    page = ST.page, seq = ST.cur and ST.cur.seq or nil, items = ST.cur and #ST.cur.items or 0, rows = #ST.slots,
    offset = ST.view and ST.view.offset or nil, max = ST.view and ST.view:maxOffset() or nil,
    queued = next(ST.queued) ~= nil, tip = ST.tip,
  }
end

-- The rows as they lie on the screen now: { kind, id, label, value, x, y, w, h } in screen canvas pixels.
function Set.rows()
  local out, v = {}, ST.view
  if v == nil then return out end
  for _, it in ipairs(ST.slots) do
    local f = it.f
    out[#out + 1] = { kind = f.kind, id = f.id, label = f.label, value = f.value, x = v.x, y = v.y + it.y - v.offset,
                      w = v.cw, h = it.h, built = it.canvas ~= nil }
  end
  return out
end

return Set
