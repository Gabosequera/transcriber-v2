# Synthetic stdio contract check. Port zero cannot connect to a host/editor.
$ErrorActionPreference = 'Stop'
$start = [System.Diagnostics.ProcessStartInfo]::new((Get-Command pwsh -ErrorAction Stop).Source)
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardInput = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$start.StandardInputEncoding = [System.Text.UTF8Encoding]::new($false)
$start.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
$start.StandardErrorEncoding = [System.Text.UTF8Encoding]::new($false)
foreach ($argument in @('-NoProfile', '-File', (Join-Path $PSScriptRoot 'mcp-client.ps1'), '-Endpoint', 'http://127.0.0.1:0/mcp', '-Stdio')) {
    $start.ArgumentList.Add($argument)
}
$start.Environment['TV2_MCP_TOKEN'] = 'synthetic-not-a-real-token'
$process = [System.Diagnostics.Process]::Start($start)
try {
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    foreach ($line in @('not-json', '[]', '{"jsonrpc":"2.0","id":"árbol","method":"ping"}', '{"jsonrpc":"2.0","method":"notifications/initialized"}')) {
        $process.StandardInput.WriteLine($line)
    }
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(20000)) {
        $process.Kill()
        throw 'Client fixture timed out.'
    }
    if ($process.ExitCode -ne 0) { throw "Client failed: $($errors.GetAwaiter().GetResult())" }
    $responses = @($output.GetAwaiter().GetResult() -split '\r?\n' | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })
    if ($responses.Count -ne 3) { throw "Expected exactly three responses, received $($responses.Count)." }
    if ($responses[0].error.code -ne -32700 -or $null -ne $responses[0].id) { throw 'Parse error response mismatch.' }
    if ($responses[1].error.code -ne -32600 -or $null -ne $responses[1].id) { throw 'Invalid request response mismatch.' }
    if ($responses[2].error.code -ne -32000 -or $responses[2].id -cne 'árbol') { throw 'HTTP failure lost request ID or UTF-8.' }
    if ($responses | Where-Object { $_.jsonrpc -ne '2.0' }) { throw 'Invalid response protocol.' }
    Write-Output 'PASS: parse/invalid-request/HTTP errors are structured; UTF-8 ID preserved; notification has no response; no editor or media launched.'
} finally {
    $process.Dispose()
}
