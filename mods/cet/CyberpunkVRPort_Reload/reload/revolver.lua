-- THE REVOLVER, on its own.
--
-- `reload.lua` is built around a gun with a slide and a box magazine, and by the twelfth pistol it knew that shape
-- very well. A revolver is not it: nothing slides, the cylinder swings out on a crane and turns about the bore, the
-- hammer is cocked and stays cocked, and what leaves the gun is six loose cases rather than one magazine. Piling
-- that into a 3400-line file made both harder to read, so it lives here.
--
-- Everything is driven from `M.frame(ctx)`, once per frame, with `ctx` carrying the handful of things the parent
-- already has: the weapon, its config, dt, which hand is which, and the pose helpers. Nothing here reaches back.
local M = {}

local hamS  = { p = 0.0, want = 0.0, prev = false, wrote = false, seq = nil, key = nil, sounded = false, last = nil }
local spinS = { ang = 0.0, ref = nil, hand = nil, wrote = false, det = nil }
local caseS = { list = nil }

local function len3(x, y, z) return math.sqrt(x * x + y * y + z * z) end

local function qmul(ai, aj, ak, ar, bi, bj, bk, br)
    return ar * bi + ai * br + aj * bk - ak * bj,
           ar * bj - ai * bk + aj * br + ak * bi,
           ar * bk + ai * bj - aj * bi + ak * br,
           ar * br - ai * bi - aj * bj - ak * bk
end

local function qrot(i, j, k, r, x, y, z)
    local tx = 2.0 * (j * z - k * y)
    local ty = 2.0 * (k * x - i * z)
    local tz = 2.0 * (i * y - j * x)
    return x + r * tx + (j * tz - k * ty),
           y + r * ty + (k * tx - i * tz),
           z + r * tz + (i * ty - j * tx)
end

-- WHEN A CASE LEAVES ITS CHAMBER, from friction rather than from taste.
--
-- A case in a chamber is held by nothing but its own weight and the wall. Tip the cylinder and gravity splits into
-- a component ALONG the chamber, which pulls the case out, and one ACROSS it, which presses the case to the wall
-- and is all the friction has to work with. With the axis at `theta` from vertical those are `g*cos(theta)` and
-- `g*sin(theta)`, so it slides when
--
--     cos(theta) > mu * sin(theta)      i.e.   theta < atan(1 / mu)
--
-- Brass on oiled steel is mu ~ 0.2, dirty ~ 0.3; at 0.25 that is 76 deg from vertical, so the bore has to come up
-- about 14 deg above level before the first case moves. That is the number this returns, as a cosine to compare
-- against the bore's own vertical component -- no hand-picked threshold anywhere.
function M.tiltThreshold(mu)
    return math.cos(math.atan(1.0 / (mu or 0.25)))
end

