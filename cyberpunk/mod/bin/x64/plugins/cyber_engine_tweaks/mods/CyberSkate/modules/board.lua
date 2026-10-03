-- A board entity riding Skate 3's deck: spawned with Codeware's
-- DynamicEntitySystem when V gets on, placed on the deck every frame, and
-- deleted on the way off. The entity's own axes must be the deck's: x right,
-- y nose, z up.
local Native = require("modules/native")

local Board = { id = nil, warned = false, status = "no board entity set", path = "" }

local F = Native.F
local TAG = "CyberSkateBoard"

local function system()
    local ok, s = pcall(function() return Game.GetDynamicEntitySystem() end)
    return ok and s or nil
end

local function placement(f)
    local q = Quaternion.new(f[F.deckRotation], f[F.deckRotation + 1], f[F.deckRotation + 2], f[F.deckRotation + 3])
    return Vector4.new(f[F.deck], f[F.deck + 1], f[F.deck + 2], 1), q
end

function Board.spawn(settings, f)
    Board.despawn()
    -- A pasted Windows path is cut down to the part inside the archive:
    -- quotes and everything up to "archive\" go, slashes become backslashes.
    local path = (settings.boardEntity or ""):gsub('"', ""):gsub("/", "\\")
    path = path:match("[Aa]rchive\\(.+)$") or path
    path = path:match("^%s*(.-)%s*$")
    Board.path = path
    if path == "" then
        Board.status = "no board entity set"
        return
    end
    local s = system()
    if not s then
        Board.status = "Codeware is not installed (needed to draw the board)"
        return
    end
    local ok, err = pcall(function()
        local position, orientation = placement(f)
        local spec = DynamicEntitySpec.new()
        local ok_ref, ref = pcall(function() return ResRef.FromString(path) end)
        spec.templatePath = ok_ref and ref or path
        spec.position = position
        spec.orientation = orientation
        spec.alwaysSpawned = true
        spec.spawnInView = true
        spec.tags = { TAG }
        Board.id = s:CreateEntity(spec)
    end)
    if ok then
        Board.status = "spawning " .. path
        Board.waited = 0
    else
        Board.status = "could not spawn " .. path .. ": " .. tostring(err)
    end
    print("[CyberSkate] board: " .. Board.status)
end

function Board.update(f, dt)
    if not Board.id then
        return
    end
    pcall(function()
        local entity = system():GetEntity(Board.id)
        if not entity then
            Board.waited = (Board.waited or 0) + (dt or 0)
            if Board.waited > 3 and not Board.status:find("not found") then
                Board.status = "spawned nothing after 3 s: is the WolvenKit project installed, and is " .. Board.path .. " its .ent?"
                print("[CyberSkate] board: " .. Board.status)
            end
        else
            Board.status = "on the deck"
            local position, orientation = placement(f)
            Game.GetTeleportationFacility():Teleport(entity, position, orientation:ToEulerAngles())
        end
    end)
end

function Board.despawn()
    local s = system()
    if s then
        pcall(function() s:DeleteTagged(TAG) end)
    end
    Board.id = nil
end

return Board
