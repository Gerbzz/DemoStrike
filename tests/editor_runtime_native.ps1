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
}
'@

function Save-WindowProbe([System.Diagnostics.Process]$process, [string]$name) {
    Start-Sleep -Seconds 4
    try {
        $process.Refresh()
        $handle = $process.MainWindowHandle
        Write-Output "EDITOR_WINDOW_HANDLE $name $handle"
        if ($handle -eq [IntPtr]::Zero) { return }
        $bounds = [EditorNativeWindow+RECT]::new()
        if (-not [EditorNativeWindow]::GetWindowRect($handle, [ref]$bounds)) { return }
        $width = $bounds.Right - $bounds.Left
        $height = $bounds.Bottom - $bounds.Top
        $screenWidth = [EditorNativeWindow]::GetSystemMetrics(0)
        $screenHeight = [EditorNativeWindow]::GetSystemMetrics(1)
        Write-Output "EDITOR_WINDOW_BOUNDS $name x=$($bounds.Left) y=$($bounds.Top) size=$($width)x$height screen=$($screenWidth)x$screenHeight"
        if ($width -lt 900 -or $height -lt 560 -or $width -gt 3000 -or $height -gt 2000) { return }
        $bitmap = [System.Drawing.Bitmap]::new($width, $height)
        try {
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bitmap.Size)
            } finally { $graphics.Dispose() }
            $probe = Join-Path $EvidenceDir ($name + '-window-probe.png')
            $bitmap.Save($probe, [System.Drawing.Imaging.ImageFormat]::Png)
            $preview = Join-Path $EvidenceDir ($name + '-window-probe.jpg')
            $parameters = [System.Drawing.Imaging.EncoderParameters]::new(1)
            $parameters.Param[0] = [System.Drawing.Imaging.EncoderParameter]::new([System.Drawing.Imaging.Encoder]::Quality, [long]35)
            try { $bitmap.Save($preview, $jpeg, $parameters) } finally { $parameters.Dispose() }
            $bytes = [System.IO.File]::ReadAllBytes($preview)
            $encoded = [Convert]::ToBase64String($bytes)
            Write-Output "EDITOR_WINDOW_PREVIEW_BEGIN $name $($bytes.Length)"
            for ($offset = 0; $offset -lt $encoded.Length; $offset += 4096) {
                Write-Output $encoded.Substring($offset, [Math]::Min(4096, $encoded.Length - $offset))
            }
            Write-Output "EDITOR_WINDOW_PREVIEW_END $name"
        } finally { $bitmap.Dispose() }
    } catch {
        Write-Output "EDITOR_WINDOW_PROBE_ERROR $name $($_.Exception.Message)"
    }
}

foreach ($case in $cases) {
    $project = $case.Project
    if (-not (Test-Path -LiteralPath $project)) { throw "Editor project is missing: $project" }
    $png = Join-Path $EvidenceDir ($case.Name + '.png')
    Write-Output "EDITOR_CAPTURE_LAUNCH $($case.Name)"
    $start = [System.Diagnostics.ProcessStartInfo]::new($app)
    $start.UseShellExecute = $false
    $start.RedirectStandardError = $true
    $start.Environment['DEMOSTRIKE_EDITOR_CAPTURE_PROOF'] = '1'
    [void]$start.ArgumentList.Add('--project')
    [void]$start.ArgumentList.Add($project)
    [void]$start.ArgumentList.Add('--capture')
    [void]$start.ArgumentList.Add($png)
    $process = [System.Diagnostics.Process]::Start($start)
    if (-not $process) { throw "Native editor process did not start for $($case.Name)" }
    $stderrTask = $process.StandardError.ReadToEndAsync()
    Save-WindowProbe $process $case.Name
    try {
        if (-not $process.WaitForExit(90000)) {
            $process.Kill($true)
            [void]$process.WaitForExit(10000)
            throw "Native editor capture exceeded 90 seconds for $($case.Name)"
        }
        $exitCode = $process.ExitCode
    } finally {
        if ($stderrTask.Wait(10000)) {
            Write-Output "EDITOR_NATIVE_STDERR $($case.Name)"
            Write-Output $stderrTask.Result
        } else {
            Write-Output "EDITOR_NATIVE_STDERR_TIMEOUT $($case.Name)"
        }
        $process.Dispose()
    }
    if ($exitCode -ne 0 -or -not (Test-Path -LiteralPath $png)) {
        throw "Actual native editor failed to open and capture $project (exit $exitCode)"
    }
    $bitmap = [System.Drawing.Bitmap]::FromFile($png)
    try {
        if ($bitmap.Width -lt 900 -or $bitmap.Height -lt 560) {
            throw "Native editor screenshot dimensions are too small: $($bitmap.Width)x$($bitmap.Height)"
        }
        $jpg = Join-Path $EvidenceDir ($case.Name + '.jpg')
        $parameters = New-Object System.Drawing.Imaging.EncoderParameters(1)
        $parameters.Param[0] = [System.Drawing.Imaging.EncoderParameter]::new([System.Drawing.Imaging.Encoder]::Quality, [long]45)
        try { $bitmap.Save($jpg, $jpeg, $parameters) } finally { $parameters.Dispose() }
        $bytes = [System.IO.File]::ReadAllBytes($jpg)
        $encoded = [Convert]::ToBase64String($bytes)
        Write-Output "EDITOR_PREVIEW_BEGIN $($case.Name) $($bytes.Length)"
        for ($offset = 0; $offset -lt $encoded.Length; $offset += 4096) {
            Write-Output $encoded.Substring($offset, [Math]::Min(4096, $encoded.Length - $offset))
        }
        Write-Output "EDITOR_PREVIEW_END $($case.Name)"
        Write-Output "EDITOR_CAPTURE $($case.Name) $($bitmap.Width)x$($bitmap.Height) SHA256=$((Get-FileHash -Algorithm SHA256 -LiteralPath $png).Hash.ToLowerInvariant())"
    } finally { $bitmap.Dispose() }
}
