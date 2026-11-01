-- Drawing on a lobby screen's canvas (the game's ink, in canvas pixels, top left = 0, 0): the few widget kinds the
-- holographic menu is made of. Colours are { r, g, b } in the game's HDR units.
local M = {}

M.FONT = "base\\gameplay\\gui\\fonts\\raj\\raj.inkfontfamily"
-- Rajdhani draws English and Russian; the other languages take the font the VR menu found draws them
-- (mods/cet/CyberpunkVRPort_Menu modules/ink.lua K.FONTS): Blender for every Latin script and Ukrainian, the game's
-- own font for the rest. Family and style.
local FOREIGN = "base\\gameplay\\gui\\fonts\\foreign\\"
local BLENDER = { "base\\gameplay\\gui\\fonts\\blender\\blender.inkfontfamily", "Bold" }
local FONTS = {
  ["de-de"] = BLENDER, ["fr-fr"] = BLENDER, ["es-es"] = BLENDER, ["es-mx"] = BLENDER, ["it-it"] = BLENDER,
  ["pt-br"] = BLENDER, ["pl-pl"] = BLENDER, ["cz-cz"] = BLENDER, ["hu-hu"] = BLENDER, ["tr-tr"] = BLENDER,
  ["ua-ua"] = BLENDER,
  ["jp-jp"] = { FOREIGN .. "japanese\\mgenplus\\mgenplus.inkfontfamily", "Medium" },
  ["kr-kr"] = { FOREIGN .. "korean\\kbiz_go\\kbiz_go.inkfontfamily", "Medium" },
  ["zh-cn"] = { FOREIGN .. "chinese\\jing_xi_heig\\jing_xi_heig.inkfontfamily", "Semi-Bold" },
  ["zh-tw"] = { FOREIGN .. "chinese_traditional\\jing_xi_heig_b5\\jing_xi_heig_b5.inkfontfamily", "Semi-Bold" },
  ["th-th"] = { FOREIGN .. "thai\\th_sarabun_new\\th_sarabun_new.inkfontfamily", "Semi-Bold" },
  ["ar-ar"] = { FOREIGN .. "arabic\\ara_es_nawar\\ara_es_nawar.inkfontfamily", "Semi-Bold" },
}

-- The game's on-screen language, read again at most once a second (a page draws dozens of texts).
local lang, langRead = "en-us", nil
local function language()
  local now = os.clock()
  if langRead ~= nil and now - langRead < 1.0 then return lang end
  langRead = now
  pcall(function()
    local ss = Game.GetSettingsSystem()
    if ss:HasGroup("/language") and ss:GetGroup("/language"):HasVar(CName.new("OnScreen")) then
      lang = NameToString(ss:GetVar("/language", CName.new("OnScreen")):GetValue())
    end
  end)
  return lang
end

function M.color(c, a) return HDRColor.new({ Red = c[1], Green = c[2], Blue = c[3], Alpha = a or 1.0 }) end

function M.place(w, parent, x, y, sx, sy)
  w:SetAnchor(inkEAnchor.TopLeft)
  w:SetAnchorPoint(Vector2.new({ X = 0.0, Y = 0.0 }))
  w:SetMargin(inkMargin.new({ left = x, top = y, right = 0, bottom = 0 }))
  w:SetSize(Vector2.new({ X = sx, Y = sy }))
  w:Reparent(parent)
  return w
end

function M.canvas(parent, x, y, sx, sy) return M.place(inkCanvas.new(), parent, x, y, sx, sy) end

function M.rect(parent, x, y, sx, sy, col, op)
  local r = M.place(inkRectangleWidget.new(), parent, x, y, sx, sy)
  r:SetTintColor(M.color(col))
  r:SetOpacity(op or 1.0)
  return r
end

-- A frame of thin lines round a box.
function M.outline(parent, x, y, w, h, col, op, t)
  M.rect(parent, x, y, w, t, col, op)
  M.rect(parent, x, y + h - t, w, t, col, op)
  M.rect(parent, x, y, t, h, col, op)
  M.rect(parent, x + w - t, y, t, h, col, op)
end

-- The four corners of a box as brackets, arms `len` long.
function M.brackets(parent, x, y, w, h, len, col, op, t)
  for _, c in ipairs({ { x, y, 1, 1 }, { x + w, y, -1, 1 }, { x, y + h, 1, -1 }, { x + w, y + h, -1, -1 } }) do
    local cx, cy, sx, sy = c[1], c[2], c[3], c[4]
    M.rect(parent, sx > 0 and cx or cx - len, sy > 0 and cy or cy - t, len, t, col, op)
    M.rect(parent, sx > 0 and cx or cx - t, sy > 0 and cy or cy - len, t, len, col, op)
  end
end

-- align: "left", "right" or "centre"; the text is upper case unless `plain`.
function M.text(parent, x, y, w, h, str, size, col, align, plain, style)
  local t = inkTextWidget.new()
  local ax = align == "centre" and 0.5 or (align == "right" and 1.0 or 0.0)
  t:SetAnchor(inkEAnchor.TopLeft)
  t:SetAnchorPoint(Vector2.new({ X = ax, Y = 0.5 }))
  t:SetMargin(inkMargin.new({ left = x + w * ax, top = y + h / 2, right = 0, bottom = 0 }))
  t:Reparent(parent)
  local font = FONTS[language()]
  if font ~= nil then
    pcall(function() t:SetFontFamily(font[1], font[2]) end)
  else
    pcall(function() t:SetFontFamily(M.FONT, style or "Semi-Bold") end)
  end
  t:SetFontSize(size)
  if not plain then pcall(function() t:SetLetterCase(textLetterCase.UpperCase) end) end
  pcall(function()
    t:SetHorizontalAlignment(align == "centre" and textHorizontalAlignment.Center or
                             (align == "right" and textHorizontalAlignment.Right or textHorizontalAlignment.Left))
  end)
  pcall(function() t:SetVerticalAlignment(textVerticalAlignment.Center) end)
  t:SetText(str or "")
  t:SetTintColor(M.color(col))
  return t
