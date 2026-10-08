<# Real TCP/HTTP client -> running native editor. Grants are trusted local script
actions, not remote tools. This does not claim a person's visual review. #>
[CmdletBinding()]
param([string]$Exe, [string]$Fixture)
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $Exe) { $Exe = Join-Path $workspace 'target/release/Transcriptor.exe' }
if (-not $Fixture) { $Fixture = Join-Path $workspace 'tests/fixtures/media/fixture-a.mp4' }
$runRoot = Join-Path $workspace ('implementation/evidence/local/mcp-real-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
$scriptPath = Join-Path $runRoot 'scenario.json'
$project = Join-Path $runRoot 'fixture.transcriptor'
$phase1 = Join-Path $runRoot 'phase1.done'
$phase2 = Join-Path $runRoot 'phase2.done'
$phase3 = Join-Path $runRoot 'phase3.done'
$pipe1Name = 'tv2-acceptance-' + [guid]::NewGuid().ToString('N')
$pipe2Name = 'tv2-acceptance-' + [guid]::NewGuid().ToString('N')
function New-TestPipe([string]$Name) {
    return [IO.Pipes.NamedPipeServerStream]::new($Name,[IO.Pipes.PipeDirection]::In,1,[IO.Pipes.PipeTransmissionMode]::Byte,([IO.Pipes.PipeOptions]::Asynchronous -bor [IO.Pipes.PipeOptions]::CurrentUserOnly))
}
$pipe1 = New-TestPipe $pipe1Name
$pipe2 = New-TestPipe $pipe2Name
function Receive-Session($Pipe) {
    $timeout = [Threading.CancellationTokenSource]::new(45000)
    try {
        $Pipe.WaitForConnectionAsync($timeout.Token).GetAwaiter().GetResult()
        $reader = [IO.StreamReader]::new($Pipe,[Text.Encoding]::UTF8,$false,4096,$true)
        try { return ($reader.ReadLineAsync().WaitAsync([TimeSpan]::FromSeconds(15)).GetAwaiter().GetResult() | ConvertFrom-Json -AsHashtable) } finally { $reader.Dispose() }
    } finally { $timeout.Dispose() }
}
function Test-DelayedRequest($Session) {
    # Real TCP: connect before sending anything, then split headers/body.
    # Accepted Windows sockets must wait instead of failing WSAEWOULDBLOCK.
    $endpoint=[uri]$Session.endpoint
    $socket=[Net.Sockets.TcpClient]::new()
    $timer=[Diagnostics.Stopwatch]::StartNew()
    try {
        $socket.Connect('127.0.0.1',$endpoint.Port)
        $socket.ReceiveTimeout=5000; $socket.SendTimeout=5000
        $stream=$socket.GetStream()
        $body=[Text.Encoding]::UTF8.GetBytes('{"jsonrpc":"2.0","id":8801,"method":"ping"}')
        $header=[Text.Encoding]::UTF8.GetBytes("POST /mcp HTTP/1.1`r`nHost: 127.0.0.1:$($endpoint.Port)`r`nAuthorization: Bearer $($Session.token)`r`nContent-Type: application/json`r`nContent-Length: $($body.Length)`r`nConnection: close`r`n`r`n")
        Start-Sleep -Milliseconds 200
        $stream.Write($header,0,10)
        Start-Sleep -Milliseconds 100
        $stream.Write($header,10,$header.Length-10)
        Start-Sleep -Milliseconds 100
        $stream.Write($body,0,$body.Length)
        $reader=[IO.StreamReader]::new($stream,[Text.Encoding]::UTF8,$false,4096,$true)
        try {
            $status=$reader.ReadLine()
            if ($status -ne 'HTTP/1.1 200 OK') { throw 'Delayed request did not receive HTTP200.' }
            $length=0
            while ($null -ne ($line=$reader.ReadLine()) -and $line -ne '') {
                if ($line -match '^Content-Length: (\d+)$') { $length=[int]$Matches[1] }
            }
            if ($length -lt 1 -or $length -gt 4096) { throw 'Delayed reply length invalid.' }
            $textBuffer=[char[]]::new($length)
            $count=0
            while ($count -lt $length) {
                $read=$reader.Read($textBuffer,$count,$length-$count)
                if ($read -eq 0) { throw 'Delayed reply was truncated.' }
                $count+=$read
            }
            $reply=(-join $textBuffer) | ConvertFrom-Json -AsHashtable
            if ($reply.id -ne 8801 -or $reply.error -or -not $reply.ContainsKey('result')) { throw 'Delayed ping reply mismatch.' }
            @{status=$status;response=$reply;elapsed_ms=$timer.ElapsedMilliseconds;delayed_headers_and_body=$true;accepted=$true} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $runRoot 'delayed-http.json') -Encoding utf8
        } finally { $reader.Dispose() }
    } finally { $socket.Dispose() }
}
$permissions = @{read=$true;propose=$true;apply=$true;selection=$true;transport=$true;jobs=$false;automatic_commands=@('rename_project','undo','redo')}
$steps = @(
    @{op='import';path=$Fixture}, @{op='insert';asset=0;at=0.0}, @{op='save';path=$project}, @{op='wait';ms=800},
    @{op='control_setup';pipe_name=$pipe1Name;permissions=$permissions}, @{op='await_signal';path=$phase1;ms=180000},
    @{op='control_revoke'}, @{op='dump';path=(Join-Path $runRoot 'revoked.json')}, @{op='await_signal';path=$phase2;ms=60000},
    @{op='save';path=$project}, @{op='wait';ms=800}, @{op='control_stop'}, @{op='wait';ms=500},
    @{op='open';path=(Join-Path $project 'project.json')}, @{op='wait';ms=800},
    @{op='control_setup';pipe_name=$pipe2Name;permissions=@{read=$true;propose=$false;apply=$false;selection=$false;transport=$false;jobs=$false;automatic_commands=@()}}, @{op='await_signal';path=$phase3;ms=60000},
    @{op='control_stop'}, @{op='dump';path=(Join-Path $runRoot 'final.json')}, @{op='quit'}
)
$steps | ConvertTo-Json -Depth 25 | Set-Content -LiteralPath $scriptPath -Encoding utf8
$oldEnvironment = @{}
foreach ($name in @('TRANSCRIPTOR_CONFIG_DIR','TRANSCRIPTOR_CACHE_DIR','TRANSCRIPTOR_LOGS_DIR')) { $oldEnvironment[$name]=[Environment]::GetEnvironmentVariable($name,'Process') }
$env:TRANSCRIPTOR_CONFIG_DIR=Join-Path $runRoot 'config'
$env:TRANSCRIPTOR_CACHE_DIR=Join-Path $runRoot 'cache'
$env:TRANSCRIPTOR_LOGS_DIR=Join-Path $runRoot 'logs'
$process = $null
$session = $null
$second = $null
$client = Join-Path $PSScriptRoot 'test-mcp-editor.ps1'
$summary = @{schema='tv2-real-mcp-acceptance/1';started_at=(Get-Date).ToString('o');build=@{path=$Exe;sha256=(Get-FileHash -LiteralPath $Exe).Hash};fixture=@{path=$Fixture;sha256=(Get-FileHash -LiteralPath $Fixture).Hash};authorization='trusted local --script grants scoped automatic commands; no remote permission escalation';accepted=$false}
try {
    $process = Start-Process -FilePath $Exe -ArgumentList @('--script',('"' + $scriptPath + '"')) -WorkingDirectory $workspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runRoot 'stdout.log') -RedirectStandardError (Join-Path $runRoot 'stderr.log')
    $session = Receive-Session $pipe1
    Test-DelayedRequest $session
    $read = Join-Path $runRoot 'read.json'
    & $client -Endpoint $session.endpoint -Token $session.token -EvidencePath $read -Stage Read
    foreach ($command in @('rename_project','undo','redo')) {
        $path = Join-Path $runRoot ($command + '.json')
        & $client -Endpoint $session.endpoint -Token $session.token -EvidencePath $path -Stage Prepare -CommandType $command
        & $client -Endpoint $session.endpoint -Token $session.token -EvidencePath $path -Stage Apply
    }
    Set-Content -LiteralPath $phase1 -Value 'done'
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (-not (Test-Path -LiteralPath (Join-Path $runRoot 'revoked.json'))) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Editor did not acknowledge local revocation.' }
        Start-Sleep -Milliseconds 50
    }
    & $client -Endpoint $session.endpoint -Token $session.token -EvidencePath (Join-Path $runRoot 'rename_project.json') -Stage Revoked
    Set-Content -LiteralPath $phase2 -Value 'done'
    $second = Receive-Session $pipe2
    & $client -Endpoint $second.endpoint -Token $second.token -EvidencePath (Join-Path $runRoot 'rename_project.json') -Stage Restart
    Set-Content -LiteralPath $phase3 -Value 'done'
    if (-not $process.WaitForExit(30000)) { throw 'Owned editor did not exit after the final signal.' }
    $log = Get-Content -LiteralPath ([IO.Path]::ChangeExtension($scriptPath,'.log')) -Raw
    if ($log -notmatch 'fallos=false') { throw 'Native editor script reports failures.' }
    $summary.accepted=$true
    Write-Output "PASS: real editor MCP transport, rename/undo/redo, verify/replay/stale, revocation/restart: $runRoot"
} catch {
    $summary.error=$_.Exception.Message
    throw
} finally {
    # Release only this isolated test. No token is persisted or printed.
    foreach ($signal in @($phase1,$phase2,$phase3)) { if (-not (Test-Path -LiteralPath $signal)) { Set-Content -LiteralPath $signal -Value 'cleanup' } }
    if ($process -and -not $process.HasExited) {
        # This process was started above exclusively with a synthetic fixture.
        # A rejected scenario may never reach Quit; collect only this owned PID.
        $null = $process.CloseMainWindow()
        if (-not $process.WaitForExit(5000)) { $process.Kill(); $process.WaitForExit() }
        $summary.cleanup_owned_pid=$process.Id
    }
    $pipe1.Dispose(); $pipe2.Dispose(); $session=$null; $second=$null
    foreach ($name in $oldEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name,$oldEnvironment[$name],'Process') }
    $summary.finished_at=(Get-Date).ToString('o')
    $summary | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $runRoot 'summary.json') -Encoding utf8
}
