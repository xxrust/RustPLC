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
$runnerVersion = 'delivery-fixture-materializer-v4'
$skillContractVersion = 'agent-harness-project-standard-v1'
$gitVersion = ((& git --version) -join ' ').Trim()
$anomalyRetryThreshold = if ($config.PSObject.Properties['anomaly_thresholds'] -and $config.anomaly_thresholds.PSObject.Properties['retry_count']) { [int]$config.anomaly_thresholds.retry_count } else { 3 }
$anomalyDurationThresholdMs = if ($config.PSObject.Properties['anomaly_thresholds'] -and $config.anomaly_thresholds.PSObject.Properties['duration_ms']) { [int64]$config.anomaly_thresholds.duration_ms } else { 60000 }
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

function Read-JsonObjectProperty {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$PropertyName)
    $text = [System.IO.File]::ReadAllText($Path)
    $propertyToken = '"' + $PropertyName + '"'
    $propertyIndex = $text.IndexOf($propertyToken, [System.StringComparison]::Ordinal)
    if ($propertyIndex -lt 0) { throw "JSON property '$PropertyName' is missing: $Path" }
    $start = $text.IndexOf('{', $propertyIndex + $propertyToken.Length)
    if ($start -lt 0) { throw "JSON object '$PropertyName' has no opening brace: $Path" }
    $depth = 0
    $inString = $false
    $escaped = $false
    for ($index = $start; $index -lt $text.Length; $index++) {
        $character = $text[$index]
        if ($inString) {
            if ($escaped) { $escaped = $false; continue }
            if ($character -eq '\') { $escaped = $true; continue }
            if ($character -eq '"') { $inString = $false }
            continue
        }
        if ($character -eq '"') { $inString = $true; continue }
        if ($character -eq '{') { $depth++ }
        elseif ($character -eq '}') {
            $depth--
            if ($depth -eq 0) {
                return $text.Substring($start, $index - $start + 1) | ConvertFrom-Json
            }
        }
    }
    throw "JSON object '$PropertyName' is not closed: $Path"
}

