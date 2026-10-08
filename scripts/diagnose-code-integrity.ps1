<# Read-only diagnosis. Does not change policy, signatures, names or locations. #>
[CmdletBinding()]
param([string]$OutputPath)
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $OutputPath) { $OutputPath = Join-Path $workspace ('.local/code-integrity-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.json') }
$report = [ordered]@{ schema='tv2-environment-diagnostic/1'; observed_at=(Get-Date).ToString('o'); read_only=$true; workspace=$workspace; errors=@() }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$report.admin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
try { $report.smart_app_control = (Get-MpComputerStatus).SmartAppControlState } catch { $report.errors += $_.Exception.Message }
try {
    $guard = Get-CimInstance -Namespace root\Microsoft\Windows\DeviceGuard -ClassName Win32_DeviceGuard
    $report.device_guard = @{ kernel=$guard.CodeIntegrityPolicyEnforcementStatus; user_mode=$guard.UsermodeCodeIntegrityPolicyEnforcementStatus }
} catch { $report.errors += $_.Exception.Message }
try { $report.policies = (& CiTool.exe -lp -json | Out-String | ConvertFrom-Json); $report.citool_exit = $LASTEXITCODE } catch { $report.errors += $_.Exception.Message }
try {
    $report.policy_files = @(Get-ChildItem -LiteralPath 'C:\Windows\System32\CodeIntegrity\CiPolicies\Active' | Select-Object Name,Length,LastWriteTime)
} catch { $report.errors += $_.Exception.Message }
$report.binary_files = @()
foreach ($dir in @('target/debug/deps','target/release')) {
    $path = Join-Path $workspace $dir
    if (-not (Test-Path -LiteralPath $path)) { continue }
    foreach ($file in Get-ChildItem -LiteralPath $path -Filter '*.exe') {
        if ($file.Name -notmatch '^(tv2_(domain|application|control|v1compat|media|pipeline)-|transcriptor(-|\.))') { continue }
        $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
        $report.binary_files += @{ path=$file.FullName; length=$file.Length; modified=$file.LastWriteTime.ToString('o'); sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash; signature_status=$signature.Status.ToString() }
    }
}
try {
    $report.events = @(Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-CodeIntegrity/Operational'; Id=3033,3076,3077} -MaxEvents 12000 -ErrorAction Stop | ForEach-Object {
        [xml]$xml = $_.ToXml()
        $data = @{}
        foreach ($entry in $xml.Event.EventData.Data) { $data[$entry.Name] = $entry.'#text' }
        if (($data.Values -join ' ') -like ('*' + (Split-Path $workspace -Leaf) + '*')) { @{ time=$_.TimeCreated.ToString('o'); id=$_.Id; correlation=$xml.Event.System.Correlation.ActivityID; data=$data } }
    })
} catch { $report.errors += $_.Exception.Message }
$parent = Split-Path -Parent $OutputPath
if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
$report | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath $OutputPath -Encoding utf8
Write-Output "Diagnóstico de solo lectura guardado: $OutputPath"
Write-Output "Administrador: $($report.admin); SAC: $($report.smart_app_control); eventos del proyecto: $($report.events.Count)"
