-- The CyberSkate RED4ext plugin's natives, reached through CET's `Game`.
-- Every call is protected: a missing or outdated plugin turns into a status
-- line, never a script error.
local Native = {
    available = false,
    version = nil,
}

-- Indices into CyberSkate_Frame (1-based here; see SkateFrame::to_floats).
Native.FRAME_LEN = 33
Native.F = {
    sequence = 1, tick = 2,
    skater = 3, skaterForward = 6, skaterUp = 9,
    deck = 12, deckRotation = 15,
    camera = 19, cameraForward = 22, fov = 25, hasCamera = 26,
    velocity = 27,
    head = 30, hasHead = 33,
}

local function call(name, ...)
    local ok, result = pcall(function(...)
        return Game[name](...)
    end, ...)
    if ok then
        return result
    end
    return nil
end

function Native.probe()
    local version = call("CyberSkate_Version")
    Native.available = type(version) == "string" and version ~= ""
    Native.version = Native.available and version or nil
    return Native.available
end

local function guard(name, fallback)
    return function(...)
        if not Native.available then
            return fallback
        end
        local result = call(name, ...)
        if result == nil then
            return fallback
        end
        return result
    end
end

Native.start = guard("CyberSkate_Start", false)
Native.status = guard("CyberSkate_Status", "plugin missing")
Native.assetsPath = guard("CyberSkate_AssetsPath", "")
Native.notice = guard("CyberSkate_Notice", "")
Native.scanBreaks = guard("CyberSkate_ScanBreaks", {})
Native.submitScan = guard("CyberSkate_SubmitScan", false)
Native.activate = guard("CyberSkate_Activate", false)
Native.deactivate = guard("CyberSkate_Deactivate", false)
Native.step = guard("CyberSkate_Step", false)
Native.frame = guard("CyberSkate_Frame", {})
Native.state = guard("CyberSkate_State", "")
Native.controller = guard("CyberSkate_Controller", false)
Native.collision = guard("CyberSkate_Collision", {})
Native.padButtons = guard("CyberSkate_PadButtons", 0)

return Native
