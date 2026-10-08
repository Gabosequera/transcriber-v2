[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$AlignmentArousalFixture,
    [Parameter(Mandatory=$true)][string]$LaughterFixture,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$generationRepo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$generationOutput = [System.IO.Path]::GetFullPath($OutputDirectory)
if (-not $generationOutput.StartsWith($generationRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Output must stay within V2.' }
if (Test-Path -LiteralPath $generationOutput) { throw 'Use a new output directory; preserve previous attempts.' }
$generationParent = (Resolve-Path -LiteralPath $AlignmentArousalFixture).Path
$generationLaughter = (Resolve-Path -LiteralPath $LaughterFixture).Path
foreach ($generationInput in @($generationParent,$generationLaughter)) {
    if (-not $generationInput.StartsWith($generationRepo+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Only V2-owned fixture inputs are permitted.' }
}
function Read-GenerationJson([string]$Path) { Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json -AsHashtable -DateKind String }
[System.IO.Directory]::CreateDirectory($generationOutput) > $null
$generationPayload = @{
    parent = Read-GenerationJson (Join-Path $generationParent 'parent-asr.json')
    parent_result = Read-GenerationJson (Join-Path $generationParent 'parent-asr-result.json')
    alignments = @{ '0' = @{
        record = Read-GenerationJson (Join-Path $generationParent 'alignment-job.json')
        result = Read-GenerationJson (Join-Path $generationParent 'alignment-result.json')
    } }
    arousals = @{ '0' = @{
        record = Read-GenerationJson (Join-Path $generationParent 'arousal-job.json')
        result = Read-GenerationJson (Join-Path $generationParent 'arousal-result.json')
    } }
    laughter = @{ '0' = @{
        record = Read-GenerationJson (Join-Path $generationLaughter 'laughter-record.json')
        result = Read-GenerationJson (Join-Path $generationLaughter 'laughter-result.json')
    } }
    runtime = @{
        python = Join-Path $generationRepo '.local/e5-venv/Scripts/python.exe'
        worker = Join-Path $generationRepo 'workers/python/finalize_worker.py'
        assembler = Join-Path $generationRepo 'workers/python/finalize.py'
        derivation = Join-Path $generationRepo 'workers/python/worker.py'
        work_root = Join-Path $generationOutput 'run'
    }
    code_hashes = @{}
}
$generationPayload | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath (Join-Path $generationOutput 'payload.json') -Encoding utf8NoBOM
Push-Location -LiteralPath $generationRepo
try {
    & pwsh -NoProfile -File scripts/cargo.ps1 build -p tv2-pipeline --example generation_host --locked 2>&1 | Tee-Object -FilePath (Join-Path $generationOutput 'build.log')
    if ($LASTEXITCODE -ne 0) { throw 'Generation build failed; preserve its log.' }
    $generationExe = Join-Path $generationRepo 'target/debug/examples/generation_host.exe'
    & $generationExe $generationRepo (Join-Path $generationOutput 'payload.json') (Join-Path $generationParent 'project.json') 2>&1 | Tee-Object -FilePath (Join-Path $generationOutput 'runtime.log')
    if ($LASTEXITCODE -ne 0) { throw 'Generation host failed; inspect this attempt before any retry.' }
    $generationReport = Read-GenerationJson (Join-Path $generationOutput 'run/result.json')
    if (-not $generationReport.asr_fixture_only -or -not $generationReport.common_command_commit -or -not $generationReport.save_reopen_undo_redo) { throw 'Unexpected generation report.' }
    Get-FileHash -Algorithm SHA256 -LiteralPath $generationExe,(Join-Path $generationRepo 'workers/python/finalize.py'),(Join-Path $generationRepo 'workers/python/finalize_worker.py'),(Join-Path $generationOutput 'run/result.json') | Format-List | Out-File -LiteralPath (Join-Path $generationOutput 'sha256.txt')
    Write-Output ('PASS generation host with explicitly invented ASR parent: '+$generationOutput)
} finally { Pop-Location }
