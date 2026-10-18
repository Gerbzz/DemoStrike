param(
    [Parameter(Mandatory=$true)][string]$HandoffDir,
    [Parameter(Mandatory=$true)][string]$EvidenceDir
)

$ErrorActionPreference = 'Stop'
$app = Join-Path $PSScriptRoot '..\target\debug\demostrike.exe'
if (-not (Test-Path -LiteralPath $app)) { throw "Native DemoStrike editor binary is missing: $app" }

$cases = @(
    @{ Name = 'original-clipstrike-cues'; Project = (Join-Path $HandoffDir 'offline-edit.cstrike') },
    @{ Name = 'original-filmcraft-cues'; Project = (Join-Path $HandoffDir 'offline-edit.fcproj') },
    @{ Name = 'edited-clipstrike-synthetic'; Project = (Join-Path $EvidenceDir 'synthetic-edited-clipstrike.fcproj') },
    @{ Name = 'edited-filmcraft-synthetic'; Project = (Join-Path $EvidenceDir 'synthetic-edited-filmcraft.fcproj') }
)

Add-Type -AssemblyName System.Drawing
$jpeg = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() |
    Where-Object { $_.MimeType -eq 'image/jpeg' } | Select-Object -First 1
if (-not $jpeg) { throw 'Windows JPEG encoder is unavailable' }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class EditorNativeWindow {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr handle, out RECT bounds);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr handle, out RECT bounds);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr handle, ref POINT point);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr handle, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr handle);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr handle, System.Text.StringBuilder name, int max);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
}
'@