function ConvertTo-NativeArgument {
    param([AllowEmptyString()][string]$Value)
    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
    $builder = New-Object System.Text.StringBuilder
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') { $backslashes++; continue }
        if ($character -eq '"') {
            for ($index = 0; $index -lt ($backslashes * 2 + 1); $index++) { [void]$builder.Append('\') }
            [void]$builder.Append('"')
            $backslashes = 0
            continue
        }
        for ($index = 0; $index -lt $backslashes; $index++) { [void]$builder.Append('\') }
        $backslashes = 0
        [void]$builder.Append($character)
    }
    for ($index = 0; $index -lt ($backslashes * 2); $index++) { [void]$builder.Append('\') }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function Invoke-NativeProcess {
    param(
        [Parameter(Mandatory)][string]$Command,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [Parameter(Mandatory)][string]$StdoutPath,
        [Parameter(Mandatory)][string]$StderrPath
    )
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $Command
    $startInfo.Arguments = (@($Arguments | ForEach-Object { ConvertTo-NativeArgument ([string]$_) }) -join ' ')
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    [void]$process.Start()
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $stdout = $stdoutTask.Result
    $stderr = $stderrTask.Result
    $exitCode = $process.ExitCode
    $process.Dispose()
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($StdoutPath, $stdout, $utf8)
    [System.IO.File]::WriteAllText($StderrPath, $stderr, $utf8)
    return $exitCode
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

function Get-ManagedOutputSnapshot {
    $paths = New-Object System.Collections.Generic.List[System.IO.FileInfo]
    if (Test-Path -LiteralPath $runRoot -PathType Container) {
        foreach ($file in Get-ChildItem -LiteralPath $runRoot -Recurse -File) { $paths.Add($file) }
    }
    foreach ($path in @(
        (Join-Path $wiringRoot 'point-checks.json'),
        (Join-Path $releaseRoot 'human-holds.json')
    )) {
        if (Test-Path -LiteralPath $path -PathType Leaf) { $paths.Add((Get-Item -LiteralPath $path)) }
    }
    $snapshot = @{}
    foreach ($file in $paths) {
        $relative = Get-RepoRelativePath $file.FullName
        $snapshot[$relative] = [ordered]@{
            path = $relative
            sha256 = Get-NormalizedSha256 $file.FullName
            size_bytes = $file.Length
            last_write_time_utc_ticks = $file.LastWriteTimeUtc.Ticks
        }
    }
    return $snapshot
}

function New-FileAttributionRecord {
    param(
        [Parameter(Mandatory)][string]$Path,
        $BeforeSha256,
        [Parameter(Mandatory)][string]$AfterSha256,
        [Parameter(Mandatory)][string]$AttributionKind,
        [string]$AgentId,
        [string]$TaskId,
        [string]$EventId,
        [Parameter(Mandatory)][string]$Basis
    )
    $record = [ordered]@{
        path = $Path
        before_sha256 = $BeforeSha256
        after_sha256 = $AfterSha256
        attribution_kind = $AttributionKind
        agent_id = $AgentId
        task_id = $TaskId
        basis = $Basis
    }
    if (-not [string]::IsNullOrWhiteSpace($EventId)) { $record['event_id'] = $EventId }
    return $record
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
    $exitCode = Invoke-NativeProcess $exe $Arguments $repoRoot $stdoutPath $stderrPath
    $watch.Stop()
    $completed = [DateTime]::UtcNow
    $status = if ($exitCode -eq 0) { 'pass' } else { 'fail' }
    $artifactRefs = @($Artifacts | Where-Object { Test-Path -LiteralPath $_ } | ForEach-Object { Get-RepoRelativePath $_ })
    $toolRef = Get-RepoRelativePath $exe
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
        tool = $toolRef
        action = (($toolRef + ' ' + ($Arguments -join ' ')).Replace('\', '/'))
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
$preRunManagedSnapshot = Get-ManagedOutputSnapshot
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

$geometryPath = Join-Path $compileRoot 'geometry.json'
$geometryStatusPath = Join-Path $compileRoot 'geometry-export-status.json'
$geometryArgs = @(
    'geometry-export', (Get-RepoRelativePath $sourceEntryPath),
    '--out', (Get-RepoRelativePath $geometryPath),
    '--output', 'json'
)
$geometryExit = Invoke-ObservedCommand 'geometry_export' 'geometry' $geometryArgs @($geometryPath)
Write-Json ([ordered]@{
    schema_version = 1
    project_id = [string]$config.project_id
    run_id = $RunId
    status = if ($geometryExit -eq 0 -and (Test-Path -LiteralPath $geometryPath -PathType Leaf)) { 'pass' } else { 'blocked' }
    error_code = if ($geometryExit -eq 0) { $null } else { 'GEOMETRY_EXPORT_FAILED' }
    exit_code = $geometryExit
    geometry_artifact = if (Test-Path -LiteralPath $geometryPath -PathType Leaf) { Get-RepoRelativePath $geometryPath } else { $null }
    stdout_log = Get-RepoRelativePath (Join-Path $runRoot 'logs/geometry_export.stdout.log')
    stderr_log = Get-RepoRelativePath (Join-Path $runRoot 'logs/geometry_export.stderr.log')
}) $geometryStatusPath
foreach ($event in $events) {
    if ($event.task -eq 'geometry_export') { $event.artifact_refs = @($event.artifact_refs) + (Get-RepoRelativePath $geometryStatusPath) }
}
foreach ($step in $steps) {
    if ($step.name -eq 'geometry_export') { $step.artifacts = @($step.artifacts) + (Get-RepoRelativePath $geometryStatusPath) }
}

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
$harnessPass = $compileExit -eq 0 -and $geometryExit -eq 0 -and ($projectCheckExit -eq 0 -or $projectCheckKnownGap) -and ($allScenarioPass -or $scenarioKnownGap) -and (-not $inputChangedDuringRun)

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
Add-Acceptance 'AC-12' $(if ($geometryExit -eq 0 -and (Test-Path -LiteralPath $geometryPath -PathType Leaf)) { 'pass' } else { 'blocked' }) 'Semantic-twin geometry must be exported by the current compiler run; failure remains an explicit blocker.' $(if ($geometryExit -eq 0) { @() } else { @('GAP-GEOMETRY-EXPORT') })

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
if ($geometryExit -ne 0 -or -not (Test-Path -LiteralPath $geometryPath -PathType Leaf)) {
    $dynamicAnomaly++
    $anomalyRecords.Add([ordered]@{
        anomaly_id = ('ANOM-{0:D3}' -f $dynamicAnomaly)
        classification = 'geometry_export'
        status = 'blocked'
        retry_count = 0
        long_search_or_trial_and_error = $false
        gap_id = 'GAP-GEOMETRY-EXPORT'
        summary = 'The current compiler run did not produce a geometry artifact; no empty or synthetic graph was substituted.'
        event_refs = @($events | Where-Object { $_.phase -eq 'geometry' } | ForEach-Object { $_.event_id })
        evidence_paths = @((Get-RepoRelativePath $geometryStatusPath), (Get-RepoRelativePath (Join-Path $runRoot 'logs/geometry_export.stderr.log')))
    })
}

$correctionRecords = @($config.corrections | ForEach-Object {
    $copy = [ordered]@{}
    foreach ($property in $_.PSObject.Properties) { $copy[$property.Name] = $property.Value }
    if (-not $copy.Contains('anomaly_id')) { $copy['anomaly_id'] = $null }
    [pscustomobject]$copy
})
$eventById = @{}
foreach ($event in $events) { $eventById[[string]$event.event_id] = $event }
$enrichedAnomalyRecords = New-Object System.Collections.Generic.List[object]
foreach ($anomaly in $anomalyRecords) {
    $enriched = [ordered]@{}
    if ($anomaly -is [System.Collections.IDictionary]) {
        foreach ($key in $anomaly.Keys) { $enriched[[string]$key] = $anomaly[$key] }
    } else {
        foreach ($property in $anomaly.PSObject.Properties) { $enriched[$property.Name] = $property.Value }
    }
    $retryCandidates = @()
    if ($enriched.Contains('retry_count')) { $retryCandidates += [int]$enriched.retry_count }
    if ($enriched.Contains('historical_retry_count')) { $retryCandidates += [int]$enriched.historical_retry_count }
    $effectiveRetryCount = if ($retryCandidates.Count -gt 0) { [int](($retryCandidates | Measure-Object -Maximum).Maximum) } else { 0 }
    $eventDurationMs = [int64]0
    foreach ($eventRef in @($enriched.event_refs)) {
        if ($eventById.ContainsKey([string]$eventRef)) { $eventDurationMs += [int64]$eventById[[string]$eventRef].duration_ms }
    }
    $isLongSearch = $effectiveRetryCount -ge $anomalyRetryThreshold -or $eventDurationMs -ge $anomalyDurationThresholdMs
    $enriched['long_search_or_trial_and_error'] = $isLongSearch
    $enriched['long_search_evaluation'] = [ordered]@{
        retry_count = $effectiveRetryCount
        retry_threshold = $anomalyRetryThreshold
        event_duration_ms = $eventDurationMs
        duration_threshold_ms = $anomalyDurationThresholdMs
        triggered_by = @(
            $(if ($effectiveRetryCount -ge $anomalyRetryThreshold) { 'retry_count' }),
            $(if ($eventDurationMs -ge $anomalyDurationThresholdMs) { 'event_duration_ms' })
        ) | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) }
    }
    $enriched['root_cause'] = [ordered]@{
        classification = [string]$enriched.classification
        summary = [string]$enriched.summary
    }
    $enriched['affected_files'] = @($enriched.evidence_paths)
    $matchingCorrections = @($correctionRecords | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_.anomaly_id) -and [string]$_.anomaly_id -eq [string]$enriched.anomaly_id })
    if ($matchingCorrections.Count -gt 0) {
        $enriched['correction'] = [ordered]@{
            status = 'recorded'
            correction_ids = @($matchingCorrections | ForEach-Object { [string]$_.correction_id })
            summaries = @($matchingCorrections | ForEach-Object { [string]$_.summary })
        }
        $verifiedCorrections = @($matchingCorrections | Where-Object { [string]$_.status -like 'verified*' })
        $enriched['verification_result'] = [ordered]@{
            status = if ($verifiedCorrections.Count -eq $matchingCorrections.Count) { 'verified' } else { 'recorded_not_verified' }
            evidence_refs = @($enriched.evidence_paths)
            correction_ids = @($matchingCorrections | ForEach-Object { [string]$_.correction_id })
        }
    } else {
        $enriched['correction'] = [ordered]@{
            status = 'not_corrected'
            correction_ids = @()
            reason = 'No correction record is bound to this anomaly_id; open gaps and warnings remain uncorrected.'
        }
        $enriched['verification_result'] = [ordered]@{
            status = 'not_corrected'
            evidence_refs = @($enriched.evidence_paths)
            correction_ids = @()
        }
    }
    $enrichedAnomalyRecords.Add([pscustomobject]$enriched)
}
$anomaliesPath = Join-Path $runRoot 'anomalies.json'
$correctionsPath = Join-Path $runRoot 'corrections.json'
$agentEventsPath = Join-Path $runRoot 'agent-events.json'
Write-Json ([ordered]@{ schema_version = 2; project_id = [string]$config.project_id; run_id = $RunId; source_commit = $sourceCommit; thresholds = [ordered]@{ retry_count = $anomalyRetryThreshold; duration_ms = $anomalyDurationThresholdMs }; records = $enrichedAnomalyRecords.ToArray() }) $anomaliesPath
Write-Json ([ordered]@{ schema_version = 1; project_id = [string]$config.project_id; run_id = $RunId; source_commit = $sourceCommit; correction_count = $correctionRecords.Count; records = $correctionRecords }) $correctionsPath
Write-Json ([ordered]@{ schema_version = 1; project_id = [string]$config.project_id; run_id = $RunId; provenance_scope = 'delivery_fixture_materialization'; attribution_state = 'deterministic_harness_observed'; execution_unattended_verdict = if ($inputChangedDuringRun) { 'human_intervention_detected' } else { 'proven' }; source_authoring_verdict = 'not_proven'; unattended_verdict = if ($inputChangedDuringRun) { 'human_intervention_detected' } else { 'not_proven' }; records = $events.ToArray() }) $agentEventsPath

$sourcePlcDocuments = @(Get-ChildItem -LiteralPath (Join-Path $projectRootResolved 'source') -Recurse -Filter '*.plc' -File | Sort-Object FullName | ForEach-Object {
    [ordered]@{ file = $_; text = [System.IO.File]::ReadAllText($_.FullName) }
})
$sourceCorpus = @($sourcePlcDocuments | ForEach-Object { [string]$_.text }) -join "`n"
$controllerIoByChannel = @{}
foreach ($match in [regex]::Matches($sourceCorpus, '(?m)^\s*(input|output)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*((?:X|Y|AI|AO)[0-9]+)\s*\{([^}]*)\}')) {
    $body = [string]$match.Groups[4].Value
    $safeStateMatch = [regex]::Match($body, '(?:^|,)\s*safe_state\s*:\s*([A-Za-z0-9_.-]+)')
    $controllerIoByChannel[[string]$match.Groups[3].Value] = [ordered]@{
        direction = [string]$match.Groups[1].Value
        alias = [string]$match.Groups[2].Value
        safe_state = if ($safeStateMatch.Success) { [string]$safeStateMatch.Groups[1].Value } else { $null }
    }
}
$expectedWiringPoints = @($config.wiring_points | ForEach-Object {
    [pscustomobject][ordered]@{
        point_id = [string]$_.point_id
        alias = [string]$_.alias
        direction = [string]$_.direction
        device_terminal = [string]$_.device_terminal
        safe_state = $_.safe_state
    }
})
$irDocument = Read-Json $irPath
$irTopology = $irDocument.topology
if ($null -eq $irTopology) { throw "Compiler IR has no topology object: $irPath" }
$irEvidenceRef = Get-RepoRelativePath $irPath
$wiringPoints = New-Object System.Collections.Generic.List[object]
foreach ($link in @($irTopology.links)) {
    $direction = $null
    $channel = $null
    $deviceName = $null
    $devicePort = $null
    if ([string]$link.to -match '^(X|AI)[0-9]+$') {
        $direction = 'input'
        $channel = [string]$link.to
        $deviceName = [string]$link.from
        $devicePort = if ($link.PSObject.Properties['from_port'] -and -not [string]::IsNullOrWhiteSpace([string]$link.from_port)) { [string]$link.from_port } else { 'self' }
    } elseif ([string]$link.from -match '^(Y|AO)[0-9]+$') {
        $direction = 'output'
        $channel = [string]$link.from
        $deviceName = [string]$link.to
        $devicePort = if ($link.PSObject.Properties['to_port'] -and -not [string]::IsNullOrWhiteSpace([string]$link.to_port)) { [string]$link.to_port } else { 'self' }
    } else {
        continue
    }
    $pointId = "plc_main.$channel"
    $deviceTerminal = "$deviceName.$devicePort"
    $controllerIo = if ($controllerIoByChannel.ContainsKey($channel)) { $controllerIoByChannel[$channel] } else { $null }
    $wiringPoints.Add([pscustomobject][ordered]@{
        wire_id = "wire:$channel`:$deviceTerminal"
        point_id = $pointId
        alias = if ($null -ne $controllerIo) { [string]$controllerIo.alias } else { $null }
        direction = $direction
        device_terminal = $deviceTerminal
        signal_type = if ($channel -match '^(AI|AO)[0-9]+$') { 'analog' } else { 'digital' }
        safe_state = if ($null -ne $controllerIo) { $controllerIo.safe_state } else { $null }
        status = 'human_action_required'
        measurement = $null
        photo_ref = $null
        evidence_source = $irEvidenceRef
    })
}
$wiringDiagnostics = New-Object System.Collections.Generic.List[object]
function Add-WiringDiagnostic {
    param([string]$Code, [string]$Kind, [string]$PointId, [string]$Message, [string[]]$EvidenceRefs = @())
    $wiringDiagnostics.Add([ordered]@{
        code = $Code
        severity = 'error'
        kind = $Kind
        point_id = $PointId
        message = $Message
        evidence_refs = @($EvidenceRefs)
    })
}

foreach ($duplicate in @($expectedWiringPoints | Group-Object point_id | Where-Object { $_.Count -gt 1 })) {
    Add-WiringDiagnostic 'WIR-001' 'duplicate_channel' ([string]$duplicate.Name) "PLC channel is declared $($duplicate.Count) times."
}
foreach ($duplicate in @($expectedWiringPoints | Group-Object alias | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_.Name) -and $_.Count -gt 1 })) {
    Add-WiringDiagnostic 'WIR-002' 'duplicate_alias' ([string]$duplicate.Group[0].point_id) "Alias '$($duplicate.Name)' is declared $($duplicate.Count) times."
}
foreach ($duplicate in @($expectedWiringPoints | Group-Object device_terminal | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_.Name) -and $_.Count -gt 1 })) {
    Add-WiringDiagnostic 'WIR-003' 'duplicate_terminal' ([string]$duplicate.Group[0].point_id) "Device terminal '$($duplicate.Name)' is bound $($duplicate.Count) times."
}

foreach ($point in $expectedWiringPoints) {
    $pointId = [string]$point.point_id
    $alias = [string]$point.alias
    $terminal = [string]$point.device_terminal
    $direction = [string]$point.direction
    $directionMatches = ($direction -eq 'input' -and $pointId -match '^plc_main\.(X|AI)[0-9]+$') -or ($direction -eq 'output' -and $pointId -match '^plc_main\.(Y|AO)[0-9]+$')
    if (-not $directionMatches) {
        Add-WiringDiagnostic 'WIR-004' 'direction_mismatch' $pointId "Direction '$direction' does not match PLC channel '$pointId'."
    }
    $aliasRef = if ([string]::IsNullOrWhiteSpace($alias)) { $null } else { "plc_main.$alias" }
    $controllerRefs = @($pointId, $aliasRef | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) })
    $resolvedControllerRef = @($controllerRefs | Where-Object { $sourceCorpus.Contains([string]$_) }) | Select-Object -First 1
    if ($null -eq $resolvedControllerRef) {
        Add-WiringDiagnostic 'WIR-005' 'unresolved_alias' $pointId "Neither canonical point '$pointId' nor alias '$aliasRef' appears in a source artifact."
    }
    $derivedMatches = @($wiringPoints | Where-Object { [string]$_.point_id -eq $pointId -and [string]$_.device_terminal -eq $terminal -and [string]$_.direction -eq $direction })
    if ($derivedMatches.Count -eq 0) {
        Add-WiringDiagnostic 'WIR-006' 'unbound_port' $pointId "The current compile/ir_bundle.json topology.links does not derive '$pointId' -> '$terminal' with direction '$direction'." @($irEvidenceRef)
    }
    if ($direction -eq 'output' -and [string]::IsNullOrWhiteSpace([string]$point.safe_state)) {
        Add-WiringDiagnostic 'WIR-007' 'missing_safe_state' $pointId 'Output point has no declared safe state.'
    }
}

