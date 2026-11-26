<#
.SYNOPSIS
  A small window over start.ps1: choose your title's executable, check this
  PC, install what is missing, run. Open it with Launcher.cmd.
.PARAMETER SelfTest
  Build the window and check it without showing it.
#>
[CmdletBinding()]
param([switch]$SelfTest)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()

$start = Join-Path $PSScriptRoot 'start.ps1'
# The choices are kept outside the source tree (docs/asset-policy.md).
$folder = Join-Path $env:LOCALAPPDATA 'victory'
$store = Join-Path $folder 'launcher.json'

function Read-Choices {
    $choices = @{ GameExe = ''; Work = 'balanced'; Lossless = $false }
    try {
        $read = Get-Content -LiteralPath $store -Raw -ErrorAction Stop | ConvertFrom-Json
        if ($read.GameExe) { $choices.GameExe = [string]$read.GameExe }
        if ($read.Work -in 'cpu', 'balanced', 'gpu') { $choices.Work = [string]$read.Work }
        $choices.Lossless = [bool]$read.Lossless
        # (A LoopLimit an earlier launcher kept there is left out on purpose:
        # it cut compute loops short.)
    } catch { }   # no file yet, or not one of ours: the defaults
    $choices
}

function Save-Choices($Choices) {
    try {
        New-Item -ItemType Directory -Force -Path $folder | Out-Null
        $Choices | ConvertTo-Json | Set-Content -LiteralPath $store -Encoding UTF8
    } catch {
        Write-Warning "the choices could not be saved: $_"
    }
}

