# Installs FLVSTX: the CLAP and VST3 plugins into the machine-wide folders FL Studio scans by type
# (needs one UAC prompt), the standalone app, the command-line tool, the composer sidecar, the FL
# piano-roll script, and the General MIDI soundfont.
#
# Runs from a release package (artifacts beside this script) or from a repo checkout (artifacts in
# target/). Everything except the two plugin files goes under %LOCALAPPDATA%\FLVSTX, so only the
# plugin copy is elevated.
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$home_ = Join-Path $env:LOCALAPPDATA "FLVSTX"

# Where each artifact lives, package layout first, then a repo build.
function Find-Artifact([string[]]$candidates, [string]$what, [switch]$Optional) {
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    if ($Optional) { return $null }
    throw "$what not found. Looked in:`n  " + ($candidates -join "`n  ") + "`nIn a repo checkout, build first: cargo xtask bundle flvstx-plugin --release"
}

$clap = Find-Artifact @("$root\FLVSTX.clap", "$root\target\bundled\FLVSTX.clap") "FLVSTX.clap"
$vst3 = Find-Artifact @("$root\FLVSTX.vst3", "$root\target\bundled\FLVSTX.vst3") "FLVSTX.vst3"
$exe = Find-Artifact @("$root\FLVSTX.exe", "$root\target\release\flvstx-standalone.exe") "the standalone app" -Optional
$cli = Find-Artifact @("$root\flvstx-cli.exe", "$root\target\release\flvstx-cli.exe") "the command-line tool" -Optional
$agentSrc = Find-Artifact @("$root\agent") "the composer sidecar" -Optional
$pyscript = Find-Artifact @("$root\FLVSTX Import.pyscript", "$root\flscript\FLVSTX Import.pyscript") "the FL piano-roll script" -Optional
$skill = Find-Artifact @("$root\skill\flvstx-song\SKILL.md") "the composer skill" -Optional

# --- plugins (the only part that needs elevation) ---------------------------------------------
$clapDir = "C:\Program Files\Common Files\CLAP"
$vst3Dir = "C:\Program Files\Common Files\VST3"
$cmd = @"
New-Item -ItemType Directory -Force '$clapDir' | Out-Null
Copy-Item -Force '$clap' '$clapDir\FLVSTX.clap'
if (Test-Path '$vst3Dir\FLVSTX.vst3') { Remove-Item -Recurse -Force '$vst3Dir\FLVSTX.vst3' }
Copy-Item -Recurse -Force '$vst3' '$vst3Dir\FLVSTX.vst3'
"@
# Everything below the plugins works without elevation, so a declined prompt is reported at the
# end rather than stopping the install.
$pluginProblem = $null
try {
    $p = Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile", "-Command", $cmd -Wait -PassThru
    if ($p.ExitCode -ne 0) { $pluginProblem = "the elevated copy failed (exit $($p.ExitCode))" }
} catch {
    $pluginProblem = "the administrator prompt was declined"
}

# Verify: a running FL Studio locks the plugin files and the copy fails silently.
if (-not $pluginProblem) {
    $inner = "Contents\x86_64-win\FLVSTX.vst3"
    $srcClapHash = (Get-FileHash $clap).Hash
    $srcVst3Hash = (Get-FileHash (Join-Path $vst3 $inner)).Hash
    $dstClap = Join-Path $clapDir "FLVSTX.clap"
    $dstVst3 = Join-Path (Join-Path $vst3Dir "FLVSTX.vst3") $inner
    $dstClapHash = if (Test-Path $dstClap) { (Get-FileHash $dstClap).Hash } else { "" }
    $dstVst3Hash = if (Test-Path $dstVst3) { (Get-FileHash $dstVst3).Hash } else { "" }
    if ($srcClapHash -ne $dstClapHash -or $srcVst3Hash -ne $dstVst3Hash) {
        $fl = Get-Process FL64* -ErrorAction SilentlyContinue
        $pluginProblem = if ($fl) {
            "FL Studio is running (PID $($fl.Id -join ',')) and locks the plugin file it has loaded; close FL Studio and run this again"
        } else {
            "the installed files do not match this build (CLAP ok: $($srcClapHash -eq $dstClapHash), VST3 ok: $($srcVst3Hash -eq $dstVst3Hash))"
        }
    }
}

# Remove stale per-user copies from earlier installs so FL does not see duplicates.
Remove-Item -Force (Join-Path $env:LOCALAPPDATA "Programs\Common\CLAP\FLVSTX.clap") -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA "Programs\Common\VST3\FLVSTX.vst3") -ErrorAction SilentlyContinue

