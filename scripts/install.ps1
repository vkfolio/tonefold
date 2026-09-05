# Installs the built FLVSTX bundles into the machine-wide folders FL Studio scans by type
# (C:\Program Files\Common Files\CLAP and \VST3; needs a UAC prompt), and the FL piano-roll script.
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$bundled = Join-Path $root "target\bundled"
$clap = Join-Path $bundled "FLVSTX.clap"
$vst3 = Join-Path $bundled "FLVSTX.vst3"
if (-not (Test-Path $clap)) { throw "Build first: cargo xtask bundle flvstx-plugin --release (expected $clap)" }

$clapDir = "C:\Program Files\Common Files\CLAP"
$vst3Dir = "C:\Program Files\Common Files\VST3"
$cmd = @"
New-Item -ItemType Directory -Force '$clapDir' | Out-Null
Copy-Item -Force '$clap' '$clapDir\FLVSTX.clap'
if (Test-Path '$vst3Dir\FLVSTX.vst3') { Remove-Item -Recurse -Force '$vst3Dir\FLVSTX.vst3' }
Copy-Item -Recurse -Force '$vst3' '$vst3Dir\FLVSTX.vst3'
"@
$p = Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile", "-Command", $cmd -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "elevated copy failed (exit $($p.ExitCode))" }

# Remove stale per-user copies from earlier installs so FL does not see duplicates.
$userClap = Join-Path $env:LOCALAPPDATA "Programs\Common\CLAP\FLVSTX.clap"
$userVst3 = Join-Path $env:LOCALAPPDATA "Programs\Common\VST3\FLVSTX.vst3"
Remove-Item -Force $userClap -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $userVst3 -ErrorAction SilentlyContinue

$scriptDir = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "Image-Line\FL Studio\Settings\Piano roll scripts"
New-Item -ItemType Directory -Force $scriptDir | Out-Null
Copy-Item -Force (Join-Path $root "flscript\FLVSTX Import.pyscript") $scriptDir
Write-Host "Installed:`n  $clapDir\FLVSTX.clap`n  $vst3Dir\FLVSTX.vst3`n  $scriptDir\FLVSTX Import.pyscript"
Write-Host "In FL Studio: Options > Manage plugins > Find installed plugins, then type FLVSTX in Find."