function Save-EditorWindow([System.Diagnostics.Process]$process, [string]$name) {
    Start-Sleep -Seconds 3
    $process.Refresh()
    if ($process.HasExited) { throw "Native editor exited before window capture: $name" }
    $handle = $process.MainWindowHandle
    Write-Output "EDITOR_WINDOW_HANDLE $name $handle"
    if ($handle -eq [IntPtr]::Zero) { throw "Native editor has no window handle: $name" }

    $screenWidth = [EditorNativeWindow]::GetSystemMetrics(0)
    $screenHeight = [EditorNativeWindow]::GetSystemMetrics(1)
    $targetWidth = [Math]::Min(1000, $screenWidth - 16)
    $targetHeight = [Math]::Min(700, $screenHeight - 64)
    if ($targetWidth -lt 900 -or $targetHeight -lt 560) { throw "Hosted desktop is too small for editor capture" }
    if (-not [EditorNativeWindow]::MoveWindow($handle, 8, 8, $targetWidth, $targetHeight, $true)) {
        throw "Could not fit native editor window inside hosted desktop: $name"
    }
    # The first WGPU window can expose its HWND before DWM installs the frame.
    # Poll the actual client geometry, reapplying the requested size if the
    # host is still presenting its temporary borderless startup surface.
    $frameReady = $false
    for ($attempt = 0; $attempt -lt 45; $attempt++) {
        Start-Sleep -Seconds 1
        $process.Refresh()
        if ($process.HasExited) { throw "Native editor exited while settling its window: $name" }
        $handle = $process.MainWindowHandle
        if ($handle -eq [IntPtr]::Zero) { continue }
        [void][EditorNativeWindow]::MoveWindow($handle, 8, 8, $targetWidth, $targetHeight, $true)
        $outerProbe = [EditorNativeWindow+RECT]::new()
        $clientProbe = [EditorNativeWindow+RECT]::new()
        $originProbe = [EditorNativeWindow+POINT]::new()
        if ([EditorNativeWindow]::GetWindowRect($handle, [ref]$outerProbe) -and
            [EditorNativeWindow]::GetClientRect($handle, [ref]$clientProbe) -and
            [EditorNativeWindow]::ClientToScreen($handle, [ref]$originProbe) -and
            ($clientProbe.Right - $clientProbe.Left) -lt ($outerProbe.Right - $outerProbe.Left) -and
            ($clientProbe.Bottom - $clientProbe.Top) -lt ($outerProbe.Bottom - $outerProbe.Top)) {
            $frameReady = $true
            Write-Output "EDITOR_FRAME_SETTLED $name attempt=$($attempt + 1)"
            break
        }
    }
    if (-not $frameReady) { throw "Native editor window frame did not settle within 45 seconds: $name" }
    Start-Sleep -Seconds 2
    $process.Refresh()
    Write-Output "EDITOR_WINDOW_RESPONDING $name $($process.Responding)"
    $bounds = [EditorNativeWindow+RECT]::new()
    if (-not [EditorNativeWindow]::GetWindowRect($handle, [ref]$bounds)) {
        throw "Could not read native editor bounds: $name"
    }
    $width = $bounds.Right - $bounds.Left
    $height = $bounds.Bottom - $bounds.Top
    Write-Output "EDITOR_WINDOW_BOUNDS $name x=$($bounds.Left) y=$($bounds.Top) size=$($width)x$height screen=$($screenWidth)x$screenHeight"
    if ($width -lt 900 -or $height -lt 560 -or $bounds.Left -lt 0 -or $bounds.Top -lt 0 -or
        $bounds.Right -gt $screenWidth -or $bounds.Bottom -gt ($screenHeight - 48)) {
        throw "Native editor extends beyond the hosted desktop work area: $name"
    }

    $client = [EditorNativeWindow+RECT]::new()
    $clientOrigin = [EditorNativeWindow+POINT]::new()
    if (-not [EditorNativeWindow]::GetClientRect($handle, [ref]$client) -or
        -not [EditorNativeWindow]::ClientToScreen($handle, [ref]$clientOrigin)) {
        throw "Could not read native editor client area: $name"
    }
    $clientWidth = $client.Right - $client.Left
    $clientHeight = $client.Bottom - $client.Top
    if ($clientWidth -ge $width -or $clientHeight -ge $height) {
        throw "Native editor window frame did not settle before capture: $name"
    }
    Write-Output "EDITOR_CLIENT_BOUNDS $name x=$($clientOrigin.X) y=$($clientOrigin.Y) size=$($clientWidth)x$clientHeight"
    if ($clientWidth -lt 850 -or $clientHeight -lt 500 -or $clientOrigin.X -lt 0 -or
        $clientOrigin.Y -lt 0 -or ($clientOrigin.X + $clientWidth) -gt $screenWidth -or
        ($clientOrigin.Y + $clientHeight) -gt ($screenHeight - 48)) {
        throw "Native editor client area extends beyond the hosted desktop work area: $name"
    }
    $bitmap = [System.Drawing.Bitmap]::new($clientWidth, $clientHeight)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($clientOrigin.X, $clientOrigin.Y, 0, 0, $bitmap.Size)
        } finally { $graphics.Dispose() }
        $png = Join-Path $EvidenceDir ($name + '.png')
        $bitmap.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
        $jpg = Join-Path $EvidenceDir ($name + '.jpg')
        $parameters = [System.Drawing.Imaging.EncoderParameters]::new(1)
        $parameters.Param[0] = [System.Drawing.Imaging.EncoderParameter]::new([System.Drawing.Imaging.Encoder]::Quality, [long]45)
        try { $bitmap.Save($jpg, $jpeg, $parameters) } finally { $parameters.Dispose() }
        $bytes = [System.IO.File]::ReadAllBytes($jpg)
        $encoded = [Convert]::ToBase64String($bytes)
        Write-Output "EDITOR_PREVIEW_BEGIN $name $($bytes.Length)"
        for ($offset = 0; $offset -lt $encoded.Length; $offset += 4096) {
            Write-Output $encoded.Substring($offset, [Math]::Min(4096, $encoded.Length - $offset))
        }
        Write-Output "EDITOR_PREVIEW_END $name"
        Write-Output "EDITOR_CAPTURE $name $($bitmap.Width)x$($bitmap.Height) SHA256=$((Get-FileHash -Algorithm SHA256 -LiteralPath $png).Hash.ToLowerInvariant())"
    } finally { $bitmap.Dispose() }
    if ($name -like 'edited-*' -and $name -notlike '*-timeline') {
        # Exercise FilmCraft's real ` Maximize Frame Under Cursor shortcut with
        # the pointer over the timeline, then capture all saved tracks at once.
        if (-not [EditorNativeWindow]::SetForegroundWindow($handle)) {
            throw "Could not focus native editor to expand its timeline: $name"
        }
        if (-not [EditorNativeWindow]::SetCursorPos($clientOrigin.X + 600, $clientOrigin.Y + 450)) {
            throw "Could not target native timeline panel: $name"
        }
        Start-Sleep -Milliseconds 250
        [EditorNativeWindow]::keybd_event([byte]0xC0, [byte]0, [uint32]0, [UIntPtr]::Zero)
        [EditorNativeWindow]::keybd_event([byte]0xC0, [byte]0, [uint32]2, [UIntPtr]::Zero)
        Write-Output "EDITOR_TIMELINE_MAXIMIZE_SENT $name"
        Start-Sleep -Seconds 3
        Save-EditorWindow $process ($name + '-timeline')
    }
}

