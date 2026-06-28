param(
    [string]$ReleaseDir = "release",
    [string]$Target = "windows-x86_64",
    [string]$OutputName = "Cbor2CliSetup-windows-x86_64.exe"
)

$ErrorActionPreference = "Stop"

function Fail($Message) {
    Write-Error $Message
    exit 1
}

$releasePath = (Resolve-Path -LiteralPath $ReleaseDir).ProviderPath
$cborAsset = Join-Path $releasePath "cbor-$Target.exe"
$outputPath = Join-Path $releasePath $OutputName

if (!(Test-Path -LiteralPath $cborAsset -PathType Leaf)) { Fail "Missing $cborAsset" }

$staging = Join-Path $env:TEMP ("cbor2-cli-installer-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $staging | Out-Null

try {
Copy-Item -LiteralPath $cborAsset -Destination (Join-Path $staging "cbor.exe") -Force

$setPathScript = @'
param(
    [Parameter(Mandatory=$true)]
    [string]$InstallDir
)

$ErrorActionPreference = "Stop"

function Add-PathPrefix($PathValue, $Directory) {
    $normalizedDirectory = [Environment]::ExpandEnvironmentVariables($Directory).TrimEnd("\")
    $entries = @()
    if (-not [string]::IsNullOrWhiteSpace($PathValue)) {
        foreach ($entry in ($PathValue -split ";")) {
            if ([string]::IsNullOrWhiteSpace($entry)) {
                continue
            }

            $normalizedEntry = [Environment]::ExpandEnvironmentVariables($entry).TrimEnd("\")
            if ([string]::Equals($normalizedEntry, $normalizedDirectory, [StringComparison]::OrdinalIgnoreCase)) {
                continue
            }

            $entries += $entry
        }
    }

    return (@($Directory) + $entries) -join ";"
}

function Send-EnvironmentChanged {
    try {
        if (-not ("Cbor2Cli.NativeMethods" -as [type])) {
            $signature = @"
using System;
using System.Runtime.InteropServices;

namespace Cbor2Cli {
    public static class NativeMethods {
        [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)]
        public static extern IntPtr SendMessageTimeout(
            IntPtr hWnd,
            UInt32 Msg,
            IntPtr wParam,
            string lParam,
            UInt32 fuFlags,
            UInt32 uTimeout,
            out IntPtr lpdwResult);
    }
}
"@
            Add-Type -TypeDefinition $signature | Out-Null
        }

        $result = [IntPtr]::Zero
        [Cbor2Cli.NativeMethods]::SendMessageTimeout(
            [IntPtr]0xffff,
            0x1a,
            [IntPtr]::Zero,
            "Environment",
            0x0002,
            5000,
            [ref]$result) | Out-Null
    } catch {
    }
}

$processPath = [Environment]::GetEnvironmentVariable("Path", "Process")
$updatedProcessPath = Add-PathPrefix $processPath $InstallDir
if ($updatedProcessPath -ne $processPath) {
    [Environment]::SetEnvironmentVariable("Path", $updatedProcessPath, "Process")
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$updatedUserPath = Add-PathPrefix $userPath $InstallDir
if ($updatedUserPath -ne $userPath) {
    [Environment]::SetEnvironmentVariable("Path", $updatedUserPath, "User")
    Send-EnvironmentChanged
}
'@

Set-Content -LiteralPath (Join-Path $staging "set-user-path.ps1") -Value $setPathScript -Encoding ASCII

$removePathScript = @'
param(
    [Parameter(Mandatory=$true)]
    [string]$InstallDir
)

$ErrorActionPreference = "Stop"

function Remove-PathEntry($PathValue, $Directory) {
    $normalizedDirectory = [Environment]::ExpandEnvironmentVariables($Directory).TrimEnd("\")
    $entries = @()
    if (-not [string]::IsNullOrWhiteSpace($PathValue)) {
        foreach ($entry in ($PathValue -split ";")) {
            if ([string]::IsNullOrWhiteSpace($entry)) {
                continue
            }

            $normalizedEntry = [Environment]::ExpandEnvironmentVariables($entry).TrimEnd("\")
            if ([string]::Equals($normalizedEntry, $normalizedDirectory, [StringComparison]::OrdinalIgnoreCase)) {
                continue
            }

            $entries += $entry
        }
    }

    return $entries -join ";"
}

function Send-EnvironmentChanged {
    try {
        if (-not ("Cbor2Cli.NativeMethods" -as [type])) {
            $signature = @"
using System;
using System.Runtime.InteropServices;

namespace Cbor2Cli {
    public static class NativeMethods {
        [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)]
        public static extern IntPtr SendMessageTimeout(
            IntPtr hWnd,
            UInt32 Msg,
            IntPtr wParam,
            string lParam,
            UInt32 fuFlags,
            UInt32 uTimeout,
            out IntPtr lpdwResult);
    }
}
"@
            Add-Type -TypeDefinition $signature | Out-Null
        }

        $result = [IntPtr]::Zero
        [Cbor2Cli.NativeMethods]::SendMessageTimeout(
            [IntPtr]0xffff,
            0x1a,
            [IntPtr]::Zero,
            "Environment",
            0x0002,
            5000,
            [ref]$result) | Out-Null
    } catch {
    }
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$updatedUserPath = Remove-PathEntry $userPath $InstallDir
if ($updatedUserPath -ne $userPath) {
    [Environment]::SetEnvironmentVariable("Path", $updatedUserPath, "User")
    Send-EnvironmentChanged
}
'@

Set-Content -LiteralPath (Join-Path $staging "remove-user-path.ps1") -Value $removePathScript -Encoding ASCII

$installScript = @'
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\cbor2-cli"
$StartMenuDir = Join-Path ([Environment]::GetFolderPath("Programs")) "cbor2-cli"
$PowerShellExe = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
if (!(Test-Path -LiteralPath $PowerShellExe)) {
    $PowerShellExe = "powershell.exe"
}

function New-InstallerForm {
    $form = New-Object System.Windows.Forms.Form
    $form.Text = "cbor2-cli Setup"
    $form.StartPosition = "CenterScreen"
    $form.FormBorderStyle = "FixedDialog"
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.ClientSize = New-Object System.Drawing.Size(460, 132)

    $label = New-Object System.Windows.Forms.Label
    $label.AutoSize = $false
    $label.Left = 18
    $label.Top = 18
    $label.Width = 424
    $label.Height = 42
    $label.Text = "Preparing cbor2-cli..."

    $progress = New-Object System.Windows.Forms.ProgressBar
    $progress.Left = 18
    $progress.Top = 72
    $progress.Width = 424
    $progress.Height = 22
    $progress.Minimum = 0
    $progress.Maximum = 100
    $progress.Style = "Continuous"

    $form.Controls.Add($label)
    $form.Controls.Add($progress)
    $form.Show()
    [System.Windows.Forms.Application]::DoEvents()

    return @{
        Form = $form
        Label = $label
        Progress = $progress
    }
}

function Set-InstallProgress($Value, $Message) {
    $value = [Math]::Max(0, [Math]::Min(100, [int]$Value))
    $script:InstallUi.Progress.Value = $value
    $script:InstallUi.Label.Text = $Message
    [System.Windows.Forms.Application]::DoEvents()
}

function Quote-ProcessArgument($Value) {
    $text = [string]$Value
    if ($text.Length -eq 0) {
        return '""'
    }
    if ($text -notmatch '[\s"]') {
        return $text
    }

    $quoted = '"'
    $backslashes = 0
    foreach ($ch in $text.ToCharArray()) {
        if ($ch -eq '\') {
            $backslashes += 1
            continue
        }
        if ($ch -eq '"') {
            $quoted += ('\' * ($backslashes * 2 + 1))
            $quoted += '"'
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            $quoted += ('\' * $backslashes)
            $backslashes = 0
        }
        $quoted += $ch
    }
    if ($backslashes -gt 0) {
        $quoted += ('\' * ($backslashes * 2))
    }
    $quoted += '"'
    return $quoted
}

function Start-HiddenProcess($FilePath, [string[]]$ArgumentList = @(), [switch]$IgnoreExitCode) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $FilePath
    $psi.Arguments = ($ArgumentList | ForEach-Object { Quote-ProcessArgument $_ }) -join " "
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden

    $process = [System.Diagnostics.Process]::Start($psi)
    $process.WaitForExit()
    if (!$IgnoreExitCode -and $process.ExitCode -ne 0) {
        throw "$FilePath failed with exit code $($process.ExitCode)."
    }
}

function Create-Shortcut($Path, $TargetPath, $WorkingDirectory, $Arguments = "") {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    $shortcut.TargetPath = $TargetPath
    $shortcut.Arguments = $Arguments
    $shortcut.WorkingDirectory = $WorkingDirectory
    $shortcut.WindowStyle = 1
    $shortcut.Save()
}

function Write-Uninstaller {
    $uninstall = Join-Path $InstallDir "uninstall.cmd"
    $lines = @(
        '@echo off',
        'setlocal EnableExtensions',
        'set "INSTALL_DIR=%LOCALAPPDATA%\Programs\cbor2-cli"',
        'set "START_MENU_DIR=%APPDATA%\Microsoft\Windows\Start Menu\Programs\cbor2-cli"',
        'set "POWERSHELL=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"',
        'if not exist "%POWERSHELL%" set "POWERSHELL=powershell.exe"',
        'if exist "%INSTALL_DIR%\remove-user-path.ps1" "%POWERSHELL%" -NoProfile -ExecutionPolicy Bypass -File "%INSTALL_DIR%\remove-user-path.ps1" -InstallDir "%INSTALL_DIR%"',
        'if exist "%START_MENU_DIR%" rmdir /S /Q "%START_MENU_DIR%"',
        'cd /D "%TEMP%"',
        'rmdir /S /Q "%INSTALL_DIR%"',
        'echo cbor2-cli has been uninstalled.',
        'pause'
    )
    Set-Content -Path $uninstall -Value $lines -Encoding ASCII
    return $uninstall
}

function Create-Shortcuts {
    New-Item -ItemType Directory -Force -Path $StartMenuDir | Out-Null
    $uninstall = Write-Uninstaller
    Create-Shortcut (Join-Path $StartMenuDir "Uninstall cbor2-cli.lnk") $uninstall $InstallDir
}

$script:InstallUi = New-InstallerForm

try {
    Set-InstallProgress 10 "Preparing folders..."
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    Set-InstallProgress 35 "Installing cbor.exe..."
    Copy-Item -Force -LiteralPath (Join-Path $ScriptRoot "cbor.exe") -Destination (Join-Path $InstallDir "cbor.exe")
    Copy-Item -Force -LiteralPath (Join-Path $ScriptRoot "remove-user-path.ps1") -Destination (Join-Path $InstallDir "remove-user-path.ps1")

    Set-InstallProgress 65 "Updating PATH..."
    Start-HiddenProcess $PowerShellExe @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $ScriptRoot "set-user-path.ps1"), "-InstallDir", $InstallDir)

    Set-InstallProgress 85 "Creating uninstall shortcut..."
    Create-Shortcuts

    Set-InstallProgress 100 "cbor2-cli has been installed."
    [System.Windows.Forms.MessageBox]::Show("cbor2-cli has been installed. Open a new terminal and run 'cbor --help'.", "cbor2-cli Setup", [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Information) | Out-Null
    $script:InstallUi.Form.Close()
    exit 0
} catch {
    Set-InstallProgress 100 "Installation failed."
    [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, "cbor2-cli Setup", [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Error) | Out-Null
    $script:InstallUi.Form.Close()
    exit 1
}
'@

Set-Content -LiteralPath (Join-Path $staging "install.ps1") -Value $installScript -Encoding ASCII

$sedPath = Join-Path $staging "cbor2-cli.sed"
$sed = @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=0
HideExtractAnimation=0
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=
DisplayLicense=
FinishMessage=
TargetName=$outputPath
FriendlyName=cbor2-cli Installer
AppLaunched=powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -WindowStyle Hidden -File install.ps1
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
SourceFiles=SourceFiles
[Strings]
FILE0=cbor.exe
FILE1=install.ps1
FILE2=set-user-path.ps1
FILE3=remove-user-path.ps1
[SourceFiles]
SourceFiles0=$staging
[SourceFiles0]
%FILE0%=
%FILE1%=
%FILE2%=
%FILE3%=
"@

Set-Content -LiteralPath $sedPath -Value $sed -Encoding ASCII
Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue

$iexpress = Join-Path $env:WINDIR "System32\iexpress.exe"
if (!(Test-Path -LiteralPath $iexpress -PathType Leaf)) { Fail "iexpress.exe not found" }

$process = Start-Process -FilePath $iexpress -ArgumentList @("/N", "/Q", $sedPath) -Wait -PassThru
$exitCode = $process.ExitCode
if ($null -ne $exitCode -and $exitCode -ne 0) {
    Fail "iexpress.exe failed with exit code $exitCode"
}

for ($i = 0; $i -lt 10 -and !(Test-Path -LiteralPath $outputPath -PathType Leaf); $i++) {
    Start-Sleep -Milliseconds 500
}

if (!(Test-Path -LiteralPath $outputPath -PathType Leaf)) { Fail "Installer was not created: $outputPath" }

Write-Host "Created $outputPath"
} finally {
    Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
}
