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
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr handle, out RECT bounds);
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

    $bitmap = [System.Drawing.Bitmap]::new($width, $height)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bitmap.Size)
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
