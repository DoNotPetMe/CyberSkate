-- Scans Night City around the skater with the game's ray casts and hands
-- the result to the plugin, which builds the skate collision from it.
--
-- A scan is a square grid of downward casts, then bisection of the grid
-- edges the plugin reports as steps (curbs, ledges, drop-offs), then a ring
-- of horizontal casts for walls. The work is spread over frames by a ray
-- budget; the skater keeps riding the previous scan meanwhile.
local Native = require("modules/native")

local Scanner = {}

local MISS = -1.0e9
local atan2 = math.atan2 or math.atan

local job = nil
Scanner.last = nil -- { x, y, time } of the last submitted scan's centre
Scanner.time = 0   -- seconds of riding, advanced by the rider
Scanner.rays = 0   -- casts in the scan being built
Scanner.submitted = 0

local function caster(groups)
    local system = Game.GetSpatialQueriesSystem()
    return function(sx, sy, sz, ex, ey, ez)
        Scanner.rays = Scanner.rays + 1
        local from = Vector4.new(sx, sy, sz, 1)
        local to = Vector4.new(ex, ey, ez, 1)
        local best, bestDistance = nil, math.huge
        for _, group in ipairs(groups) do
            local ok, hit, trace = pcall(function()
                return system:SyncRaycastByCollisionGroup(from, to, group, false, false)
            end)
            if ok and hit and trace then
                local p, n = trace.position, trace.normal
                local dx, dy, dz = p.x - sx, p.y - sy, p.z - sz
                local distance = dx * dx + dy * dy + dz * dz
                if distance < bestDistance then
                    bestDistance = distance
                    best = { p.x, p.y, p.z, n.x, n.y, n.z }
                end
            end
        end
        return best
    end
end

local function snap(value, spacing)
    return math.floor(value / spacing + 0.5) * spacing
end