# --- built-in sounds ---------------------------------------------------------------------------
# GeneralUser GS, a free General MIDI soundfont (~31 MB), downloaded once.
$sf = Join-Path $home_ "soundfont\GeneralUser-GS.sf2"
if (-not (Test-Path $sf)) {
    New-Item -ItemType Directory -Force (Split-Path $sf) | Out-Null
    Write-Host "Downloading GeneralUser GS soundfont (31 MB)..."
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/main/GeneralUser-GS.sf2" -OutFile $sf -UseBasicParsing
}

# --- standalone app and command-line tool ------------------------------------------------------
$appDir = Join-Path $home_ "app"
New-Item -ItemType Directory -Force $appDir | Out-Null
if ($exe) {
    $running = Get-Process FLVSTX -ErrorAction SilentlyContinue
    if ($running) { throw "The FLVSTX app is running (PID $($running.Id -join ',')); close it and run the installer again." }
    Copy-Item -Force $exe (Join-Path $appDir "FLVSTX.exe")
    $ws = New-Object -ComObject WScript.Shell
    $lnk = $ws.CreateShortcut((Join-Path ([Environment]::GetFolderPath("Programs")) "FLVSTX.lnk"))
    $lnk.TargetPath = Join-Path $appDir "FLVSTX.exe"
    $lnk.Arguments = "--backend auto"
    $lnk.WorkingDirectory = $appDir
    $lnk.Save()
}
if ($cli) { Copy-Item -Force $cli (Join-Path $appDir "flvstx-cli.exe") }

# --- composer sidecar --------------------------------------------------------------------------
# Node modules are installed here rather than shipped: the SDK alone is far larger than everything
# else in the package.
$agentDst = Join-Path $home_ "agent"
if ($agentSrc) {
    New-Item -ItemType Directory -Force $agentDst | Out-Null
    foreach ($item in @("dist", "prompts", "skills", "reference", "package.json", "package-lock.json")) {
        $src = Join-Path $agentSrc $item
        if (Test-Path $src) {
            $dst = Join-Path $agentDst $item
            if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
            Copy-Item -Recurse -Force $src $dst
        }
    }
    $npm = Get-Command npm -ErrorAction SilentlyContinue
    if ($npm) {
        Write-Host "Installing composer dependencies (npm, one time)..."
        Push-Location $agentDst
        try { & npm ci --omit=dev --no-audit --no-fund 2>&1 | Out-Null } catch { Write-Warning "npm install failed: $_" }
        Pop-Location
    } else {
        Write-Warning "Node.js was not found: the chat composer will not start. Install Node 20+ and re-run this script. Everything else works without it."
    }
}

# --- FL piano-roll script and the external-AI skill ---------------------------------------------
if ($pyscript) {
    $scriptDir = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "Image-Line\FL Studio\Settings\Piano roll scripts"
    New-Item -ItemType Directory -Force $scriptDir | Out-Null
    Copy-Item -Force $pyscript $scriptDir
}
if ($skill) {
    $skillDir = Join-Path $env:USERPROFILE ".claude\skills\flvstx-song"
    New-Item -ItemType Directory -Force $skillDir | Out-Null
    Copy-Item -Force $skill (Join-Path $skillDir "SKILL.md")
}

$version = if (Test-Path "$root\VERSION") { (Get-Content "$root\VERSION" -Raw).Trim() } else { "dev" }
Set-Content -Path (Join-Path $home_ "VERSION") -Value $version -Encoding utf8

Write-Host ""
Write-Host "FLVSTX $version installed:"
if ($pluginProblem) {
    Write-Warning "The FL Studio plugins were NOT installed: $pluginProblem."
    Write-Warning "Run this script again and accept the prompt. Everything below works either way."
} else {
    Write-Host "  $clapDir\FLVSTX.clap"
    Write-Host "  $vst3Dir\FLVSTX.vst3"
}
if ($exe) { Write-Host "  $appDir\FLVSTX.exe            (Start Menu > FLVSTX)" }
if ($cli) { Write-Host "  $appDir\flvstx-cli.exe        (add this folder to PATH to use it anywhere)" }
if ($agentSrc) { Write-Host "  $agentDst                     (chat composer)" }
if ($pyscript) { Write-Host "  Documents\Image-Line\FL Studio\Settings\Piano roll scripts\FLVSTX Import.pyscript" }
if ($skill) { Write-Host "  ~\.claude\skills\flvstx-song  (compose from Claude Code / Codex)" }
Write-Host ""
Write-Host "In FL Studio: Options > Manage plugins > Find installed plugins, then type FLVSTX in Find."
