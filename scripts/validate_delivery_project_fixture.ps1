[CmdletBinding()]
param(
    [string]$ManifestPath,
    [string]$OutputPath,
    [string]$RegistryRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $ManifestPath) {
    $ManifestPath = Join-Path $repoRoot 'delivery-projects/station.dual-slot-shuttle-press-cell/delivery-project.json'
}

$checks = New-Object System.Collections.Generic.List[object]
$errors = New-Object System.Collections.Generic.List[string]

function Add-Check {
    param([string]$Name, [bool]$Ok, [string]$Detail)
    $checks.Add([ordered]@{ name = $Name; ok = $Ok; detail = $Detail })
    if (-not $Ok) { $errors.Add("${Name}: ${Detail}") }
}

function Read-Json {
    param([string]$Path)
    return Get-Content -Raw -Encoding UTF8 $Path | ConvertFrom-Json
}

function Write-Json {
    param($Value, [string]$Path)
    $parent = Split-Path -Parent $Path
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    $json = $Value | ConvertTo-Json -Depth 40
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, (New-Object System.Text.UTF8Encoding($false)))
}

function Has-Property {
    param($Object, [string]$Name)
    return $null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]
}

function Get-NormalizedSha256 {
    param([string]$Path)
    $text = [System.IO.File]::ReadAllText($Path).Replace("`r`n", "`n").Replace("`r", "`n")
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($text)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Resolve-RepoPath {
    param([string]$ArtifactRef)
    if ([System.IO.Path]::IsPathRooted($ArtifactRef)) { throw "absolute artifact_ref is not allowed: $ArtifactRef" }
    $candidate = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $ArtifactRef.Replace('/', '\')))
    $prefix = $repoRoot.TrimEnd('\') + '\'
    if (-not $candidate.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "artifact_ref escapes repository: $ArtifactRef"
    }
    return $candidate
}

function Test-ArtifactBinding {
    param($Binding, [string]$Context, [string]$ExpectedCommit)
    foreach ($field in @('artifact_ref', 'digest', 'source_commit', 'freshness')) {
        Add-Check "$Context.$field" (Has-Property $Binding $field) "artifact binding requires $field"
    }
    if (@('artifact_ref', 'digest', 'source_commit', 'freshness') | Where-Object { -not (Has-Property $Binding $_) }) { return }
    $ref = [string]$Binding.artifact_ref
    Add-Check "$Context.no_ignored_legacy_dependency" (-not $ref.Replace('\', '/').StartsWith('out/complex_selftest/')) $ref
    try { $path = Resolve-RepoPath $ref } catch { Add-Check "$Context.path_resolves" $false $_.Exception.Message; return }
    $exists = Test-Path -LiteralPath $path -PathType Leaf
    Add-Check "$Context.path_exists" $exists $ref
    if ($exists) {
        $actual = Get-NormalizedSha256 $path
        Add-Check "$Context.digest" ($actual -eq [string]$Binding.digest.value) "expected=$($Binding.digest.value); actual=$actual"
    }
    Add-Check "$Context.source_commit" ([string]$Binding.source_commit -eq $ExpectedCommit) "expected=$ExpectedCommit; actual=$($Binding.source_commit)"
    Add-Check "$Context.freshness" (-not [string]::IsNullOrWhiteSpace([string]$Binding.freshness.status)) ([string]$Binding.freshness.status)
}

function Visit-ArtifactBindings {
    param($Node, [string]$Context, [string]$ExpectedCommit)
    if ($null -eq $Node -or $Node -is [string] -or $Node -is [ValueType]) { return }
    if ($Node -is [System.Collections.IEnumerable] -and -not ($Node -is [pscustomobject])) {
        $index = 0
        foreach ($item in $Node) { Visit-ArtifactBindings $item "$Context[$index]" $ExpectedCommit; $index++ }
        return
    }
    $bindingShape = (Has-Property $Node 'artifact_ref') -and (Has-Property $Node 'digest') -and (Has-Property $Node 'source_commit') -and (Has-Property $Node 'freshness')
    if ($bindingShape) { Test-ArtifactBinding $Node $Context $ExpectedCommit }
    foreach ($property in $Node.PSObject.Properties) { Visit-ArtifactBindings $property.Value "$Context.$($property.Name)" $ExpectedCommit }
}

$manifestPathResolved = (Resolve-Path -LiteralPath $ManifestPath).Path
$manifestDir = Split-Path -Parent $manifestPathResolved
$manifest = Read-Json $manifestPathResolved
$projectId = [string]$manifest.project_id
if (-not $OutputPath) {
    $safeId = $projectId.Replace('.', '_')
    $OutputPath = Join-Path $repoRoot "out/delivery-project-validation/$safeId/result.json"
}

$schemaPath = Join-Path $repoRoot 'delivery-projects/schema/delivery-project.schema.json'
$schema = Read-Json $schemaPath
Add-Check 'schema.exists' (Test-Path -LiteralPath $schemaPath -PathType Leaf) $schemaPath
foreach ($required in $schema.required) { Add-Check "manifest.required.$required" (Has-Property $manifest ([string]$required)) 'required by schema' }
Add-Check 'manifest.schema_version' ($manifest.schema_version -eq 1) ([string]$manifest.schema_version)
Add-Check 'manifest.project_id' ($projectId -match '^(module|station|line)\.[a-z0-9._-]+$') $projectId
Add-Check 'manifest.delivery_layer' (@('module', 'station', 'line') -contains [string]$manifest.delivery_layer) ([string]$manifest.delivery_layer)
Add-Check 'manifest.layer_prefix' ($projectId.StartsWith(([string]$manifest.delivery_layer) + '.')) "$projectId / $($manifest.delivery_layer)"
Add-Check 'manifest.source_commit_shape' ([string]$manifest.source_commit -match '^[0-9a-f]{40}$') ([string]$manifest.source_commit)

$sourceEntryPath = Join-Path $manifestDir ([string]$manifest.source_entry).Replace('/', '\')
$systemContractPath = Join-Path $manifestDir ([string]$manifest.system_contract).Replace('/', '\')
Add-Check 'manifest.source_entry_project_local' (Test-Path -LiteralPath $sourceEntryPath -PathType Leaf) ([string]$manifest.source_entry)
Add-Check 'manifest.system_contract_project_local' (Test-Path -LiteralPath $systemContractPath -PathType Leaf) ([string]$manifest.system_contract)

$registryRootResolved = if ($RegistryRoot) {
    (Resolve-Path -LiteralPath $RegistryRoot).Path
} else {
    (Resolve-Path -LiteralPath (Join-Path $repoRoot 'delivery-projects')).Path
}
$repoPrefix = $repoRoot.TrimEnd('\') + '\'
Add-Check 'registry.root_within_repository' ($registryRootResolved.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)) $registryRootResolved
$discovered = @(Get-ChildItem -LiteralPath $registryRootResolved -Recurse -Filter delivery-project.json -File | ForEach-Object { $_.FullName })
Add-Check 'registry.discovery' ($discovered -contains $manifestPathResolved) "$registryRootResolved/**/delivery-project.json"
foreach ($property in $manifest.artifact_roots.PSObject.Properties) {
    $raw = [string]$property.Value
    $path = if ($raw.StartsWith('out/')) { Resolve-RepoPath $raw } else { Join-Path $manifestDir $raw.Replace('/', '\') }
    Add-Check "manifest.artifact_root.$($property.Name)" (Test-Path -LiteralPath $path) $raw
}

$requiredFixtureNames = @('api_run_result', 'provenance', 'input_manifest', 'anomalies', 'corrections', 'compiler_stages', 'agent_events', 'wiring_point_checks', 'human_holds')
$fixtures = @{}
$fixturePaths = @{}
foreach ($name in $requiredFixtureNames) {
    $binding = if (Has-Property $manifest.fixtures $name) { $manifest.fixtures.$name } else { $null }
    Add-Check "fixture.$name.binding" ($null -ne $binding) 'fixture binding required'
    if ($null -eq $binding) { continue }
    $path = Join-Path $manifestDir ([string]$binding.fixture_ref).Replace('/', '\')
    $exists = Test-Path -LiteralPath $path -PathType Leaf
    Add-Check "fixture.$name.path_exists" $exists ([string]$binding.fixture_ref)
    if (-not $exists) { continue }
    Add-Check "fixture.$name.digest" ((Get-NormalizedSha256 $path) -eq [string]$binding.digest.value) ([string]$binding.fixture_ref)
    $fixtures[$name] = Read-Json $path
    $fixturePaths[$name] = $path
}

if (-not $fixtures.ContainsKey('api_run_result')) {
    Add-Check 'result.available' $false 'api_run_result is required'
    $result = $null
    $expectedCommit = [string]$manifest.source_commit
} else {
    $result = $fixtures['api_run_result']
    $expectedCommit = [string]$result.git_head
    Add-Check 'result.project_id' ([string]$result.project_id -eq $projectId) "result=$($result.project_id); manifest=$projectId"
    Add-Check 'result.source_commit' ($expectedCommit -eq [string]$manifest.source_commit) "result=$expectedCommit; manifest=$($manifest.source_commit)"
    foreach ($field in @('harness_status', 'delivery_status', 'freshness', 'error_code')) {
        Add-Check "result.shared_schema.$field" (Has-Property $result $field) 'v2 shared result field required'
    }
    if (Has-Property $result 'harness_status') {
        Add-Check 'result.harness_status_compat' ([string]$result.harness_status -eq [string]$result.status.harness_execution) "$($result.harness_status) / $($result.status.harness_execution)"
    }
    if (Has-Property $result 'delivery_status') {
        Add-Check 'result.delivery_status_enum' (@('pass', 'blocked', 'fail') -contains [string]$result.delivery_status) ([string]$result.delivery_status)
        $legacyDeliveryStatus = if ([string]$result.delivery_status -eq 'fail') { 'failed' } else { [string]$result.delivery_status }
        Add-Check 'result.delivery_status_compat' ($legacyDeliveryStatus -eq [string]$result.status.delivery) "$legacyDeliveryStatus / $($result.status.delivery)"
        Add-Check 'result.manifest_delivery_status' ($legacyDeliveryStatus -eq [string]$manifest.delivery_status) "$legacyDeliveryStatus / $($manifest.delivery_status)"
    }
}

Visit-ArtifactBindings $manifest 'manifest' $expectedCommit
foreach ($name in $fixtures.Keys) { Visit-ArtifactBindings $fixtures[$name] "fixture.$name" $expectedCommit }

$passCount = 0; $blockedCount = 0; $failCount = 0; $strictPercent = 0.0
if ($null -ne $result) {
    $acceptance = @($result.acceptance)
    $passCount = @($acceptance | Where-Object { $_.status -eq 'pass' }).Count
    $blockedCount = @($acceptance | Where-Object { $_.status -eq 'blocked' }).Count
    $failCount = @($acceptance | Where-Object { $_.status -eq 'fail' }).Count
    if ($acceptance.Count -gt 0) { $strictPercent = [Math]::Round(100.0 * $passCount / $acceptance.Count, 1) }
    Add-Check 'facts.acceptance_pass' ($passCount -eq [int]$result.status.acceptance_pass -and $passCount -eq [int]$manifest.evidence_summary.acceptance_pass) "computed=$passCount"
    Add-Check 'facts.acceptance_blocked' ($blockedCount -eq [int]$result.status.acceptance_blocked -and $blockedCount -eq [int]$manifest.evidence_summary.acceptance_blocked) "computed=$blockedCount"
    Add-Check 'facts.acceptance_fail' ($failCount -eq [int]$result.status.acceptance_fail -and $failCount -eq [int]$manifest.evidence_summary.acceptance_fail) "computed=$failCount"
    Add-Check 'facts.strict_acceptance_percent' ($strictPercent -eq [double]$manifest.evidence_summary.strict_acceptance_percent) "computed=$strictPercent; manifest=$($manifest.evidence_summary.strict_acceptance_percent)"
    $expectedDelivery = if ($failCount -gt 0) { 'fail' } else { 'blocked' }
    Add-Check 'facts.delivery_projection' ([string]$result.delivery_status -eq $expectedDelivery) "expected=$expectedDelivery; actual=$($result.delivery_status)"
    Add-Check 'facts.harness_projection' ([string]$result.harness_status -eq 'pass') ([string]$result.harness_status)
}

if ($fixtures.ContainsKey('input_manifest')) {
    $input = $fixtures['input_manifest']
    foreach ($field in @('dirty_worktree', 'changed_paths', 'compiler_version', 'runner_version', 'files')) {
        Add-Check "input.required.$field" (Has-Property $input $field) 'input provenance field required'
    }
    $files = @($input.files)
    Add-Check 'input.file_count' ($files.Count -eq [int]$input.file_count) "entries=$($files.Count); declared=$($input.file_count)"
    $paths = @($files | ForEach-Object { [string]$_.path })
    Add-Check 'input.path_uniqueness' (@($paths | Sort-Object -Unique).Count -eq $paths.Count) "entries=$($paths.Count)"
    foreach ($entry in $files) {
        $relative = [string]$entry.path
        $path = Join-Path $manifestDir $relative.Replace('/', '\')
        $exists = Test-Path -LiteralPath $path -PathType Leaf
        Add-Check "input.snapshot.$relative.exists" $exists $relative
        if ($exists) { Add-Check "input.snapshot.$relative.digest" ((Get-NormalizedSha256 $path) -eq [string]$entry.sha256) $relative }
    }
    Add-Check 'input.contains_source_entry' ($paths -contains ([string]$manifest.source_entry).Replace('\', '/')) ([string]$manifest.source_entry)
    Add-Check 'input.contains_system_contract' ($paths -contains ([string]$manifest.system_contract).Replace('\', '/')) ([string]$manifest.system_contract)
}

$eventIds = @()
if ($fixtures.ContainsKey('agent_events')) {
    $eventDoc = $fixtures['agent_events']
    $events = @($eventDoc.records)
    Add-Check 'events.not_coarse_single_event' ($events.Count -ge 4) "count=$($events.Count)"
    $eventIds = @($events | ForEach-Object { [string]$_.event_id })
    Add-Check 'events.unique_ids' (@($eventIds | Sort-Object -Unique).Count -eq $eventIds.Count) "count=$($eventIds.Count)"
    for ($index = 0; $index -lt $events.Count; $index++) {
        $event = $events[$index]
        $expectedSequence = $index + 1
        Add-Check "events.sequence.$expectedSequence" ([int]$event.sequence -eq $expectedSequence) "actual=$($event.sequence)"
        foreach ($field in @('task', 'tool', 'duration_ms', 'result', 'artifact_refs')) {
            Add-Check "events.$expectedSequence.$field" (Has-Property $event $field) 'event field required'
        }
        Add-Check "events.$expectedSequence.duration_nonnegative" ([int64]$event.duration_ms -ge 0) ([string]$event.duration_ms)
        $hasTimes = -not [string]::IsNullOrWhiteSpace([string]$event.started_at) -and -not [string]::IsNullOrWhiteSpace([string]$event.completed_at)
        $explicitMissingTime = (Has-Property $event 'timestamp_state') -and [string]$event.timestamp_state -eq 'not_recorded'
        Add-Check "events.$expectedSequence.time_or_explicit_gap" ($hasTimes -or $explicitMissingTime) 'timestamps or timestamp_state=not_recorded required'
        if ($hasTimes) { Add-Check "events.$expectedSequence.time_order" ([DateTime]$event.completed_at -ge [DateTime]$event.started_at) "$($event.started_at) -> $($event.completed_at)" }
        foreach ($artifactRef in @($event.artifact_refs)) {
            try { $artifactPath = Resolve-RepoPath ([string]$artifactRef); Add-Check "events.$expectedSequence.artifact.$artifactRef" (Test-Path -LiteralPath $artifactPath) ([string]$artifactRef) } catch { Add-Check "events.$expectedSequence.artifact.$artifactRef" $false $_.Exception.Message }
        }
    }
}

if ($fixtures.ContainsKey('anomalies')) {
    $anomalies = @($fixtures['anomalies'].records)
    $gapIds = @($anomalies | Where-Object { Has-Property $_ 'gap_id' } | ForEach-Object { [string]$_.gap_id } | Sort-Object -Unique)
    $resultGaps = if ($null -ne $result) { @($result.known_gaps | ForEach-Object { [string]$_.id } | Sort-Object -Unique) } else { @() }
    Add-Check 'anomalies.all_result_gaps_preserved' (@($resultGaps | Where-Object { $gapIds -notcontains $_ }).Count -eq 0) "result_gaps=$($resultGaps.Count); anomaly_gaps=$($gapIds.Count)"
    foreach ($record in $anomalies) {
        $id = [string]$record.anomaly_id
        Add-Check "anomalies.$id.event_refs" ((Has-Property $record 'event_refs') -and @($record.event_refs).Count -gt 0) 'event_refs required'
        Add-Check "anomalies.$id.evidence_paths" ((Has-Property $record 'evidence_paths') -and @($record.evidence_paths).Count -gt 0) 'evidence_paths required'
        foreach ($eventRef in @($record.event_refs)) { Add-Check "anomalies.$id.event.$eventRef" ($eventIds -contains [string]$eventRef) ([string]$eventRef) }
        $retryCount = if (Has-Property $record 'retry_count') { [int]$record.retry_count } else { 0 }
        if ($retryCount -gt 3) {
            Add-Check "anomalies.$id.long_retry_declared" ([bool]$record.long_search_or_trial_and_error) "retry_count=$retryCount"
            Add-Check "anomalies.$id.retry_events" (@($record.event_refs).Count -ge $retryCount) "events=$(@($record.event_refs).Count); retries=$retryCount"
            Add-Check "anomalies.$id.root_cause_classification" (-not [string]::IsNullOrWhiteSpace([string]$record.classification)) ([string]$record.classification)
        }
    }
}

if ($fixtures.ContainsKey('corrections')) {
    $corrections = @($fixtures['corrections'].records)
    Add-Check 'corrections.count' ($corrections.Count -eq [int]$fixtures['corrections'].correction_count) "entries=$($corrections.Count); declared=$($fixtures['corrections'].correction_count)"
    foreach ($record in $corrections) {
        $id = [string]$record.correction_id
        Add-Check "corrections.$id.status" (-not [string]::IsNullOrWhiteSpace([string]$record.status)) ([string]$record.status)
        Add-Check "corrections.$id.summary" (-not [string]::IsNullOrWhiteSpace([string]$record.summary)) ([string]$record.summary)
    }
}

if ($fixtures.ContainsKey('compiler_stages')) {
    $stages = @($fixtures['compiler_stages'].stages)
    $stageNames = @($stages | ForEach-Object { [string]$_.stage })
    $requiredStages = @('Parser', 'AST', 'Semantic', 'IR', 'Safety', 'Liveness', 'Timing', 'Causality', 'Runtime Bridge / Simulation', 'Process Model Check', 'Intent Alignment', 'Codegen')
    Add-Check 'compiler_stages.complete' (@($requiredStages | Where-Object { $stageNames -notcontains $_ }).Count -eq 0) "count=$($stageNames.Count)"
    $codegen = @($stages | Where-Object { $_.stage -eq 'Codegen' })
    Add-Check 'compiler_stages.codegen_honest' ($codegen.Count -eq 1 -and @('not_exercised', 'verified') -contains [string]$codegen[0].status) ([string]$codegen[0].status)
}

if ($fixtures.ContainsKey('wiring_point_checks')) {
    $wiring = $fixtures['wiring_point_checks']
    $points = @($wiring.points)
    $pointIds = @($points | ForEach-Object { [string]$_.point_id })
    Add-Check 'wiring.unique_points' (@($pointIds | Sort-Object -Unique).Count -eq $pointIds.Count) "count=$($pointIds.Count)"
    Add-Check 'wiring.summary_count' ([int]$wiring.summary.declared_points -eq $points.Count) "declared=$($wiring.summary.declared_points); entries=$($points.Count)"
    foreach ($point in $points) {
        Add-Check "wiring.$($point.point_id).alias" (-not [string]::IsNullOrWhiteSpace([string]$point.alias)) ([string]$point.alias)
        Add-Check "wiring.$($point.point_id).terminal" (-not [string]::IsNullOrWhiteSpace([string]$point.device_terminal)) ([string]$point.device_terminal)
        if ([string]$point.direction -eq 'output') { Add-Check "wiring.$($point.point_id).safe_state" (-not [string]::IsNullOrWhiteSpace([string]$point.safe_state)) ([string]$point.safe_state) }
    }
}

if ($fixtures.ContainsKey('human_holds')) {
    $holds = $fixtures['human_holds']
    $requiredHolds = @('wiring_review', 'point_check_completion', 'safety_review', 'hil_review', 'release_approval')
    $holdIds = @($holds.holds | ForEach-Object { [string]$_.hold_id })
    Add-Check 'holds.complete_set' (@($requiredHolds | Where-Object { $holdIds -notcontains $_ }).Count -eq 0) ($holdIds -join ',')
    $signed = @($holds.holds | Where-Object { $null -ne $_.signature }).Count
    Add-Check 'holds.no_synthetic_signatures' ($signed -eq 0) "signed=$signed"
    Add-Check 'holds.release_not_approved' (@('blocked', 'human_action_required', 'failed') -contains [string]$holds.release_status) ([string]$holds.release_status)
}

$reviewPath = Resolve-RepoPath ([string]$manifest.evidence_bindings.reviewed_completeness.artifact_ref)
$reviewText = Get-Content -Raw -Encoding UTF8 $reviewPath
$expectedCompleteness = [int]$manifest.evidence_summary.implementation_completeness_percent
Add-Check 'review.completeness_percent' ($reviewText -match ([regex]::Escape([string]$expectedCompleteness) + '%')) "expected=$expectedCompleteness%"

$ok = $errors.Count -eq 0
$validation = [ordered]@{
    schema_version = 1
    command = 'validate_delivery_project_fixture'
    ok = $ok
    status = if ($ok) { 'pass' } else { 'fail' }
    error_code = if ($ok) { $null } else { 'DELIVERY_FIXTURE_VALIDATION_FAILED' }
    project_id = $projectId
    source_commit = $expectedCommit
    manifest = $manifestPathResolved.Substring($repoRoot.Length + 1).Replace('\', '/')
    facts = [ordered]@{
        harness_status = if ($null -ne $result) { [string]$result.harness_status } else { 'missing' }
        delivery_status = if ($null -ne $result) { [string]$result.delivery_status } else { 'missing' }
        acceptance_pass = $passCount
        acceptance_blocked = $blockedCount
        acceptance_fail = $failCount
        strict_acceptance_percent = $strictPercent
        implementation_completeness_percent = [int]$manifest.evidence_summary.implementation_completeness_percent
        event_count = if ($fixtures.ContainsKey('agent_events')) { @($fixtures['agent_events'].records).Count } else { 0 }
    }
    check_count = $checks.Count
    error_count = $errors.Count
    errors = $errors.ToArray()
    checks = $checks.ToArray()
}
Write-Json $validation $OutputPath
$validation | ConvertTo-Json -Depth 40
if (-not $ok) { exit 1 }
