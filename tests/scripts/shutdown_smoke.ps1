$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$evidence = Join-Path $workspace "implementation/evidence/e2/shutdown-$stamp"
New-Item -ItemType Directory -Path $evidence | Out-Null
$steps = @(
    @{op='import';path=(Join-Path $workspace 'tests/fixtures/media/fixture-a.mp4')},
    @{op='insert';asset=0;at=0},
    @{op='export';preset='h264-2160p';dest=(Join-Path $evidence 'active.mp4')},
    @{op='export';preset='h264-2160p';dest=(Join-Path $evidence 'queued.mp4')},
    @{op='wait';ms=200},
    @{op='quit'}
)
$scriptPath=Join-Path $evidence 'close.json'
$steps | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 -LiteralPath $scriptPath
$env:TRANSCRIPTOR_CONFIG_DIR = Join-Path $evidence 'config'
$env:TRANSCRIPTOR_CACHE_DIR = Join-Path $evidence 'cache'
$process = Start-Process -FilePath (Join-Path $workspace 'target/release/Transcriptor.exe') -ArgumentList '--script',$scriptPath -WorkingDirectory $env:TEMP -WindowStyle Hidden -PassThru
$owned = @{}
$owned[[int]$process.Id] = $true
$children = @{}
$clock = [Diagnostics.Stopwatch]::StartNew()
while (-not $process.HasExited -and $clock.Elapsed.TotalSeconds -lt 30) {
    $all = @(Get-CimInstance Win32_Process -Filter "Name='ffmpeg.exe' OR Name='ffprobe.exe'")
    foreach ($entry in $all) {
        if ($owned.ContainsKey([int]$entry.ParentProcessId)) {
            $owned[[int]$entry.ProcessId] = $true
            $children[[int]$entry.ProcessId] = @{pid=$entry.ProcessId;parent=$entry.ParentProcessId;name=$entry.Name;created=$entry.CreationDate}
        }
    }
    Start-Sleep -Milliseconds 50
    $process.Refresh()
}
if (-not $process.HasExited) { throw 'La app no terminó en30s; no se matan procesos para ocultar el fallo' }
$remaining=@(Get-CimInstance Win32_Process -Filter "Name='ffmpeg.exe' OR Name='ffprobe.exe'" | Where-Object {
    $children.ContainsKey([int]$_.ProcessId) -and $children[[int]$_.ProcessId].created -eq $_.CreationDate
})
$log=Get-Content -LiteralPath (Join-Path $evidence 'close.log') -Raw -Encoding UTF8
$outputs=@(Get-ChildItem -LiteralPath $evidence -Force -File | Where-Object {$_.Name -match 'active|queued|partial|audio.wav'})
$report=@{app_pid=$process.Id;elapsed_s=$clock.Elapsed.TotalSeconds;children=@($children.Values);remaining=@($remaining.ProcessId);outputs=@($outputs.Name);script_ok=($log -match 'fallos=false')}
$report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $evidence 'processes.json') -Encoding UTF8
if ($remaining.Count -or $outputs.Count -or -not $report.script_ok -or -not $children.Count) { throw 'Fallo: procesos/salidas residuales o no se observaron procesos reales' }
$evidence | Set-Content -LiteralPath (Join-Path $workspace 'implementation/evidence/e2/latest-shutdown.txt') -Encoding UTF8
$report | ConvertTo-Json -Depth 6