# One argument as a Windows command line takes it.
function Format-Argument([string]$Text) { '"' + ($Text -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"' }

function Get-StartArguments([string[]]$More) {
    $line = @('-NoProfile', '-NoExit', '-ExecutionPolicy', 'Bypass', '-File', (Format-Argument $start))
    ($line + $More) -join ' '
}

function Get-PlayArguments([string]$Exe, [string]$Work, [bool]$Prebuild, [bool]$Lossless = $false) {
    '-GameExe'
    Format-Argument $Exe
    if ($Work -ne 'balanced') { "-Work $Work" }
    if ($Prebuild) { '-Prebuild' }
    if ($Lossless -and -not $Prebuild) { '-LosslessScaling' }
}

function Open-Console([string[]]$More) {
    Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') `
        -WorkingDirectory $PSScriptRoot -ArgumentList (Get-StartArguments $More)
}

$choices = Read-Choices

$form = New-Object System.Windows.Forms.Form
$form.Text = 'Vertex'
$form.ClientSize = New-Object System.Drawing.Size(560, 244)
$form.FormBorderStyle = 'FixedSingle'
$form.MaximizeBox = $false
$form.StartPosition = 'CenterScreen'
$form.Font = New-Object System.Drawing.Font('Segoe UI', 9)

function Add-Control($Control, [int]$X, [int]$Y, [int]$W, [int]$H, [string]$Text) {
    $Control.SetBounds($X, $Y, $W, $H)
    if ($Text) { $Control.Text = $Text }
    $form.Controls.Add($Control)
    $Control
}

$null = Add-Control (New-Object System.Windows.Forms.Label) 16 14 528 20 `
    'Executable of your own copy of the title (no game files come with Vertex):'
$path = Add-Control (New-Object System.Windows.Forms.TextBox) 16 36 430 24 $choices.GameExe
$browse = Add-Control (New-Object System.Windows.Forms.Button) 454 34 90 27 'Browse...'
$null = Add-Control (New-Object System.Windows.Forms.Label) 16 77 70 20 'Work split:'
# Not named $work: PowerShell names are not case-sensitive, and a parameter
# called $Work on the call stack would hide it (the "array index evaluated
# to null" of Play, Browse and Pre-build).
$workBox = Add-Control (New-Object System.Windows.Forms.ComboBox) 90 73 226 24
$workBox.DropDownStyle = 'DropDownList'
$works = 'cpu', 'balanced', 'gpu'
$workBox.Items.AddRange(@('Processor (saves card memory)', 'Balanced', 'Graphics card (more memory)'))
$index = [Array]::IndexOf($works, $choices.Work)
$workBox.SelectedIndex = if ($index -ge 0) { $index } else { 1 }

# The chosen work split; balanced when nothing is chosen.
function Get-WorkChoice {
    $index = $script:workBox.SelectedIndex
    if ($index -ge 0 -and $index -lt $works.Count) { $works[$index] } else { 'balanced' }
}
$prebuild = Add-Control (New-Object System.Windows.Forms.Button) 330 70 214 30 'Pre-build shaders'
$check = Add-Control (New-Object System.Windows.Forms.Button) 16 112 126 34 'Check this PC'
$install = Add-Control (New-Object System.Windows.Forms.Button) 150 112 160 34 'Install what is missing'
$keys = Add-Control (New-Object System.Windows.Forms.Button) 318 112 90 34 'Keys...'
$play = Add-Control (New-Object System.Windows.Forms.Button) 416 112 128 34 'Play'
$lossless = Add-Control (New-Object System.Windows.Forms.CheckBox) 16 154 528 22 'Start Lossless Scaling with the title (a Steam application of its own)'
$lossless.Checked = $choices.Lossless
$status = Add-Control (New-Object System.Windows.Forms.Label) 16 188 528 44 `
    'The first build takes minutes. The first start translates shaders and loads slowly: pre-build before it, and once more after it.'
$form.AcceptButton = $play

function Save-Form {
    Save-Choices @{ GameExe = $path.Text.Trim(); Work = (Get-WorkChoice); Lossless = $lossless.Checked }
}

# A button's work; what goes wrong is shown, not thrown out of the window.
function Invoke-Safely([scriptblock]$Action) {
    try { & $Action } catch { $status.Text = "Failed: $($_.Exception.Message)" }
}

$browse.Add_Click({ Invoke-Safely {
    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Filter = 'Executables (*.exe)|*.exe'
    if ($dialog.ShowDialog($form) -eq 'OK') { $path.Text = $dialog.FileName; Save-Form }
} })
$check.Add_Click({ Invoke-Safely { Open-Console @(); $status.Text = 'Building and checking in the other window.' } })
$install.Add_Click({ Invoke-Safely {
    Open-Console @('-Install')
    $status.Text = 'Installing, building and checking in the other window.'
} })

# The keys file is made by the first start of a title.
$keys.Add_Click({ Invoke-Safely {
    $data = if ($env:VICTORY_DATA_DIR) { $env:VICTORY_DATA_DIR } else { Join-Path $env:LOCALAPPDATA 'victory\data' }
    $file = Join-Path $data 'keys.txt'
    if (Test-Path -LiteralPath $file) {
        Start-Process -FilePath notepad.exe -ArgumentList (Format-Argument $file)
        $status.Text = 'Change the keys and save: a running title takes them within a second.'
    } else { $status.Text = 'Start the title once: that makes the file with the keys.' }
} })

# Plays the chosen title, or pre-builds its shaders.
function Start-Title([bool]$Ahead, [string]$Said) {
    $exe = $path.Text.Trim()
    if (-not $exe -or -not (Test-Path -LiteralPath $exe -PathType Leaf)) {
        $status.Text = 'Choose the executable first (Browse...).'
        return
    }
    Save-Form
    Open-Console (Get-PlayArguments $exe (Get-WorkChoice) $Ahead $lossless.Checked)
    $status.Text = $Said
}
$play.Add_Click({ Invoke-Safely { Start-Title $false 'Building, checking and starting in the other window.' } })
$prebuild.Add_Click({ Invoke-Safely { Start-Title $true 'Pre-building in the other window; Ctrl+C there stops it.' } })

if ($SelfTest) {
    if (-not (Test-Path -LiteralPath $start)) { throw "no $start" }
    if ((Format-Argument 'D:\a b\t.exe') -ne '"D:\a b\t.exe"') { throw 'quoting' }
    if ($form.Controls.Count -ne 12) { throw "controls: $($form.Controls.Count)" }
    $line = (Get-PlayArguments 'D:\a b\t.exe' 'gpu' $false) -join ' '
    if ($line -ne '-GameExe "D:\a b\t.exe" -Work gpu') { throw "play: $line" }
    $line = (Get-PlayArguments 'D:\a b\t.exe' 'balanced' $true) -join ' '
    if ($line -ne '-GameExe "D:\a b\t.exe" -Prebuild') { throw "pre-build: $line" }
    $line = (Get-PlayArguments 'D:\a b\t.exe' 'balanced' $false $true) -join ' '
    if ($line -ne '-GameExe "D:\a b\t.exe" -LosslessScaling') { throw "lossless: $line" }
    # The work split as a button's handler reads it (through Invoke-Safely).
    $status.Text = ''
    $script:seen = $null
    Invoke-Safely { $script:seen = Get-WorkChoice }
    if ($status.Text -or $script:seen -ne $choices.Work) { throw "work split in a handler: '$($script:seen)' $($status.Text)" }
    $workBox.SelectedIndex = 2
    Invoke-Safely { $script:seen = Get-WorkChoice }
    if ($status.Text -or $script:seen -ne 'gpu') { throw "work split after a change: '$($script:seen)' $($status.Text)" }
    $workBox.SelectedIndex = -1
    Invoke-Safely { $script:seen = Get-WorkChoice }
    if ($status.Text -or $script:seen -ne 'balanced') { throw "work split fallback: '$($script:seen)' $($status.Text)" }
    $form.Dispose()
    'launcher: ok'
    exit 0
}
[void]$form.ShowDialog()
Save-Form
$form.Dispose()
