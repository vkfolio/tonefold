# Installs Tonefold: the app, the command-line tool, the composer sidecar and the General MIDI
# soundfont, all under %LOCALAPPDATA%\Tonefold with no administrator prompt.
#
#   -Plugins   also installs the optional CLAP and VST3 plugins for DAWs into the machine-wide
#              folders DAWs scan (one UAC prompt).
#
# The FL Studio piano-roll import script is added when FL Studio's settings folder exists.
# Runs from a release package (artifacts beside this script) or from a repo checkout (artifacts in
# target/).
param([switch]$Plugins)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$home_ = Join-Path $env:LOCALAPPDATA "Tonefold"

# Tonefold was called FLVSTX before 1.4.0: carry its folder (soundfont, exports, logs) over once.
$oldHome = Join-Path $env:LOCALAPPDATA "FLVSTX"
if ((Test-Path $oldHome) -and -not (Test-Path $home_)) { Move-Item $oldHome $home_ }

# Where each artifact lives, package layout first, then a repo build.
function Find-Artifact([string[]]$candidates, [string]$what, [switch]$Optional) {
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    if ($Optional) { return $null }
    throw "$what not found. Looked in:`n  " + ($candidates -join "`n  ") + "`nIn a repo checkout, build first: cargo xtask bundle tonefold-plugin --release"
}

$exe = Find-Artifact @("$root\Tonefold.exe", "$root\target\release\tonefold-standalone.exe") "the Tonefold app"
if ($Plugins) {
    $clap = Find-Artifact @("$root\Tonefold.clap", "$root\target\bundled\Tonefold.clap") "Tonefold.clap"
    $vst3 = Find-Artifact @("$root\Tonefold.vst3", "$root\target\bundled\Tonefold.vst3") "Tonefold.vst3"
}
$cli = Find-Artifact @("$root\tonefold-cli.exe", "$root\target\release\tonefold-cli.exe") "the command-line tool" -Optional
$agentSrc = Find-Artifact @("$root\agent") "the composer sidecar" -Optional
$pyscript = Find-Artifact @("$root\Tonefold Import.pyscript", "$root\flscript\Tonefold Import.pyscript") "the FL piano-roll script" -Optional
$skill = Find-Artifact @("$root\skill\tonefold-song\SKILL.md") "the composer skill" -Optional

# --- optional DAW plugins (the only part that needs elevation) ---------------------------------
$pluginProblem = $null
if ($Plugins) {
    $clapDir = "C:\Program Files\Common Files\CLAP"
    $vst3Dir = "C:\Program Files\Common Files\VST3"
    $cmd = @"
New-Item -ItemType Directory -Force '$clapDir' | Out-Null
Copy-Item -Force '$clap' '$clapDir\Tonefold.clap'
if (Test-Path '$vst3Dir\Tonefold.vst3') { Remove-Item -Recurse -Force '$vst3Dir\Tonefold.vst3' }
Copy-Item -Recurse -Force '$vst3' '$vst3Dir\Tonefold.vst3'
Remove-Item -Force '$clapDir\FLVSTX.clap' -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force '$vst3Dir\FLVSTX.vst3' -ErrorAction SilentlyContinue
"@
    # Everything below the plugins works without elevation, so a declined prompt is reported at the
    # end rather than stopping the install.
    try {
        $p = Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile", "-Command", $cmd -Wait -PassThru
        if ($p.ExitCode -ne 0) { $pluginProblem = "the elevated copy failed (exit $($p.ExitCode))" }
    } catch {
        $pluginProblem = "the administrator prompt was declined"
    }

    # Verify: a running FL Studio locks the plugin files and the copy fails silently.
    if (-not $pluginProblem) {
        $inner = "Contents\x86_64-win\Tonefold.vst3"
        $srcClapHash = (Get-FileHash $clap).Hash
        $srcVst3Hash = (Get-FileHash (Join-Path $vst3 $inner)).Hash
        $dstClap = Join-Path $clapDir "Tonefold.clap"
        $dstVst3 = Join-Path (Join-Path $vst3Dir "Tonefold.vst3") $inner
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
    Remove-Item -Force (Join-Path $env:LOCALAPPDATA "Programs\Common\CLAP\Tonefold.clap") -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA "Programs\Common\VST3\Tonefold.vst3") -ErrorAction SilentlyContinue
}

