<#
.SYNOPSIS
Local HTTP client or MCP stdio bridge for the running Transcriptor application.
.DESCRIPTION
Copy endpoint/token from the GUI External Control panel. Supply TV2_MCP_TOKEN
through the environment or the secure prompt, never a URL or persisted config.
Start with tv2_context, then use its session_id/project_id/revision/digest in
ArgumentsJson. Additional tools appear only after local permission grants.
.EXAMPLE
./scripts/mcp-client.ps1 -Endpoint http://127.0.0.1:12345/mcp -ListTools
.EXAMPLE
$scope = @{ session_id='control-...'; project_id='proj-...'; revision=7 }
$request = $scope + @{ idempotency_key='import-choice-1' }
./scripts/mcp-client.ps1 -Endpoint http://127.0.0.1:12345/mcp -Tool tv2_import -ArgumentsJson ($request | ConvertTo-Json -Compress)
Requests local selection only: pending_local_selection is not an imported asset.
Use tv2_jobs with the scope to follow the resulting ticket/job.
.EXAMPLE
$request = $scope + @{ proposal_id='proposal-...'; digest='current context digest'; idempotency_key='reprepare-2' }
./scripts/mcp-client.ps1 -Endpoint http://127.0.0.1:12345/mcp -Tool tv2_reprepare -ArgumentsJson ($request | ConvertTo-Json -Compress)
Review the NEW preview in the GUI, or explicitly grant its command types in the
local automatic scope, before tv2_apply and tv2_verify. The client cannot grant scope.
.EXAMPLE
$request = $scope + @{ asset_id='source-id'; section='words'; text='árbol'; limit=50 }
./scripts/mcp-client.ps1 -Endpoint http://127.0.0.1:12345/mcp -Tool tv2_evidence -ArgumentsJson ($request | ConvertTo-Json -Compress)
Text matches original scalar record fields case-insensitively before pagination.
.EXAMPLE
$request = $scope + @{ limit=50 }
./scripts/mcp-client.ps1 -Endpoint http://127.0.0.1:12345/mcp -Tool tv2_audit -ArgumentsJson ($request | ConvertTo-Json -Compress)
Pass next_cursor as cursor for the next page; changed archive digest requires a fresh first page.
.EXAMPLE
$request = @{ session_id='control-...'; project_id='proj-...'; proposal_id='proposal-...' }
./scripts/mcp-client.ps1 -Endpoint http://127.0.0.1:12345/mcp -Tool tv2_verify -ArgumentsJson ($request | ConvertTo-Json -Compress) -WaitForVerification
Waits up to 60 seconds while verification=pending. Without the switch, retry the
same tv2_verify request after retry_after_ms; null matches_preview is not success.
.EXAMPLE
pwsh -NoProfile -File scripts/mcp-client.ps1 -Endpoint http://127.0.0.1:12345/mcp -Stdio
Requires TV2_MCP_TOKEN in the environment; stdout contains JSON-RPC only.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][uri]$Endpoint,
    [string]$Token = $env:TV2_MCP_TOKEN,
    [string]$Tool = 'tv2_context',
    [string]$ArgumentsJson = '{}',
    [switch]$ListTools,
    [switch]$Stdio,
    [switch]$WaitForVerification
)
$ErrorActionPreference = 'Stop'
if ($WaitForVerification -and ($Tool -ne 'tv2_verify' -or $Stdio -or $ListTools)) {
    throw 'WaitForVerification requires -Tool tv2_verify without -Stdio or -ListTools.'
}
if ($Endpoint.Scheme -ne 'http' -or $Endpoint.Host -ne '127.0.0.1' -or $Endpoint.AbsolutePath -ne '/mcp' -or $Endpoint.Query -or $Endpoint.UserInfo) {
    throw 'Endpoint must be http://127.0.0.1:<port>/mcp.'
}
if (-not $Token) {
    if ($Stdio) { throw 'Set TV2_MCP_TOKEN before starting the stdio bridge.' }
    $secret = Read-Host 'Session token from the External Control panel' -AsSecureString
    $Token = [System.Net.NetworkCredential]::new('', $secret).Password
}
$headers = @{ Authorization = "Bearer $Token"; Accept = 'application/json, text/event-stream'; 'MCP-Protocol-Version' = '2025-11-25' }
function Send-Rpc([object]$Request) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($Request | ConvertTo-Json -Depth 100 -Compress))
    # Names are whitelisted, string IDs hashed: remote request contents may
    # contain private data even in method/tool/ID fields. Never log messages,
    # arguments, endpoint, headers or tokens, including exception messages.
    $methods = @('initialize','ping','tools/list','tools/call','notifications/initialized','notifications/cancelled')
    $tools = @('tv2_context','tv2_query','tv2_evidence','tv2_jobs','tv2_events','tv2_audit','tv2_propose','tv2_reprepare','tv2_proposals','tv2_preview','tv2_apply','tv2_verify','tv2_select','tv2_transport','tv2_cancel_job','tv2_export','tv2_import')
    $method = if ($Request.method -cin $methods) { $Request.method } else { 'other' }
    $toolName = if ($method -ceq 'tools/call' -and $Request.params.name -cin $tools) { $Request.params.name } else { 'none' }
    $id = if ($null -eq $Request.id) { 'none' }
        elseif ($Request.id -is [string]) { 'sha256:' + [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Request.id))).ToLowerInvariant() }
        elseif ($Request.id -is [ValueType] -and $Request.id -isnot [bool]) { 'number' }
        else { 'other' }
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $diagnostic = @{schema='tv2-mcp-client-diagnostic/1';method=$method;tool=$toolName;rpc_id=$id;status='complete'}
    # Normal client IDs are integer counters; arbitrary non-string values are
    # never formatted from the request into diagnostics.
    if ($Request.id -is [int] -or $Request.id -is [long]) { $diagnostic.rpc_id=$Request.id }
    try {
        Invoke-RestMethod -Method Post -Uri $Endpoint -Headers $headers -ContentType 'application/json' -Body $bytes -TimeoutSec 35
    } catch {
        $diagnostic.status='failed'
        $diagnostic.exceptions=@()
        $exception=$_.Exception
        for ($depth=0; $exception -and $depth -lt 8; $depth++) {
            $entry=@{type=$exception.GetType().FullName}
            if ($exception -is [Net.Sockets.SocketException]) {
                $entry.socket_error=$exception.SocketErrorCode.ToString()
                $entry.native_error=$exception.NativeErrorCode
            }
            $diagnostic.exceptions += $entry
            $exception=$exception.InnerException
        }
        throw
    } finally {
        $timer.Stop()
        $diagnostic.elapsed_ms=$timer.ElapsedMilliseconds
        [Console]::Error.WriteLine(($diagnostic | ConvertTo-Json -Depth 6 -Compress))
    }
}
if ($Stdio) {
    [Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false)
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
    # MCP stdio bridge: one JSON-RPC message per line, diagnostics on stderr.
    function Write-RpcFailure([object]$Id, [int]$Code, [string]$Message) {
        [Console]::WriteLine((@{jsonrpc='2.0';id=$Id;error=@{code=$Code;message=$Message}} | ConvertTo-Json -Depth 100 -Compress))
    }
    while ($null -ne ($line = [Console]::ReadLine())) {
        try {
            $request = $line | ConvertFrom-Json -AsHashtable
        } catch {
            Write-RpcFailure $null -32700 'Invalid JSON.'
            continue
        }
        if ($request -isnot [System.Collections.IDictionary] -or $request.jsonrpc -ne '2.0' -or $request.method -isnot [string]) {
            Write-RpcFailure $null -32600 'Invalid JSON-RPC request.'
            continue
        }
        try {
            $result = Send-Rpc $request
            # Invoke-RestMethod represents an empty HTTP 202 body as ''. MCP
            # notifications must never emit that string (or any reply) on stdout.
            if ($request.Contains('id') -and $null -ne $result -and $result -ne '') {
                [Console]::WriteLine(($result | ConvertTo-Json -Depth 100 -Compress))
            }
        } catch {
            # Send-Rpc already emitted safe phase/type diagnostics on stderr.
            # Do not strand the caller's request when HTTP fails. Never retry a
            # mutation: the host may already have committed before a timeout.
            if ($request.Contains('id')) {
                Write-RpcFailure $request.id -32000 'Local HTTP request failed; execution status is unknown. Query the receipt before retrying a mutation.'
            }
        }
    }
    exit
}
$initialize = Send-Rpc @{jsonrpc='2.0';id=1;method='initialize';params=@{protocolVersion='2025-11-25';capabilities=@{};clientInfo=@{name='tv2-local-client';version='1'}}}
if ($initialize.error) { throw ($initialize.error | ConvertTo-Json -Compress) }
$null = Send-Rpc @{jsonrpc='2.0';method='notifications/initialized'}
if ($ListTools) {
    Send-Rpc @{jsonrpc='2.0';id=2;method='tools/list';params=@{}} | ConvertTo-Json -Depth 100
} else {
    $arguments = $ArgumentsJson | ConvertFrom-Json -AsHashtable
    $verificationDeadline = [DateTime]::UtcNow.AddSeconds(60)
    $requestId = 2
    do {
        $response = Send-Rpc @{jsonrpc='2.0';id=$requestId;method='tools/call';params=@{name=$Tool;arguments=$arguments}}
        if (-not $WaitForVerification -or $response.error -or $response.result.isError -or $response.result.structuredContent.verification -ne 'pending') { break }
        if ([DateTime]::UtcNow -ge $verificationDeadline) { throw 'Verification is still pending after 60 seconds; retry the same request later.' }
        $verificationDelay = [Math]::Clamp([int]$response.result.structuredContent.retry_after_ms, 20, 1000)
        Start-Sleep -Milliseconds $verificationDelay
        $requestId++
    } while ($true)
    $response | ConvertTo-Json -Depth 100
}
