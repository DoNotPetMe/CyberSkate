# Installs CyberSkate into Cyberpunk 2077 and prepares your Skate 3.
#
# Run Install-CyberSkate.bat from wherever this zip was extracted. It finds
# the game (Steam, GOG or Epic, or asks), copies the mod's bin\ and red4ext\
# folders into it, checks that RED4ext and Cyber Engine Tweaks are there, and
# converts your Skate 3 if that has not been done yet. Run it again any time;
# it only replaces CyberSkate's own files.
param(
    [string] $Game,
    [string] $Xex,
    [switch] $SkipSkate,
    [switch] $NoPrompt
)
$ErrorActionPreference = 'Stop'
$here = (Resolve-Path -LiteralPath (Split-Path -Parent $MyInvocation.MyCommand.Path)).Path

function Test-Game([string] $path) {
    $path -and (Test-Path -LiteralPath (Join-Path $path 'bin\x64\Cyberpunk2077.exe'))
}

function Find-Game {
    $candidates = @()
    $steam = (Get-ItemProperty -Path 'HKCU:\Software\Valve\Steam' -Name SteamPath -ErrorAction SilentlyContinue).SteamPath
    $steams = @($steam, "${env:ProgramFiles(x86)}\Steam", "$env:ProgramFiles\Steam") | Where-Object { $_ }
    foreach ($root in $steams) {
        $root = $root -replace '/', '\'
        $candidates += Join-Path $root 'steamapps\common\Cyberpunk 2077'
        $vdf = Join-Path $root 'steamapps\libraryfolders.vdf'
        if (Test-Path -LiteralPath $vdf) {
            foreach ($match in [regex]::Matches((Get-Content -Raw -LiteralPath $vdf), '"path"\s+"([^"]+)"')) {
                $candidates += Join-Path ($match.Groups[1].Value -replace '\\\\', '\') 'steamapps\common\Cyberpunk 2077'
            }
        }
    }
    foreach ($key in 'HKLM:\SOFTWARE\WOW6432Node\GOG.com\Games\1423049311', 'HKLM:\SOFTWARE\GOG.com\Games\1423049311') {
        $gog = (Get-ItemProperty -Path $key -Name path -ErrorAction SilentlyContinue).path
        if ($gog) { $candidates += $gog }
    }
    $candidates += "$env:ProgramFiles\Epic Games\Cyberpunk2077", "$env:ProgramFiles\GOG Galaxy\Games\Cyberpunk 2077"
    foreach ($drive in Get-PSDrive -PSProvider FileSystem -ErrorAction SilentlyContinue) {
        $candidates += Join-Path $drive.Root 'SteamLibrary\steamapps\common\Cyberpunk 2077'
        $candidates += Join-Path $drive.Root 'GOG Games\Cyberpunk 2077'
    }
    $candidates | Where-Object { Test-Game $_ } | Select-Object -First 1
}

function Ask-Folder {
    Add-Type -AssemblyName System.Windows.Forms
    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.Description = 'Select your Cyberpunk 2077 folder (the one with bin, r6 and archive)'
    if ($dialog.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { return $null }
    $dialog.SelectedPath
}

if (-not (Test-Game $Game)) {
    if ($Game) { Write-Host "Not a Cyberpunk 2077 folder: $Game" }
    $Game = Find-Game
    if ($Game -and -not $NoPrompt) {
        Write-Host "Found Cyberpunk 2077 in $Game"
        $answer = Read-Host 'Install there? [Y/n]'
        if ($answer -match '^[nN]') { $Game = $null }
    }
    while (-not (Test-Game $Game)) {
        if ($NoPrompt) { throw 'Cyberpunk 2077 was not found; pass -Game <folder>.' }
        $Game = Ask-Folder
        if (-not $Game) { Write-Host 'No folder selected; nothing was installed.'; exit 1 }
        if (-not (Test-Game $Game)) { Write-Host "$Game has no bin\x64\Cyberpunk2077.exe; pick the game's top folder." }
    }
}
$Game = (Resolve-Path -LiteralPath $Game).Path
Write-Host "Installing CyberSkate into $Game"

# The mod's own files, merged into the game's folders file by file.
$copied = 0
if ($here -ne $Game) {
    foreach ($top in 'bin', 'red4ext') {
        $source = Join-Path $here $top
        foreach ($file in Get-ChildItem -LiteralPath $source -Recurse -File) {
            $relative = $file.FullName.Substring($here.Length).TrimStart('\', '/')
            $target = Join-Path $Game $relative
            New-Item -ItemType Directory -Force (Split-Path -Parent $target) | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $target -Force
            $copied++
        }
    }
}
Write-Host "  $copied files copied"

$problems = @()
$mod = Join-Path $Game 'bin\x64\plugins\cyber_engine_tweaks\mods\CyberSkate\init.lua'
$plugin = Join-Path $Game 'red4ext\plugins\CyberSkate\CyberSkate.dll'
foreach ($needed in $mod, $plugin) {
    if (-not (Test-Path -LiteralPath $needed)) { $problems += "Missing $needed" }
}
if (-not (Test-Path -LiteralPath (Join-Path $Game 'red4ext\RED4ext.dll'))) {
    $problems += 'RED4ext is not installed: get it from https://github.com/WopsS/RED4ext/releases and extract it into the game folder.'
}
if (-not (Test-Path -LiteralPath (Join-Path $Game 'bin\x64\plugins\cyber_engine_tweaks.asi'))) {
    $problems += 'Cyber Engine Tweaks is not installed: get it from https://github.com/maximegmd/CyberEngineTweaks/releases and extract it into the game folder.'
}

$assets = Join-Path $Game 'red4ext\plugins\CyberSkate\skate-data\assets\private\game.json'
if (-not $SkipSkate -and -not (Test-Path -LiteralPath $assets)) {
    Write-Host ''
    Write-Host 'Next: your Skate 3. Select its default.xex; its data folder must be beside it.'
    $setup = Join-Path $Game 'red4ext\plugins\CyberSkate\Setup-CyberSkate.ps1'
    if ($Xex) { & $setup -Xex $Xex } else { & $setup }
}
if (-not $SkipSkate -and -not (Test-Path -LiteralPath $assets)) {
    $problems += 'Skate 3 data is not converted yet: run red4ext\plugins\CyberSkate\Setup-CyberSkate.bat.'
}

Write-Host ''
if ($problems.Count -eq 0) {
    Write-Host 'CyberSkate is installed.'
    Write-Host 'Start the game, open the CET overlay > Bindings > CyberSkate and bind "Toggle skateboard",'
    Write-Host 'or click both sticks in at the same time on the controller.'
    exit 0
} else {
    Write-Host 'CyberSkate is installed, but:'
    foreach ($problem in $problems) { Write-Host "  - $problem" }
    exit 3
}
