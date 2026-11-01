-- The menu's drawing kit: game ink widgets placed in canvas pixels, top-left anchored.
--
-- The menu's screen is alpha blended (the wrist's material with AdditiveAlphaBlend 0), so a dark fill
-- darkens the world behind it; the beam's is additive, where black is nothing.

local L = require("modules/locale")

local K = {}

K.FONT = "base\\gameplay\\gui\\fonts\\raj\\raj.inkfontfamily"
K.FONT_THIN = "base\\gameplay\\gui\\fonts\\blender\\blender.inkfontfamily"
-- Rajdhani and Blender have no glyphs for these scripts (the text came out empty), so their languages take the
-- game's own font for them (base\gameplay\gui\fonts\foreign): family, style of the regular text, of the thin.
-- Rajdhani draws far fewer letters in game than its font file maps: no ö ü ß, œ, ã õ, nothing of Latin
-- Extended-A (č ř ž ů ő ű ş ğ İ ź) and no ї є ґ, while Blender draws them all (measured in the menu,
-- 2026-10-26), so every Latin-script language and Ukrainian draws in Blender.
local FOREIGN = "base\\gameplay\\gui\\fonts\\foreign\\"
local BLENDER = { K.FONT_THIN, "Bold", "Book" }
K.FONTS = {
  ["de-de"] = BLENDER, ["fr-fr"] = BLENDER, ["es-es"] = BLENDER, ["es-mx"] = BLENDER, ["it-it"] = BLENDER,
  ["pt-br"] = BLENDER, ["pl-pl"] = BLENDER, ["cz-cz"] = BLENDER, ["hu-hu"] = BLENDER, ["tr-tr"] = BLENDER,
  ["ua-ua"] = BLENDER,
  ["jp-jp"] = { FOREIGN .. "japanese\\mgenplus\\mgenplus.inkfontfamily", "Medium", "Regular" },
  ["kr-kr"] = { FOREIGN .. "korean\\kbiz_go\\kbiz_go.inkfontfamily", "Medium", "Regular" },
  ["zh-cn"] = { FOREIGN .. "chinese\\jing_xi_heig\\jing_xi_heig.inkfontfamily", "Semi-Bold", "Regular" },
  ["zh-tw"] = { FOREIGN .. "chinese_traditional\\jing_xi_heig_b5\\jing_xi_heig_b5.inkfontfamily", "Semi-Bold", "Medium" },
  ["th-th"] = { FOREIGN .. "thai\\th_sarabun_new\\th_sarabun_new.inkfontfamily", "Semi-Bold", "Regular" },
  ["ar-ar"] = { FOREIGN .. "arabic\\ara_es_nawar\\ara_es_nawar.inkfontfamily", "Semi-Bold", "Regular" },
}

-- Holographic chrome: ice blue / white at rest, amber for focus. Red remains a warning color.
K.RED = { 1.0, 0.30, 0.26 }
K.CYAN = { 0.43, 0.86, 1.0 }
K.YELLOW = { 1.0, 0.69, 0.16 }
K.WHITE = { 0.94, 0.98, 1.0 }
K.GREY = { 0.43, 0.62, 0.71 }
K.BLACK = { 0.0, 0.0, 0.0 }
K.TEXT = { 0.83, 0.94, 1.0 }
K.ICE = { 0.42, 0.72, 0.84 }
K.NAVY = { 0.012, 0.012, 0.012 }
-- the cells' glass, one per state
K.DARKRED = { 0.30, 0.03, 0.04 }
K.DARKCYAN = { 0.02, 0.22, 0.26 }
K.DARKYELLOW = { 0.24, 0.21, 0.0 }
-- item quality, as the inventory colours it
K.QUALITY = { Common = K.GREY, Uncommon = { .25, .85, .53 }, Rare = { .25, .65, 1 }, Epic = { .75, .3, 1 },
  Legendary = { 1, .62, .16 } }

