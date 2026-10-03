# Converts your own Skate 3 (Xbox 360) into the data CyberSkate reads.
#
# Select the extracted game's default.xex, with the game's `data` folder
# beside it. Only what skating needs is written, into skate-data\ next to
# this script; your game files are only read. Run again to redo it.
param([string] $Xex)
$ErrorActionPreference = 'Stop'

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$converter = Join-Path $here 'converter\iw4l-skate-convert.exe'
if (-not (Test-Path -LiteralPath $converter)) {
    Write-Host "The Skate 3 converter is missing: $converter"
    Write-Host 'Re-extract the CyberSkate zip into your Cyberpunk 2077 folder.'
    exit 2
}

if (-not $Xex) {
    Add-Type -AssemblyName System.Windows.Forms
    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = 'Select your Skate 3 default.xex (its data folder must be beside it)'
    $dialog.Filter = 'Skate 3 executable (default.xex)|default.xex|Xbox 360 executables (*.xex)|*.xex'
    if ($dialog.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) {
        Write-Host 'No default.xex selected.'
        exit 1
    }
    $Xex = $dialog.FileName
}

$out = Join-Path $here 'skate-data'
Write-Host "Converting Skate 3 data from $Xex"
& $converter --xex $Xex --out $out
if ($LASTEXITCODE -ne 0) {
    Write-Host 'The conversion failed; the converter said why above.'
    exit $LASTEXITCODE
}
$board_tool = Join-Path $here 'cyberskate-board.exe'
if (Test-Path -LiteralPath $board_tool) {
    & $board_tool (Join-Path $out 'assets') (Join-Path $out 'board.glb')
}
Write-Host ''
Write-Host "Skate 3 data ready in $out\assets"
Write-Host 'Start Cyberpunk 2077 and bind "Toggle skateboard" in the CET overlay, or click both sticks in.'