end

function M.image(parent, x, y, sx, sy, atlas, part, col, op)
  local q = M.place(inkImageWidget.new(), parent, x, y, sx, sy)
  pcall(function() q:SetAtlasResource(ResRef.FromString(atlas)) end)
  pcall(function() q:SetTexturePart(CName.new(part)) end)
  q:SetTintColor(M.color(col))
  q:SetOpacity(op or 1.0)
  return q
end

-- The game's language code (en-us, ru-ru ...): what a page's localised art is chosen by.
function M.language() return language() end

-- A bar from one point to another, `width` thick (the VR menu's K.line).
function M.line(parent, x1, y1, x2, y2, col, op, width)
  local dx, dy = x2 - x1, y2 - y1
  local r = M.rect(parent, x1, y1, math.sqrt(dx * dx + dy * dy), width or 2, col, op)
  pcall(function()
    r:SetRenderTransformPivot(Vector2.new({ X = 0, Y = 0 }))
    r:SetRotation(math.deg(math.atan2(dy, dx)))
  end)
  return r
end

-- A ring of `segments` bars round cx, cy, every `gaps`-th left out (the VR menu's K.ring: its pointer's mark).
function M.ring(parent, cx, cy, r, width, col, op, segments, gaps)
  local out = {}
  segments = segments or 32
  for i = 0, segments - 1 do
    if not gaps or i % gaps ~= gaps - 1 then
      local a, b = i * 2 * math.pi / segments, (i + 1) * 2 * math.pi / segments
      out[#out + 1] = M.line(parent, cx + math.cos(a) * r, cy + math.sin(a) * r, cx + math.cos(b) * r,
                             cy + math.sin(b) * r, col, op, width)
    end
  end
  return out
end

-- An arc of a ring round cx, cy from angle a to b (radians; 0 points right, positive turns down the canvas), drawn as
-- `segments` bars.
function M.arc(parent, cx, cy, r, a, b, width, col, op, segments)
  local out = {}
  segments = segments or 8
  for i = 0, segments - 1 do
    local p, q = a + (b - a) * i / segments, a + (b - a) * (i + 1) / segments
    out[#out + 1] = M.line(parent, cx + math.cos(p) * r, cy + math.sin(p) * r, cx + math.cos(q) * r,
                           cy + math.sin(q) * r, col, op, width)
  end
  return out
end

-- A loudspeaker in the box (x, y, size square), facing right: its body and its cone, and `waves` arcs before it --
-- the sound it gives (0: none). The game's atlases have no loudspeaker. Every piece, for tinting.
function M.speaker(parent, x, y, size, waves, col, op)
  local s, out = size / 100, {}
  out[#out + 1] = M.rect(parent, x + 6 * s, y + 37 * s, 18 * s, 26 * s, col, op)
  -- the cone: columns from the body's height out to its mouth
  local steps, x0, w = 12, x + 24 * s, 22 * s
  for i = 0, steps - 1 do
    local h = (26 + (i + 1) / steps * 44) * s
    out[#out + 1] = M.rect(parent, x0 + i * w / steps, y + 50 * s - h / 2, w / steps + 1, h, col, op)
  end
  for k = 1, waves or 0 do
    for _, bar in ipairs(M.arc(parent, x + 48 * s, y + 50 * s, (12 + 11 * k) * s, -0.75, 0.75, 6 * s, col, op, 8)) do
      out[#out + 1] = bar
    end
  end
  return out
end

-- The game's screen frame: a box with its top left and bottom right corners cut off at 45 degrees; its lines.
function M.cutFrame(parent, x, y, w, h, cut, col, op, width)
  local pts = { { x + cut, y }, { x + w, y }, { x + w, y + h - cut }, { x + w - cut, y + h }, { x, y + h },
                { x, y + cut } }
  local out = {}
  for i, a in ipairs(pts) do
    local b = pts[i % #pts + 1]
    out[i] = M.line(parent, a[1], a[2], b[1], b[2], col, op, width)
  end
  return out
end

-- The font size, at most `size` and at least `least`, that fits `str` into `width` px (a character ~0.55 of the size).
function M.fit(str, width, size, least)
  local n = 0
  for _ in string.gmatch(str or "", "[%z\1-\127\194-\244][\128-\191]*") do n = n + 1 end
  if n == 0 then return size end
  return math.max(least, math.min(size, math.floor(width / (n * 0.55))))
end

-- A square turned on its corner, centred on x, y: the menu's marker.
function M.diamond(parent, x, y, size, col, op)
  local r = M.rect(parent, x - size / 2, y - size / 2, size, size, col, op)
  pcall(function()
    r:SetRenderTransformPivot(Vector2.new({ X = 0.5, Y = 0.5 }))
    r:SetRotation(45)
  end)
  return r
end

return M
