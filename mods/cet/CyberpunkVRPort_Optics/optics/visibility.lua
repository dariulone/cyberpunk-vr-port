local M = {}

-- Both ADS tracks contain physical geometry. The generated reticle panels
-- are selected separately after the optical path has been chosen.
function M.enabled(name, gate)
    if gate == 'visibleWhenAiming' or gate == 'invisibleWhenAiming' then return true end
end

local shaderHousings = {
    w_att__scope_long_01_5273='long_01',
    w_att__scope_long_02_6688='long_02',
}

function M.housing(name)
    local key=shaderHousings[name]
    if not key then return nil end
    local stem='w_att__scope_'..key
    return 'base\\weapons\\firearms\\attachments\\scope\\'..stem..'\\entities\\meshes\\'..stem..'_01_cap.mesh',
        'base\\vrport\\optics\\'..key..'_housing.mesh'
end
return M
