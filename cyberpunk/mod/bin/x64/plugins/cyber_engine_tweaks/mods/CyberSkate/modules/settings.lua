-- Player settings, kept in settings.json beside init.lua. Missing keys fall
-- back to these defaults, so an old file keeps working after an update.
local Settings = {}

local FILE = "settings.json"

local defaults = {
    -- Converted Skate 3 data. Empty: red4ext/plugins/CyberSkate/skate-data/assets.
    assetsPath = "",
    -- "first": V's own eyes, turned to where Skate 3's camera looks.
    -- "chase": the view moved to Skate 3's camera behind the skater.
    camera = "first",
    -- First person: the view dips and rises with the skater's head.
    headBob = true,
    -- Click both sticks in to get on or off the board.
    padToggle = true,
    showHud = true,
    -- Restrictions held on V while skating, so the controller only skates.
    restrictions = {
        "NoMovement", "NoJump", "NoSprint", "NoCombat", "NoCameraControl",
        "NoScanning", "NoZooming", "NoRadialMenus", "NoWorldInteractions",
        "VehicleNoSummoning", "NoPhone",
    },
    scan = {
        -- Samples per side and metres between them: 33 x 0.5 m is 16 m.
        size = 33,
        spacing = 0.5,
        -- Ray casts spent per frame; a full scan spreads over several frames.
        raysPerFrame = 240,
        -- Ground is looked for from this far above the skater's floor to
        -- this far below it.
        above = 2.5,
        below = 10.0,
        -- Horizontal casts for walls taller than `above`, at this height.
        ringRays = 64,
        ringHeight = 2.0,
        ringRange = 12.0,
        -- A new scan starts once the skater is this far from the last
        -- scan's centre, or after this many seconds.
        recenter = 4.0,
        interval = 1.0,
        -- The next scan is centred where the skater will be this many
        -- seconds ahead.
        lead = 0.35,
        -- Bisection steps per step edge: 3 finds a ledge to 6 cm.
        refine = 3,
        groups = { "Static", "Terrain" },
    },
    -- Off the board and falling with nothing scanned below for this far:
    -- give V back to the game.
    fallLimit = 12.0,
}

local function merge(into, from)
    for key, value in pairs(from) do
        if type(value) == "table" and type(into[key]) == "table" and #value == 0 then
            merge(into[key], value)
        elseif into[key] == nil or type(into[key]) == type(value) then
            into[key] = value
        end
    end
    return into
end

local function copy(value)
    if type(value) ~= "table" then
        return value
    end
    local out = {}
    for k, v in pairs(value) do
        out[k] = copy(v)
    end
    return out
end

function Settings.load()
    local current = copy(defaults)
    local file = io.open(FILE, "r")
    if file then
        local text = file:read("*a")
        file:close()
        local ok, saved = pcall(json.decode, text)
        if ok and type(saved) == "table" then
            merge(current, saved)
        else
            print("[CyberSkate] settings.json is unreadable; using defaults")
        end
    end
    Settings.current = current
    return current
end

function Settings.save()
    local file = io.open(FILE, "w")
    if not file then
        return false
    end
    file:write(json.encode(Settings.current))
    file:close()
    return true
end

function Settings.reset()
    Settings.current = copy(defaults)
    Settings.save()
    return Settings.current
end

return Settings
