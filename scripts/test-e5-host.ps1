<# Real Rust supervisor / Python hello / Windows kill-on-close smoke.
The generated fixture tests process lifecycle, not model inference. It launches
only its own paced FFmpeg silence stream to a null sink; no files/media output.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [string]$Source = (Join-Path $PSScriptRoot '../.local/e5-python-tests-20260930/fixtures/multistream.mkv'),
    [string]$Python = (Join-Path $PSScriptRoot '../.local/e5-venv/Scripts/python.exe'),
    [string]$Model = (Join-Path $PSScriptRoot '../.local/models/whisper-tiny')
)
$ErrorActionPreference='Stop'
$root=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$output=[System.IO.Path]::GetFullPath($OutputDirectory)
$sourcePath=(Resolve-Path -LiteralPath $Source).Path
foreach ($path in @($output,$sourcePath)) {
    if (-not $path.StartsWith($root+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Fixtures/output must stay inside V2.' }
}
if (Test-Path -LiteralPath $output) { throw 'Use a new output directory to preserve previous evidence.' }
[System.IO.Directory]::CreateDirectory($output) > $null
$fixture=Join-Path $output 'transport-only-owned-child.py'
@'
"""Synthetic NDJSON lifecycle fixture. No models, inference or project writes."""
import json, os, subprocess, sys, time
PROTOCOL='tv2-worker/1'
def emit(value):
    print(json.dumps({'protocol':PROTOCOL,**value}),flush=True)
for line in sys.stdin:
    request=json.loads(line)
    if request['method']=='hello':
        emit({'id':request['id'],'result':{'protocol':PROTOCOL,'fixture':'transport-only; no inference'}})
    elif request['method']=='run':
        params=request['params']
        child=subprocess.Popen([params['ffmpeg_path'],'-hide_banner','-loglevel','error','-nostdin','-re','-f','lavfi','-i','anullsrc=r=16000:cl=mono','-t','3600','-f','null','-'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
        emit({'event':'progress','job_id':params['job_id'],'stage':'owned-native-child','fraction':0,'worker_pid':os.getpid(),'ffmpeg_pid':child.pid})
    # Deliberately ignore cancel/shutdown to exercise host grace/kill-on-close.
while True:
    time.sleep(30)
'@ | Set-Content -LiteralPath $fixture -Encoding utf8
$buildLog=Join-Path $output 'build-debug-example.log'
& pwsh -NoProfile -File (Join-Path $PSScriptRoot 'cargo.ps1') build --locked -p tv2-pipeline --example runtime_smoke 2>&1 | Tee-Object -FilePath $buildLog
if ($LASTEXITCODE -ne 0) { throw "Debug example build failed ($LASTEXITCODE); see $buildLog." }
$binary=Join-Path $root 'target/debug/examples/runtime_smoke.exe'
$worker=Join-Path $root 'workers/python/worker.py'
$workRoot=Join-Path $output 'work'
$result=Join-Path $output 'result.json'
$oldFfmpeg=$env:TRANSCRIPTOR_FFMPEG_DIR
try {
    $env:TRANSCRIPTOR_FFMPEG_DIR=Split-Path (Get-Command ffmpeg -ErrorAction Stop).Source
    & $binary (Resolve-Path -LiteralPath $Python).Path $worker (Resolve-Path -LiteralPath $Model).Path $workRoot $sourcePath $fixture $result 2>&1 | Tee-Object -FilePath (Join-Path $output 'runtime.log')
    if ($LASTEXITCODE -ne 0) { throw "Runtime smoke failed ($LASTEXITCODE); preserve evidence and diagnose before retrying." }
    $observed=Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
    if (-not $observed.accepted_host_smoke -or $observed.inference_accepted) { throw 'Host/inference acceptance classification mismatch.' }
    Get-FileHash -LiteralPath $binary,$worker,$fixture,$sourcePath,$result -Algorithm SHA256 | Format-List | Out-File -LiteralPath (Join-Path $output 'sha256.txt') -Encoding utf8
    Write-Output "PASS: Rust/Python host lifecycle verified; inference remains blocked/unaccepted. $result"
} finally { $env:TRANSCRIPTOR_FFMPEG_DIR=$oldFfmpeg }
