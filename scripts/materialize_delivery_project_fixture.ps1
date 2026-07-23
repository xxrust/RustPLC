[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ProjectRoot,
    [string]$RunId
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$projectRootResolved = (Resolve-Path -LiteralPath (Join-Path $repoRoot $ProjectRoot)).Path
$configPath = Join-Path $projectRootResolved 'delivery-project.config.json'
$config = Get-Content -Raw -Encoding UTF8 $configPath | ConvertFrom-Json
$sourceCommit = (& git -C $repoRoot rev-parse HEAD).Trim()
$gitStatusLines = @(& git -C $repoRoot status --porcelain=v1)
$changedPaths = @($gitStatusLines | ForEach-Object { if ($_.Length -gt 3) { $_.Substring(3).Replace('\', '/') } })
$dirtyWorktree = $changedPaths.Count -gt 0
$cargoVersionLine = Get-Content -Encoding UTF8 (Join-Path $repoRoot 'Cargo.toml') | Where-Object { $_ -match '^version\s*=\s*"([^"]+)"' } | Select-Object -First 1
$compilerVersion = if ($cargoVersionLine -match '^version\s*=\s*"([^"]+)"') { $Matches[1] } else { 'not_recorded' }
$runnerVersion = 'delivery-fixture-materializer-v2'
if (-not $RunId) {
    $RunId = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')
}

$runRoot = Join-Path $projectRootResolved "runs/$RunId"
$compileRoot = Join-Path $runRoot 'compile'
$projectCheckRoot = Join-Path $runRoot 'project-check'
$scenarioRoot = Join-Path $runRoot 'scenario-validation'
$wiringRoot = Join-Path $projectRootResolved 'wiring'
$releaseRoot = Join-Path $projectRootResolved 'release'
New-Item -ItemType Directory -Force -Path $compileRoot, $projectCheckRoot, $scenarioRoot, $wiringRoot, $releaseRoot | Out-Null

$exe = Join-Path $repoRoot 'target/debug/rust_plc.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "rust_plc executable is missing: $exe"
}

function Read-Json {
    param([Parameter(Mandatory)][string]$Path)
    return Get-Content -Raw -Encoding UTF8 $Path | ConvertFrom-Json
}

function Get-NormalizedSha256 {
    param([Parameter(Mandatory)][string]$Path)
    $text = [System.IO.File]::ReadAllText($Path).Replace("`r`n", "`n").Replace("`r", "`n")
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($text)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Get-TextSha256 {
    param([Parameter(Mandatory)][string]$Text)
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($Text)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Get-InputFiles {
    $candidates = New-Object System.Collections.Generic.List[System.IO.FileInfo]
    $candidates.Add((Get-Item -LiteralPath $configPath))
    foreach ($directoryName in @('source', 'review')) {
        $directoryPath = Join-Path $projectRootResolved $directoryName
        if (Test-Path -LiteralPath $directoryPath -PathType Container) {
            foreach ($file in Get-ChildItem -LiteralPath $directoryPath -Recurse -File) {
                $candidates.Add($file)
            }
        }
    }
    return @($candidates | Sort-Object FullName | ForEach-Object {
        [ordered]@{
            path = $_.FullName.Substring($projectRootResolved.Length + 1).Replace('\', '/')
            size_bytes = $_.Length
            sha256 = Get-NormalizedSha256 $_.FullName
        }
    })
}

function Get-FileSetSha256 {
    param([Parameter(Mandatory)][object[]]$Files)
    $canonical = @($Files | ForEach-Object { "$($_.path)`t$($_.size_bytes)`t$($_.sha256)" }) -join "`n"
    return Get-TextSha256 $canonical
}

function Get-RepoRelativePath {
    param([Parameter(Mandatory)][string]$Path)
    $resolved = [System.IO.Path]::GetFullPath($Path)
    $prefix = $repoRoot.TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "path escapes repository root: $resolved"
    }
    return $resolved.Substring($prefix.Length).Replace('\', '/')
}

function Write-Json {
    param(
        [Parameter(Mandatory)]$Value,
        [Parameter(Mandatory)][string]$Path
    )
    $parent = Split-Path -Parent $Path
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    $json = $Value | ConvertTo-Json -Depth 30
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, (New-Object System.Text.UTF8Encoding($false)))
}

function New-ArtifactBinding {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$FreshnessStatus,
        [Parameter(Mandatory)][string]$Basis
    )
    return [ordered]@{
        artifact_ref = Get-RepoRelativePath $Path
        digest = [ordered]@{
            algorithm = 'sha256'
            value = Get-NormalizedSha256 $Path
            normalization = 'utf8_lf'
        }
        source_commit = $sourceCommit
        freshness = [ordered]@{ status = $FreshnessStatus; basis = $Basis }
    }
}

function New-FixtureBinding {
    param([Parameter(Mandatory)][string]$Path)
    return [ordered]@{
        fixture_ref = $Path.Substring($projectRootResolved.Length + 1).Replace('\', '/')
        digest = [ordered]@{
            algorithm = 'sha256'
            value = Get-NormalizedSha256 $Path
            normalization = 'utf8_lf'
        }
    }
}

$events = New-Object System.Collections.Generic.List[object]
$steps = New-Object System.Collections.Generic.List[object]
$sequence = 0

function Invoke-ObservedCommand {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Phase,
        [Parameter(Mandatory)][string[]]$Arguments,
        [string[]]$Artifacts = @()
    )
    $script:sequence++
    $started = [DateTime]::UtcNow
    $stdoutPath = Join-Path $runRoot "logs/$Name.stdout.log"
    $stderrPath = Join-Path $runRoot "logs/$Name.stderr.log"
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $stdoutPath) | Out-Null
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $previousErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $exe @Arguments 1> $stdoutPath 2> $stderrPath
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $previousErrorActionPreference
    $watch.Stop()
    $completed = [DateTime]::UtcNow
    $status = if ($exitCode -eq 0) { 'pass' } else { 'fail' }
    $artifactRefs = @($Artifacts | Where-Object { Test-Path -LiteralPath $_ } | ForEach-Object { Get-RepoRelativePath $_ })
    $events.Add([ordered]@{
        schema_version = 1
        event_id = ('EVT-{0:D3}' -f $script:sequence)
        project_id = [string]$config.project_id
        agent_id = 'root.delivery_fixture_materializer'
        attribution_state = 'tool_observed'
        sequence = $script:sequence
        started_at = $started.ToString('o')
        completed_at = $completed.ToString('o')
        duration_ms = [int64]$watch.ElapsedMilliseconds
        phase = $Phase
        task = $Name
        tool = Get-RepoRelativePath $exe
        action = (($exe + ' ' + ($Arguments -join ' ')).Replace('\', '/'))
        result = $status
        exit_code = $exitCode
        retry_index = 0
        route = 'public_cli'
        artifact_refs = $artifactRefs
    })
    $steps.Add([ordered]@{
        name = $Name
        classification = $status
        exit_code = $exitCode
        stdout_log = Get-RepoRelativePath $stdoutPath
        stderr_log = Get-RepoRelativePath $stderrPath
        artifacts = $artifactRefs
    })
    return $exitCode
}

$runStarted = [DateTime]::UtcNow
$sourceEntryPath = Join-Path $projectRootResolved ([string]$config.source_entry).Replace('/', '\')
$systemContractPath = Join-Path $projectRootResolved ([string]$config.system_contract).Replace('/', '\')
$reviewPath = Join-Path $projectRootResolved ([string]$config.review_ref).Replace('/', '\')

$inputFiles = @(Get-InputFiles)
$sourceFiles = @($inputFiles | Where-Object { [string]$_.path -like 'source/*' })
$sourceSetSha256 = Get-FileSetSha256 $sourceFiles
$inputSetSha256 = Get-FileSetSha256 $inputFiles
$inputManifestPath = Join-Path $runRoot 'input-manifest.json'
$inputManifest = [ordered]@{
    schema_version = 1
    project_id = [string]$config.project_id
    run_id = $RunId
    source_commit = $sourceCommit
    dirty_worktree = $dirtyWorktree
    changed_paths = $changedPaths
    runner_version = $runnerVersion
    compiler_version = $compilerVersion
    file_count = $inputFiles.Count
    implementation_file_count = $sourceFiles.Count
    missing_required = @()
    path_uniqueness_required = $true
    digest_policy = 'sha256 over UTF-8 text with CRLF and CR normalized to LF'
    source_set_sha256 = $sourceSetSha256
    input_set_sha256 = $inputSetSha256
    files = $inputFiles
}
Write-Json $inputManifest $inputManifestPath

$compileReportPath = Join-Path $compileRoot 'verification_report.json'
$irPath = Join-Path $compileRoot 'ir_bundle.json'
$compileArgs = @(
    (Get-RepoRelativePath $sourceEntryPath),
    '--report', (Get-RepoRelativePath $compileReportPath),
    '--ir-out', (Get-RepoRelativePath $irPath),
    '--no-print-ir'
)
$compileExit = Invoke-ObservedCommand 'compile_verify' 'compile' $compileArgs @($compileReportPath, $irPath)

$scenarioResults = New-Object System.Collections.Generic.List[object]
$scenarioIndex = 0
foreach ($scenarioRef in $config.scenarios) {
    $scenarioIndex++
    $scenarioPath = Join-Path $projectRootResolved ([string]$scenarioRef).Replace('/', '\')
    $scenarioOutputPath = Join-Path $scenarioRoot ("{0:D2}-{1}.json" -f $scenarioIndex, [System.IO.Path]::GetFileNameWithoutExtension($scenarioPath))
    $args = @('scenario-validate', (Get-RepoRelativePath $sourceEntryPath), '--scenario', (Get-RepoRelativePath $scenarioPath), '--output', 'json')
    $exitCode = Invoke-ObservedCommand ("scenario_validate_{0:D2}" -f $scenarioIndex) 'scenario_validation' $args @()
    $stdoutLog = Join-Path $runRoot ("logs/scenario_validate_{0:D2}.stdout.log" -f $scenarioIndex)
    if ($exitCode -eq 0 -and (Test-Path -LiteralPath $stdoutLog -PathType Leaf)) {
        Copy-Item -LiteralPath $stdoutLog -Destination $scenarioOutputPath -Force
    }
    $scenarioResults.Add([ordered]@{
        scenario_ref = [string]$scenarioRef
        status = if ($exitCode -eq 0) { 'pass' } else { 'fail' }
        exit_code = $exitCode
        artifact_ref = if (Test-Path -LiteralPath $scenarioOutputPath) { Get-RepoRelativePath $scenarioOutputPath } else { $null }
    })
}

$nominalScenarioPath = Join-Path $projectRootResolved ([string]$config.nominal_scenario).Replace('/', '\')
$projectCheckArgs = @(
    'project-check', (Get-RepoRelativePath $sourceEntryPath),
    '--scenario', (Get-RepoRelativePath $nominalScenarioPath),
    '--out-dir', (Get-RepoRelativePath $projectCheckRoot),
    '--output', 'json'
)
$projectCheckReportPath = Join-Path $projectCheckRoot 'project_check_report.json'
$projectCheckExit = Invoke-ObservedCommand 'project_check' 'project_check' $projectCheckArgs @($projectCheckReportPath)

$verification = if (Test-Path -LiteralPath $compileReportPath) { Read-Json $compileReportPath } else { $null }
$projectCheck = if (Test-Path -LiteralPath $projectCheckReportPath) { Read-Json $projectCheckReportPath } else { $null }
$allScenarioPass = @($scenarioResults | Where-Object { $_.status -ne 'pass' }).Count -eq 0
$allowedScenarioGapIds = @()
if ($config.PSObject.Properties['allowed_scenario_failure_gap_ids']) {
    $allowedScenarioGapIds = @($config.allowed_scenario_failure_gap_ids)
}
$scenarioKnownGap = -not $allScenarioPass -and $allowedScenarioGapIds.Count -gt 0
$allowedProjectCheckGapIds = @()
if ($config.PSObject.Properties['allowed_project_check_failure_gap_ids']) {
    $allowedProjectCheckGapIds = @($config.allowed_project_check_failure_gap_ids)
}
$projectCheckKnownGap = $projectCheckExit -ne 0 -and $allowedProjectCheckGapIds.Count -gt 0
if ($projectCheckKnownGap) {
    foreach ($step in $steps) {
        if ($step.name -eq 'project_check') {
            $step.classification = 'known_gap'
            $step.gap_ids = $allowedProjectCheckGapIds
        }
    }
}
$postRunInputFiles = @(Get-InputFiles)
$postRunSourceFiles = @($postRunInputFiles | Where-Object { [string]$_.path -like 'source/*' })
$postRunSourceSetSha256 = Get-FileSetSha256 $postRunSourceFiles
$postRunInputSetSha256 = Get-FileSetSha256 $postRunInputFiles
$inputChangedDuringRun = $inputSetSha256 -ne $postRunInputSetSha256
$harnessPass = $compileExit -eq 0 -and ($projectCheckExit -eq 0 -or $projectCheckKnownGap) -and ($allScenarioPass -or $scenarioKnownGap) -and (-not $inputChangedDuringRun)

$acceptance = New-Object System.Collections.Generic.List[object]
function Add-Acceptance {
    param([string]$Id, [string]$Status, [string]$Basis, [string[]]$BlockerIds = @())
    $acceptance.Add([ordered]@{ id = $Id; status = $Status; basis = $Basis; blocker_ids = @($BlockerIds) })
}
Add-Acceptance 'AC-01' 'pass' 'The project-local source snapshot and digest manifest exist.'
Add-Acceptance 'AC-02' $(if ($compileExit -eq 0) { 'pass' } else { 'fail' }) 'Parser, AST lowering, semantic analysis, and IR emission were invoked.'
$noSafetyGap = @($config.known_gaps | Where-Object { $_.id -eq 'GAP-NO-SAFETY-CONSTRAINTS' }).Count -gt 0
Add-Acceptance 'AC-03' $(if ($compileExit -ne 0) { 'fail' } elseif ($noSafetyGap) { 'blocked' } else { 'pass' }) 'Safety verification executed; project-specific coverage limitations remain explicit.' $(if ($noSafetyGap) { @('GAP-NO-SAFETY-CONSTRAINTS') } else { @() })
Add-Acceptance 'AC-04' $(if ($compileExit -eq 0) { 'pass' } else { 'fail' }) 'Liveness verification executed.'
Add-Acceptance 'AC-05' $(if ($compileExit -eq 0) { 'pass' } else { 'fail' }) 'Timing verification executed for the no-board model.'
Add-Acceptance 'AC-06' $(if ($compileExit -eq 0) { 'pass' } else { 'fail' }) 'Causality verification executed.'
Add-Acceptance 'AC-07' $(if ($allScenarioPass) { 'pass' } else { 'fail' }) 'Declared scenario validation commands must exit successfully; product gaps remain failed execution evidence.' $allowedScenarioGapIds
Add-Acceptance 'AC-08' $(if ($projectCheckExit -eq 0) { 'pass' } else { 'fail' }) 'The nominal project-check pipeline must exit successfully; product gaps remain failed execution evidence.' $allowedProjectCheckGapIds
Add-Acceptance 'AC-09' 'blocked' 'Physical point checks require observed commissioning evidence.' @('GAP-PHYSICAL-POINT-CHECK')
Add-Acceptance 'AC-10' 'blocked' 'Required human holds have no attributable signatures.'
Add-Acceptance 'AC-11' 'blocked' 'Target-hardware timing evidence is absent.' @('GAP-HARDWARE-TIMING-EVIDENCE')

$passCount = @($acceptance | Where-Object { $_.status -eq 'pass' }).Count
$blockedCount = @($acceptance | Where-Object { $_.status -eq 'blocked' }).Count
$failCount = @($acceptance | Where-Object { $_.status -eq 'fail' }).Count
$strictPercent = [Math]::Round(100.0 * $passCount / $acceptance.Count, 1)

$anomalyRecords = New-Object System.Collections.Generic.List[object]
foreach ($record in $config.anomalies) {
    $copy = [ordered]@{}
    foreach ($property in $record.PSObject.Properties) { $copy[$property.Name] = $property.Value }
    $copy['event_refs'] = @($events | Select-Object -First 1 | ForEach-Object { $_.event_id })
    $copy['evidence_paths'] = @((Get-RepoRelativePath $configPath))
    $anomalyRecords.Add($copy)
}
$dynamicAnomaly = $anomalyRecords.Count
foreach ($gap in $config.known_gaps) {
    $dynamicAnomaly++
    $anomalyRecords.Add([ordered]@{
        anomaly_id = ('ANOM-{0:D3}' -f $dynamicAnomaly)
        classification = [string]$gap.classification
        status = if (($allowedScenarioGapIds -contains [string]$gap.id) -or ($allowedProjectCheckGapIds -contains [string]$gap.id)) { 'failed' } else { 'blocked' }
        retry_count = 0
        long_search_or_trial_and_error = $false
        gap_id = [string]$gap.id
        summary = [string]$gap.summary
        event_refs = @($events | Select-Object -Last 1 | ForEach-Object { $_.event_id })
        evidence_paths = @((Get-RepoRelativePath $runRoot))
    })
}
if ($verification) {
    foreach ($stageName in @('safety', 'liveness', 'timing', 'causality')) {
        $stage = $verification.verification.$stageName
        foreach ($warning in @($stage.warnings)) {
            $dynamicAnomaly++
            $anomalyRecords.Add([ordered]@{
                anomaly_id = ('ANOM-{0:D3}' -f $dynamicAnomaly)
                classification = 'verification_warning'
                status = 'open_warning'
                retry_count = 0
                long_search_or_trial_and_error = $false
                summary = [string]$warning.message
                event_refs = @($events | Where-Object { $_.phase -eq 'compile' } | ForEach-Object { $_.event_id })
                evidence_paths = @((Get-RepoRelativePath $compileReportPath))
            })
        }
    }
}
foreach ($scenarioResult in $scenarioResults) {
    if (-not $scenarioResult.artifact_ref) {
        continue
    }
    $scenarioArtifactPath = Join-Path $repoRoot ([string]$scenarioResult.artifact_ref).Replace('/', '\')
    if (-not (Test-Path -LiteralPath $scenarioArtifactPath -PathType Leaf)) {
        continue
    }
    try {
        $scenarioDocument = Read-Json $scenarioArtifactPath
        foreach ($issue in @($scenarioDocument.issues | Where-Object { $_.severity -eq 'warn' })) {
            $dynamicAnomaly++
            $anomalyRecords.Add([ordered]@{
                anomaly_id = ('ANOM-{0:D3}' -f $dynamicAnomaly)
                classification = 'scenario_warning'
                status = 'open_warning'
                retry_count = 0
                long_search_or_trial_and_error = $false
                summary = "[$($issue.code)] $($issue.message)"
                event_refs = @($events | Where-Object { $_.phase -eq 'scenario_validation' } | ForEach-Object { $_.event_id })
                evidence_paths = @([string]$scenarioResult.artifact_ref)
            })
        }
    } catch {
        $dynamicAnomaly++
        $anomalyRecords.Add([ordered]@{
            anomaly_id = ('ANOM-{0:D3}' -f $dynamicAnomaly)
            classification = 'scenario_result_parse'
            status = 'blocked'
            retry_count = 0
            long_search_or_trial_and_error = $false
            summary = "Failed to parse scenario validation result: $($_.Exception.Message)"
            event_refs = @($events | Where-Object { $_.phase -eq 'scenario_validation' } | ForEach-Object { $_.event_id })
            evidence_paths = @([string]$scenarioResult.artifact_ref)
        })
    }
}
if ($inputChangedDuringRun) {
    $dynamicAnomaly++
    $anomalyRecords.Add([ordered]@{
        anomaly_id = ('ANOM-{0:D3}' -f $dynamicAnomaly)
        classification = 'input_mutation'
        status = 'failed'
        retry_count = 0
        long_search_or_trial_and_error = $false
        summary = 'The project source-set digest changed while the delivery pipeline was running.'
        event_refs = @($events | Select-Object -Last 1 | ForEach-Object { $_.event_id })
        evidence_paths = @((Get-RepoRelativePath $inputManifestPath))
    })
}

$correctionRecords = @($config.corrections)
$anomaliesPath = Join-Path $runRoot 'anomalies.json'
$correctionsPath = Join-Path $runRoot 'corrections.json'
$agentEventsPath = Join-Path $runRoot 'agent-events.json'
Write-Json ([ordered]@{ schema_version = 1; project_id = [string]$config.project_id; run_id = $RunId; source_commit = $sourceCommit; records = $anomalyRecords.ToArray() }) $anomaliesPath
Write-Json ([ordered]@{ schema_version = 1; project_id = [string]$config.project_id; run_id = $RunId; source_commit = $sourceCommit; correction_count = $correctionRecords.Count; records = $correctionRecords }) $correctionsPath
Write-Json ([ordered]@{ schema_version = 1; project_id = [string]$config.project_id; run_id = $RunId; attribution_state = 'interactive_tool_observed'; unattended_verdict = 'not_proven'; records = $events.ToArray() }) $agentEventsPath

$wiringPoints = @($config.wiring_points | ForEach-Object {
    [ordered]@{
        point_id = [string]$_.point_id
        alias = [string]$_.alias
        direction = [string]$_.direction
        device_terminal = [string]$_.device_terminal
        signal_type = if ([string]$_.point_id -match 'AI|AO') { 'analog' } else { 'digital' }
        safe_state = $_.safe_state
        status = 'human_action_required'
        measurement = $null
        photo_ref = $null
    }
})
$wiringPath = Join-Path $wiringRoot 'point-checks.json'
Write-Json ([ordered]@{
    schema_version = 1
    project_id = [string]$config.project_id
    source_commit = $sourceCommit
    status = 'human_action_required'
    summary = [ordered]@{ declared_points = $wiringPoints.Count; verified_points = 0; blocked_points = 0; human_action_required_points = $wiringPoints.Count }
    points = $wiringPoints
}) $wiringPath

$holdsPath = Join-Path $releaseRoot 'human-holds.json'
$holds = @(
    [ordered]@{ hold_id = 'wiring_review'; required_role = 'electrical_engineer'; status = 'human_action_required'; signature = $null; reason = 'No attributable wiring review is recorded.' },
    [ordered]@{ hold_id = 'point_check_completion'; required_role = 'commissioning_engineer'; status = 'human_action_required'; signature = $null; reason = 'Physical point checks are incomplete.' },
    [ordered]@{ hold_id = 'safety_review'; required_role = 'safety_reviewer'; status = 'human_action_required'; signature = $null; reason = 'Compiler verification does not replace human safety review.' },
    [ordered]@{ hold_id = 'hil_review'; required_role = 'commissioning_engineer'; status = 'blocked'; signature = $null; reason = 'Target hardware timing and HIL evidence are absent.'; blocker_ids = @('GAP-HARDWARE-TIMING-EVIDENCE') },
    [ordered]@{ hold_id = 'release_approval'; required_role = 'release_approver'; status = 'blocked'; signature = $null; reason = 'Prerequisite holds and delivery blockers remain open.' }
)
Write-Json ([ordered]@{ schema_version = 1; project_id = [string]$config.project_id; source_commit = $sourceCommit; release_status = 'blocked'; holds = $holds }) $holdsPath

$compileEvidencePath = if (Test-Path -LiteralPath $compileReportPath) { $compileReportPath } else { $anomaliesPath }
$irEvidencePath = if (Test-Path -LiteralPath $irPath) { $irPath } else { $anomaliesPath }
$compilerStages = @(
    [ordered]@{ stage = 'Parser'; status = if ($compileExit -eq 0) { 'verified' } else { 'failed' }; diagnostics = @(); evidence = New-ArtifactBinding $compileEvidencePath 'same_run' 'Compiler-owned report from this run.' },
    [ordered]@{ stage = 'AST'; status = if ($compileExit -eq 0) { 'derived' } else { 'failed' }; diagnostics = @('No standalone AST artifact is emitted.'); evidence = New-ArtifactBinding $irEvidencePath 'same_run' 'IR emission proves AST lowering was exercised.' },
    [ordered]@{ stage = 'Semantic'; status = if ($compileExit -eq 0) { 'verified' } else { 'failed' }; diagnostics = @(); evidence = New-ArtifactBinding $irEvidencePath 'same_run' 'Semantic lowering produced the current-run IR.' },
    [ordered]@{ stage = 'IR'; status = if ($compileExit -eq 0) { 'verified' } else { 'failed' }; diagnostics = @(); evidence = New-ArtifactBinding $irEvidencePath 'same_run' 'Current-run IR artifact.' },
    [ordered]@{ stage = 'Safety'; status = if ($compileExit -eq 0) { if ($noSafetyGap) { 'verified_with_blocker' } else { 'verified' } } else { 'failed' }; diagnostics = @($config.known_gaps | Where-Object { $_.layer -eq 'verification.safety' } | ForEach-Object { $_.id }); evidence = New-ArtifactBinding $compileEvidencePath 'same_run' 'Safety section in compiler-owned report.' },
    [ordered]@{ stage = 'Liveness'; status = if ($compileExit -eq 0) { 'verified' } else { 'failed' }; diagnostics = @(); evidence = New-ArtifactBinding $compileEvidencePath 'same_run' 'Liveness section in compiler-owned report.' },
    [ordered]@{ stage = 'Timing'; status = if ($compileExit -eq 0) { 'verified_with_blocker' } else { 'failed' }; diagnostics = @('GAP-HARDWARE-TIMING-EVIDENCE'); evidence = New-ArtifactBinding $compileEvidencePath 'same_run' 'Formal timing is distinct from target-hardware timing.' },
    [ordered]@{ stage = 'Causality'; status = if ($compileExit -eq 0) { 'verified' } else { 'failed' }; diagnostics = @(); evidence = New-ArtifactBinding $compileEvidencePath 'same_run' 'Causality section in compiler-owned report.' },
    [ordered]@{ stage = 'Runtime Bridge / Simulation'; status = if ($projectCheckExit -eq 0) { 'observed' } else { 'failed' }; diagnostics = $allowedProjectCheckGapIds; evidence = New-ArtifactBinding $(if (Test-Path -LiteralPath $projectCheckReportPath) { $projectCheckReportPath } else { $anomaliesPath }) 'same_run' 'Nominal project-check and no-board-gate evidence.' },
    [ordered]@{ stage = 'Process Model Check'; status = 'not_applicable'; diagnostics = @('No process model is declared by this project.'); evidence = New-ArtifactBinding $systemContractPath 'input_snapshot' 'System contract defines this project boundary.' },
    [ordered]@{ stage = 'Intent Alignment'; status = 'not_exercised'; diagnostics = @('No intent-alignment contract is declared.'); evidence = New-ArtifactBinding $systemContractPath 'input_snapshot' 'No alignment claim is made.' },
    [ordered]@{ stage = 'Codegen'; status = 'not_exercised'; diagnostics = @('No same-run codegen artifact exists.'); evidence = New-ArtifactBinding $anomaliesPath 'same_run' 'The run records codegen as not exercised.' }
)
$compilerStagesPath = Join-Path $runRoot 'compiler-stages.json'
Write-Json ([ordered]@{ schema_version = 1; project_id = [string]$config.project_id; run_id = $RunId; source_commit = $sourceCommit; stages = $compilerStages }) $compilerStagesPath

$runCompleted = [DateTime]::UtcNow
$resultPath = Join-Path $runRoot 'result.json'
$result = [ordered]@{
    schema_version = 1
    run_id = $RunId
    harness_execution_id = $RunId
    project_id = [string]$config.project_id
    harness_status = if ($harnessPass) { 'pass' } else { 'fail' }
    delivery_status = if ($failCount -gt 0) { 'fail' } else { 'blocked' }
    freshness = 'same_run'
    error_code = if ($harnessPass) { $null } else { 'DELIVERY_MATERIALIZATION_FAILED' }
    artifact_root = Get-RepoRelativePath $runRoot
    git_head = $sourceCommit
    started_at_utc = $runStarted.ToString('o')
    completed_at_utc = $runCompleted.ToString('o')
    elapsed_ms = [int64]($runCompleted - $runStarted).TotalMilliseconds
    inputs = [ordered]@{ manifest = Get-RepoRelativePath $inputManifestPath; file_count = $inputFiles.Count }
    digests = [ordered]@{
        input_manifest_sha256 = Get-NormalizedSha256 $inputManifestPath
        source_set_sha256 = $sourceSetSha256
        post_run_source_set_sha256 = $postRunSourceSetSha256
        input_set_sha256 = $inputSetSha256
        post_run_input_set_sha256 = $postRunInputSetSha256
    }
    status = [ordered]@{
        harness_execution = if ($harnessPass) { 'pass' } else { 'fail' }
        acceptance = if ($failCount -gt 0) { 'failed' } else { 'blocked' }
        delivery = if ($failCount -gt 0) { 'failed' } else { 'blocked' }
        acceptance_pass = $passCount
        acceptance_blocked = $blockedCount
        acceptance_fail = $failCount
        implementation_completeness_percent = [int]$config.implementation_completeness_percent
        input_changed_during_run = $inputChangedDuringRun
    }
    steps = $steps.ToArray()
    acceptance = $acceptance.ToArray()
    known_gaps = @($config.known_gaps)
    scenario_summary = [ordered]@{ declared = $scenarioResults.Count; passed = @($scenarioResults | Where-Object { $_.status -eq 'pass' }).Count; failed = @($scenarioResults | Where-Object { $_.status -eq 'fail' }).Count; records = $scenarioResults.ToArray() }
    attribution = [ordered]@{ unattended_verdict = 'not_proven'; reason = 'Commands were tool-observed in an interactive Codex session; no unattended execution claim is made.' }
}
Write-Json $result $resultPath

$provenancePath = Join-Path $runRoot 'provenance.json'
Write-Json ([ordered]@{
    schema_version = 1
    project_id = [string]$config.project_id
    run_id = $RunId
    source_commit = $sourceCommit
    started_at_utc = $runStarted.ToString('o')
    completed_at_utc = $runCompleted.ToString('o')
    elapsed_ms = [int64]($runCompleted - $runStarted).TotalMilliseconds
    model = 'Codex interactive tool execution'
    unattended_verdict = 'not_proven'
    unattended_reason = 'Attribution is limited to the recorded interactive tool events.'
    event_stream = Get-RepoRelativePath $agentEventsPath
    models = @([ordered]@{ role = 'materializer'; model = $null; status = 'not_recorded'; reason = 'The tool session does not expose a durable model identifier.' })
    skills = @([ordered]@{ name = 'agent-harness-project-standard'; version = $null; status = 'version_not_recorded' })
    tool_versions = @(
        [ordered]@{ name = 'rust_plc'; version = $compilerVersion; status = if ($compilerVersion -eq 'not_recorded') { 'not_recorded' } else { 'recorded_from_Cargo.toml' } },
        [ordered]@{ name = 'fixture_materializer'; version = $runnerVersion; status = 'recorded' },
        [ordered]@{ name = 'PowerShell'; version = [string]$PSVersionTable.PSVersion; status = 'recorded' }
    )
}) $provenancePath

$manifestPath = Join-Path $projectRootResolved 'delivery-project.json'
$manifest = [ordered]@{
    schema_version = 1
    project_id = [string]$config.project_id
    title = [string]$config.title
    delivery_layer = [string]$config.delivery_layer
    source_commit = $sourceCommit
    source_entry = [string]$config.source_entry
    system_contract = [string]$config.system_contract
    delivery_status = if ($failCount -gt 0) { 'failed' } else { 'blocked' }
    artifact_roots = [ordered]@{
        agent_runs = 'runs'
        verification = "runs/$RunId"
        wiring = 'wiring'
        execution = "runs/$RunId/project-check/no_board_gate/artifacts"
        release = 'release'
    }
    evidence_summary = [ordered]@{
        acceptance_pass = $passCount
        acceptance_blocked = $blockedCount
        acceptance_fail = $failCount
        strict_acceptance_percent = $strictPercent
        implementation_completeness_percent = [int]$config.implementation_completeness_percent
    }
    evidence_bindings = [ordered]@{
        source_entry = New-ArtifactBinding $sourceEntryPath 'input_snapshot' 'Project-local source entry included in the same-run input manifest.'
        system_contract = New-ArtifactBinding $systemContractPath 'input_snapshot' 'Project-local system contract included in the source snapshot.'
        authoritative_result = New-ArtifactBinding $resultPath 'same_run' 'Current materialization result.'
        reviewed_completeness = New-ArtifactBinding $reviewPath 'reviewed_post_run' 'Committed bounded completeness review.'
    }
    fixtures = [ordered]@{
        api_run_result = New-FixtureBinding $resultPath
        provenance = New-FixtureBinding $provenancePath
        input_manifest = New-FixtureBinding $inputManifestPath
        anomalies = New-FixtureBinding $anomaliesPath
        corrections = New-FixtureBinding $correctionsPath
        compiler_stages = New-FixtureBinding $compilerStagesPath
        agent_events = New-FixtureBinding $agentEventsPath
        wiring_point_checks = New-FixtureBinding $wiringPath
        human_holds = New-FixtureBinding $holdsPath
    }
}
Write-Json $manifest $manifestPath

$output = [ordered]@{
    schema_version = 1
    command = 'materialize_delivery_project_fixture'
    status = if ($harnessPass) { 'pass' } else { 'fail' }
    project_id = [string]$config.project_id
    run_id = $RunId
    manifest = Get-RepoRelativePath $manifestPath
    result = Get-RepoRelativePath $resultPath
    event_count = $events.Count
    acceptance_pass = $passCount
    acceptance_blocked = $blockedCount
    acceptance_fail = $failCount
    delivery_status = if ($failCount -gt 0) { 'fail' } else { 'blocked' }
}
$output | ConvertTo-Json -Depth 10
if (-not $harnessPass) { exit 1 }
