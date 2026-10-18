param(
    [Parameter(Mandatory=$true)][string]$Project,
    [Parameter(Mandatory=$true)][string]$EvidenceDir
)
$ErrorActionPreference = 'Stop'
if (-not [IO.Path]::IsPathFullyQualified($Project) -or -not (Test-Path -LiteralPath $Project)) {
    throw "Synthetic FilmCraft project is missing or relative: $Project"
}
[void](New-Item -ItemType Directory -Path $EvidenceDir -Force)
$app = (Resolve-Path (Join-Path $PSScriptRoot '..\target\debug\demostrike.exe')).Path
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LegacySmokeWindow {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr handle, out RECT bounds);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr handle, ref POINT point);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr handle, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr handle);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr handle);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr handle, System.Text.StringBuilder name, int max);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
}
'@
function Send-Chord([byte[]]$keys) {
    foreach ($key in $keys) { [LegacySmokeWindow]::keybd_event($key, 0, 0, [UIntPtr]::Zero) }
    Start-Sleep -Milliseconds 80
    for ($index = $keys.Length - 1; $index -ge 0; $index--) {
        [LegacySmokeWindow]::keybd_event($keys[$index], 0, 2, [UIntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 180
}
function Client-Origin([IntPtr]$handle) {
    $point = [LegacySmokeWindow+POINT]::new()
    if (-not [LegacySmokeWindow]::ClientToScreen($handle, [ref]$point)) { throw 'ClientToScreen failed' }
    return $point
}
function Click-Client([IntPtr]$handle, [int]$x, [int]$y) {
    $point = Client-Origin $handle
    [void][LegacySmokeWindow]::SetForegroundWindow($handle)
    [void][LegacySmokeWindow]::SetCursorPos($point.X + $x, $point.Y + $y)
    Start-Sleep -Milliseconds 200
    [LegacySmokeWindow]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [LegacySmokeWindow]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 500
}
function Save-Desktop-Diagnostic([IntPtr]$handle, [string]$name) {
    $client = [LegacySmokeWindow+RECT]::new()
    if (-not [LegacySmokeWindow]::GetClientRect($handle, [ref]$client)) { throw 'GetClientRect failed' }
    $origin = Client-Origin $handle
    $width = $client.Right - $client.Left
    $height = $client.Bottom - $client.Top
    $bitmap = [System.Drawing.Bitmap]::new($width, $height)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try { $graphics.CopyFromScreen($origin.X, $origin.Y, 0, 0, $bitmap.Size) }
        finally { $graphics.Dispose() }
        $path = Join-Path $EvidenceDir "$name-desktop-diagnostic.png"
        $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        $colors = [System.Collections.Generic.HashSet[int]]::new()
        for ($x = 40; $x -lt $width; $x += 47) {
            for ($y = 40; $y -lt $height; $y += 47) {
                [void]$colors.Add($bitmap.GetPixel($x, $y).ToArgb())
            }
        }
        $foreground = [LegacySmokeWindow]::GetForegroundWindow()
        Write-Output "LEGACY_DESKTOP_DIAGNOSTIC $name window=$handle foreground=$foreground origin=$($origin.X),$($origin.Y) size=${width}x$height sampled_colors=$($colors.Count) sha256=$((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash)"
    } finally { $bitmap.Dispose() }
}
function Wait-Name([IntPtr]$handle, [string]$name) {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($handle)
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::NameProperty, $name)
    for ($i = 0; $i -lt 60; $i++) {
        $element = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
        if ($element) {
            Write-Output "LEGACY_UI_FOUND $name"
            return $element
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Native UI element absent: $name"
}
function Wait-Dialog {
    for ($i = 0; $i -lt 80; $i++) {
        Start-Sleep -Milliseconds 250
        $foreground = [LegacySmokeWindow]::GetForegroundWindow()
        $class = [Text.StringBuilder]::new(128)
        [void][LegacySmokeWindow]::GetClassName($foreground, $class, $class.Capacity)
        if ($class.ToString() -eq '#32770') { return $foreground }
    }
    throw 'Open editable project did not open a native file dialog'
}
function Open-Dialog-Project([IntPtr]$dialog, [string]$project) {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($dialog)
    $editCondition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Edit)
    $edits = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $editCondition)
    $fileName = $null
    foreach ($edit in $edits) {
        $name = $edit.Current.Name
        $id = $edit.Current.AutomationId
        Write-Output "LEGACY_DIALOG_EDIT name=$name automation_id=$id"
        if ($id -eq '1148' -or $name -match '^File name:?$') { $fileName = $edit; break }
    }
    if (-not $fileName) { throw 'Native file dialog has no verified File name edit control' }
    $value = $null
    if (-not $fileName.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$value)) {
        throw 'Native File name control has no ValuePattern'
    }
    $value.SetValue($project)
    if ($value.Current.Value -ne $project) { throw 'Native File name control did not retain the project path' }
    Write-Output 'LEGACY_DIALOG_FILENAME_SET'
    $buttonCondition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)
    $buttons = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttonCondition)
    $open = $null
    foreach ($button in $buttons) {
        $name = $button.Current.Name
        $id = $button.Current.AutomationId
        Write-Output "LEGACY_DIALOG_BUTTON name=$name automation_id=$id"
        if ($name -eq 'Open' -or $id -eq '1') { $open = $button; break }
    }
    if (-not $open -or -not $open.Current.IsEnabled) { throw 'Native file dialog has no enabled Open button' }
    $invoke = $null
    if (-not $open.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) {
        throw 'Native Open button has no InvokePattern'
    }
    $invoke.Invoke()
    for ($i = 0; $i -lt 80; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not [LegacySmokeWindow]::IsWindow($dialog)) {
            Write-Output 'LEGACY_DIALOG_CLOSED_AFTER_OPEN'
            return
        }
    }
    throw 'Native file dialog remained open after invoking Open'
}
function Invoke-Return([IntPtr]$handle) {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($handle)
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::NameProperty, 'Return to analysis')
    for ($i = 0; $i -lt 60; $i++) {
        $button = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
        if ($button) {
            $pattern = $null
            if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke()
                return
            }
            $bounds = $button.Current.BoundingRectangle
            [void][LegacySmokeWindow]::SetForegroundWindow($handle)
            [void][LegacySmokeWindow]::SetCursorPos(
                [int][Math]::Round($bounds.X + $bounds.Width / 2),
                [int][Math]::Round($bounds.Y + $bounds.Height / 2))
            [LegacySmokeWindow]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
            [LegacySmokeWindow]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw 'Return to analysis button was absent from the native UI Automation tree'
}
$start = [Diagnostics.ProcessStartInfo]::new($app)
$start.UseShellExecute = $false
$start.RedirectStandardError = $true
$isolated = Join-Path $env:RUNNER_TEMP 'demostrike-legacy-native-profile'
[void](New-Item -ItemType Directory -Path $isolated -Force)
$start.Environment['LOCALAPPDATA'] = Join-Path $isolated 'local'
$start.Environment['APPDATA'] = Join-Path $isolated 'roaming'
$start.Environment['USERPROFILE'] = Join-Path $isolated 'profile'
$start.Environment['DEMOSTRIKE_EDITOR_CAPTURE_PROOF'] = '1'
foreach ($path in @($start.Environment['LOCALAPPDATA'], $start.Environment['APPDATA'], $start.Environment['USERPROFILE'])) {
    [void](New-Item -ItemType Directory -Path $path -Force)
}
$process = [Diagnostics.Process]::Start($start)
if (-not $process) { throw 'Native DemoStrike app did not start' }
$stderr = $process.StandardError.ReadToEndAsync()
try {
    $handle = [IntPtr]::Zero
    for ($i = 0; $i -lt 100; $i++) {
        Start-Sleep -Milliseconds 300
        $process.Refresh()
        if ($process.HasExited) { throw "Native DemoStrike exited before interaction: $($process.ExitCode)" }
        $handle = $process.MainWindowHandle
        if ($handle -ne [IntPtr]::Zero) { break }
    }
    if ($handle -eq [IntPtr]::Zero) { throw 'Native DemoStrike did not expose a window' }
    if (-not [LegacySmokeWindow]::MoveWindow($handle, 8, 8, 1000, 700, $true)) { throw 'Cannot size native window' }
    [void][LegacySmokeWindow]::SetForegroundWindow($handle)
    Start-Sleep -Seconds 3
    try { Save-Desktop-Diagnostic $handle 'empty' } catch { Write-Output "LEGACY_DESKTOP_DIAGNOSTIC_ERROR empty $_" }
    [void](Wait-Name $handle 'Other sources and projects')
    Click-Client $handle 220 55 # Saved matches
    try { Save-Desktop-Diagnostic $handle 'saved-matches' } catch { Write-Output "LEGACY_DESKTOP_DIAGNOSTIC_ERROR saved-matches $_" }
    [void](Wait-Name $handle 'No matches saved yet. Import a CS2 demo to build your searchable event library.')
    Click-Client $handle 395 55 # Other sources and projects
    Write-Output 'LEGACY_UI_MENU_OPENED'
    Click-Client $handle 390 111 # Open editable project
    $dialog = Wait-Dialog
    Open-Dialog-Project $dialog $Project
    Start-Sleep -Seconds 4
    $process.Refresh()
    if ($process.HasExited) { throw 'Native app exited while opening the FilmCraft project' }
    try { Save-Desktop-Diagnostic $handle 'filmcraft-editor' } catch { Write-Output "LEGACY_DESKTOP_DIAGNOSTIC_ERROR editor $_" }
    [void](Wait-Name $handle 'Return to analysis')
    Invoke-Return $handle
    Start-Sleep -Seconds 2
    try { Save-Desktop-Diagnostic $handle 'returned-analysis' } catch { Write-Output "LEGACY_DESKTOP_DIAGNOSTIC_ERROR returned $_" }
    [void](Wait-Name $handle 'Other sources and projects')
    Write-Output 'LEGACY_NATIVE_SMOKE menu project open and return completed'
} finally {
    if (-not $process.HasExited) {
        [void]$process.CloseMainWindow()
        if (-not $process.WaitForExit(3000)) { $process.Kill($true); [void]$process.WaitForExit(10000) }
    }
    if ($stderr.Wait(10000)) {
        $stderr.Result | Set-Content (Join-Path $EvidenceDir 'native-stderr.txt')
        Write-Output 'LEGACY_NATIVE_STDERR_BEGIN'
        Write-Output $stderr.Result
        Write-Output 'LEGACY_NATIVE_STDERR_END'
    }
    $process.Dispose()
}
if (-not ((Get-Content (Join-Path $EvidenceDir 'native-stderr.txt') -Raw) -match 'EDITOR_FRAME show_embedded_editor_done')) {
    throw 'Native FilmCraft editor frame was not observed'
}