K.ATLAS = {
  phone = "base\\gameplay\\gui\\widgets\\phone\\new_phone_assets.inkatlas",
  phone_old = "base\\gameplay\\gui\\widgets\\phone\\atlas_phone.inkatlas",
  mappin = "base\\gameplay\\gui\\common\\icons\\mappin_icons.inkatlas",
  dpad = "base\\gameplay\\gui\\widgets\\dpad_hint\\dpad_hint.inkatlas",
  shapes = "base\\gameplay\\gui\\common\\shapes\\atlas_shapes_sync.inkatlas",
  common = "base\\gameplay\\gui\\common\\icons\\atlas_common.inkatlas",
}

function K.color(c, a)
  return HDRColor.new({ Red = c[1], Green = c[2], Blue = c[3], Alpha = a or 1.0 })
end

local function place(w, parent, x, y, sx, sy)
  w:SetAnchor(inkEAnchor.TopLeft)
  w:SetAnchorPoint(Vector2.new({ X = 0.0, Y = 0.0 }))
  w:SetMargin(inkMargin.new({ left = x, top = y, right = 0, bottom = 0 }))
  w:SetSize(Vector2.new({ X = sx, Y = sy }))
  w:Reparent(parent)
  return w
end

function K.move(w, x, y)
  w:SetMargin(inkMargin.new({ left = x, top = y, right = 0, bottom = 0 }))
end

function K.size(w, sx, sy)
  w:SetSize(Vector2.new({ X = sx, Y = sy }))
end

function K.canvas(parent, x, y, sx, sy)
  return place(inkCanvas.new(), parent, x, y, sx, sy)
end

function K.rect(parent, x, y, sx, sy, col, op)
  local r = place(inkRectangleWidget.new(), parent, x, y, sx, sy)
  r:SetTintColor(K.color(col))
  r:SetOpacity(op or 1.0)
  return r
end

function K.line(parent,x1,y1,x2,y2,col,op,width)
  local dx,dy=x2-x1,y2-y1
  local r=K.rect(parent,x1,y1,math.sqrt(dx*dx+dy*dy),width or 2,col,op)
  r:SetRenderTransformPivot(Vector2.new({X=0,Y=0}));r:SetRotation(math.deg(math.atan2(dy,dx)))
  return r
end