-- ------------------------------------------------------------------ SPENT CASES IN FLIGHT
--
-- Each leaves its OWN chamber, along the cylinder's axis, a moment after the one before -- six leaving as a ring is
-- the one thing a revolver never does. After that each is ballistic and tumbling on its own, and gone when its time
-- is up: a case is 30 mm of brass and nobody watches where it lands.
--
-- Their own list rather than the magazine's falling solver, which carries ONE body with contacts, resting and a
-- settle -- everything a magazine needs and nothing a case does.
-- `live` is how many of the chambers still hold an UNFIRED round, and it decides what is spawned in each: a live
-- round is a case with a bullet in it, a spent one is the same case without. They are two entities, because they
-- are two objects -- the game's own rig says as much, with a `bulletUsed01..06` track per chamber whose whole job
-- is to take the bullet off a case that has been fired.
--
-- The live ones fill the LAST chambers and the spent ones the first, so the two are contiguous the way a
-- part-fired cylinder really is -- a revolver indexes round in order, it does not fire at random.
function M.eject(ctx, cyl, live)
    local ec = ctx.cfg.eject
    if not ec or not ctx.spawner then return end
    local n = ec.count or 6
    live = live or 0
    if live > n then live = n end
    local list = {}
    for i = 1, n do
        local isLive = (i > n - live)
        -- the chambers are evenly spaced about the axis, and the mesh of the round we spawn already carries the
        -- FIRST chamber's offset in its vertices, so the spawn point is corrected by that one
        local a = (i - 1) * (2.0 * math.pi / n) + math.rad(ec.first or 0.0)
        local rr = ec.radius or 0.0134
        -- ...and along the axis, where the ROUNDS SIT. The game draws them at `bullet_NN`, whose rig hangs off the
        -- weapon's `mag_slot` -- and that bone is 23.9 mm from the cylinder's centre, on its REAR FACE. The slot we
        -- read is the cylinder itself, so without this the cases are seated from the middle of the cylinder and sit
        -- a whole half-length too deep: "как только наши появляются, они глубже".
        local lx, ly = math.cos(a) * rr, math.sin(a) * rr
        local lz = ec.seatZ or 0.0
        local ox, oy, oz = qrot(cyl.qi, cyl.qj, cyl.qk, cyl.qr, lx, ly, lz)
        -- THE MESH IS NOT BUILT ON THE CHAMBER'S AXIS. A case's mesh runs along its own Y -- 35 mm of it, against
        -- 11 across -- while the chamber runs along Z, so an entity spawned with the cylinder's own orientation
        -- lies ACROSS the bore: "гильза смотрит в бок ствола". `meshRot` is the turn between the two, and it also
        -- has to carry the offset, since `meshAt` is read in the mesh's frame like everything else about it.
        local mr = ec.meshRot or { 0, 0, 0, 1 }
        local s1, s2, s3, s4 = qmul(cyl.qi, cyl.qj, cyl.qk, cyl.qr, mr[1], mr[2], mr[3], mr[4])
        local ma = ec.meshAt or { 0, 0, 0 }
        local mx, my, mz = qrot(s1, s2, s3, s4, ma[1], ma[2], ma[3])
        local px, py, pz = cyl.x + ox - mx, cyl.y + oy - my, cyl.z + oz - mz
        local id = nil
        local ok = pcall(function()
            local tr = WorldTransform.new()
            tr:SetPosition(Vector4.new(px, py, pz, 1.0))
            tr:SetOrientation(Quaternion.new(s1, s2, s3, s4))
            id = ctx.spawner.Spawn((isLive and ec.liveEntity) or ec.entity, tr, '')
        end)
        if not (ok and id) then break end
        -- IT DOES NOT FALL, IT SLIDES OUT FIRST. A case in a chamber cannot go anywhere but along that chamber,
        -- and until it clears the mouth gravity is only allowed to act ALONG the axis, less what friction takes:
        --
        --     a = g * (cos(theta) - mu * sin(theta))
        --
        -- with theta the axis against vertical. So a gun held barely past the threshold gives them up slowly and
        -- one held straight up spits them, which is the whole behaviour a revolver has here and what "они должны
        -- скатываться, а сейчас падают сразу" was asking for. It goes ballistic at the moment it clears.
        --
        -- The direction is the chamber's, and it is -Z: `mag_slot` sits on the cylinder's -Z face, so that is the
        -- end the rounds go in and the end the cases come out. Taken as +Z they left forwards, through the frame.
        -- WHICH END IT LEAVES BY IS NOT A GUESS. A cylinder swung out of the frame is open at BOTH ends, and a
        -- case falls out of whichever one is lower -- so gravity picks the direction and no sign has to be got
        -- right in a config. Two attempts at naming it by hand were both wrong, and each read the same way: the
        -- acceleration came out negative, the clamp held it at zero, and the cases hung where they were.
        local axc = ec.axis or { 0, 0, 1 }
        local ax, ay, az2 = qrot(cyl.qi, cyl.qj, cyl.qk, cyl.qr, axc[1], axc[2], axc[3])
        if az2 > 0.0 then ax, ay, az2 = -ax, -ay, -az2 end     -- take the end that points DOWN
        local sp = ec.spread or 0.12
        list[#list + 1] = {
            id = id,
            p = { px, py, pz },
            -- starts at REST in its chamber, as it has been sitting there
            v = { 0.0, 0.0, 0.0 },
            axis = { ax, ay, az2 },
            slide = 0.0,
            depth = ec.chamber or 0.048,      -- how far it has to travel before it is clear of the cylinder
            radial = { (ox / rr) * sp, (oy / rr) * sp, (oz / rr) * sp },
            mu = ec.mu or 0.25,
            exitSpeed = ec.minExitSpeed or 0.0,
            releaseTilt = M.tiltThreshold(ctx.cfg.dropMu),
            bias = ec.bias or 0.35,
            live = isLive,
            q = { s1, s2, s3, s4 },
            -- WHICH CHAMBER IT IS IN, kept so the axis can be re-read from the live cylinder every frame while it
            -- is still inside. A case in a chamber is part of the gun: open the cylinder level and then tip it and
            -- it must start moving THEN, which a direction latched at spawn can never do.
            slot = ec.slot or 'vrp_cylinder',
            localAxis = axc,
            -- WHERE IN THE CYLINDER IT IS, kept so its world place can be rebuilt from the live cylinder every
            -- frame while it is still inside. Holding a spawned POSITION instead leaves it where the gun was when
            -- it was spawned -- and since an entity takes a frame or two to resolve, that is a frame or two of
            -- staleness the eye catches at once: "я уже наклонил, а пули заспавнились на предыдущем кадре".
            chamber = { lx, ly, lz },
            meshAt = { ma[1], ma[2], ma[3] },
            mrot = { mr[1], mr[2], mr[3], mr[4] },
            -- IT LEAVES WITH NO SPIN AT ALL. Nothing in free flight can twist it, so whatever it turns at is what
            -- it EARNED on the way out -- see the overhang below. Starting it at a fixed rate was the "они как-то
            -- поворачиваются" -- a case flipping the instant it cleared, for no reason the eye could credit.
            wa = { 0.0, 0.0, 1.0 },
            w = 0.0,
            len = ec.caseLen or 0.035,
            t = 0.0,
            -- ONE AT A TIME. They are all released by the same tip of the wrist, but a chamber at the bottom of the
            -- circle gives its case up before one at the top, and staggering by where each SITS reproduces that
            -- without simulating it.
            -- the stagger is a head start on SLIDING, not on existing: they are all in the cylinder from the
            -- moment it opens, and the delay only decides who is first out when the gun finally tips
            delay = (ec.stagger or 0.03) * (i - 1),
            life = ec.life or 3.0,
        }
    end
    caseS.list = (#list > 0) and list or nil
end

-- A CASE STILL IN ITS CHAMBER IS PART OF THE GUN. Its world place is rebuilt from the cylinder every frame --
-- chamber offset, the mesh's own correction, and however far it has slid along the axis -- so it follows the hand
-- exactly and nothing is remembered that the gun could have changed since.
function M.place(ctx, c)
    if not (ctx.cyl and ctx.cyl.x) then return end
    local cy = ctx.cyl
    local ox, oy, oz = qrot(cy.qi, cy.qj, cy.qk, cy.qr, c.chamber[1], c.chamber[2], c.chamber[3])
    local q1, q2, q3, q4 = qmul(cy.qi, cy.qj, cy.qk, cy.qr, c.mrot[1], c.mrot[2], c.mrot[3], c.mrot[4])
    local mx, my, mz = qrot(q1, q2, q3, q4, c.meshAt[1], c.meshAt[2], c.meshAt[3])
    local sl = c.slide or 0.0
    c.p = { cy.x + ox - mx + c.axis[1] * sl,
            cy.y + oy - my + c.axis[2] * sl,
            cy.z + oz - mz + c.axis[3] * sl }
    c.q = { q1, q2, q3, q4 }
    local e = Game.FindEntityByID(c.id)
    if e then
        pcall(function()
            local tr = WorldTransform.new()
            tr:SetPosition(Vector4.new(c.p[1], c.p[2], c.p[3], 1.0))
            tr:SetOrientation(Quaternion.new(q1, q2, q3, q4))
            e:SetWorldTransform(tr)
        end)
    end
end

function M.tick(ctx)
    if not caseS.list then return end
    local dt, alive = ctx.dt, 0
    for i = 1, #caseS.list do
        local c = caseS.list[i]
        if c.id then
            c.t = c.t + dt
            if false then
                alive = alive + 1
            elseif c.free and (c.t - c.free) >= c.life then
                pcall(function() ctx.spawner.Despawn(Game.FindEntityByID(c.id)) end)
                c.id = nil
            elseif c.slide and c.slide < c.depth then
                -- RIDE THE GUN while still in the chamber, and take the axis from where the cylinder is NOW. This
                -- is what makes "open it level, then tilt" work: at spawn the chamber was horizontal and nothing
                -- could move, and a latched axis would have kept it that way however far the gun was then turned.
                if ctx.cyl then
                    local nx, ny, nz = qrot(ctx.cyl.qi, ctx.cyl.qj, ctx.cyl.qk, ctx.cyl.qr,
                                            c.localAxis[1], c.localAxis[2], c.localAxis[3])
                    if not c.exitStarted and nz > 0.0 then nx, ny, nz = -nx, -ny, -nz end
                    if c.exitStarted and c.exitSign == -1 then nx, ny, nz = -nx, -ny, -nz end
                    c.axis = { nx, ny, nz }
                end
                -- STILL IN THE CHAMBER: only the axial part of gravity can act, and friction takes its share of
                -- the rest.
                --
                -- `axis` was chosen at spawn as the DOWNWARD end of the chamber, so `-axis[3]` is a cosine between
                -- 0 and 1 by construction and the case can only ever be pushed out, never held in by its own sign.
                local ct = -c.axis[3]
                local st = math.sqrt(math.max(0.0, 1.0 - ct * ct))
                -- EACH CHAMBER LETS GO IN ITS OWN TIME, and it is its ANGLE that decides, not a queue. A case does
                -- not sit centred in its chamber -- there is clearance, and it rests on whichever side of the bore
                -- is lower. So the wall it presses on, and how hard, depends on where its chamber has been rolled
                -- to: the one at the bottom of the circle is pinched hardest and is the last to move, the one at
                -- the top is barely held. `bias` is that, and it is read from the LIVE cylinder, so rolling the gun
                -- in your hand changes the order -- which is the thing that stops six cases leaving as a block.
                local rw = 1.0
                if ctx.cyl and c.chamber then
                    local rx, ry, rz = qrot(ctx.cyl.qi, ctx.cyl.qj, ctx.cyl.qk, ctx.cyl.qr,
                                            c.chamber[1], c.chamber[2], 0.0)
                    local rl = math.sqrt(rx*rx + ry*ry + rz*rz)
                    if rl > 1e-6 then rw = 1.0 + (c.bias or 0.35) * (rz / rl) end
                end
                local a = 9.81 * (ct - c.mu * rw * st)
                if a < 0.0 then a = 0.0 end            -- not steep enough yet: it stays where it is
                -- ...and the head start is counted from the moment it COULD move, not from when it was put in the
                -- cylinder. Counted from the spawn it was spent while the gun was still being raised, and all six
                -- then started together: "они как-то синхронно все выезжают".
                if c.exitSpeed > 0 then
                    -- Deliberate tilt releases the extractor. Once released, finish the
                    -- short travel even if the hand levels out to reach for a loader.
                    if not c.exitStarted then
                        if ctx.cyl and ct >= c.releaseTilt then c.armed = (c.armed or 0.0) + dt
                        else c.armed = 0.0 end
                        if (c.armed or 0.0) >= math.max(c.delay, 0.025) then
                            c.exitStarted = true
                            c.sv = c.exitSpeed
                            local _, _, localZ = qrot(ctx.cyl.qi, ctx.cyl.qj, ctx.cyl.qk, ctx.cyl.qr,
                                c.localAxis[1], c.localAxis[2], c.localAxis[3])
                            c.exitSign = localZ > 0 and -1 or 1
                        else a = 0.0 end
                    end
                else
                    if a > 0.0 then c.armed = (c.armed or 0.0) + dt end
                    if a > 0.0 and (c.armed or 0.0) < c.delay then a = 0.0 end
                end
                c.sv = (c.sv or 0.0) + a * dt
                c.slide = c.slide + c.sv * dt
                M.place(ctx, c)
                -- TIPPING OVER THE MOUTH. Once more than half of it is out, the case is a rod overhanging an edge:
                -- gravity pulls at its centre, the mouth holds the rest, and the pair is a torque. Angular
                -- acceleration for a rod pivoting about a point `d` from its centre is
                --
                --     alpha = m*g*sin(theta)*d / (m*(L^2/12 + d^2))
                --
                -- and the mass falls out. `st` is that sin(theta) -- how much of gravity is across the case rather
                -- than along it -- so a case sliding straight down earns almost no turn and one leaving a level
                -- cylinder tips hard, which is exactly what a revolver looks like.
                local half = (c.len or 0.035) * 0.5
                local over = c.slide - (c.depth - half)
                if over > 0.0 then
                    local al = 9.81 * st * over / ((c.len or 0.035) ^ 2 / 12.0 + over * over)
                    c.w = (c.w or 0.0) + al * dt
                    -- it topples about the line across both its own axis and gravity: the way an overhang falls
                    local tx = c.axis[2] * 0.0 - c.axis[3] * 0.0
                    local ax2, ay2 = c.axis[2], -c.axis[1]
                    local tl = math.sqrt(ax2 * ax2 + ay2 * ay2)
                    if tl > 1e-6 then c.wa = { ax2 / tl, ay2 / tl, 0.0 } end
                end
                if c.slide >= c.depth then
                    -- clear of the mouth: it keeps what it built up, plus the little radial kick that opens the six
                    for k = 1, 3 do c.v[k] = c.axis[k] * c.sv + c.radial[k] end
                    -- ITS LIFE STARTS HERE, not at the spawn. Counted from the spawn, a gun held just at the
                    -- release angle ran the clock down while the cases were still sitting in their chambers and
                    -- they vanished where they sat: "держу угол, а они просто исчезают".
                    c.free = c.t
                end
                alive = alive + 1
            else
                c.v[3] = c.v[3] - 9.81 * dt
                for k = 1, 3 do c.p[k] = c.p[k] + c.v[k] * dt end
                local a = c.w * dt * 0.5
                local sa = math.sin(a)
                local q1, q2, q3, q4 = qmul(c.q[1], c.q[2], c.q[3], c.q[4],
                                            c.wa[1] * sa, c.wa[2] * sa, c.wa[3] * sa, math.cos(a))
                local l = math.sqrt(q1 * q1 + q2 * q2 + q3 * q3 + q4 * q4)
                if l > 1e-6 then c.q = { q1 / l, q2 / l, q3 / l, q4 / l } end
                local e = Game.FindEntityByID(c.id)
                if e then
                    pcall(function()
                        local tr = WorldTransform.new()
                        tr:SetPosition(Vector4.new(c.p[1], c.p[2], c.p[3], 1.0))
                        tr:SetOrientation(Quaternion.new(c.q[1], c.q[2], c.q[3], c.q[4]))
                        e:SetWorldTransform(tr)
                    end)
                end
                alive = alive + 1
            end
        end
    end
    if alive == 0 then caseS.list = nil end
end

function M.clear(ctx)
    if not caseS.list then return end
    for i = 1, #caseS.list do
        local c = caseS.list[i]
        if c.id then pcall(function() ctx.spawner.Despawn(Game.FindEntityByID(c.id)) end) end
    end
    caseS.list = nil
end

-- Are any still sitting in their chambers? While that is true the gun still has its rounds and nothing has been
-- given up, however long the cylinder has been open.
function M.held()
    if not caseS.list then return false end
    for i = 1, #caseS.list do
        local c = caseS.list[i]
        if c.id and not c.free then return true end
    end
    return false
end

-- ...and how many of the ones still in their chambers are LIVE. That is what the gun gets back if the crane is
-- shut again before everything has fallen out: the spent ones were never ammunition and the live ones that did drop
-- are on the floor, so neither belongs in the count.
function M.heldLive()
    if not caseS.list then return 0 end
    local n = 0
    for i = 1, #caseS.list do
        local c = caseS.list[i]
        if c.id and not c.free and c.live then n = n + 1 end
    end
    return n
end

-- ...and have they all gone? That is the moment the weapon is empty, not the moment the cylinder opened.
function M.spent()
    if not caseS.list then return false end
    for i = 1, #caseS.list do
        if caseS.list[i].id and not caseS.list[i].free then return false end
    end
    return true
end

function M.busy()
    return (hamS.p or 0.0) > 0.0 or (spinS.wrote or false) or (caseS.list ~= nil)
end

function M.diag()
    local c = caseS.list and caseS.list[1]
    return string.format('rev[cases=%s%s]', caseS.list and #caseS.list or 0,
        c and string.format(' t=%.2f slide=%.3f/%.3f ct=%+.2f sv=%.2f', c.t or 0, c.slide or -1, c.depth or -1,
                            -(c.axis and c.axis[3] or 0), c.sv or 0) or '')
end

return M
