param(
    [Parameter(Mandatory=$true)][string]$SourceRoot,
    [string]$DestinationRoot = (Join-Path $PSScriptRoot 'capture-runtime')
)

$ErrorActionPreference = 'Stop'
$sourceCs2 = Join-Path $SourceRoot 'cs2-1.41.6.7'
$sourceHlae = Join-Path $SourceRoot 'hlae-2.190.2'
$targetCs2 = Join-Path $DestinationRoot 'cs2-1.41.6.7'
$targetHlae = Join-Path $DestinationRoot 'hlae-2.190.2'
$destinationFull = [IO.Path]::GetFullPath($DestinationRoot)
$ownedFull = [IO.Path]::GetFullPath($PSScriptRoot)
if (-not $destinationFull.StartsWith($ownedFull + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Capture runtime must stay within the DemoStrike directory.'
}
if (-not (Test-Path -LiteralPath (Join-Path $sourceCs2 'game\bin\win64\cs2.exe'))) { throw 'Historical CS2 is missing.' }
if (-not (Test-Path -LiteralPath (Join-Path $sourceHlae 'portable\HLAE.exe'))) { throw 'Historical HLAE is missing.' }
if (Test-Path -LiteralPath $DestinationRoot) { throw 'Capture runtime destination already exists; refusing partial overwrite.' }
$sourceGame = Join-Path $sourceCs2 'game'
$files = @(Get-ChildItem -LiteralPath $sourceGame -Recurse -File | Where-Object {
    $_.FullName -notmatch '\\bin\\win64\\recordings\\'
})
$copiedBytes = ($files | Where-Object Extension -ne '.vpk' | Measure-Object Length -Sum).Sum
$hlaeBytes = (Get-ChildItem -LiteralPath $sourceHlae -Recurse -File | Measure-Object Length -Sum).Sum
$freeBytes = (Get-PSDrive -Name ([IO.Path]::GetPathRoot($destinationFull).Substring(0,1))).Free
if ($freeBytes -lt ($copiedBytes + $hlaeBytes + 5GB)) { throw 'Insufficient free disk after a 5 GB reserve.' }

New-Item -ItemType Directory -Path $DestinationRoot -ErrorAction Stop | Out-Null
New-Item -ItemType Directory -Path $targetCs2 -ErrorAction Stop | Out-Null
Copy-Item -LiteralPath (Join-Path $sourceCs2 'daily-cs-nades-runtime-manifest.json') -Destination $targetCs2 -ErrorAction Stop
Copy-Item -LiteralPath (Join-Path $sourceCs2 'installscript.vdf') -Destination $targetCs2 -ErrorAction Stop
$linked = 0; $copied = 0
foreach ($file in $files) {
    $relative = $file.FullName.Substring($sourceCs2.Length + 1)
    $target = Join-Path $targetCs2 $relative
    $parent = Split-Path -Parent $target
    if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
    if ($file.Extension -ieq '.vpk') {
        New-Item -ItemType HardLink -Path $target -Target $file.FullName -ErrorAction Stop | Out-Null
        $linked++
    } else {
        Copy-Item -LiteralPath $file.FullName -Destination $target -ErrorAction Stop
        $copied++
    }
}
Copy-Item -LiteralPath $sourceHlae -Destination $targetHlae -Recurse -ErrorAction Stop
$result = [ordered]@{
    version = 1
    sourceCs2 = $sourceCs2
    sourceHlae = $sourceHlae
    targetCs2 = $targetCs2
    targetHlae = $targetHlae
    copiedGameFiles = $copied
    hardlinkedPackagedVpkAssets = $linked
    excluded = @('game/bin/win64/recordings')
    sourceCs2Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $sourceCs2 'game\bin\win64\cs2.exe')).Hash.ToLowerInvariant()
    targetCs2Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $targetCs2 'game\bin\win64\cs2.exe')).Hash.ToLowerInvariant()
    sourceHlaeSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $sourceHlae 'portable\HLAE.exe')).Hash.ToLowerInvariant()
    targetHlaeSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $targetHlae 'portable\HLAE.exe')).Hash.ToLowerInvariant()
}
$result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $DestinationRoot 'staging-provenance.json') -Encoding utf8
Write-Output ($result | ConvertTo-Json -Compress -Depth 5)
