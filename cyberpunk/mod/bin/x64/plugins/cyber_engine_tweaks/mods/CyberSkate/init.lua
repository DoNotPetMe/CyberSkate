-- CyberSkate: Skate 3's skating in Night City.
--
-- The RED4ext plugin runs the Skate 3 engine; this mod feeds it the world
-- (ray-cast scans), steps it every frame and carries V along. Toggle with
-- the "Toggle skateboard" binding (CET overlay > Bindings) or by clicking
-- both sticks in on the controller. Without a controller the keyboard
-- stands in for one.
local Settings = require("modules/settings")
local Native = require("modules/native")
local Rider = require("modules/rider")
local Scanner = require("modules/scanner")

local CyberSkate = {
    overlay = false,
    padWasDown = false,
}

-- XInput button bits for the thumbsticks clicked in.
local LEFT_THUMB, RIGHT_THUMB = 0x0040, 0x0080

local function held(buttons, bit)
    return math.floor(buttons / bit) % 2 == 1
end

registerForEvent("onInit", function()
    local settings = Settings.load()
    Rider.init(settings)
    if Native.available then
        print(("[CyberSkate] plugin %s; Skate 3 data: %s"):format(Native.version, Native.assetsPath()))
    else
        print("[CyberSkate] RED4ext plugin not found: install red4ext/plugins/CyberSkate/CyberSkate.dll")
    end
    -- A new session or a load starts with V unrestricted and off the board.
    Observe("PlayerPuppet", "OnGameAttached", function(self)
        Rider.reset(self)
    end)
    Observe("PlayerPuppet", "OnDetach", function()
        Rider.reset(nil)
    end)
end)

registerHotkey("cyberskate_toggle", "Toggle skateboard", function()
    Rider.toggle()
end)

registerForEvent("onUpdate", function(dt)
    if Rider.paused() then
        return
    end
    if Settings.current and Settings.current.padToggle and Native.available then
        local buttons = Native.padButtons()
        local down = held(buttons, LEFT_THUMB) and held(buttons, RIGHT_THUMB)
        if down and not CyberSkate.padWasDown then
            Rider.toggle()
        end
        CyberSkate.padWasDown = down
    end
    Rider.update(dt)
end)

registerForEvent("onOverlayOpen", function()
    CyberSkate.overlay = true
end)

registerForEvent("onOverlayClose", function()
    CyberSkate.overlay = false
    Settings.save()
end)

registerForEvent("onShutdown", function()
    if Rider.state ~= "off" then
        Rider.exit()
    end
end)

local function hud(settings)
    if not settings.showHud then
        return
    end
    local message = Rider.message ~= "" and Rider.message or nil
    if Rider.state == "off" and not message then
        return
    end
    ImGui.SetNextWindowPos(24, 220, ImGuiCond.FirstUseEver)
    local flags = ImGuiWindowFlags.NoTitleBar + ImGuiWindowFlags.AlwaysAutoResize
        + ImGuiWindowFlags.NoFocusOnAppearing + ImGuiWindowFlags.NoNav
    if ImGui.Begin("CyberSkate HUD", flags) then
        if Rider.state == "entering" then
            ImGui.Text("Dropping in...")
        elseif Rider.state == "riding" then
            ImGui.Text(("%.0f km/h"):format(Rider.speed * 3.6))
            local score = Rider.score
            if score then
                if score[Native.S.active] == 1 then
                    local multiplier = score[Native.S.multiplier]
                    local line = Rider.trick ~= "" and Rider.trick or "..."
                    if multiplier > 1.001 then
                        line = ("%s  %d x%.1f"):format(line, math.floor(score[Native.S.sequence] + 0.5), multiplier)
                    else
                        line = ("%s  %d"):format(line, math.floor(score[Native.S.sequence] + 0.5))
                    end
                    ImGui.TextColored(1, 0.85, 0.3, 1, line)
                end
                ImGui.Text(("Score %d"):format(math.floor(score[Native.S.total] + 0.5)))
            end
            for _, p in ipairs(Rider.popups) do
                ImGui.TextColored(p.color[1], p.color[2], p.color[3], 1, p.text)
            end
            if Rider.input == Native.INPUT.none then
                ImGui.TextColored(1, 0.6, 0.2, 1, "Connect a controller, or turn on keyboard skating")
            elseif Rider.input == Native.INPUT.keyboard then
                ImGui.TextDisabled("Keys: WASD lean, arrows flick, Space A, Shift X, Ctrl B, F Y, Q/E grabs")
            end
        end
        if message then
            ImGui.TextWrapped(message)
            if Rider.state == "off" and ImGui.SmallButton("OK") then
                Rider.message = ""
            end
        end
    end
    ImGui.End()