-- STATIC ORNAMENTS ARE KEPT between panel rebuilds. A click redraws its panel, and a 1160 px panel
-- frame alone is ~95 widgets (~2 ms of ink calls); a kept one is moved into the new panel instead
-- (place = five calls). Only inside K.poolBegin / K.poolEnd -- the menu's panel layer, which is
-- emptied before every build, so a kept widget is never still in use when it is taken again.
local pool,poolGen,pooling={},0,false
function K.poolBegin() poolGen=poolGen+1;pooling=true end
function K.poolEnd() pooling=false end
function K.poolFlush() pool={} end
-- A canvas at (x, y) of w x h holding what build(canvas) drew in its own pixels; build returns the
-- parts the caller gets back (a kept one returns the same parts).
local function kept(key,parent,x,y,w,h,build)
  if not pooling then
    local c=K.canvas(parent,x,y,w,h)
    return build(c),false
  end
  local list=pool[key]
  if not list then list={};pool[key]=list end
  for _,e in ipairs(list) do
    if e.gen~=poolGen then
      e.gen=poolGen
      place(e.canvas,parent,x,y,w,h)
      return e.parts,true
    end
  end
  local e={gen=poolGen,canvas=K.canvas(parent,x,y,w,h)}
  e.parts=build(e.canvas)
  list[#list+1]=e
  return e.parts,false
end

-- For a view's own large static drawing (the perk tree): kept like the ornaments above, keyed by
-- everything it shows. K.poolDrop forgets a key the view will not draw again.
function K.kept(key,parent,x,y,w,h,build) return kept(key,parent,x,y,w,h,build) end
function K.poolDrop(key) pool[key]=nil end

local function cutLines(parent,x,y,w,h,col,op,cut,width)
  local points={{x+cut,y},{x+w-cut,y},{x+w,y+cut},{x+w,y+h-cut},
    {x+w-cut,y+h},{x+cut,y+h},{x,y+h-cut},{x,y+cut}}
  local out={}
  for i,a in ipairs(points) do local b=points[i%8+1];out[#out+1]=K.line(parent,a[1],a[2],b[1],b[2],col,op,width or 2) end
  return out
end

function K.cutFrame(parent,x,y,w,h,col,op,cut,width)
  cut=math.min(cut or 12,w*.2,h*.35)
  local key=string.format("cut:%g:%g:%g:%g",w,h,cut,width or 2)
  local parts,reused=kept(key,parent,x,y,w,h,function(c) return cutLines(c,0,0,w,h,col,op,cut,width) end)
  if reused then K.tint(parts,col,op or 1.0) end
  return parts
end

-- True transparent ring interiors; no opaque black disk is needed to cut out the middle.
function K.ring(parent,cx,cy,r,width,col,op,segments,gaps)
  local out={};segments=segments or 32
  for i=0,segments-1 do if not gaps or i%gaps~=gaps-1 then
    local a,b=i*2*math.pi/segments,(i+1)*2*math.pi/segments
    out[#out+1]=K.line(parent,cx+math.cos(a)*r,cy+math.sin(a)*r,cx+math.cos(b)*r,cy+math.sin(b)*r,col,op,width)
  end end
  return out
end

-- Static panel ornament, batched as ink rectangles. No video, puppet or per-frame animation.
local holoDraw
function K.holoPanel(parent,w,h)
  kept(string.format("holo:%g:%g",w,h),parent,0,0,w,h,function(c) holoDraw(c,w,h) end)
end
function holoDraw(parent,w,h)
  local cut=h<100 and 10 or 24
  K.roundFill(parent,10,10,w-20,h-20,K.NAVY,.18,4)
  if h>=180 then
    for y=32,h-28,math.max(18,math.ceil(h/64)) do K.rect(parent,24,y,w-48,1,K.CYAN,.025) end
  end
  K.cutFrame(parent,2,2,w-4,h-4,K.CYAN,.07,cut,6)
  K.cutFrame(parent,2,2,w-4,h-4,K.CYAN,.72,cut,2)
  K.cutFrame(parent,10,10,w-20,h-20,K.CYAN,.17,math.max(6,cut-6),1)
  if h>=180 then
    K.line(parent,2,cut+2,cut+2,2,K.CYAN,.95,5)
    K.line(parent,w-cut-2,h-2,w-2,h-cut-2,K.CYAN,.95,5)
    K.rect(parent,cut+12,2,90,3,K.CYAN,.9)
    K.rect(parent,w-cut-102,h-4,90,3,K.CYAN,.9)
    for i=0,2 do
      K.rect(parent,w-58,30+i*8,28-i*7,2,K.CYAN,.3)
      K.rect(parent,22,h-54+i*8,22+i*5,2,K.CYAN,.25)
    end
  end
end

-- A border of four bars, inside the box.
function K.frame(parent, x, y, sx, sy, t, col, op)
  return {
    K.rect(parent, x, y, sx, t, col, op),
    K.rect(parent, x, y + sy - t, sx, t, col, op),
    K.rect(parent, x, y + t, t, sy - 2 * t, col, op),
    K.rect(parent, x + sx - t, y + t, t, sy - 2 * t, col, op),
  }
end

function K.tint(list, col, op)
  for _, w in ipairs(list) do
    w:SetTintColor(K.color(col))
    if op ~= nil then w:SetOpacity(op) end
  end
end

-- A bar turned 45 degrees: the cut corner the game's panels carry.
function K.slash(parent, cx, cy, len, t, col, op)
  local r = place(inkRectangleWidget.new(), parent, cx - len / 2, cy - t / 2, len, t)
  r:SetTintColor(K.color(col))
  r:SetOpacity(op or 1.0)
  pcall(function() r:SetRotation(-45.0) end)
  return r
end

local ALIGN = { left = "Left", center = "Center", right = "Right" }
local PIN = { left = 0.0, center = 0.5, right = 1.0 }

-- Text in the box (x, y, sx, sy), aligned in it. A text widget sizes itself to its text whatever size
-- it is given (measured: a 400 px box with vertical Center put the line at the box's top), so the
-- alignment is done by pinning the widget's own anchor point to the box: its middle to the box's
-- middle line, its left / centre / right to the box's.
function K.text(parent, x, y, sx, sy, str, size, col, align, thin)
  local ax = PIN[align or "left"]
  local t = inkTextWidget.new()
  t:SetAnchor(inkEAnchor.TopLeft)
  t:SetAnchorPoint(Vector2.new({ X = ax, Y = 0.5 }))
  t:SetMargin(inkMargin.new({ left = x + sx * ax, top = y + sy / 2, right = 0, bottom = 0 }))
  t:Reparent(parent)
  local foreign = K.FONTS[L.code]
  if foreign ~= nil then
    pcall(function() t:SetFontFamily(foreign[1], thin and foreign[3] or foreign[2]) end)
  elseif thin then
    pcall(function() t:SetFontFamily(K.FONT_THIN, "Book") end)
  else
    pcall(function() t:SetFontFamily(K.FONT, "Medium") end)
  end
  t:SetFontSize(size)
  pcall(function() t:SetHorizontalAlignment(textHorizontalAlignment[ALIGN[align or "left"]]) end)
  pcall(function() t:SetVerticalAlignment(textVerticalAlignment.Center) end)
  pcall(function() t.scrollTextSpeed = 0.0 end)
  t:SetText(str or "")
  t:SetTintColor(K.color(col))
  return t
end

-- An atlas sprite. Untinted is white: a tint multiplies the sprite's own colours.
function K.image(parent, x, y, sx, sy, atlas, part, col, op)
  local q = place(inkImageWidget.new(), parent, x, y, sx, sy)
  pcall(function()
    if type(atlas) == "string" then atlas = ResRef.FromString(atlas) end
    q:SetAtlasResource(atlas)
  end)
  pcall(function() q:SetTexturePart(type(part) == "string" and CName.new(part) or part) end)
  q:SetTintColor(K.color(col or K.WHITE))
  q:SetOpacity(op or 1.0)
  return q
end

-- An atlas part stretched over the box NINE-SLICED: its corners and edges keep their pixels (the atlas
-- carries where to cut -- atlas_common frame_bg / frame_fg are a rounded box, atlas_shapes_sync
-- label_frame / label_frame_line a pill).
function K.slice(parent, x, y, sx, sy, atlas, part, col, op)
  local q = K.image(parent, x, y, sx, sy, atlas, part, col, op)
  pcall(function() q.useNineSliceScale = true end)
  return q
end

-- THE GLASS SHAPES: atlas_common's rounded box (frame_bg filled, frame_fg its outline), nine-sliced and
-- drawn at 1/k of the box then scaled k times about its top-left, so the corner (12 atlas px) becomes
-- 12 k: r is the radius wanted. The outline's stroke grows with it.
local ROUND = 12

local function roundK(r) return math.max((r or ROUND) / ROUND, 0.5) end

local function rounded(parent, x, y, sx, sy, part, col, op, r)
  local k = roundK(r)
  local q = K.slice(parent, x, y, sx / k, sy / k, K.ATLAS.common, part, col, op)
  pcall(function() q:SetRenderTransformPivot(Vector2.new({ X = 0.0, Y = 0.0 })) end)
  pcall(function() q:SetScale(Vector2.new({ X = k, Y = k })) end)
  return q
end

-- A rounded fill: an image, tinted and dimmed like a rect.
function K.roundFill(parent, x, y, sx, sy, col, op, r)
  return rounded(parent, x, y, sx, sy, "frame_bg", col, op, r)
end

-- A rounded outline, as a list (what K.tint takes, like K.frame's bars).
function K.roundEdge(parent, x, y, sx, sy, col, op, r)
  return { rounded(parent, x, y, sx, sy, "frame_fg", col, op, r) }
end

-- K.size for a rounded shape made with radius r.
function K.roundSize(q, sx, sy, r)
  local k = roundK(r)
  K.size(q, sx / k, sy / k)
end

-- At most n characters of a UTF-8 string, with an ellipsis when cut.
function K.clip(s, n)
  if s == nil then return "" end
  local out, count = {}, 0
  for ch in string.gmatch(s, "[%z\1-\127\194-\244][\128-\191]*") do
    count = count + 1
    if count > n then
      out[#out] = "…"
      break
    end
    out[#out + 1] = ch
  end
  return table.concat(out)
end

-- Upper case beyond ASCII (string.upper only knows ASCII): Latin-1 and Latin Extended-A (the menu's European
-- languages: ä é ł ő ř ş ...) and Cyrillic with Ukrainian's ґ. Scripts without case pass through.
local function upperCp(cp)
  if cp >= 0xE0 and cp <= 0xFE and cp ~= 0xF7 then return cp - 32 end
  if cp == 0xFF then return 0x178 end
  if cp == 0x131 then return 0x49 end
  if (cp >= 0x100 and cp <= 0x137) or (cp >= 0x14A and cp <= 0x177) then return cp % 2 == 1 and cp - 1 or cp end
  if (cp >= 0x139 and cp <= 0x148) or (cp >= 0x179 and cp <= 0x17E) then return cp % 2 == 0 and cp - 1 or cp end
  if cp >= 0x430 and cp <= 0x44F then return cp - 32 end
  if cp >= 0x450 and cp <= 0x45F then return cp - 80 end
  if cp == 0x491 then return 0x490 end
  return cp
end

function K.upper(s)
  if s == nil then return "" end
  s = string.upper(s)
  return (string.gsub(s, "[\195-\197\208-\210][\128-\191]", function(ch)
    local a, b = string.byte(ch, 1, 2)
    local cp = (a - 0xC0) * 64 + (b - 0x80)
    local up = upperCp(cp)
    if up == cp then return ch end
    if up < 0x80 then return string.char(up) end
    return string.char(0xC0 + math.floor(up / 64), 0x80 + up % 64)
  end))
end

-- THE POINTER BEAM's strip (vr_menu_beam.ent: BEAM.cw x BEAM.ch px over BEAM.len m, the canvas's top at the
-- hand): a glow and a core that are not there for the first BEAM.hide m from the hand and come in over the
-- next BEAM.fade m in BEAM.steps rects of rising opacity, then run whole to where the ray lands -- the hand
-- does not hold a bar of light, the light starts out in front of it.
K.BEAM = { len = 1.5, cw = 16, ch = 2048, hide = 0.12, fade = 0.12, steps = 6 }

function K.beamBuild(root, glowCol, glowOp, coreCol, coreOp)
  local B = K.BEAM
  local parts = { pieces = {}, px = -1 }
  for k = 1, B.steps + 1 do
    local f = k <= B.steps and k / (B.steps + 1) or 1
    parts.pieces[#parts.pieces + 1] = { k = k, rect = K.rect(root, 0, 0, B.cw, 0, glowCol, glowOp * f), x = 0, w = B.cw }
    parts.pieces[#parts.pieces + 1] = { k = k, rect = K.rect(root, (B.cw - 6) / 2, 0, 6, 0, coreCol, coreOp * f),
      x = (B.cw - 6) / 2, w = 6 }
  end
  return parts
end

-- Laid out for a ray `length` m long (nothing to do while it stays within a pixel).
function K.beamLength(parts, length)
  local B = K.BEAM
  local perM = B.ch / B.len
  local px = math.max(0, math.min(B.len, length)) * perM
  if math.abs(px - parts.px) < 1 then return end
  parts.px = px
  local hide, step = B.hide * perM, B.fade * perM / B.steps
  for _, p in ipairs(parts.pieces) do
    local top = hide + (p.k - 1) * step
    local bottom = p.k <= B.steps and top + step or px
    local h = math.max(0, math.min(bottom, px) - top)
    K.move(p.rect, p.x, top)
    K.size(p.rect, p.w, h)
  end
end

function K.beamShow(parts, on)
  for _, p in ipairs(parts.pieces) do p.rect:SetVisible(on) end
end

return K