# --- built-in sounds ---------------------------------------------------------------------------
# GeneralUser GS, a free General MIDI soundfont (~31 MB). Release packages carry it; a repo
# checkout downloads it once.
$sf = Join-Path $home_ "soundfont\GeneralUser-GS.sf2"
$sfBundled = Join-Path $root "soundfont\GeneralUser-GS.sf2"
if (-not (Test-Path $sf)) {
    New-Item -ItemType Directory -Force (Split-Path $sf) | Out-Null
    if (Test-Path $sfBundled) {
        Copy-Item -Force $sfBundled $sf
        $sfLicense = Join-Path $root "soundfont\LICENSE.txt"
        if (Test-Path $sfLicense) { Copy-Item -Force $sfLicense (Join-Path (Split-Path $sf) "LICENSE.txt") }
    } else {
        Write-Host "Downloading GeneralUser GS soundfont (31 MB)..."
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/main/GeneralUser-GS.sf2" -OutFile $sf -UseBasicParsing
    }
}

# --- standalone app and command-line tool ------------------------------------------------------
$appDir = Join-Path $home_ "app"
New-Item -ItemType Directory -Force $appDir | Out-Null
if ($exe) {
    $running = Get-Process Tonefold -ErrorAction SilentlyContinue
    if ($running) { throw "The Tonefold app is running (PID $($running.Id -join ',')); close it and run the installer again." }
    Copy-Item -Force $exe (Join-Path $appDir "Tonefold.exe")
    $ws = New-Object -ComObject WScript.Shell
    $lnk = $ws.CreateShortcut((Join-Path ([Environment]::GetFolderPath("Programs")) "Tonefold.lnk"))
    $lnk.TargetPath = Join-Path $appDir "Tonefold.exe"
    $lnk.Arguments = "--backend auto"
    $lnk.WorkingDirectory = $appDir
    $lnk.Save()
    Remove-Item -Force (Join-Path ([Environment]::GetFolderPath("Programs")) "FLVSTX.lnk") -ErrorAction SilentlyContinue
    Remove-Item -Force (Join-Path $appDir "FLVSTX.exe") -ErrorAction SilentlyContinue
}
if ($cli) { Copy-Item -Force $cli (Join-Path $appDir "tonefold-cli.exe") }

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
$flSettings = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "Image-Line\FL Studio\Settings"
if (-not (Test-Path $flSettings)) { $pyscript = $null }
if ($pyscript) {
    $scriptDir = Join-Path $flSettings "Piano roll scripts"
    New-Item -ItemType Directory -Force $scriptDir | Out-Null
    Copy-Item -Force $pyscript $scriptDir
    Remove-Item -Force (Join-Path $scriptDir "FLVSTX Import.pyscript") -ErrorAction SilentlyContinue
}
if ($skill) {
    $skillDir = Join-Path $env:USERPROFILE ".claude\skills\tonefold-song"
    New-Item -ItemType Directory -Force $skillDir | Out-Null
    Copy-Item -Force $skill (Join-Path $skillDir "SKILL.md")
    Remove-Item -Recurse -Force (Join-Path $env:USERPROFILE ".claude\skills\flvstx-song") -ErrorAction SilentlyContinue
}

$version = if (Test-Path "$root\VERSION") { (Get-Content "$root\VERSION" -Raw).Trim() } else { "dev" }
Set-Content -Path (Join-Path $home_ "VERSION") -Value $version -Encoding utf8

Write-Host ""
Write-Host "Tonefold $version installed:"
Write-Host "  $appDir\Tonefold.exe            (Start Menu > Tonefold)"
if ($pluginProblem) {
    Write-Warning "The DAW plugins were NOT installed: $pluginProblem."
    Write-Warning "Run this script again with -Plugins and accept the prompt. The app works either way."
} elseif ($Plugins) {
    Write-Host "  $clapDir\Tonefold.clap"
    Write-Host "  $vst3Dir\Tonefold.vst3"
}
if ($cli) { Write-Host "  $appDir\tonefold-cli.exe        (add this folder to PATH to use it anywhere)" }
if ($agentSrc) { Write-Host "  $agentDst                     (chat composer)" }
if ($pyscript) { Write-Host "  Documents\Image-Line\FL Studio\Settings\Piano roll scripts\Tonefold Import.pyscript" }
if ($skill) { Write-Host "  ~\.claude\skills\tonefold-song  (compose from Claude Code / Codex)" }
Write-Host ""
Write-Host "Start Tonefold from the Start Menu."
if ($Plugins -and -not $pluginProblem) {
    Write-Host "In your DAW, rescan plugins (FL Studio: Options > Manage plugins > Find installed plugins)."
    Write-Host "Upgrading from FLVSTX? Projects that used it need Tonefold inserted in its place."
} elseif (-not $Plugins) {
    Write-Host "Want Tonefold inside a DAW too? Run this again with -Plugins."
}
