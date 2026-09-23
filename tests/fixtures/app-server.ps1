$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$pendingApproval = $false
$pendingKind = ''
$currentTurn = 't'
$turnIndex = 0

function Send-Message($Value) {
    [Console]::WriteLine(($Value | ConvertTo-Json -Depth 20 -Compress))
    [Console]::Out.Flush()
}

function Complete-Turn([string]$Text, [string]$Status = 'completed') {
    Send-Message @{
        method = 'turn/completed'
        params = @{
            threadId = 'fixture'
            turn = @{
                id = $script:currentTurn
                status = $Status
                items = @(@{ id = 'answer'; type = 'agentMessage'; phase = 'final_answer'; text = $Text })
            }
        }
    }
}

while ($null -ne ($line = [Console]::ReadLine())) {
    $request = $line | ConvertFrom-Json
    switch ($request.method) {
        'initialize' {
            Send-Message @{ id = $request.id; result = @{ userAgent = 'fixture' } }
        }
        'model/list' {
            Send-Message @{ id = $request.id; result = @{ data = @() } }
        }
        'thread/start' {
            Send-Message @{ id = $request.id; result = @{ thread = @{ id = 'fixture' }; model = 'fixture'; approvalPolicy = 'on-request'; sandbox = @{ type = 'readOnly' } } }
        }
        'mcpServerStatus/list' {
            Send-Message @{ id = $request.id; result = @{ data = @(); nextCursor = $null } }
        }
        'thread/read' {
            Send-Message @{ id = $request.id; result = @{ thread = @{ id = 'fixture'; turns = @(); itemsView = 'full' } } }
        }
        'fixture/approval' {
            $pendingApproval = $true
            $pendingKind = 'unit'
            Send-Message @{ id = $request.id; result = @{} }
            Send-Message @{ id = 'approval-1'; method = 'item/commandExecution/requestApproval'; params = @{ threadId = 'fixture'; turnId = 't'; itemId = 'command'; command = 'fixture-only-no-real-execution'; cwd = 'C:\TEMP'; reason = 'Test explicit permission' } }
        }
        'fixture/spawn-owned-child' {
            $start = [Diagnostics.ProcessStartInfo]::new()
            $start.FileName = 'powershell.exe'
            $start.Arguments = '-NoProfile -NonInteractive -Command "Start-Sleep -Seconds 60"'
            $start.UseShellExecute = $false
            $start.CreateNoWindow = $true
            $child = [Diagnostics.Process]::Start($start)
            Send-Message @{ id = $request.id; result = @{ pid = $child.Id } }
        }
        'fixture/block-input' {
            Send-Message @{ id = $request.id; result = @{} }
            Start-Sleep -Seconds 60
        }
        'turn/start' {
            $turnIndex++
            $currentTurn = "turn-$turnIndex"
            $prompt = $request.params.input[0].text
            Send-Message @{ id = $request.id; result = @{ turn = @{ id = $currentTurn; status = 'inProgress' } } }
            Send-Message @{ method = 'turn/started'; params = @{ threadId = 'fixture'; turn = @{ id = $currentTurn } } }
            Send-Message @{ method = 'item/completed'; params = @{ threadId = 'fixture'; turnId = $currentTurn; item = @{ id = 'user'; type = 'userMessage'; content = @(@{ type = 'text'; text = $prompt }) } } }
            if ($prompt -eq 'QUESTION') {
                $pendingKind = 'question'
                Send-Message @{
                    id = 'question-1'; method = 'item/tool/requestUserInput'
                    params = @{
                        threadId = 'fixture'; turnId = $currentTurn; itemId = 'questions'; isBlocking = $true
                        questions = @(
                            @{ id = 'choice'; header = 'Choice'; question = 'Choose a color'; options = @(@{ label = 'Blue'; description = 'First' }, @{ label = 'Gold'; description = 'Second' }) },
                            @{ id = 'secret'; header = 'Secret'; question = 'Enter the synthetic secret'; isSecret = $true }
                        )
                    }
                }
            } elseif ($prompt -eq 'WAIT') {
                $pendingKind = 'wait'
            } elseif ($prompt -eq 'UNKNOWN') {
                $pendingKind = 'unknown'
                Send-Message @{ id = 'unsupported-1'; method = 'item/tool/call'; params = @{ threadId = 'fixture'; turnId = $currentTurn } }
            } elseif ($prompt -match '^(COMMAND|FILE)') {
                $pendingKind = if ($prompt.StartsWith('FILE')) { 'file' } else { 'command' }
                $pendingApproval = $true
                if ($pendingKind -eq 'file') {
                    Send-Message @{ method = 'item/started'; params = @{ threadId = 'fixture'; turnId = $currentTurn; item = @{ id = 'file'; type = 'fileChange'; status = 'inProgress'; changes = @(@{ path = $env:MAGICODEX_FIXTURE_MARKER; kind = @{ type = 'add' }; diff = '+ approved fixture marker' }) } } }
                    $method = 'item/fileChange/requestApproval'
                } else {
                    Send-Message @{ method = 'item/started'; params = @{ threadId = 'fixture'; turnId = $currentTurn; item = @{ id = 'command'; type = 'commandExecution'; command = 'fixture: write marker only after approval'; cwd = (Get-Location).Path; status = 'inProgress' } } }
                    $method = 'item/commandExecution/requestApproval'
                }
                Send-Message @{ id = 'approval-1'; method = $method; params = @{ threadId = 'fixture'; turnId = $currentTurn; itemId = $pendingKind; reason = 'Controlled local fixture. No action before an explicit decision.'; command = 'fixture marker write' } }
            } else {
                Complete-Turn 'FIXTURE_OK'
            }
        }
        'turn/interrupt' {
            Send-Message @{ id = $request.id; result = @{} }
            Send-Message @{ method = 'serverRequest/resolved'; params = @{ threadId = 'fixture'; requestId = 'approval-1' } }
            $pendingApproval = $false
            $pendingKind = ''
            Complete-Turn '' 'interrupted'
        }
        default {
            if ($request.id -eq 'approval-1' -and $pendingApproval) {
                $pendingApproval = $false
                $accepted = $request.result.decision -eq 'accept'
                if ($pendingKind -eq 'unit') {
                    Send-Message @{ method = 'fixture/decision'; params = @{ executed = $accepted } }
                } else {
                    if ($accepted) {
                        if (-not $env:MAGICODEX_FIXTURE_MARKER) { throw 'The test marker path is required' }
                        [IO.File]::WriteAllText($env:MAGICODEX_FIXTURE_MARKER, 'approved')
                    }
                    if ($pendingKind -eq 'command') {
                        Send-Message @{ method = 'item/completed'; params = @{ threadId = 'fixture'; turnId = $currentTurn; item = @{ id = 'command'; type = 'commandExecution'; command = 'fixture: write marker only after approval'; cwd = (Get-Location).Path; status = $(if ($accepted) { 'completed' } else { 'failed' }); exitCode = $(if ($accepted) { 0 } else { 1 }); aggregatedOutput = $(if ($accepted) { 'TOOL_ACCEPTED' } else { 'TOOL_DECLINED' }) } } }
                    } elseif ($pendingKind -eq 'file') {
                        Send-Message @{ method = 'item/completed'; params = @{ threadId = 'fixture'; turnId = $currentTurn; item = @{ id = 'file'; type = 'fileChange'; status = $(if ($accepted) { 'completed' } else { 'failed' }); changes = @(@{ path = $env:MAGICODEX_FIXTURE_MARKER; kind = @{ type = 'add' }; diff = '+ approved fixture marker' }) } } }
                    }
                    Complete-Turn $(if ($accepted) { 'TOOL_ACCEPTED' } else { 'TOOL_DECLINED' })
                }
                $pendingKind = ''
            } elseif ($request.id -eq 'question-1') {
                if ($request.result.answers.choice -and $request.result.answers.secret) {
                    $valid = $request.result.answers.choice.answers[0] -eq 'Gold' -and $request.result.answers.secret.answers[0] -eq ' synthetic_secret_731 '
                    Complete-Turn $(if ($valid) { 'ANSWERS_OK' } else { 'ANSWERS_WRONG' })
                }
                $pendingKind = ''
            } elseif ($request.id -eq 'unsupported-1') {
                Complete-Turn $(if ($request.error) { 'UNSUPPORTED_REJECTED' } else { 'UNSUPPORTED_ACCEPTED' })
                $pendingKind = ''
            }
        }
    }
}