$diagnosticPointIds = @($wiringDiagnostics | ForEach-Object { [string]$_.point_id } | Sort-Object -Unique)
foreach ($point in $wiringPoints) {
    if ($diagnosticPointIds -contains [string]$point.point_id) { $point.status = 'blocked' }
}
$controllerEvidence = @($sourcePlcDocuments | Where-Object { [string]$_.text -match '(?m)^\s*(device\s+plc_main\s*:|controller_io\s+plc_main\s*\{)' } | ForEach-Object { New-ArtifactBinding $_.file.FullName 'input_snapshot' 'Source artifact declares the project controller or controller I/O aliases.' })
$deviceEvidence = @($sourcePlcDocuments | Where-Object { [string]$_.text -match '(?m)^\s*device\s+' } | ForEach-Object { New-ArtifactBinding $_.file.FullName 'input_snapshot' 'Source artifact declares one or more topology devices.' })
$connectionEvidence = @($sourcePlcDocuments | Where-Object { [string]$_.text -match '(?m)^\s*relation\s*\{' } | ForEach-Object { New-ArtifactBinding $_.file.FullName 'input_snapshot' 'Source artifact declares topology relations used to bind controller points and device terminals.' })
$wiringValidationStatus = if ($wiringDiagnostics.Count -eq 0) { 'pass' } else { 'fail' }
$wiringPath = Join-Path $wiringRoot 'point-checks.json'
Write-Json ([ordered]@{
    schema_version = 2
    project_id = [string]$config.project_id
    source_commit = $sourceCommit
    status = if ($wiringDiagnostics.Count -eq 0) { 'human_action_required' } else { 'blocked' }
    summary = [ordered]@{ declared_points = $expectedWiringPoints.Count; compiler_derived_points = $wiringPoints.Count; verified_points = 0; blocked_points = $diagnosticPointIds.Count; human_action_required_points = $wiringPoints.Count - @($wiringPoints | Where-Object { $diagnosticPointIds -contains [string]$_.point_id }).Count }
    validation_summary = [ordered]@{
        status = $wiringValidationStatus
        error_count = $wiringDiagnostics.Count
        checked_rules = @('duplicate_channel', 'duplicate_alias', 'duplicate_terminal', 'direction_mismatch', 'unresolved_alias', 'unbound_port', 'missing_safe_state')
    }
    diagnostics = $wiringDiagnostics.ToArray()
    evidence_sources = [ordered]@{
        controller = $controllerEvidence
        devices = $deviceEvidence
        connections = $connectionEvidence
        compiler_ir = @(New-ArtifactBinding $irPath 'same_run' 'Authoritative wiring row set derived from the current compile/ir_bundle.json topology.links projection.')
    }
    expected_vs_derived = [ordered]@{
        expected_count = $expectedWiringPoints.Count
        derived_count = $wiringPoints.Count
        contract = 'config is expected metadata/audit input; points are authoritative compiler-derived topology links'
    }
    points = $wiringPoints.ToArray()
}) $wiringPath
$harnessPass = $harnessPass -and $wiringDiagnostics.Count -eq 0
Add-Acceptance 'AC-13' $(if ($wiringDiagnostics.Count -eq 0) { 'pass' } else { 'fail' }) 'Wiring points must have unique controller channels, aliases, and terminals; correct direction and safe-state metadata; and source-backed controller/device relation bindings.'
$passCount = @($acceptance | Where-Object { $_.status -eq 'pass' }).Count
$blockedCount = @($acceptance | Where-Object { $_.status -eq 'blocked' }).Count
$failCount = @($acceptance | Where-Object { $_.status -eq 'fail' }).Count
$strictPercent = [Math]::Round(100.0 * $passCount / $acceptance.Count, 1)

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
    attribution = [ordered]@{
        provenance_scope = 'delivery_fixture_materialization'
        execution_unattended_verdict = if ($inputChangedDuringRun) { 'human_intervention_detected' } else { 'proven' }
        source_authoring_verdict = 'not_proven'
        unattended_verdict = if ($inputChangedDuringRun) { 'human_intervention_detected' } else { 'not_proven' }
        human_intervention_detected = $inputChangedDuringRun
        reason = if ($inputChangedDuringRun) { 'At least one immutable input changed between the run start and completion snapshots.' } else { 'Materialization execution is attributable, while source authoring provenance is not recorded by this run.' }
        evidence = @((Get-RepoRelativePath $inputManifestPath), (Get-RepoRelativePath $agentEventsPath), (Get-RepoRelativePath (Join-Path $runRoot 'provenance.json')))
    }
}
Write-Json $result $resultPath

