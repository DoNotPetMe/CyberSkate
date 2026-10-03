-- A board entity riding Skate 3's deck: spawned when V gets on, placed on
-- the deck every frame, and deleted on the way off. The entity's own axes
-- must be the deck's: x right, y nose, z up.
--
-- Spawning takes the template path as a plain string through the game's
-- WorldFunctionalTests, which every CET prop spawner uses; Codeware's
-- DynamicEntitySystem is the fallback.
local Native = require("modules/native")

local Board = { status = "get on the board to check", path = "" }

local F = Native.F
local TAG = "CyberSkateBoard"
local id = nil
local via = nil
local waited = 0
local reported = false

local function report(text)
    Board.status = text
    print("[CyberSkate] board: " .. text)
end

local function placement(f)
    local q = Quaternion.new(f[F.deckRotation], f[F.deckRotation + 1], f[F.deckRotation + 2], f[F.deckRotation + 3])
    return Vector4.new(f[F.deck], f[F.deck + 1], f[F.deck + 2], 1), q
end

local function clean(path)
    path = (path or ""):gsub('"', ""):gsub("/", "\\")
    path = path:match("[Aa]rchive\\(.+)$") or path
    return path:match("^%s*(.-)%s*$")
end

local function find()
    if not id then
        return nil
    end
    local ok, entity = pcall(function()
        if via == "codeware" then
            return Game.GetDynamicEntitySystem():GetEntity(id)
        end
        return Game.FindEntityByID(id)
    end)
    return ok and entity or nil
end

function Board.spawn(settings, f)
    Board.despawn()
    local path = clean(settings.boardEntity)
    Board.path = path
    waited, reported = 0, false
    if path == "" then
        Board.status = "no board entity set"
        return
    end
    local position, orientation = placement(f)
    local errors = {}
    local ok, result = pcall(function()
        local transform = WorldTransform.new()
        transform:SetPosition(position)
        transform:SetOrientation(orientation)
        return WorldFunctionalTests.SpawnEntity(path, transform, "")
    end)
    if ok and result then
        id, via = result, "game"
        report("spawning " .. path)
        return
    end
    errors[#errors + 1] = "game spawner: " .. tostring(result)
    ok, result = pcall(function()
        local spec = DynamicEntitySpec.new()
        spec.templatePath = ResRef.FromString(path)
        spec.position = position
        spec.orientation = orientation
        spec.alwaysSpawned = true
        spec.spawnInView = true
        spec.tags = { TAG }
        return Game.GetDynamicEntitySystem():CreateEntity(spec)
    end)
    if ok and result then
        id, via = result, "codeware"
        report("spawning " .. path .. " (through Codeware)")
        return
    end
    errors[#errors + 1] = "Codeware: " .. tostring(result)
    report("could not spawn " .. path .. " (" .. table.concat(errors, "; ") .. ")")
end

function Board.update(f, dt)
    if not id then
        return
    end
    local entity = find()
    if not entity then
        waited = waited + (dt or 0)
        if waited > 3 and not reported then
            reported = true
            report("the game spawned nothing for " .. Board.path .. " after 3 s")
        end
        return
    end
    if Board.status ~= "on the deck" then
        report("on the deck")
    end
    pcall(function()
        local position, orientation = placement(f)
        Game.GetTeleportationFacility():Teleport(entity, position, orientation:ToEulerAngles())
    end)
end

function Board.despawn()
    if id then
        pcall(function()
            if via == "codeware" then
                Game.GetDynamicEntitySystem():DeleteEntity(id)
            else
                local entity = Game.FindEntityByID(id)
                if entity then
                    WorldFunctionalTests.DespawnEntity(entity)
                end
            end
        end)
    end
    pcall(function() Game.GetDynamicEntitySystem():DeleteTagged(TAG) end)
    id, via = nil, nil
end

return Board
