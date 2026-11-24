-- CyberFinger's braced hip holster. Distances are to the virtual scabbard in metres.
-- This state belongs to one item. The caller drops it on tracking loss or a change of scene.
local M = {}

function M.isHipKatana(copy)
  return copy.typeName == "Wea_Katana" and (copy.place.name == "hip_l" or copy.place.name == "hip_r")
end

function M.returnStep(brace, now, canStow, supportClosed, swordClosed, gripEdge, distance, atHip)
  -- A sword-hand release commits immediately. Keep the previous short grace
  -- period for players who happen to open the scabbard hand first.
  if brace.releaseAt and (now - brace.releaseAt > 0.6 or supportClosed) then
    return "cancel"
  end
  if not canStow or not atHip or distance > 0.17 then
    if brace.docked then return "cancel" end
    brace.approaching = canStow and swordClosed and (brace.approaching or distance > 0.17)
  elseif supportClosed and swordClosed and distance <= 0.13 and (gripEdge or brace.approaching) then
    brace.docked = true
  end
  if brace.docked then
    if not swordClosed then return "stow" end
    if not supportClosed then brace.releaseAt = brace.releaseAt or now end
  elseif not supportClosed then
    return "cancel"
  end
  return "hold"
end

return M
