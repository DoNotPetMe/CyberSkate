# Lays out the CyberSkate release: Install-CyberSkate.bat at the top, and the
# bin\ and red4ext\ folders it copies into the Cyberpunk 2077 folder.
#
#   pwsh cyberpunk/scripts/package.ps1 -Plugin <CyberSkate.dll> -Out <folder>
#       [-Converter <iw4l-skate-convert.exe>] [-ConverterLicenses <files>]
param(
    [Parameter(Mandatory)] [string] $Plugin,
    [Parameter(Mandatory)] [string] $Out,
    [string] $Converter,
    [string[]] $ConverterLicenses = @()
)
$ErrorActionPreference = 'Stop'
$cyberpunk = Split-Path -Parent $PSScriptRoot
$repo = Split-Path -Parent $cyberpunk

Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item -Recurse -Force (Join-Path $cyberpunk 'mod\*') $Out
Copy-Item -Force (Join-Path $cyberpunk 'installer\*') $Out

$plugin_dir = Join-Path $Out 'red4ext\plugins\CyberSkate'
Copy-Item -LiteralPath $Plugin -Destination (Join-Path $plugin_dir 'CyberSkate.dll')

$licenses = Join-Path $plugin_dir 'licenses'
New-Item -ItemType Directory -Force $licenses | Out-Null
Copy-Item (Join-Path $repo 'LICENSE') $licenses
Copy-Item (Join-Path $repo 'NOTICE') $licenses
Copy-Item (Join-Path $cyberpunk 'licenses\*') $licenses

if ($Converter) {
    $converter_dir = Join-Path $plugin_dir 'converter'
    New-Item -ItemType Directory -Force $converter_dir | Out-Null
    Copy-Item -LiteralPath $Converter -Destination (Join-Path $converter_dir 'iw4l-skate-convert.exe')
    foreach ($license in $ConverterLicenses) {
        Copy-Item -LiteralPath $license -Destination $licenses
    }
} else {
    Write-Warning 'No converter given: the package cannot convert Skate 3 on its own.'
}
Get-ChildItem -Recurse -File $Out | ForEach-Object { $_.FullName.Substring($Out.Length) }
