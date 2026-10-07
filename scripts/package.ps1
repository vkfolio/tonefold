# Builds everything and stages a release package: dist\Tonefold-<version>\ plus a .zip beside it.
# The package installs on a machine with no Rust and no repo checkout (Node is optional; only the
# chat composer needs it).
param([switch]$SkipBuild)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent

# Windows PowerShell turns a native command's stderr into a terminating error while
# ErrorActionPreference is Stop, and cargo reports progress on stderr — so let exit codes, not
# streams, decide whether a build failed.
function Invoke-Native {
    param([Parameter(Mandatory)][string]$Exe, [string[]]$Arguments, [string]$WorkingDirectory)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    if ($WorkingDirectory) { Push-Location $WorkingDirectory }
    try {
        & $Exe @Arguments
        $code = $LASTEXITCODE
    } finally {
        if ($WorkingDirectory) { Pop-Location }
        $ErrorActionPreference = $prev
    }
    if ($code -ne 0) { throw "$Exe $($Arguments -join ' ') failed (exit $code)" }
}

Push-Location $root
try {
    $version = ([regex]::Match((Get-Content "$root\Cargo.toml" -Raw), '(?m)^version = "([^"]+)"')).Groups[1].Value
    if (-not $version) { throw "could not read the version from Cargo.toml" }
    Write-Host "Packaging Tonefold $version"

    if (-not $SkipBuild) {
        Write-Host "  building plugins..."
        Invoke-Native cargo @("xtask", "bundle", "tonefold-plugin", "--release")
        Write-Host "  building the app and CLI..."
        Invoke-Native cargo @("build", "--release", "-p", "tonefold-plugin", "--features", "standalone", "--bin", "tonefold-standalone")
        Invoke-Native cargo @("build", "--release", "-p", "tonefold-cli")
        if (Get-Command npm -ErrorAction SilentlyContinue) {
            Write-Host "  building the composer sidecar..."
            Invoke-Native npm.cmd @("run", "build") (Join-Path $root "agent")
        } else {
            Write-Warning "npm not found: packaging the sidecar as it stands in agent\dist"
        }
    }

    $out = Join-Path $root "dist\Tonefold-$version"
    if (Test-Path $out) { Remove-Item -Recurse -Force $out }
    New-Item -ItemType Directory -Force $out | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $out "scripts") | Out-Null

    Copy-Item -Recurse -Force "$root\target\bundled\Tonefold.clap" $out
    Copy-Item -Recurse -Force "$root\target\bundled\Tonefold.vst3" $out
    Copy-Item -Force "$root\target\release\tonefold-standalone.exe" (Join-Path $out "Tonefold.exe")
    Copy-Item -Force "$root\target\release\tonefold-cli.exe" $out
    Copy-Item -Force "$root\scripts\install.ps1" (Join-Path $out "scripts")
    Copy-Item -Force "$root\flscript\Tonefold Import.pyscript" $out
    Copy-Item -Recurse -Force "$root\skill" $out
    Copy-Item -Recurse -Force "$root\docs" $out
    # Built-in sounds. Its author asks that projects ship their own copy rather than link to his files.
    $sf = Join-Path $root "target\GeneralUser-GS.sf2"
    $sfLicense = Join-Path $root "target\GeneralUser-GS-LICENSE.txt"
    if (-not (Test-Path $sf)) {
        Write-Host "  fetching the GeneralUser GS soundfont..."
        Invoke-WebRequest -UseBasicParsing -OutFile $sf -Uri "https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/main/GeneralUser-GS.sf2"
        Invoke-WebRequest -UseBasicParsing -OutFile $sfLicense -Uri "https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/main/documentation/LICENSE.txt"
    }
    New-Item -ItemType Directory -Force (Join-Path $out "soundfont") | Out-Null
    Copy-Item -Force $sf (Join-Path $out "soundfont\GeneralUser-GS.sf2")
    if (Test-Path $sfLicense) { Copy-Item -Force $sfLicense (Join-Path $out "soundfont\LICENSE.txt") }

    foreach ($f in @("README.md", "CHANGELOG.md", "LICENSE", "THIRD_PARTY_NOTICES.md")) {
        if (Test-Path "$root\$f") { Copy-Item -Force "$root\$f" $out }
    }

    # The sidecar without its node_modules: the installer runs npm ci, because the SDK alone is
    # bigger than everything else here put together.
    $agent = Join-Path $out "agent"
    New-Item -ItemType Directory -Force $agent | Out-Null
    foreach ($item in @("dist", "prompts", "skills", "reference", "package.json", "package-lock.json")) {
        Copy-Item -Recurse -Force (Join-Path "$root\agent" $item) $agent
    }

    Set-Content -Path (Join-Path $out "VERSION") -Value $version -Encoding utf8

    $zip = Join-Path $root "dist\Tonefold-$version.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path "$out\*" -DestinationPath $zip
    $mb = [math]::Round((Get-Item $zip).Length / 1MB, 1)
    Write-Host ""
    Write-Host "Package: $out"
    Write-Host "Zip:     $zip ($mb MB)"
    Write-Host "Install: unzip, then run scripts\install.ps1"
} finally {
    Pop-Location
}
