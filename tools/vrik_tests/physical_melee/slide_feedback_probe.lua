-- Evaluated by the test fixture inside the script VM. Positions are relative
-- to the animated target; equal motion of target and blade leaves station=0.
return function()
    local function run(kind,hz)
        local pin=VrBladeSlotAttachment.new()
        local count,last=0,nil
        for i=0,hz do
            local t=i/hz
            local station,grip=.16*t,.16*t
            if kind=='co_moving' then station=0 end
            if kind=='target_only' then grip=0 end
            if kind=='blocked_then_idle' then
                grip=math.min(t,.30)*.2
                station=math.max(0,t-.55)*.2
            end
            if kind=='jitter' then station=math.sin(i)*.002;grip=math.cos(i)*.002 end
            if pin:SlideFeedback(Vector4.new(station,0,0,1),Vector4.new(grip,0,0,1),t) then
                assert(not last or t-last>=.074, 'slip pulses too close')
                count=count+1;last=t
            end
        end
        return count
    end
    local results={}
    for _,hz in ipairs({30,60,90,144}) do
        local rate={}
        for _,kind in ipairs({'slide','co_moving','target_only','blocked_then_idle','jitter'}) do
            local n=run(kind,hz)
            assert((kind=='slide' and n>=5 and n<=6) or (kind~='slide' and n==0), kind..' at '..hz..' Hz: '..n)
            rate[kind]=n
        end
        results[tostring(hz)]=rate
    end
    return results
end
