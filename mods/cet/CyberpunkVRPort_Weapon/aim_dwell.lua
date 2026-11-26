-- Each physical hand earns its own dwell. Only mature observations are merged by NPC.
local M = {}
local stampModulus = 16777216 -- VRMeleeInputFrame publishes sample milliseconds modulo 2^24

function M.new()
    return {candidates = {}, active = {}}
end

function M.clear(s)
    local stopped = s.active
    s.candidates, s.active, s.last = {}, {}, nil
    s.stamp, s.lastSampleAt = nil, nil
    return stopped
end

function M.step(s, now, stamp, observations)
    local previousActive = s.active
    local active, stopped = {}, {}
    local continuous = s.last and now >= s.last and now - s.last <= .30
    local stale = s.lastSampleAt and now - s.lastSampleAt > .25
    if not stamp or (stamp == s.stamp and stale) then observations = {} end
    if stamp ~= s.stamp or not continuous then s.lastSampleAt = now end
    for side, hit in pairs(observations) do
        local prev = continuous and not stale and s.candidates[side]
        hit.since, hit.trackedMs = now, 0
        if prev and prev.weapon == hit.weapon and prev.slotSide == hit.slotSide
            and prev.id == hit.id and prev.origin == hit.origin then
            local elapsedMs = (stamp - s.stamp) % stampModulus
            if elapsedMs <= 250 then
                hit.since = prev.since
                hit.trackedMs = math.min(2000, prev.trackedMs + elapsedMs)
                hit.mature = prev.mature or (elapsedMs > 0 and now - hit.since >= 2 and hit.trackedMs >= 2000)
            end
        end
        if hit.mature then
            active[hit.id] = hit.target
        end
    end
    for id, target in pairs(previousActive) do
        if not active[id] then stopped[id] = target end
    end
    s.candidates, s.active, s.last, s.stamp = observations, active, now, stamp
    return active, stopped
end

return M
