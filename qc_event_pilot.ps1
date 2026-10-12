param(
    [Parameter(Mandatory=$true)][string]$PilotRoot,
    [Parameter(Mandatory=$true)][string]$Ffmpeg,
    [Parameter(Mandatory=$true)][string]$Ffprobe,
    [string]$ReviewName = 'review'
)
$ErrorActionPreference = 'Stop'
function Get-LocalSha256([string]$Path) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $stream = [IO.File]::OpenRead($Path)
        try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant() }
        finally { $stream.Dispose() }
    } finally { $sha.Dispose() }
}
if ($ReviewName -cnotmatch '^review(?:-[A-Za-z0-9_-]{1,48})?$') {
    throw 'ReviewName must be review or a bounded review-* folder name.'
}
$pilot = [IO.Path]::GetFullPath($PilotRoot)
if (-not (Test-Path -LiteralPath (Join-Path $pilot 'event-pilot-manifest.json') -PathType Leaf)) {
    throw 'QC requires a staged event-pilot manifest in its selected output folder.'
}
$createdReview = $false
try {
if (Get-Process cs2,HLAE -ErrorAction SilentlyContinue) { throw 'Wait for the offline pilot to exit before media QC.' }
foreach ($path in @($Ffmpeg,$Ffprobe,(Join-Path $pilot 'runtime-console.log'))) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing input: $path" }
}
$review = Join-Path $pilot $ReviewName
if (Test-Path -LiteralPath $review) { throw 'Review folder already exists; refusing to overwrite media.' }
New-Item -ItemType Directory -Path $review | Out-Null
$createdReview = $true
$summary = @()
foreach ($name in @('pov','projectile','third')) {
    $folder = Join-Path $pilot "raw-$name\take0000"
    $video = Join-Path $folder 'dsEventClean\video.mp4'
    $wav = Join-Path $folder 'audio.wav'
    foreach ($path in @($video,$wav)) { if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing take: $path" } }
    foreach ($item in @(@{path=$video;map='0:v:0';label='raw-video'},@{path=$wav;map='0:a:0';label='raw-audio'})) {
        $diagnostic = Join-Path $review "$name-$($item.label)-decode.log"
        & $Ffmpeg -nostdin -hide_banner -v error -xerror -i $item.path -map $item.map -f null - 2>&1 |
            Out-File -LiteralPath $diagnostic -Encoding utf8
        if ($LASTEXITCODE -ne 0) { throw "Full decode failed: $($item.path)" }
    }
    $output = Join-Path $review "$name.mp4"
    $muxDiagnostic = Join-Path $review "$name-mux.log"
    & $Ffmpeg -nostdin -hide_banner -v error -xerror -i $video -i $wav -map 0:v:0 -map 1:a:0 -c:v copy -c:a aac -b:a 192k -shortest $output 2>&1 |
        Out-File -LiteralPath $muxDiagnostic -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw "Mux failed for $name" }
    $decodeDiagnostic = Join-Path $review "$name-mux-decode.log"
    & $Ffmpeg -nostdin -hide_banner -v error -xerror -i $output -map 0:v:0 -map 0:a:0 -f null - 2>&1 |
        Out-File -LiteralPath $decodeDiagnostic -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw "Muxed full decode failed for $name" }
    $signalDiagnostic = Join-Path $review "$name-signal.log"
    # volumedetect reports its measurements on stderr even when FFmpeg succeeds.
    # Windows PowerShell 5.1 promotes that native stderr to ErrorRecords under Stop.
    $savedErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Ffmpeg -nostdin -hide_banner -v info -xerror -i $output -map 0:a:0 -af volumedetect -f null - 2>&1 |
            Out-File -LiteralPath $signalDiagnostic -Encoding utf8
    } finally {
        $ErrorActionPreference = $savedErrorAction
    }
    if ($LASTEXITCODE -ne 0) { throw "Audio measurement failed for $name" }
    $signal = Select-String -LiteralPath $signalDiagnostic -Pattern 'max_volume: (-?[0-9.]+) dB' |
        Select-Object -Last 1
    if (-not $signal) { throw "No measured game audio for $name" }
    $peak = [double]$signal.Matches[0].Groups[1].Value
    if ($peak -le -60.0) { throw "Game audio too quiet for $name" }
    $probe = (& $Ffprobe -v error -show_streams -show_format -of json $output | Out-String | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw "FFprobe failed for $name" }
    $v = @($probe.streams | Where-Object codec_type -eq 'video')
    $a = @($probe.streams | Where-Object codec_type -eq 'audio')
    if ($v.Count -ne 1 -or $a.Count -ne 1) { throw "Unexpected streams for $name" }
    $summary += [pscustomobject]@{
        role=$name; rawVideo=$video; rawAudio=$wav; muxed=$output
        rawVideoSha256=(Get-LocalSha256 $video)
        rawAudioSha256=(Get-LocalSha256 $wav)
        muxedSha256=(Get-LocalSha256 $output)
        muxedBytes=(Get-Item -LiteralPath $output).Length
        rawAudioBytes=(Get-Item -LiteralPath $wav).Length
        videoCodec=$v[0].codec_name; width=$v[0].width; height=$v[0].height
        fps=$v[0].avg_frame_rate; frames=$v[0].nb_frames
        audioCodec=$a[0].codec_name; audioChannels=$a[0].channels
        audioSampleRate=$a[0].sample_rate; durationSeconds=$probe.format.duration
        maxAudioDb=$peak; rawVideoFullDecode=$true; rawAudioFullDecode=$true
        muxedFullDecode=$true
    }
}
$summaryJson = $summary | ConvertTo-Json -Depth 5
[IO.File]::WriteAllText((Join-Path $review 'media-qc.json'), $summaryJson,
    (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'Media QC summary written.'
} catch {
    if ($createdReview) {
        $message = [string]$_.Exception.Message
        if ($message.Length -gt 4000) { $message = $message.Substring(0,4000) }
        $failure = Join-Path $review 'qc-error.txt'
        try {
            $stream = [IO.File]::Open($failure, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
            try {
                $bytes = [Text.Encoding]::UTF8.GetBytes($message)
                $stream.Write($bytes,0,$bytes.Length)
                $stream.Flush($true)
            } finally { $stream.Dispose() }
        } catch { }
    }
    throw
}