end

local function window(settings)
    ImGui.SetNextWindowSize(420, 0, ImGuiCond.FirstUseEver)
    if not ImGui.Begin("CyberSkate") then
        ImGui.End()
        return
    end
    ImGui.Text("Plugin: " .. (Native.available and Native.version or "not loaded"))
    ImGui.Text("Skate 3: " .. tostring(Native.status()))
    ImGui.TextWrapped("Data: " .. tostring(Native.assetsPath()))
    local collision = Native.collision()
    if #collision == 3 and collision[1] > 0 then
        ImGui.Text(("World: scan %d, %d triangles, %d rails"):format(collision[1], collision[2], collision[3]))
    end
    if ImGui.Button(Rider.state == "off" and "Get on the board" or "Get off the board") then
        Rider.toggle()
    end

    ImGui.Separator()
    local changed
    settings.assetsPath, changed = ImGui.InputText("Skate 3 data", settings.assetsPath, 512)
    if ImGui.Button("Reload Skate 3 data") then
        Settings.save()
        Rider.restart(settings)
    end
    ImGui.SameLine()
    ImGui.TextDisabled("(empty: next to the plugin)")

    local cameras = { "first", "chase" }
    local index = settings.camera == "chase" and 1 or 0
    index, changed = ImGui.Combo("Camera", index, { "First person", "Skate 3 chase (experimental)" }, 2)
    if changed then
        settings.camera = cameras[index + 1]
    end
    settings.headBob = ImGui.Checkbox("Head follows the skater's crouch", settings.headBob)
    settings.padToggle = ImGui.Checkbox("Click both sticks to get on/off", settings.padToggle)
    settings.keyboard, changed = ImGui.Checkbox("Keyboard skating when no controller", settings.keyboard)
    if changed then
        Native.setKeyboard(settings.keyboard)
    end
    settings.solidVehicles = ImGui.Checkbox("Cars are solid (traffic may leave ghosts)", settings.solidVehicles)
    settings.showHud = ImGui.Checkbox("Show HUD", settings.showHud)

    if ImGui.CollapsingHeader("World scan") then
        local s = settings.scan
        s.size = ImGui.SliderInt("Samples per side", s.size, 9, 81)
        s.spacing = ImGui.SliderFloat("Sample spacing (m)", s.spacing, 0.25, 1.5, "%.2f")
        s.raysPerFrame = ImGui.SliderInt("Ray casts per frame", s.raysPerFrame, 40, 2000)
        s.ringRays = ImGui.SliderInt("Wall rays", s.ringRays, 0, 256)
        s.lowRingRays = ImGui.SliderInt("Pole rays", s.lowRingRays, 0, 256)
        s.recenter = ImGui.SliderFloat("Rescan after (m)", s.recenter, 1, 12, "%.1f")
        s.lead = ImGui.SliderFloat("Look ahead (s)", s.lead, 0, 1.5, "%.2f")
        ImGui.Text(("Last scan: %d casts"):format(Scanner.rays))
    end
    if ImGui.Button("Save") then
        Settings.save()
    end
    ImGui.SameLine()
    if ImGui.Button("Defaults") then
        Settings.reset()
    end
    ImGui.End()
end

registerForEvent("onDraw", function()
    local settings = Settings.current
    if not settings then
        return
    end
    hud(settings)
    if CyberSkate.overlay then
        window(settings)
    end
end)

return CyberSkate
