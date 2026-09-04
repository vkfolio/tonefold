# Installs the built FLVSTX bundles and the FL piano-roll script into per-user locations FL Studio scans.
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$clapDir = Join-Path $env:LOCALAPPDATA "Programs\Common\CLAP"
$vst3Dir = Join-Path $env:LOCALAPPDATA "Programs\Common\VST3"
$scriptDir = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "Image-Line\FL Studio\Settings\Piano roll scripts"
New-Item -ItemType Directory -Force $clapDir, $vst3Dir, $scriptDir | Out-Null
Copy-Item -Force (Join-Path $root "target\bundled\flvstx-plugin.clap") (Join-Path $clapDir "FLVSTX.clap")
Remove-Item -Recurse -Force (Join-Path $vst3Dir "FLVSTX.vst3") -ErrorAction SilentlyContinue
Copy-Item -Recurse -Force (Join-Path $root "target\bundled\flvstx-plugin.vst3") (Join-Path $vst3Dir "FLVSTX.vst3")
Copy-Item -Force (Join-Path $root "flscript\FLVSTX Import.pyscript") $scriptDir
Write-Host "Installed:`n  $clapDir\FLVSTX.clap`n  $vst3Dir\FLVSTX.vst3`n  $scriptDir\FLVSTX Import.pyscript"
