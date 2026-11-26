<#
.SYNOPSIS
  Checks this PC, builds Vertex and, given a title's executable, runs it.

.DESCRIPTION
  Without -GameExe: checks the build tools, builds, and runs `victory doctor`.
  With -GameExe: the same, then runs your own copy of the title. The copy must
  be outside this folder. No game file comes with Vertex.

  The build is the `diag` profile with the `diagnostics` feature: the switches
  for finding faults (README, "Debugging") are there when a run needs them.

.PARAMETER GameExe
  The title's executable.

.PARAMETER Install
  Install what is missing with winget (Rust, the C++ build tools, the Visual
  C++ runtime). Windows may ask for permission.

.PARAMETER Prebuild
  With -GameExe: translates the title's shaders, and makes the pipelines that
  earlier runs asked for, ahead of a run instead of starting it. On every
  core; an interrupted one goes on from where it last saved. On a PC without
  an earlier run it translates only: run once, then pre-build again.

.PARAMETER PrebuildAll
  As -Prebuild, but with every pipeline the title lists (tens of thousands,
  most of which a run may never ask for): hours, and many gigabytes.

.PARAMETER LoopLimit
  For finding faults: iterations after which every translated shader loop
  stops. Without it the translator's own limits hold (256 in pixel, vertex
  and mesh shaders, 4,096 in compute). Another limit has its own shader cache
  entries: everything is translated again. 0: the translator's own limits,
  also where VICTORY_SHADER_LOOP_LIMIT is set.

.PARAMETER Work
  Where work is done that the processor and the graphics card can both do:
  cpu (less on the card and less of its memory), balanced, gpu (more on it).

.PARAMETER Tuned
  The faster submission settings: submissions go ahead of the wait for the
  card (VICTORY_LATE_WAIT=3) and shader results come back on several threads
  (VICTORY_WIDE_BACK=1). With the relay on (the default) a run already does
  both, so this mostly matters where VICTORY_RELAY=0 is set. What is set in
  the environment already stays.

.PARAMETER LosslessScaling
  Also start Lossless Scaling (a Steam application of its own, not part of
  VIctory), so that its profile for victory.exe or its hotkey can be used.

.PARAMETER Force
  Run the title although `victory doctor` found something missing.

.EXAMPLE
  .\start.ps1
.EXAMPLE
  .\start.ps1 -Install
.EXAMPLE
  .\start.ps1 -GameExe 'D:\my-dump\title.exe'
.EXAMPLE
  .\start.ps1 -GameExe 'D:\my-dump\title.exe' -Tuned
#>
[CmdletBinding()]
param(
    [string]$GameExe,
    [switch]$Install,
    [ValidateRange(0, 1048576)][int]$LoopLimit = 0,
    [ValidateSet('cpu', 'balanced', 'gpu')][string]$Work = 'balanced',
    [switch]$Prebuild,
    [switch]$PrebuildAll,
    [switch]$Tuned,
    [switch]$LosslessScaling,
    [switch]$Force
)

# Programs write progress to stderr, which is no error: exit codes are checked.
$ErrorActionPreference = 'Continue'

function Stop-Start([string]$Message) {
    Write-Host "FAILED: $Message" -ForegroundColor Red
    exit 1
}

function Install-Package([string]$Id, [string]$What, [string[]]$More) {
    if (-not $Install) {
        Stop-Start "$What is missing. Run .\start.ps1 -Install, or: winget install $Id"
    }
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        Stop-Start "$What is missing and winget is not here; install $What by hand."
    }
    Write-Host "Installing $What ($Id)..." -ForegroundColor Cyan
    & winget install --id $Id --exact --accept-source-agreements --accept-package-agreements @More
    if ($LASTEXITCODE -ne 0) { Stop-Start "winget could not install $What (exit code $LASTEXITCODE)." }
}

# Windows PowerShell 5.1 exists on every supported Windows; so does this check.
if (-not [Environment]::Is64BitOperatingSystem) { Stop-Start 'a 64-bit Windows is needed.' }

# The Visual C++ runtime (titles import it; VIctory itself does not).
$system = Join-Path $env:SystemRoot 'System32'
$runtime = 'vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll'
if ($runtime | Where-Object { -not (Test-Path -LiteralPath (Join-Path $system $_)) }) {
    Install-Package 'Microsoft.VCRedist.2015+.x64' 'the Visual C++ runtime'
}

