local clip = require("grenade/poses/ring_flick")
local M = { duration = clip.keys[#clip.keys][1] }


local function ease(t, a, b)
    local u = math.min(1, math.max(0, (t-a)/(b-a)))
    return u*u*(3-2*u)
end

function M.fingerKeys(t)
    local keys = clip.keys
    for n = 2, #keys do
        if t <= keys[n][1] then
            return keys[n-1], keys[n], ease(t, keys[n-1][1], keys[n][1])
        end
    end
    return keys[#keys], keys[#keys], 0
end

function M.finger(name, x, y, z, w, a, b, u)
    local n = clip.joints[name]
    if not n then return x, y, z, w end
    local i = a[n] + (b[n]-a[n])*u
    local j = a[n+1] + (b[n+1]-a[n+1])*u
    local k = a[n+2] + (b[n+2]-a[n+2])*u
    local r = a[n+3] + (b[n+3]-a[n+3])*u
    local norm = math.sqrt(i*i+j*j+k*k+r*r)
    return (w*i+x*r+y*k-z*j)/norm, (w*j+y*r+z*i-x*k)/norm,
           (w*k+z*r+x*j-y*i)/norm, (w*r-x*i-y*j-z*k)/norm
end


return M
