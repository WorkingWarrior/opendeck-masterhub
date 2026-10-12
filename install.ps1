# Builds the plugin and copies it into OpenDeck's plugin directory.
# Run with OpenDeck closed (otherwise the plugin .exe is locked).

Set-Location $PSScriptRoot

# cargo writes progress to stderr, so don't treat stderr as an error here.
cmd /c "cargo build --release 2>&1"
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
$ErrorActionPreference = 'Stop'

$dest = Join-Path $env:APPDATA 'opendeck\plugins\io.github.opendeck-masterhub.sdPlugin'
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item manifest.json, assets\icon.png, assets\icon@2x.png $dest -Force
Copy-Item assets\icons, pi $dest -Recurse -Force
Copy-Item target\release\opendeck-masterhub.exe $dest -Force

Write-Host "Installed to $dest"