# The C++ build tools (the linker and the Windows SDK).
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$tools = $null
if (Test-Path -LiteralPath $vswhere) {
    $tools = & $vswhere -products * -latest -property installationPath `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64
}
if (-not $tools) {
    Install-Package 'Microsoft.VisualStudio.2022.BuildTools' 'the C++ build tools' @(
        '--override',
        '--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
}

# Rust; rustup takes the version in rust-toolchain.toml by itself.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    if (-not (Test-Path -LiteralPath (Join-Path $cargoBin 'cargo.exe'))) {
        Install-Package 'Rustlang.Rustup' 'Rust'
    }
    $env:PATH = "$cargoBin;$env:PATH"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Stop-Start 'cargo is not found; open a new terminal and run this again.'
}

# Before the build, which takes minutes.
if ($PrebuildAll) { $Prebuild = $true }
if ($Prebuild -and -not $GameExe) { Stop-Start '-Prebuild needs -GameExe.' }
if ($GameExe) {
    if (-not (Test-Path -LiteralPath $GameExe -PathType Leaf)) { Stop-Start "no file $GameExe" }
    $GameExe = (Resolve-Path -LiteralPath $GameExe).ProviderPath
}

$manifest = Join-Path $PSScriptRoot 'Cargo.toml'
Write-Host 'Building Vertex (minutes the first time)...' -ForegroundColor Cyan
& cargo build --locked --profile diag -p victory --features diagnostics --manifest-path $manifest
if ($LASTEXITCODE -ne 0) { Stop-Start "the build failed (exit code $LASTEXITCODE)." }
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot 'target' }
$victory = Join-Path $target 'diag\victory.exe'
if (-not (Test-Path -LiteralPath $victory)) { Stop-Start "built, but no $victory" }

Write-Host 'Checking this PC...' -ForegroundColor Cyan
& $victory doctor
$missing = $LASTEXITCODE -ne 0
if (-not $GameExe) {
    if ($missing) { exit 1 }
    Write-Host "Ready: .\start.ps1 -GameExe <the title's executable>" -ForegroundColor Green
    exit 0
}
if ($missing -and -not $Force) { Stop-Start 'see the FAIL lines above (-Force runs it all the same).' }

$saved = @{}
$settings = @{ VICTORY_GAME_DIR = (Split-Path -Parent $GameExe) }
# What is set already stays.
if ($PSBoundParameters.ContainsKey('LoopLimit')) {
    $settings.VICTORY_SHADER_LOOP_LIMIT = if ($LoopLimit) { "$LoopLimit" } else { $null }
}
if ($Tuned) {
    foreach ($pair in @(@('VICTORY_LATE_WAIT', '3'), @('VICTORY_WIDE_BACK', '1'))) {
        if ($null -eq [Environment]::GetEnvironmentVariable($pair[0])) { $settings[$pair[0]] = $pair[1] }
    }
}
$code = 1
try {
    foreach ($name in $settings.Keys) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name)
        [Environment]::SetEnvironmentVariable($name, $settings[$name])
    }
    if ($Prebuild) {
        # The files that hold shaders (README, "Getting started").
        $folder = Split-Path -Parent $GameExe
        $files = @($GameExe) + @('shaders.rpf', 'XboxBvhBuilder.dll', 'amd_fidelityfx_upscaler_xbox.dll' |
            ForEach-Object { Join-Path $folder $_ } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf })
        $used = if ($PrebuildAll) { @() } else { @('--used-pipelines') }
        Write-Host "Pre-building from $($files.Count) files. Ctrl+C stops it; the next one goes on from there." -ForegroundColor Cyan
        & $victory translate --cache --pipelines @used @files
    } else {
        if ($LosslessScaling -and -not (Get-Process LosslessScaling -ErrorAction SilentlyContinue)) {
            # Steam's number for it; Steam starts it, and says so if it is not there.
            try { Start-Process 'steam://rungameid/993090' } catch { Write-Warning "Lossless Scaling was not started: $_" }
        }
        Write-Host 'Starting. The first start translates shaders: parts of the picture come in late.' -ForegroundColor Cyan
        & $victory run $GameExe --execute --work $Work
    }
    $code = $LASTEXITCODE
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
}
exit $code
