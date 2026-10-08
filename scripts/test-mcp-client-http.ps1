# Synthetic loopback HTTP fixture: exercises the actual PowerShell client,
# never a V2 editor or ControlEngine. It cannot establish E4 GUI acceptance.
$ErrorActionPreference = 'Stop'
function Test-HttpClient([string[]]$Options, [string[]]$Lines, [object[]]$Replies, [string[]]$Methods) {
    $listener=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
    $listener.Start()
    $start=[System.Diagnostics.ProcessStartInfo]::new((Get-Command pwsh).Source)
    $start.UseShellExecute=$false
    $start.CreateNoWindow=$true
    $start.RedirectStandardInput=$true
    $start.RedirectStandardOutput=$true
    $start.RedirectStandardError=$true
    $start.StandardInputEncoding=[System.Text.UTF8Encoding]::new($false)
    $start.StandardOutputEncoding=[System.Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding=[System.Text.UTF8Encoding]::new($false)
    foreach ($argument in (@('-NoProfile','-File',(Join-Path $PSScriptRoot 'mcp-client.ps1'),'-Endpoint',"http://127.0.0.1:$($listener.LocalEndpoint.Port)/mcp")+$Options)) { $start.ArgumentList.Add($argument) }
    $start.Environment['TV2_MCP_TOKEN']='synthetic-http-token'
    $process=[System.Diagnostics.Process]::Start($start)
    try {
        $output=$process.StandardOutput.ReadToEndAsync()
        $errors=$process.StandardError.ReadToEndAsync()
        foreach ($line in $Lines) { $process.StandardInput.WriteLine($line) }
        $process.StandardInput.Close()
        for ($index=0; $index -lt $Replies.Count; $index++) {
            $accept=$listener.AcceptTcpClientAsync()
            if (-not $accept.Wait(15000)) { throw "Expected HTTP request $index did not arrive." }
            $connection=$accept.Result
            try {
                $stream=$connection.GetStream()
                $stream.ReadTimeout=5000
                $header=[System.Collections.Generic.List[byte]]::new()
                do {
                    $value=$stream.ReadByte()
                    if ($value -lt 0 -or $header.Count -ge 16384) { throw 'Invalid fixture HTTP header.' }
                    $header.Add([byte]$value)
                } until ($header.Count -ge 4 -and [System.Text.Encoding]::ASCII.GetString($header.GetRange($header.Count-4,4).ToArray()) -eq "`r`n`r`n")
                $headerText=[System.Text.Encoding]::ASCII.GetString($header.ToArray())
                if ($headerText -notmatch '(?im)^Authorization: Bearer synthetic-http-token\r?$') { throw 'Client authorization header mismatch.' }
                if ($headerText -notmatch '(?im)^MCP-Protocol-Version: 2025-11-25\r?$') { throw 'Client protocol header mismatch.' }
                if ($headerText -notmatch '(?im)^Content-Length: ([0-9]+)\r?$') { throw 'Missing content length.' }
                $body=[byte[]]::new([int]$Matches[1])
                $offset=0
                while ($offset -lt $body.Length) {
                    $count=$stream.Read($body,$offset,$body.Length-$offset)
                    if ($count -eq 0) { throw 'Truncated HTTP fixture request.' }
                    $offset+=$count
                }
                $request=[System.Text.Encoding]::UTF8.GetString($body) | ConvertFrom-Json -AsHashtable
                if ($request.method -cne $Methods[$index]) { throw "Unexpected method at request $index." }
                $reply=$Replies[$index]
                if ($null -eq $reply) { $status='202 Accepted'; $bytes=[byte[]]::new(0) }
                else {
                    $status='200 OK'
                    $reply.id=$request.id
                    $reply.jsonrpc='2.0'
                    $bytes=[System.Text.Encoding]::UTF8.GetBytes(($reply | ConvertTo-Json -Depth 100 -Compress))
                }
                $responseHeader=[System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: application/json`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
                $stream.Write($responseHeader,0,$responseHeader.Length)
                $stream.Write($bytes,0,$bytes.Length)
                $stream.Flush()
            } finally { $connection.Dispose() }
        }
        if (-not $process.WaitForExit(10000)) { throw 'Client did not finish.' }
        if ($process.ExitCode -ne 0) { throw ('Client failed: '+$errors.GetAwaiter().GetResult()) }
        if ($listener.Pending()) { throw 'Client emitted an unexpected extra HTTP request.' }
        return $output.GetAwaiter().GetResult()
    } finally {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
        $listener.Stop()
    }
}
$bridge=Test-HttpClient -Options @('-Stdio') -Lines @(
    '{"jsonrpc":"2.0","id":"árbol","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"synthetic","version":"1"}}}',
    '{"jsonrpc":"2.0","method":"notifications/initialized"}',
    '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"tv2_apply","arguments":{}}}'
) -Replies @(@{result=@{protocolVersion='2025-11-25'}},$null,@{result=@{isError=$true;structuredContent=@{error='E_REVIEW_REQUIRED: synthetic'}}}) -Methods @('initialize','notifications/initialized','tools/call')
$responses=@($bridge -split '\r?\n' | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })
if ($responses.Count -ne 2 -or $responses[0].id -cne 'árbol' -or $responses[1].id -ne 7 -or -not $responses[1].result.isError) { throw 'Successful stdio HTTP forwarding mismatch.' }
$verification=Test-HttpClient -Options @('-Tool','tv2_verify','-ArgumentsJson','{"session_id":"synthetic","project_id":"synthetic","proposal_id":"synthetic"}','-WaitForVerification') -Lines @() -Replies @(
    @{result=@{protocolVersion='2025-11-25'}},$null,
    @{result=@{isError=$false;structuredContent=@{verification='pending';matches_preview=$null;retry_after_ms=1}}},
    @{result=@{isError=$false;structuredContent=@{verification='complete';matches_preview=$true}}}
) -Methods @('initialize','notifications/initialized','tools/call','tools/call')
$verified=$verification | ConvertFrom-Json
if ($verified.result.structuredContent.verification -ne 'complete' -or $verified.result.structuredContent.matches_preview -ne $true -or $verified.id -ne 3) { throw 'Verification polling did not return the completed response.' }
Write-Output 'PASS: synthetic HTTP success forwards UTF-8 IDs/tool rejection; 202 notification is silent; mutation sent once; verification polls pending then complete. No editor or ControlEngine exercised.'
