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

# Verify: a running FL Studio locks the plugin files and the copy fails silently.
$srcClapHash = (Get-FileHash $clap).Hash
$srcVst3Hash = (Get-FileHash (Join-Path $vst3 "Contents\x86_64-win\FLVSTX.vst3")).Hash
$dstClapHash = (Get-FileHash "$clapDir\FLVSTX.clap").Hash
$dstVst3Path = "$vst3Dir\FLVSTX.vst3\Contents\x86_64-win\FLVSTX.vst3"
$dstVst3Hash = if (Test-Path $dstVst3Path) { (Get-FileHash $dstVst3Path).Hash } else { "" }
if ($srcClapHash -ne $dstClapHash -or $srcVst3Hash -ne $dstVst3Hash) {
    $fl = Get-Process FL64* -ErrorAction SilentlyContinue
    $hint = if ($fl) { " FL Studio is running (PID $($fl.Id -join ',')) and locks the loaded plugin file; close FL Studio and run the installer again." } else { "" }
    throw "Installed files do not match the build (CLAP ok: $($srcClapHash -eq $dstClapHash), VST3 ok: $($srcVst3Hash -eq $dstVst3Hash)).$hint"
}

# Remove stale per-user copies from earlier installs so FL does not see duplicates.
$userClap = Join-Path $env:LOCALAPPDATA "Programs\Common\CLAP\FLVSTX.clap"
$userVst3 = Join-Path $env:LOCALAPPDATA "Programs\Common\VST3\FLVSTX.vst3"
Remove-Item -Force $userClap -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $userVst3 -ErrorAction SilentlyContinue

# Built-in sounds: GeneralUser GS (free General MIDI soundfont, ~31 MB), downloaded once.
$sfDir = Join-Path $env:LOCALAPPDATA "FLVSTX\soundfont"
$sf = Join-Path $sfDir "GeneralUser-GS.sf2"
if (-not (Test-Path $sf)) {
    New-Item -ItemType Directory -Force $sfDir | Out-Null
    Write-Host "Downloading GeneralUser GS soundfont (31 MB)..."
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/main/GeneralUser-GS.sf2" -OutFile $sf -UseBasicParsing
}

# Standalone app (built-in sounds, no DAW needed) + Start Menu shortcut.
$exe = Join-Path $root "target\release\flvstx-standalone.exe"
if (Test-Path $exe) {
    $appDir = Join-Path $env:LOCALAPPDATA "FLVSTX\app"
    New-Item -ItemType Directory -Force $appDir | Out-Null
    Copy-Item -Force $exe (Join-Path $appDir "FLVSTX.exe")
    $ws = New-Object -ComObject WScript.Shell
    $lnk = $ws.CreateShortcut((Join-Path ([Environment]::GetFolderPath("Programs")) "FLVSTX.lnk"))
    $lnk.TargetPath = Join-Path $appDir "FLVSTX.exe"
    $lnk.Arguments = "--backend auto"
    $lnk.WorkingDirectory = $appDir
    $lnk.Save()
    Write-Host "Standalone app: $appDir\FLVSTX.exe (Start Menu > FLVSTX)"
}

$scriptDir = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "Image-Line\FL Studio\Settings\Piano roll scripts"
New-Item -ItemType Directory -Force $scriptDir | Out-Null
Copy-Item -Force (Join-Path $root "flscript\FLVSTX Import.pyscript") $scriptDir
Write-Host "Installed:`n  $clapDir\FLVSTX.clap`n  $vst3Dir\FLVSTX.vst3`n  $scriptDir\FLVSTX Import.pyscript"
Write-Host "In FL Studio: Options > Manage plugins > Find installed plugins, then type FLVSTX in Find."
