[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [string]$ExistingParentFixture=''
)
$ErrorActionPreference='Stop'
$e5LaughterRepo=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$e5LaughterOutput=[System.IO.Path]::GetFullPath($OutputDirectory)
if (-not $e5LaughterOutput.StartsWith($e5LaughterRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Output must remain inside V2.' }
if (Test-Path -LiteralPath $e5LaughterOutput) { throw 'Use a new evidence directory; originals are preserved.' }
$e5LaughterParent=$null
if ($ExistingParentFixture) {
    $e5LaughterParent=(Resolve-Path -LiteralPath $ExistingParentFixture).Path
    if (-not $e5LaughterParent.StartsWith($e5LaughterRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Existing parent fixture must remain inside V2.' }
}
[System.IO.Directory]::CreateDirectory($e5LaughterOutput) > $null
Push-Location -LiteralPath $e5LaughterRepo
try {
    & pwsh -NoProfile -File scripts/cargo.ps1 build --locked -p tv2-pipeline --example laughter_host 2>&1 | Tee-Object -FilePath (Join-Path $e5LaughterOutput 'build.log')
    if ($LASTEXITCODE -ne 0) { throw 'laughter_host build failed; see build.log.' }
    $e5LaughterExample=Join-Path $e5LaughterRepo 'target/debug/examples/laughter_host.exe'
    $e5LaughterRun=Join-Path $e5LaughterOutput 'run'
    $e5LaughterArguments=@($e5LaughterRepo,$e5LaughterRun)
    if ($e5LaughterParent) { $e5LaughterArguments+=$e5LaughterParent }
    & $e5LaughterExample @e5LaughterArguments 2>&1 | Tee-Object -FilePath (Join-Path $e5LaughterOutput 'runtime.log')
    if ($LASTEXITCODE -ne 0) { throw 'laughter_host first execution failed; preserve logs and do not retry a blocked binary.' }
    $e5LaughterReport=Get-Content -LiteralPath (Join-Path $e5LaughterRun 'host-result.json') -Raw | ConvertFrom-Json
    if (-not $e5LaughterReport.laughter_host_accepted -or $e5LaughterReport.asr_exercised -or -not $e5LaughterReport.project_unchanged) { throw 'Unexpected laughter host report.' }
    Get-FileHash -Algorithm SHA256 -LiteralPath $e5LaughterExample,(Join-Path $e5LaughterRepo 'workers/python/laughter_worker.py'),(Join-Path $e5LaughterRun 'host-result.json') | Format-List | Out-File -LiteralPath (Join-Path $e5LaughterOutput 'sha256.txt')
    Write-Output ('PASS standalone Rust/laughter host over invented parent: '+$e5LaughterOutput)
} finally { Pop-Location }