function Send-Chord([byte[]]$keys) {
    foreach ($key in $keys) {
        [EditorNativeWindow]::keybd_event($key, [byte]0, [uint32]0, [UIntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 80
    for ($index = $keys.Length - 1; $index -ge 0; $index--) {
        [EditorNativeWindow]::keybd_event($keys[$index], [byte]0, [uint32]2, [UIntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 180
}

function Wait-FileDialog([string]$action) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        Start-Sleep -Milliseconds 250
        $foreground = [EditorNativeWindow]::GetForegroundWindow()
        $class = [System.Text.StringBuilder]::new(128)
        [void][EditorNativeWindow]::GetClassName($foreground, $class, $class.Capacity)
        if ($class.ToString() -eq '#32770') {
            Write-Host "EDITOR_GUI_DIALOG $action attempt=$($attempt + 1) handle=$foreground"
            return $foreground
        }
    }
    throw "Native $action file dialog did not appear"
}

function Choose-DialogPath([IntPtr]$dialog, [string]$path, [string]$action) {
    [void][EditorNativeWindow]::SetForegroundWindow($dialog)
    Set-Clipboard -Value $path
    Send-Chord ([byte[]]@(0x12, 0x4E)) # Alt+N focuses File name in Windows common dialogs.
    Send-Chord ([byte[]]@(0x11, 0x41)) # Ctrl+A
    Send-Chord ([byte[]]@(0x11, 0x56)) # Ctrl+V
    Send-Chord ([byte[]]@(0x0D))       # Enter
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        Start-Sleep -Milliseconds 250
        $foreground = [EditorNativeWindow]::GetForegroundWindow()
        $class = [System.Text.StringBuilder]::new(128)
        [void][EditorNativeWindow]::GetClassName($foreground, $class, $class.Capacity)
        if ($class.ToString() -ne '#32770') {
            Write-Output "EDITOR_GUI_DIALOG_CLOSED $action attempt=$($attempt + 1)"
            return
        }
    }
    throw "Native $action file dialog did not close after selecting $path"
}

function Wait-SavedEditorProject([string]$path, [string]$action) {
    # The common dialog can close before the embedded editor finishes writing.
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            Write-Output "EDITOR_GUI_SAVED $action path=$path attempt=$($attempt + 1)"
            return
        }
        Start-Sleep -Milliseconds 250
    }
    $directory = Split-Path -Parent $path
    $nearby = @(Get-ChildItem -LiteralPath $directory -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'gui-*' } |
        ForEach-Object { "$($_.Name):$($_.Length)" })
    Write-Output "EDITOR_GUI_SAVE_NEARBY $action directory=$directory files=$($nearby -join ',')"
    throw "GUI Save As did not write the $action project"
}

