$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$python = (Get-Command python -ErrorAction Stop).Source
$report = Get-Content -LiteralPath (Join-Path $workspace 'implementation/evidence/e2/package-build.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$testRoot = Join-Path $env:TEMP "Transcriptor prueba ñ $stamp"
Expand-Archive -LiteralPath $report.archive -DestinationPath $testRoot
$package = Join-Path $testRoot (Split-Path $report.directory -Leaf)
$evidence = Join-Path $workspace "implementation/evidence/e2/package-smoke-$stamp"
New-Item -ItemType Directory -Path $evidence | Out-Null
& (Join-Path $package 'VERIFY.ps1') | Out-File -LiteralPath (Join-Path $evidence 'checksums.txt') -Encoding UTF8
if ($report.PSObject.Properties.Name -contains 'ffmpeg_bundled' -and -not $report.ffmpeg_bundled) {
    & (Join-Path $package 'Install-FFmpeg.ps1') | Out-File -LiteralPath (Join-Path $evidence 'install-ffmpeg.txt') -Encoding UTF8
    & (Join-Path $package 'Install-FFmpeg.ps1') | Out-File -LiteralPath (Join-Path $evidence 'install-ffmpeg-idempotent.txt') -Encoding UTF8
}

$steps = @(
    @{op='open';path=(Join-Path $package 'Demo.transcriptor/project.json')},
    @{op='assert';clips=4;markers=1},
    @{op='action';id='view.fit'},
    @{op='wait';ms=5500},
    @{op='pause'},
    @{op='seek';t=5.2},
    @{op='wait_frame'},
    @{op='wait';ms=500},
    @{op='assert';position=@(5.199,5.201);playing=$false},
    @{op='assert_caches';wave_columns_min=1;thumb_tiles_min=1},
    @{op='save_frame';path=(Join-Path $evidence 'viewer.png')},
    @{op='screenshot';path=(Join-Path $evidence 'portable.png')},
    @{op='export';preset='h264-720p';dest=(Join-Path $evidence 'portable.mp4')},
    @{op='wait_export'},
    @{op='quit'}
)
$scriptPath = Join-Path $evidence 'smoke.json'
$steps | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $scriptPath -Encoding UTF8
$oldPath = $env:PATH
$oldFfmpeg = $env:TRANSCRIPTOR_FFMPEG_DIR
$oldConfig = $env:TRANSCRIPTOR_CONFIG_DIR
$oldCache = $env:TRANSCRIPTOR_CACHE_DIR
try {
    $env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    $env:TRANSCRIPTOR_FFMPEG_DIR = $null
    $env:TRANSCRIPTOR_CONFIG_DIR = Join-Path $testRoot 'config'
    $env:TRANSCRIPTOR_CACHE_DIR = Join-Path $testRoot 'cache'
    $process = Start-Process -FilePath (Join-Path $package 'Transcriptor.exe') -ArgumentList '--script',('"' + $scriptPath + '"') -WorkingDirectory $testRoot -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(120000)) { throw 'Timeout del paquete; proceso conservado para diagnóstico' }
    $log = Get-Content -LiteralPath (Join-Path $evidence 'smoke.log') -Raw -Encoding UTF8
    $log | Write-Output
    if ($log -notmatch 'fallos=false') { throw 'El guion portable no terminó sin fallos' }
    $expected = @{
        source=(Join-Path $workspace 'tests/fixtures/media/fixture-sync.mp4'); duration=14
        checks=@(@{t=1.2;source_t=1.2;wrong_t=4.5},@{t=5.2;source_t=3.2;wrong_t=8.5})
        viewer_checks=@(@{t=5.2;viewer=(Join-Path $evidence 'viewer.png')})
        black_checks=@(@{t=3})
    }
    $expectPath = Join-Path $evidence 'expect.json'
    $expected | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $expectPath -Encoding UTF8
    & $python (Join-Path $workspace 'tests/scripts/verify_export.py') (Join-Path $evidence 'portable.mp4') $expectPath | Out-File -LiteralPath (Join-Path $evidence 'verify-export.txt') -Encoding UTF8
    if ($LASTEXITCODE -ne 0) { throw 'El medio portable no coincide con fuente/visor' }
    & (Join-Path $package 'VERIFY.ps1') | Out-File -LiteralPath (Join-Path $evidence 'checksums-after.txt') -Encoding UTF8
    @{package=$package;cwd=$testRoot;path=$env:PATH;archive_sha256=$report.sha256;evidence=$evidence;clean_windows_tested=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'environment.json') -Encoding UTF8
    $evidence | Set-Content -LiteralPath (Join-Path $workspace 'implementation/evidence/e2/latest-package-smoke.txt') -Encoding UTF8
} finally {
    $env:PATH = $oldPath
    $env:TRANSCRIPTOR_FFMPEG_DIR = $oldFfmpeg
    $env:TRANSCRIPTOR_CONFIG_DIR = $oldConfig
    $env:TRANSCRIPTOR_CACHE_DIR = $oldCache
}