-- Starts a scan centred on (x, y), looking for ground around height `floor`,
-- with walls looked for from `eye` (the skater's own position).
function Scanner.begin(settings, x, y, floor, eyeX, eyeY)
    local s = settings.scan
    local size = math.max(5, math.min(161, math.floor(s.size)))
    if size % 2 == 0 then
        size = size + 1
    end
    local spacing = math.max(0.1, s.spacing)
    local half = (size - 1) / 2 * spacing
    local ox = snap(x, spacing) - half
    local oy = snap(y, spacing) - half
    job = {
        settings = s,
        cast = caster(s.groups),
        size = size,
        spacing = spacing,
        ox = ox,
        oy = oy,
        top = floor + s.above,
        bottom = floor - s.below,
        floor = floor,
        eye = { eyeX, eyeY, floor + s.ringHeight },
        ground = {},
        heights = {},
        next = 0,
        phase = "grid",
        edges = {},
        breaks = nil,
        breakIndex = 1,
        walls = {},
        ringIndex = 0,
        centre = { x = x, y = y },
    }
    Scanner.rays = 0
end

function Scanner.busy()
    return job ~= nil
end

function Scanner.cancel()
    job = nil
end

local function header(j)
    return { j.ox, j.oy, j.spacing, j.size, j.eye[1], j.eye[2], j.eye[3], j.floor }
end

local function down(j, x, y)
    local hit = j.cast(x, y, j.top, x, y, j.bottom)
    if hit then
        return hit[3], hit
    end
    return nil, nil
end

local function sampleXY(j, index)
    local i = index % j.size
    local k = math.floor(index / j.size)
    return j.ox + i * j.spacing, j.oy + k * j.spacing
end

-- Which side of a step a probe height belongs to: true for side a.
local function onSideA(h, ha, hb)
    if h == nil then
        return ha == nil
    end
    if ha == nil then
        return false
    end
    if hb == nil then
        return math.abs(h - ha) < 0.05
    end
    return math.abs(h - ha) <= math.abs(h - hb)
end

local function grid(j, budget)
    local total = j.size * j.size
    while budget > 0 and j.next < total do
        local x, y = sampleXY(j, j.next)
        local h, hit = down(j, x, y)
        local base = j.next * 4
        if hit then
            j.ground[base + 1] = hit[3]
            j.ground[base + 2] = hit[4]
            j.ground[base + 3] = hit[5]
            j.ground[base + 4] = hit[6]
        else
            j.ground[base + 1] = MISS
            j.ground[base + 2] = 0
            j.ground[base + 3] = 0
            j.ground[base + 4] = 1
        end
        j.heights[j.next] = h
        j.next = j.next + 1
        budget = budget - 1
    end
    if j.next >= total then
        j.breaks = Native.scanBreaks(header(j), j.ground)
        j.phase = "refine"
    end
    return budget
end

local function refine(j, budget)
    local steps = math.max(1, math.floor(j.settings.refine))
    local breaks = j.breaks
    while budget >= steps and j.breakIndex + 1 <= #breaks do
        local a = math.floor(breaks[j.breakIndex] + 0.5)
        local axis = math.floor(breaks[j.breakIndex + 1] + 0.5)
        local b = axis == 0 and a + 1 or a + j.size
        local ax, ay = sampleXY(j, a)
        local bx, by = sampleXY(j, b)
        local ha, hb = j.heights[a], j.heights[b]
        local lo, hi = 0, 1
        for _ = 1, steps do
            local m = (lo + hi) / 2
            local h = down(j, ax + (bx - ax) * m, ay + (by - ay) * m)
            if onSideA(h, ha, hb) then
                lo = m
            else
                hi = m
            end
        end
        local edges = j.edges
        edges[#edges + 1] = a
        edges[#edges + 1] = axis
        edges[#edges + 1] = (lo + hi) / 2
        j.breakIndex = j.breakIndex + 2
        budget = budget - steps
    end
    if j.breakIndex + 1 > #breaks then
        j.phase = "ring"
    end
    return budget
end

local function ring(j, budget)
    local s = j.settings
    local count = math.max(0, math.floor(s.ringRays))
    local ex, ey, ez = j.eye[1], j.eye[2], j.eye[3]
    while budget > 0 and j.ringIndex < count do
        local angle = j.ringIndex / count * 2 * math.pi
        local dx, dy = math.cos(angle), math.sin(angle)
        local hit = j.cast(ex, ey, ez, ex + dx * s.ringRange, ey + dy * s.ringRange, ez)
        local base = j.ringIndex * 6
        for k = 1, 6 do
            j.walls[base + k] = hit and hit[k] or MISS
        end
        j.ringIndex = j.ringIndex + 1
        budget = budget - 1
    end
    if j.ringIndex >= count then
        j.phase = "submit"
    end
    return budget
end

-- Spends up to `budget` casts. Returns true when a scan was submitted.
function Scanner.advance(budget)
    local j = job
    if not j then
        return false
    end
    if j.phase == "grid" then
        budget = grid(j, budget)
    end
    if j.phase == "refine" and budget > 0 then
        budget = refine(j, budget)
    end
    if j.phase == "ring" and budget > 0 then
        budget = ring(j, budget)
    end
    if j.phase == "submit" then
        job = nil
        local ok = Native.submitScan(header(j), j.ground, j.edges, j.walls)
        if ok then
            Scanner.submitted = Scanner.submitted + 1
            Scanner.last = { x = j.centre.x, y = j.centre.y, time = Scanner.time }
        end
        return ok
    end
    return false
end

-- A whole scan at once, for getting on the board.
function Scanner.complete()
    while job do
        if Scanner.advance(100000) then
            return true
        end
    end
    return false
end

-- Height of the ground straight below (x, y, z), or nil.
function Scanner.groundBelow(settings, x, y, z)
    local cast = caster(settings.scan.groups)
    local hit = cast(x, y, z + 0.5, x, y, z - settings.scan.below)
    return hit and hit[3] or nil
end

-- Yaw in degrees for a horizontal direction, in the game's convention:
-- calibrated once against the player's own forward and yaw.
local yawSign = nil
function Scanner.calibrateYaw(player)
    local ok, forward, yaw = pcall(function()
        return player:GetWorldForward(), player:GetWorldYaw()
    end)
    if not (ok and forward and yaw) then
        yawSign = -1
        return
    end
    local function diff(a, b)
        local d = (a - b) % 360
        return math.min(d, 360 - d)
    end
    local ccw = math.deg(atan2(-forward.x, forward.y))
    local cw = math.deg(atan2(forward.x, forward.y))
    yawSign = diff(ccw, yaw) <= diff(cw, yaw) and -1 or 1
end

function Scanner.yaw(fx, fy)
    return math.deg(atan2((yawSign or -1) * fx, fy))
end

return Scanner
