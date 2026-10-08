[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$OutputDirectory,
    [string]$GenerationPayload='.local/e5-generation-host-02/payload.json')
$ErrorActionPreference='Stop'
$e5WorkflowRepo=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$e5WorkflowOutput=[System.IO.Path]::GetFullPath($OutputDirectory)
if (-not $e5WorkflowOutput.StartsWith($e5WorkflowRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) {throw 'Output must remain inside V2.'}
if (Test-Path -LiteralPath $e5WorkflowOutput) {throw 'Use a new evidence directory; preserve originals.'}
Push-Location -LiteralPath $e5WorkflowRepo
try {
    $e5WorkflowPayload=(Resolve-Path -LiteralPath $GenerationPayload).Path
    if (-not $e5WorkflowPayload.StartsWith($e5WorkflowRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) {throw 'Fixture payload must remain inside V2.'}
    [System.IO.Directory]::CreateDirectory($e5WorkflowOutput)>$null
    & pwsh -NoProfile -File scripts/cargo.ps1 build --locked -p tv2-pipeline --example workflow_host 2>&1 | Tee-Object -FilePath (Join-Path $e5WorkflowOutput 'build.log')
    if ($LASTEXITCODE -ne 0) {throw 'workflow_host build failed.'}
    $e5WorkflowExample=Join-Path $e5WorkflowRepo 'target/debug/examples/workflow_host.exe'
    & $e5WorkflowExample $e5WorkflowRepo (Join-Path $e5WorkflowOutput 'run') $e5WorkflowPayload 2>&1 | Tee-Object -FilePath (Join-Path $e5WorkflowOutput 'runtime.log')
    if ($LASTEXITCODE -ne 0) {throw 'First workflow_host execution failed; preserve logs and never retry a blocked binary.'}
    $e5WorkflowReport=Get-Content -LiteralPath (Join-Path $e5WorkflowOutput 'run/host-result.json') -Raw | ConvertFrom-Json
    if (-not $e5WorkflowReport.workflow_host_accepted -or $e5WorkflowReport.asr_inference -or $e5WorkflowReport.project_mutated) {throw 'Unexpected workflow host report.'}
    Get-FileHash -Algorithm SHA256 -LiteralPath $e5WorkflowExample,(Join-Path $e5WorkflowOutput 'run/host-result.json') | Format-List | Out-File -LiteralPath (Join-Path $e5WorkflowOutput 'sha256.txt')
    Write-Output ('PASS workflow generation-only over exact invented ASR parent: '+$e5WorkflowOutput)
} finally {Pop-Location}