function Drag-EditorClient([IntPtr]$handle, [int]$fromX, [int]$fromY, [int]$toX, [int]$toY, [string]$action) {
    $origin = [EditorNativeWindow+POINT]::new()
    if (-not [EditorNativeWindow]::ClientToScreen($handle, [ref]$origin)) {
        throw "Could not locate editor client for $action"
    }
    if (-not [EditorNativeWindow]::SetForegroundWindow($handle)) {
        throw "Could not focus editor before $action"
    }
    $focused = $false
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        if ([EditorNativeWindow]::GetForegroundWindow() -eq $handle) {
            $focused = $true
            break
        }
        Start-Sleep -Milliseconds 100
    }
    if (-not $focused) { throw "Editor did not receive focus before $action" }
    [void][EditorNativeWindow]::SetCursorPos($origin.X + $fromX, $origin.Y + $fromY)
    Start-Sleep -Milliseconds 250
    if ($action -eq 'project-bin-to-V1') {
        # Give the freshly imported media tile a focused selection before
        # starting its native drag; otherwise the first mouse-down can be lost.
        [EditorNativeWindow]::mouse_event([uint32]2, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 100
        [EditorNativeWindow]::mouse_event([uint32]4, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 650
        Write-Output 'EDITOR_GUI_SOURCE_TILE_SELECTED'
    }
    [EditorNativeWindow]::mouse_event([uint32]2, 0, 0, 0, [UIntPtr]::Zero)
    for ($step = 1; $step -le 20; $step++) {
        $x = $origin.X + [int][Math]::Round($fromX + ($toX - $fromX) * $step / 20.0)
        $y = $origin.Y + [int][Math]::Round($fromY + ($toY - $fromY) * $step / 20.0)
        [void][EditorNativeWindow]::SetCursorPos($x, $y)
        Start-Sleep -Milliseconds 40
    }
    [EditorNativeWindow]::mouse_event([uint32]4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Seconds 2
    Write-Output "EDITOR_GUI_DRAG $action from=$fromX,$fromY to=$toX,$toY"
}

foreach ($case in $cases) {
    $project = $case.Project
    if (-not (Test-Path -LiteralPath $project)) { throw "Editor project is missing: $project" }
    Write-Output "EDITOR_CAPTURE_LAUNCH $($case.Name)"
    $start = [System.Diagnostics.ProcessStartInfo]::new($app)
    $start.UseShellExecute = $false
    $start.RedirectStandardError = $true
    $start.Environment['DEMOSTRIKE_EDITOR_CAPTURE_PROOF'] = '1'
    $recoveryRoot = Join-Path $env:RUNNER_TEMP ('demostrike-editor-recovery-' + $case.Name)
    [void](New-Item -ItemType Directory -Path $recoveryRoot -Force)
    $start.Environment['LOCALAPPDATA'] = $recoveryRoot
    [void]$start.ArgumentList.Add('--project')
    [void]$start.ArgumentList.Add($project)
    $process = [System.Diagnostics.Process]::Start($start)
    if (-not $process) { throw "Native editor process did not start for $($case.Name)" }
    $stderrTask = $process.StandardError.ReadToEndAsync()
    try {
        Save-EditorWindow $process $case.Name
    } finally {
        if (-not $process.HasExited) {
            [void]$process.CloseMainWindow()
            if (-not $process.WaitForExit(3000)) {
                $process.Kill($true)
                [void]$process.WaitForExit(10000)
            }
        }
        if ($stderrTask.Wait(10000)) {
            Write-Output "EDITOR_NATIVE_STDERR $($case.Name)"
            Write-Output $stderrTask.Result
        } else {
            Write-Output "EDITOR_NATIVE_STDERR_TIMEOUT $($case.Name)"
        }
        $process.Dispose()
    }
}

# This separate session exercises native controls rather than Session.execute:
# File > Import, project-bin drag, timeline move, Save As, and File > Open.
$guiSource = Join-Path $HandoffDir 'offline-edit.fcproj'
$guiSourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $guiSource).Hash
$guiMedia = Join-Path $EvidenceDir 'clearly-synthetic-stills\SYNTHETIC-pov-play.png'
$afterDrag = Join-Path $EvidenceDir 'gui-after-drag.fcproj'
$afterEdit = Join-Path $EvidenceDir 'gui-after-edit.fcproj'
if (-not (Test-Path -LiteralPath $guiMedia)) { throw "Synthetic GUI test image is missing: $guiMedia" }
$guiStart = [System.Diagnostics.ProcessStartInfo]::new($app)
$guiStart.UseShellExecute = $false
$guiStart.RedirectStandardError = $true
$guiStart.Environment['DEMOSTRIKE_EDITOR_CAPTURE_PROOF'] = '1'
$guiRecovery = Join-Path $env:RUNNER_TEMP 'demostrike-editor-recovery-gui-controls'
[void](New-Item -ItemType Directory -Path $guiRecovery -Force)
$guiStart.Environment['LOCALAPPDATA'] = $guiRecovery
[void]$guiStart.ArgumentList.Add('--project')
[void]$guiStart.ArgumentList.Add($guiSource)
Write-Output 'EDITOR_GUI_LAUNCH original FilmCraft handoff'
$guiProcess = [System.Diagnostics.Process]::Start($guiStart)
if (-not $guiProcess) { throw 'Native editor process did not start for GUI controls' }
$guiStderr = $guiProcess.StandardError.ReadToEndAsync()
try {
    Save-EditorWindow $guiProcess 'gui-original'
    $guiProcess.Refresh()
    $guiHandle = $guiProcess.MainWindowHandle
    if (-not [EditorNativeWindow]::SetForegroundWindow($guiHandle)) { throw 'Cannot focus GUI editor' }
    Send-Chord ([byte[]]@(0x11, 0x49)) # Ctrl+I, File > Import
    Choose-DialogPath (Wait-FileDialog 'import') $guiMedia 'import'
    Save-EditorWindow $guiProcess 'gui-imported'

    # One imported still appears in the second project-bin tile beside the sequence.
    Drag-EditorClient $guiHandle 195 430 605 475 'project-bin-to-V1'
    Save-EditorWindow $guiProcess 'gui-after-drag'
    [void][EditorNativeWindow]::SetForegroundWindow($guiHandle)
    Send-Chord ([byte[]]@(0x11, 0x10, 0x53)) # Ctrl+Shift+S, File > Save As
    Choose-DialogPath (Wait-FileDialog 'save-after-drag') $afterDrag 'save-after-drag'
    Wait-SavedEditorProject $afterDrag 'after-drag'

    Drag-EditorClient $guiHandle 660 475 730 475 'move-V1-clip'
    Save-EditorWindow $guiProcess 'gui-after-edit'
    [void][EditorNativeWindow]::SetForegroundWindow($guiHandle)
    Send-Chord ([byte[]]@(0x11, 0x10, 0x53)) # Ctrl+Shift+S, File > Save As
    Choose-DialogPath (Wait-FileDialog 'save-after-edit') $afterEdit 'save-after-edit'
    Wait-SavedEditorProject $afterEdit 'after-edit'

    [void][EditorNativeWindow]::SetForegroundWindow($guiHandle)
    Send-Chord ([byte[]]@(0x11, 0x4F)) # Ctrl+O, File > Open
    Choose-DialogPath (Wait-FileDialog 'reopen') $afterEdit 'reopen'
    Save-EditorWindow $guiProcess 'gui-reopened'
    Write-Output 'EDITOR_GUI_CONTROLS_COMPLETED native import/drag/edit/save/reopen'
} finally {
    if (-not $guiProcess.HasExited) {
        [void]$guiProcess.CloseMainWindow()
        if (-not $guiProcess.WaitForExit(3000)) {
            $guiProcess.Kill($true)
            [void]$guiProcess.WaitForExit(10000)
        }
    }
    if ($guiStderr.Wait(10000)) {
        Write-Output 'EDITOR_NATIVE_STDERR gui-controls'
        Write-Output $guiStderr.Result
    } else { Write-Output 'EDITOR_NATIVE_STDERR_TIMEOUT gui-controls' }
    $guiProcess.Dispose()
}
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $guiSource).Hash -ne $guiSourceHash) {
    throw 'Native GUI session modified the original offline handoff'
}

