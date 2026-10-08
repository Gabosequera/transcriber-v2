[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [string]$KnownTranscript='.local/e5-mms-real-02/known-transcript.json'
)
$ErrorActionPreference='Stop'
$e5AlignmentRepo=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$e5AlignmentOutput=[System.IO.Path]::GetFullPath($OutputDirectory)
if (-not $e5AlignmentOutput.StartsWith($e5AlignmentRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Output must remain inside V2.' }
if (Test-Path -LiteralPath $e5AlignmentOutput) { throw 'Use a new evidence directory; originals are preserved.' }
$e5AlignmentTranscript=(Resolve-Path -LiteralPath $KnownTranscript).Path
if (-not $e5AlignmentTranscript.StartsWith($e5AlignmentRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Known invented transcript must remain inside V2.' }
$e5AlignmentWords=(Get-Content -LiteralPath $e5AlignmentTranscript -Raw | ConvertFrom-Json).words
$e5AlignmentText=($e5AlignmentWords | ForEach-Object { $_.text }) -join ' '
if ($e5AlignmentText -ne 'This is a synthetic recording for a local software test. The blue notebook is on the table. We will meet tomorrow at nine.') { throw 'This smoke only accepts the known invented SAPI transcript.' }
[System.IO.Directory]::CreateDirectory($e5AlignmentOutput) > $null
Push-Location -LiteralPath $e5AlignmentRepo
try {
    & pwsh -NoProfile -File scripts/cargo.ps1 build --locked -p tv2-pipeline --example alignment_host 2>&1 | Tee-Object -FilePath (Join-Path $e5AlignmentOutput 'build.log')
    if ($LASTEXITCODE -ne 0) { throw 'alignment_host build failed; see build.log.' }
    $e5AlignmentExample=Join-Path $e5AlignmentRepo 'target/debug/examples/alignment_host.exe'
    $e5AlignmentRun=Join-Path $e5AlignmentOutput 'run'
    & $e5AlignmentExample $e5AlignmentRepo $e5AlignmentRun $e5AlignmentTranscript 2>&1 | Tee-Object -FilePath (Join-Path $e5AlignmentOutput 'runtime.log')
    if ($LASTEXITCODE -ne 0) { throw 'alignment_host first execution failed; preserve logs and do not retry a blocked binary.' }
    $e5AlignmentReport=Get-Content -LiteralPath (Join-Path $e5AlignmentRun 'host-result.json') -Raw | ConvertFrom-Json
    if (-not $e5AlignmentReport.mms_host_accepted -or $e5AlignmentReport.asr_exercised -or -not $e5AlignmentReport.project_unchanged) { throw 'Unexpected MMS host report.' }
    Get-FileHash -Algorithm SHA256 -LiteralPath $e5AlignmentExample,(Join-Path $e5AlignmentRepo 'workers/python/alignment_worker.py'),(Join-Path $e5AlignmentRun 'host-result.json') | Format-List | Out-File -LiteralPath (Join-Path $e5AlignmentOutput 'sha256.txt')
    Write-Output ('PASS standalone Rust/MMS host over invented parent transcript: '+$e5AlignmentOutput)
} finally { Pop-Location }
