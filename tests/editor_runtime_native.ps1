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