$postRunManagedSnapshot = Get-ManagedOutputSnapshot
$initialInputByPath = @{}
foreach ($file in $inputFiles) { $initialInputByPath[[string]$file.path] = $file }
$finalInputByPath = @{}
foreach ($file in $postRunInputFiles) { $finalInputByPath[[string]$file.path] = $file }
$fileAttributionRecords = New-Object System.Collections.Generic.List[object]

foreach ($relative in @($postRunManagedSnapshot.Keys | Sort-Object)) {
    $after = $postRunManagedSnapshot[$relative]
    $before = if ($preRunManagedSnapshot.ContainsKey($relative)) { $preRunManagedSnapshot[$relative] } else { $null }
    if ($null -ne $before -and [string]$before.sha256 -eq [string]$after.sha256 -and [int64]$before.last_write_time_utc_ticks -eq [int64]$after.last_write_time_utc_ticks) { continue }
    $kind = if ($null -eq $before) { 'agent_generated' } else { 'agent_modified' }
    $fileAttributionRecords.Add((New-FileAttributionRecord `
        -Path $relative `
        -BeforeSha256 $(if ($null -eq $before) { $null } else { [string]$before.sha256 }) `
        -AfterSha256 ([string]$after.sha256) `
        -AttributionKind $kind `
        -AgentId 'root.delivery_fixture_materializer' `
        -TaskId 'materialize_delivery_project_fixture' `
        -Basis 'The path is inside the harness-owned run, wiring, or release output set.'))
}

foreach ($relative in @($initialInputByPath.Keys | Sort-Object)) {
    $before = $initialInputByPath[$relative]
    $after = if ($finalInputByPath.ContainsKey($relative)) { $finalInputByPath[$relative] } else { $null }
    $workspacePath = Get-RepoRelativePath (Join-Path $projectRootResolved $relative.Replace('/', '\'))
    $changedDuringRun = $null -eq $after -or [string]$before.sha256 -ne [string]$after.sha256
    if ($changedDuringRun) {
        $afterDigest = if ($null -eq $after) { '0000000000000000000000000000000000000000000000000000000000000000' } else { [string]$after.sha256 }
        $fileAttributionRecords.Add((New-FileAttributionRecord `
            -Path $workspacePath `
            -BeforeSha256 ([string]$before.sha256) `
            -AfterSha256 $afterDigest `
            -AttributionKind 'human_intervention_detected' `
            -Basis 'An immutable input changed during the run and no agent task declared ownership of that input path.'))
    } elseif ($changedPaths -contains $workspacePath) {
        $fileAttributionRecords.Add((New-FileAttributionRecord `
            -Path $workspacePath `
            -BeforeSha256 ([string]$before.sha256) `
            -AfterSha256 ([string]$after.sha256) `
            -AttributionKind 'pre_existing_user_change' `
            -Basis 'Git reported this project input as changed before the run; the before and after snapshots are identical.'))
    }
}

$attributionSummary = [ordered]@{
    agent_generated = @($fileAttributionRecords | Where-Object { $_.attribution_kind -eq 'agent_generated' }).Count
    agent_modified = @($fileAttributionRecords | Where-Object { $_.attribution_kind -eq 'agent_modified' }).Count
    pre_existing_user_change = @($fileAttributionRecords | Where-Object { $_.attribution_kind -eq 'pre_existing_user_change' }).Count
    human_intervention_detected = @($fileAttributionRecords | Where-Object { $_.attribution_kind -eq 'human_intervention_detected' }).Count
}
$taskDefinitions = New-Object System.Collections.Generic.List[object]
$taskDefinitions.Add([ordered]@{
    task_id = 'materialize_delivery_project_fixture'
    agent_id = 'root.delivery_fixture_materializer'
    task_kind = 'materialization'
    definition_ref = Get-RepoRelativePath $configPath
    acceptance = 'Run all declared compiler/scenario/project gates, freeze evidence, and preserve failures.'
})
foreach ($event in $events) {
    $taskDefinitions.Add([ordered]@{
        task_id = [string]$event.task
        agent_id = [string]$event.agent_id
        task_kind = 'validation'
        phase = [string]$event.phase
        tool = [string]$event.tool
        command = [string]$event.action
    })
}

$provenancePath = Join-Path $runRoot 'provenance.json'
Write-Json ([ordered]@{
    schema_version = 2
    project_id = [string]$config.project_id
    run_id = $RunId
    source_commit = $sourceCommit
    git_base_commit = $sourceCommit
    dirty_worktree_at_start = $dirtyWorktree
    started_at_utc = $runStarted.ToString('o')
    completed_at_utc = $runCompleted.ToString('o')
    elapsed_ms = [int64]($runCompleted - $runStarted).TotalMilliseconds
    model = 'deterministic_harness'
    provenance_scope = 'delivery_fixture_materialization'
    execution_unattended_verdict = if ($inputChangedDuringRun) { 'human_intervention_detected' } else { 'proven' }
    source_authoring_verdict = 'not_proven'
    unattended_verdict = if ($inputChangedDuringRun) { 'human_intervention_detected' } else { 'not_proven' }
    unattended_reason = if ($inputChangedDuringRun) { 'At least one immutable input changed without agent ownership during the run.' } else { 'Immutable inputs and harness outputs prove unattended materialization only; source authoring provenance is absent.' }
    event_stream = Get-RepoRelativePath $agentEventsPath
    input_manifest = New-ArtifactBinding $inputManifestPath 'input_snapshot' 'Immutable input snapshot captured before compiler execution.'
    models = @([ordered]@{ role = 'materializer'; model = 'deterministic_harness'; status = 'recorded'; basis = 'PowerShell orchestrator identity, not an inferred chat model.' })
    agents = @([ordered]@{ agent_id = 'root.delivery_fixture_materializer'; identity_kind = 'deterministic_orchestrator'; model = 'deterministic_harness'; role = 'materialize_and_validate' })
    task_definitions = $taskDefinitions.ToArray()
    skills = @([ordered]@{ name = 'agent-harness-project-standard'; version = $skillContractVersion; status = 'recorded_by_runner_contract' })
    tool_versions = @(
        [ordered]@{ name = 'rust_plc'; version = $compilerVersion; status = if ($compilerVersion -eq 'not_recorded') { 'not_recorded' } else { 'recorded_from_Cargo.toml' } },
        [ordered]@{ name = 'fixture_materializer'; version = $runnerVersion; status = 'recorded' },
        [ordered]@{ name = 'PowerShell'; version = [string]$PSVersionTable.PSVersion; status = 'recorded' },
        [ordered]@{ name = 'git'; version = $gitVersion; status = 'recorded' }
    )
    file_attribution = [ordered]@{
        policy_version = 'rustplc-file-attribution-v1'
        human_intervention_detected = $inputChangedDuringRun
        summary = $attributionSummary
        records = $fileAttributionRecords.ToArray()
        evidence_envelopes = @(
            [ordered]@{ path = Get-RepoRelativePath $provenancePath; reason = 'This document contains the attribution ledger and cannot recursively digest itself.' },
            [ordered]@{ path = Get-RepoRelativePath (Join-Path $projectRootResolved 'delivery-project.json'); reason = 'The project manifest externally binds the provenance digest after the ledger is closed.' }
        )
    }
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
    scenario_roots = @('source/scenarios')
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
$manifest.evidence_bindings['geometry_export'] = New-ArtifactBinding $geometryStatusPath 'same_run' 'Current-run geometry-export status; a failed export remains a blocker and never substitutes an empty graph.'
$manifest.fixtures['geometry_export'] = New-FixtureBinding $geometryStatusPath
if ($geometryExit -eq 0 -and (Test-Path -LiteralPath $geometryPath -PathType Leaf)) {
    $manifest.evidence_bindings['geometry'] = New-ArtifactBinding $geometryPath 'same_run' 'Compiler-owned semantic-twin geometry from this run.'
    $manifest.fixtures['geometry'] = New-FixtureBinding $geometryPath
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
