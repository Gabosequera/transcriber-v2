<#
.SYNOPSIS
Stage-by-stage acceptance client for a real running V2 editor on an isolated fixture.
.DESCRIPTION
This script never starts an editor, grants permissions or approves proposals.
Set TV2_MCP_TOKEN from the Control externo panel; never save the token in evidence.
Run Read, then grant proposal/application in the GUI, run Prepare, review both
proposals in the GUI, and run Apply. Apply verifies rename, exact preview, replay,
stale rejection and events. Undo/redo use Prepare -CommandType undo/redo with a
fresh EvidencePath followed by Apply after local review. LocalUndo verifies the
GUI undo of an applied rename. Revoked follows GUI permission revocation.
Restart follows save/close/reopen/start-read-only and takes the NEW endpoint/token.
Successful stages validate protocol/session state; human GUI visibility, focus,
media behavior and persistence still need their separate observations.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][uri]$Endpoint,
    [Parameter(Mandatory=$true)][string]$EvidencePath,
    [ValidateSet('Read','Prepare','Apply','LocalUndo','Revoked','Restart')][string]$Stage = 'Read',
    [ValidateSet('rename_project','undo','redo')][string]$CommandType = 'rename_project',
    [string]$Token = $env:TV2_MCP_TOKEN,
    [switch]$Interactive,
    [string]$BuildPath,
    [string]$FixturePath
)
$ErrorActionPreference = 'Stop'
if ($Interactive -and $Stage -ne 'Prepare') { throw 'Interactive requires Stage Prepare.' }
if (-not $Token) {
    $secret = Read-Host 'Token del panel Control externo (entrada oculta, no se guarda)' -AsSecureString
    $Token = [System.Net.NetworkCredential]::new('', $secret).Password
}
if (-not $Token) { throw 'A running isolated editor token is required.' }
if ($Stage -in @('Read','Prepare') -and (Test-Path -LiteralPath $EvidencePath)) { throw 'Use a new EvidencePath for Read or Prepare.' }
$client = Join-Path $PSScriptRoot 'mcp-client.ps1'
$record = @{ transport='real-editor-http'; endpoint=$Endpoint.AbsoluteUri; stage=$Stage; started_utc=[DateTime]::UtcNow.ToString('o'); calls=@(); accepted=$false }
$state = $null
if ($Stage -notin @('Read','Prepare')) {
    $state = Get-Content -LiteralPath $EvidencePath -Raw | ConvertFrom-Json -AsHashtable
}
foreach ($entry in @(@('build',$BuildPath), @('fixture',$FixturePath))) {
    if ($entry[1]) {
        $record[$entry[0]] = @{ path=(Resolve-Path -LiteralPath $entry[1]).Path; sha256=(Get-FileHash -LiteralPath $entry[1] -Algorithm SHA256).Hash }
    }
}
function Assert-Case([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Invoke-Editor([string]$Name, [hashtable]$Arguments = @{}, [switch]$ExpectError, [switch]$AllowError, [switch]$Wait) {
    $options = @{ Endpoint=$Endpoint; Token=$Token; Tool=$Name; ArgumentsJson=($Arguments | ConvertTo-Json -Depth 100 -Compress) }
    if ($Wait) { $options.WaitForVerification = $true }
    $reply = (& $client @options | Out-String) | ConvertFrom-Json -AsHashtable
    $record.calls += @{ tool=$Name; arguments=$Arguments; response=$reply }
    Assert-Case (-not $reply.error) ("JSON-RPC failure calling $Name")
    if ($ExpectError) {
        Assert-Case ([bool]$reply.result.isError) ("Expected rejection from $Name")
    } elseif (-not $AllowError) {
        Assert-Case (-not $reply.result.isError) ("Tool failure calling ${Name}: " + $reply.result.structuredContent.error)
    }
    return $reply.result.structuredContent
}
function Get-Scope([hashtable]$Context, [switch]$Revision) {
    $scope = @{ session_id=$Context.session_id; project_id=$Context.project_id }
    if ($Revision) { $scope.revision=$Context.revision }
    return $scope
}
function Get-ApplyArgs([hashtable]$Context, [hashtable]$Proposal) {
    return (Get-Scope $Context) + @{ proposal_id=$Proposal.id; preview_digest=$Proposal.preview_digest; idempotency_key=$Proposal.idempotency_key }
}
function Get-Prepared([hashtable]$Saved) {
    while ($Saved -and -not ($Saved.before -and $Saved.proposal)) { $Saved=$Saved.scenario }
    Assert-Case ([bool]$Saved) 'Evidence has no prepared scenario.'
    return $Saved
}
try {
    $toolsReply = (& $client -Endpoint $Endpoint -Token $Token -ListTools | Out-String) | ConvertFrom-Json -AsHashtable
    $record.tools = $toolsReply
    Assert-Case (-not $toolsReply.error) 'tools/list failed.'
    if ($Stage -eq 'Revoked') {
        $prepared = Get-Prepared $state
        $observedContext = Invoke-Editor 'tv2_context' -AllowError
        if ($observedContext.error) {
            Assert-Case ($observedContext.error -like 'E_PERMISSION:*') 'Unexpected error reading context after revocation.'
            $context = $prepared.before
        } else { $context = $observedContext }
    } else { $context = Invoke-Editor 'tv2_context' }
    $record.context = $context
    $scope = Get-Scope $context -Revision
    switch ($Stage) {
        'Read' {
            foreach ($kind in @('clips','layers','sequences')) {
                $null = Invoke-Editor 'tv2_query' ($scope + @{kind=$kind;limit=20})
            }
            $null = Invoke-Editor 'tv2_events' ((Get-Scope $context) + @{after=0;limit=100})
        }
        'Prepare' {
            Assert-Case ($context.permissions.propose -and $context.permissions.apply) 'Grant proposal and application locally before Prepare.'
            $command = @{type=$CommandType}
            if ($CommandType -eq 'rename_project') { $command.name='Aceptación MCP árbol ' + [guid]::NewGuid().ToString('N').Substring(0,8) }
            $key = 'e4-' + [guid]::NewGuid().ToString('N')
            $arguments = $scope + @{digest=$context.digest;idempotency_key=$key;command=$command}
            $proposal = Invoke-Editor 'tv2_propose' $arguments
            $replay = Invoke-Editor 'tv2_propose' $arguments
            Assert-Case ($proposal.id -ceq $replay.id) 'Identical proposal retry generated a second ID.'
            $null = Invoke-Editor 'tv2_preview' ($scope + @{proposal_id=$proposal.id;kind='sequences';limit=20})
            $after = Invoke-Editor 'tv2_context'
            Assert-Case ($context.revision -eq $after.revision -and $context.digest -ceq $after.digest) 'Dry-run changed the live project.'
            if (-not $proposal.automatic_eligible) {
                $rejection = Invoke-Editor 'tv2_apply' (Get-ApplyArgs $context $proposal) -ExpectError
                Assert-Case ($rejection.error -like 'E_REVIEW_REQUIRED:*') 'Unreviewed apply returned an unexpected rejection.'
            }
            $record.proposal=$proposal
            $record.command=$command
            $record.before=$context
            # Prepare another rename on exactly the same base for a stale test.
            $stale = $scope + @{digest=$context.digest;idempotency_key=($key+'-stale');command=@{type='rename_project';name='Stale must not apply'}}
            $record.stale_proposal = Invoke-Editor 'tv2_propose' $stale
            if ($context.permissions.automatic_commands -contains $CommandType -and $context.permissions.automatic_commands -contains 'rename_project') {
                Write-Output 'Prepared under locally granted automatic scope; Apply verifies the same reviewed contract.'
            } else {
                Write-Output ("Review proposal {0} and stale proposal {1} in the local GUI, then run Apply with the same evidence path." -f $proposal.id,$record.stale_proposal.id)
            }
        }
        'Apply' {
            $prepared=Get-Prepared $state
            $state=$prepared
            Assert-Case ($state.before.session_id -ceq $context.session_id) 'Control session changed; reprepare and review.'
            Assert-Case ($state.before.digest -ceq $context.digest) 'Base changed before Apply.'
            $applied = Invoke-Editor 'tv2_apply' (Get-ApplyArgs $context $state.proposal)
            Assert-Case ($applied.matches_preview -eq $true -and -not $applied.replayed) 'First commit did not match the exact prepared state.'
            $verified = Invoke-Editor 'tv2_verify' ((Get-Scope $context) + @{proposal_id=$state.proposal.id}) -Wait
            Assert-Case ($verified.verification -eq 'complete' -and $verified.matches_preview -eq $true -and $verified.receipt_in_project_journal -eq $true) 'Hash/receipt verification did not complete successfully.'
            $after = Invoke-Editor 'tv2_context'
            Assert-Case ($after.revision -eq ($state.before.revision+1)) 'Apply did not create exactly one revision.'
            if ($state.command.type -eq 'rename_project') { Assert-Case ($after.name -ceq $state.command.name) 'Renamed project not visible in live context.' }
            $replayed = Invoke-Editor 'tv2_apply' (Get-ApplyArgs $after $state.proposal)
            Assert-Case ($replayed.replayed -eq $true) 'Apply retry did not replay its receipt.'
            Assert-Case (($replayed.receipt | ConvertTo-Json -Depth 100 -Compress) -ceq ($applied.receipt | ConvertTo-Json -Depth 100 -Compress)) 'Replay receipt differs from original commit.'
            $afterRetry = Invoke-Editor 'tv2_context'
            Assert-Case ($afterRetry.revision -eq $after.revision -and $afterRetry.digest -ceq $after.digest) 'Apply retry changed the project.'
            $staleError = Invoke-Editor 'tv2_apply' (Get-ApplyArgs $after $state.stale_proposal) -ExpectError
            Assert-Case ($staleError.error -like 'E_STALE_REVISION:*') 'Stale proposal must first be reviewed locally to test its stale-base rejection.'
            $null = Invoke-Editor 'tv2_query' ((Get-Scope $state.before -Revision) + @{kind='sequences';limit=20}) -ExpectError
            $record.events = Invoke-Editor 'tv2_events' ((Get-Scope $after) + @{after=0;limit=200})
            Assert-Case ([bool]($record.events.events | Where-Object { $_.kind -eq 'proposal_applied' -and $_.detail.proposal_id -eq $state.proposal.id })) 'Apply event is absent.'
            $record.after=$after
            $record.scenario=$state
        }
        'LocalUndo' {
            Assert-Case ($state.scenario.command.type -eq 'rename_project') 'LocalUndo requires an applied rename scenario.'
            Assert-Case ($context.project_id -ceq $state.after.project_id -and $context.revision -eq ($state.after.revision+1)) 'GUI undo must create one new revision in the same project.'
            Assert-Case ($context.name -ceq $state.scenario.before.name) 'GUI undo did not restore the prior project name.'
            $record.scenario=$state
        }
        'Revoked' {
            Assert-Case (-not ($toolsReply.result.tools | Where-Object name -eq 'tv2_apply')) 'Revoke application locally; tv2_apply is still advertised.'
            $prepared = Get-Prepared $state
            $null = Invoke-Editor 'tv2_apply' (Get-ApplyArgs $context $prepared.proposal) -ExpectError
            $record.scenario=$state
        }
        'Restart' {
            $prepared = Get-Prepared $state
            Assert-Case ($context.project_id -ceq $prepared.before.project_id -and $context.session_id -cne $prepared.before.session_id) 'Reopen the saved fixture and start a fresh control session.'
            Assert-Case (-not $context.permissions.propose -and -not $context.permissions.apply -and $context.permissions.automatic_commands.Count -eq 0) 'Restart retained write authorization.'
            $proposals = Invoke-Editor 'tv2_proposals' ($scope + @{limit=200})
            Assert-Case (-not ($proposals.items | Where-Object { $_.reviewed -eq $true })) 'Restart retained local proposal review.'
            Assert-Case ([bool]($proposals.items | Where-Object id -eq $prepared.proposal.id)) 'Prepared proposal absent after restart.'
            $record.scenario=$state
        }
    }
    $record.accepted=$true
    Write-Output "PASS: real-editor stage $Stage. GUI visibility and physical behavior require separate observations."
} catch {
    $record.failure=$_.Exception.Message
    throw
} finally {
    $record.finished_utc=[DateTime]::UtcNow.ToString('o')
    # On failure preserve the prepared scenario so the caller can correct local
    # authorization and retry explicitly without losing IDs or creating edits.
    if ($state -and -not $record.accepted) {
        $failedPath=$EvidencePath+'.failed-'+[guid]::NewGuid().ToString('N')+'.json'
        $record | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $failedPath -Encoding utf8
    } else { $record | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $EvidencePath -Encoding utf8 }
}
if ($Interactive) {
    $null = Read-Host 'En la GUI revisa/aprueba AMBAS propuestas sin aplicar ni editar. Pulsa Enter cuando termines'
    & $PSCommandPath -Endpoint $Endpoint -Token $Token -EvidencePath $EvidencePath -Stage Apply
}
