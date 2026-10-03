-- Getting on and off the board, and carrying V along with the skater while
-- the plugin simulates Skate 3.
local Native = require("modules/native")
local Scanner = require("modules/scanner")

local Rider = {
    state = "off", -- off | entering | riding
    message = "",
    frame = nil,
    speed = 0,
    skaterState = "",
}

local F = Native.F
local EYE = 1.65
local ENTER_TIMEOUT = 5.0

local settings = nil
local restrictionIds = {}
local enterClock = 0
local lastSequence = -1
local floor = 0
local headRest = nil
local savedCamera = nil

local function player()
    local p = Game.GetPlayer()
    if p and IsDefined(p) then
        return p
    end
    return nil
end

local function protected(f, ...)
    local ok, result = pcall(f, ...)
    if ok then
        return result
    end
    return nil
end

-- Copies of the game's restrictions that are never written into a save, so
-- a crash or a lost "off" can never leave V restricted for good.
local function prepareRestrictions()
    restrictionIds = {}
    for _, name in ipairs(settings.restrictions) do
        local base = "GameplayRestriction." .. name
        local own = "GameplayRestriction.CyberSkate_" .. name
        local ok = pcall(function()
            if not TweakDB:GetRecord(own) and TweakDB:GetRecord(base) then
                TweakDB:CloneRecord(own, base)
                TweakDB:SetFlat(own .. ".savable", false)
            end
        end)
        if ok and TweakDB:GetRecord(own) then
            restrictionIds[#restrictionIds + 1] = own
        else
            print("[CyberSkate] restriction " .. base .. " is not in this game version; skipped")
        end
    end
end

local function restrict(p, on)
    for _, id in ipairs(restrictionIds) do
        pcall(function()
            if on then
                StatusEffectHelper.ApplyStatusEffect(p, TweakDBID.new(id))
            else
                StatusEffectHelper.RemoveStatusEffect(p, TweakDBID.new(id))
            end
        end)
    end
end

local function camera(p)
    return protected(function()
        return p:GetFPPCameraComponent()
    end)
end

-- The view is only moved when its own transform could be read, so it can
-- always be put back exactly.
local function saveCamera(p)
    local c = camera(p)
    local position = c and protected(function() return c:GetLocalPosition() end)
    local orientation = c and protected(function() return c:GetLocalOrientation() end)
    if position and orientation then
        savedCamera = { position = position, orientation = orientation }
    else
        savedCamera = nil
    end
end

local function restoreCamera(p)
    local saved = savedCamera
    savedCamera = nil
    local c = saved and camera(p)
    if not c then
        return
    end
    pcall(function()
        c:SetLocalPosition(saved.position)
        c:SetLocalOrientation(saved.orientation)
    end)
end

-- Hamilton product a * b of {i, j, k, r} quaternions.
local function multiply(a, b)
    return Quaternion.new(
        a.r * b.i + a.i * b.r + a.j * b.k - a.k * b.j,
        a.r * b.j - a.i * b.k + a.j * b.r + a.k * b.i,
        a.r * b.k + a.i * b.j - a.j * b.i + a.k * b.r,
        a.r * b.r - a.i * b.i - a.j * b.j - a.k * b.k
    )
end

local function inVehicle(p)
    return protected(function()
        return Game.GetMountedVehicle(p) ~= nil
    end) == true
end

function Rider.paused()
    local paused = protected(function()
        return Game.GetSystemRequestsHandler():IsGamePaused()
    end)
    if paused then
        return true
    end
    return protected(function()
        return Game.GetTimeSystem():IsPausedState()
    end) == true
end

function Rider.init(current)
    settings = current
    prepareRestrictions()
    if Native.probe() then
        Native.start(settings.assetsPath or "")
    end
end

function Rider.restart(current)
    settings = current
    if Rider.state ~= "off" then
        Rider.exit("Engine restarted")
    end
    if Native.probe() then
        Native.start(settings.assetsPath or "")
    end
end

-- Forget everything without touching the game: a new session or a load.
function Rider.reset(p)
    if p then
        restrict(p, false)
        restoreCamera(p)
    end
    if Rider.state ~= "off" then
        Native.deactivate()
    end
    Scanner.cancel()
    Rider.state = "off"
    Rider.frame = nil
end

function Rider.enter()
    local p = player()
    if not p then
        return
    end
    if not Native.available then
        Rider.message = "The CyberSkate RED4ext plugin is not loaded."
        return
    end
    local status = Native.status()
    if status ~= "ready" and status ~= "active" then
        if status == "loading" then
            Rider.message = "Skate 3 is still loading, try again in a moment."
        else
            Rider.message = "Skate 3 is unavailable: " .. tostring(status)
        end
        return
    end
    if inVehicle(p) then
        Rider.message = "Get out of the vehicle first."
        return
    end
    local position = p:GetWorldPosition()
    local forward = p:GetWorldForward()
    Scanner.calibrateYaw(p)
    Scanner.cancel()
    Scanner.begin(settings, position.x, position.y, position.z, position.x, position.y)
    if not Scanner.complete() then
        Rider.message = "Could not scan the ground here."
        return
    end
    floor = position.z
    if not Native.activate(position.x, position.y, position.z + 0.05, forward.x, forward.y) then
        Rider.message = "Skate 3 refused to start: " .. tostring(Native.status())
        return
    end
    saveCamera(p)
    Rider.state = "entering"
    Rider.message = ""
    enterClock = 0
    lastSequence = -1
    headRest = nil
end

function Rider.exit(reason)
    local p = player()
    Native.deactivate()
    Scanner.cancel()
    if p then
        restoreCamera(p)
        restrict(p, false)
        local f = Rider.frame
        if f then
            local yaw = Scanner.yaw(f[F.skaterForward], f[F.skaterForward + 1])
            pcall(function()
                Game.GetTeleportationFacility():Teleport(
                    p,
                    Vector4.new(f[F.skater], f[F.skater + 1], f[F.skater + 2] + 0.1, 1),
                    EulerAngles.new(0, 0, yaw)
                )
            end)
        end
    end
    Rider.state = "off"
    Rider.frame = nil
    Rider.message = reason or ""
end

function Rider.toggle()
    if Rider.state == "off" then
        Rider.enter()
    else
        Rider.exit()
    end
end

local function view(p, f)
    local hasCamera = f[F.hasCamera] == 1
    local fx, fy
    if hasCamera then
        fx, fy = f[F.cameraForward], f[F.cameraForward + 1]
    end
    if not fx or fx * fx + fy * fy < 1e-4 then
        fx, fy = f[F.skaterForward], f[F.skaterForward + 1]
    end
    local length = math.sqrt(fx * fx + fy * fy)
    if length < 1e-4 then
        return nil
    end
    fx, fy = fx / length, fy / length
    local px, py, pz = f[F.skater], f[F.skater + 1], f[F.skater + 2]
    pcall(function()
        Game.GetTeleportationFacility():Teleport(
            p, Vector4.new(px, py, pz, 1), EulerAngles.new(0, 0, Scanner.yaw(fx, fy))
        )
    end)

    local saved = savedCamera
    local c = saved and camera(p)
    if not c then
        return
    end
    local home = saved.position
    if settings.camera == "chase" and hasCamera then
        -- Skate 3's camera, expressed in V's own frame: x right, y forward.
        local rx, ry = fy, -fx
        local dx = f[F.camera] - px
        local dy = f[F.camera + 1] - py
        local dz = f[F.camera + 2] - (pz + EYE)
        local pitch = math.asin(math.max(-1, math.min(1, f[F.cameraForward + 2])))
        local tilt = Quaternion.new(math.sin(pitch / 2), 0, 0, math.cos(pitch / 2))
        pcall(function()
            c:SetLocalPosition(Vector4.new(
                home.x + dx * rx + dy * ry, home.y + dx * fx + dy * fy, home.z + dz, 1
            ))
            c:SetLocalOrientation(multiply(saved.orientation, tilt))
        end)
    elseif settings.headBob and f[F.hasHead] == 1 then
        -- The view dips as the skater crouches, measured from the tallest
        -- the head has stood above the root.
        local height = f[F.head + 2] - pz
        headRest = math.max(headRest or height, height)
        local bob = math.max(-0.9, math.min(0, height - headRest))
        pcall(function()
            c:SetLocalPosition(Vector4.new(home.x, home.y, home.z + bob, 1))
        end)
    end
end

local function rescan(f, dt)
    local s = settings.scan
    local px, py, pz = f[F.skater], f[F.skater + 1], f[F.skater + 2]
    Scanner.time = Scanner.time + dt
    if not Scanner.busy() then
        local last = Scanner.last
        local due = not last
            or (px - last.x) ^ 2 + (py - last.y) ^ 2 > s.recenter ^ 2
            or Scanner.time - last.time > s.interval
        if due then
            local vx, vy = f[F.velocity], f[F.velocity + 1]
            Scanner.begin(settings, px + vx * s.lead, py + vy * s.lead, floor, px, py)
        end
    end
    if Scanner.busy() then
        Scanner.advance(s.raysPerFrame)
    end
end

function Rider.update(dt)
    if Rider.state == "off" then
        return
    end
    local p = player()
    if not p then
        Rider.reset(nil)
        return
    end
    if protected(function() return p:IsDead() end) then
        Rider.exit("")
        return
    end
    if inVehicle(p) then
        Rider.exit("")
        return
    end
    if not Native.step(dt) then
        local status = Native.status()
        Rider.exit("Skate 3 stopped: " .. tostring(status))
        return
    end
    local notice = Native.notice()
    if notice ~= "" then
        print("[CyberSkate] " .. notice)
        Rider.message = notice
    end
    local f = Native.frame()
    if #f < Native.FRAME_LEN then
        if Rider.state == "entering" then
            enterClock = enterClock + dt
            if enterClock > ENTER_TIMEOUT then
                local why = Rider.message ~= "" and Rider.message or tostring(Native.status())
                Rider.exit("Skate 3 did not start: " .. why)
            end
        end
        return
    end
    if Rider.state == "entering" then
        Rider.state = "riding"
        restrict(p, true)
    end
    Rider.frame = f
    if f[F.sequence] ~= lastSequence then
        lastSequence = f[F.sequence]
        Rider.skaterState = Native.state()
        local vx, vy, vz = f[F.velocity], f[F.velocity + 1], f[F.velocity + 2]
        Rider.speed = math.sqrt(vx * vx + vy * vy + vz * vz)
        if not Rider.skaterState:find("Air") then
            floor = f[F.skater + 2]
        elseif f[F.skater + 2] < floor - settings.fallLimit then
            Rider.exit("Fell off the scanned world.")
            return
        end
    end
    view(p, f)
    rescan(f, dt)
end

return Rider
