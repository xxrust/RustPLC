[CmdletBinding()]
param(
    [string]$ManifestPath,
    [string]$OutputPath,
    [string]$RegistryRoot,
    [switch]$RunWiringContractSelfTest
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
            if ($depth -eq 0) { return $text.Substring($start, $index - $start + 1) | ConvertFrom-Json }
        }
    }
    throw "JSON object '$PropertyName' is not closed: $Path"
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

if ($RunWiringContractSelfTest) {
    $selfTestId = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
    $selfTestRoot = Join-Path $repoRoot "out/delivery-fixture-contract-selftest/$selfTestId"
    $projectRoot = Join-Path $selfTestRoot 'module-axis'
    New-Item -ItemType Directory -Force -Path $projectRoot | Out-Null
    $sourceProject = Join-Path $repoRoot 'delivery-projects/module.axis-move-blocking-baseline'
    Copy-Item -LiteralPath (Join-Path $sourceProject 'delivery-project.config.json') -Destination $projectRoot -Force
    Copy-Item -LiteralPath (Join-Path $sourceProject 'source') -Destination $projectRoot -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $sourceProject 'review') -Destination $projectRoot -Recurse -Force
    $selfTestConfigPath = Join-Path $projectRoot 'delivery-project.config.json'
    $selfTestConfig = Read-Json $selfTestConfigPath
    $selfTestConfig.wiring_points = @(
        [pscustomobject]@{ point_id = 'plc_main.Y0'; alias = 'axis_enable'; direction = 'output'; device_terminal = 'axis_x.enable'; safe_state = 'off' },
        [pscustomobject]@{ point_id = 'plc_main.Y0'; alias = 'axis_enable'; direction = 'output'; device_terminal = 'axis_x.enable'; safe_state = 'off' },
        [pscustomobject]@{ point_id = 'plc_main.X99'; alias = 'missing_alias'; direction = 'output'; device_terminal = 'missing_device.out'; safe_state = $null }
    )
    Write-Json $selfTestConfig $selfTestConfigPath

    $materializeLog = Join-Path $selfTestRoot 'materialize.log'
    $projectRef = $projectRoot.Substring($repoRoot.Length + 1).Replace('\', '/')
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $repoRoot 'scripts/materialize_delivery_project_fixture.ps1') -ProjectRoot $projectRef -RunId 'wiring-negative' *> $materializeLog
    $materializeExit = $LASTEXITCODE
    $wiringPath = Join-Path $projectRoot 'wiring/point-checks.json'
    $wiring = Read-Json $wiringPath
    $expectedCodes = @('WIR-001', 'WIR-002', 'WIR-003', 'WIR-004', 'WIR-005', 'WIR-006', 'WIR-007')
    $recordedCodes = @($wiring.diagnostics | ForEach-Object { [string]$_.code } | Sort-Object -Unique)
    $missingCodes = @($expectedCodes | Where-Object { $recordedCodes -notcontains $_ })

    $wiring.diagnostics = @()
    $wiring.validation_summary.error_count = 0
    $wiring.validation_summary.status = 'pass'
    Write-Json $wiring $wiringPath
    $tamperedResultPath = Join-Path $selfTestRoot 'tampered-validation.json'
    $tamperedLog = Join-Path $selfTestRoot 'tampered-validation.log'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -ManifestPath (Join-Path $projectRoot 'delivery-project.json') -RegistryRoot $projectRoot -OutputPath $tamperedResultPath *> $tamperedLog
    $tamperedExit = $LASTEXITCODE
    $tamperedResult = Read-Json $tamperedResultPath
    $diagnosticRecalculationError = @($tamperedResult.errors | Where-Object { [string]$_ -like 'wiring.diagnostics.complete:*' } | Select-Object -First 1)
    $recalculationMentionsEveryCode = $diagnosticRecalculationError.Count -eq 1 -and @($expectedCodes | Where-Object { -not ([string]$diagnosticRecalculationError[0]).Contains($_) }).Count -eq 0
    $ok = $materializeExit -ne 0 -and $missingCodes.Count -eq 0 -and $tamperedExit -ne 0 -and $recalculationMentionsEveryCode
    $selfTestResult = [ordered]@{
        schema_version = 1
        command = 'validate_delivery_project_fixture_wiring_contract_selftest'
        ok = $ok
        status = if ($ok) { 'pass' } else { 'fail' }
        error_code = if ($ok) { $null } else { 'WIRING_CONTRACT_SELFTEST_FAILED' }
        artifact_root = $selfTestRoot.Substring($repoRoot.Length + 1).Replace('\', '/')
        materializer_exit_code = $materializeExit
        expected_codes = $expectedCodes
        recorded_codes = $recordedCodes
        missing_codes = $missingCodes
        tampered_validator_exit_code = $tamperedExit
        validator_recalculation_error = $diagnosticRecalculationError
    }
    $selfTestResult | ConvertTo-Json -Depth 10
    if (-not $ok) { exit 1 }
    exit 0
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
foreach ($scenarioRoot in @($manifest.scenario_roots)) {
    $scenarioRootPath = Join-Path $manifestDir ([string]$scenarioRoot).Replace('/', '\')
    Add-Check "manifest.scenario_root.$scenarioRoot" (Test-Path -LiteralPath $scenarioRootPath -PathType Container) ([string]$scenarioRoot)
}

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
$fixtureNames = @($requiredFixtureNames + @($manifest.fixtures.PSObject.Properties | ForEach-Object { [string]$_.Name }) | Sort-Object -Unique)
$fixtures = @{}
$fixturePaths = @{}
foreach ($name in $fixtureNames) {
    $binding = if (Has-Property $manifest.fixtures $name) { $manifest.fixtures.$name } else { $null }
    Add-Check "fixture.$name.binding" ($null -ne $binding) $(if ($requiredFixtureNames -contains $name) { 'fixture binding required' } else { 'declared fixture binding must resolve' })
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

if ($fixtures.ContainsKey('provenance')) {
    $provenance = $fixtures['provenance']
    $provenanceSchemaPath = Join-Path $repoRoot 'delivery-projects/schema/run-provenance.schema.json'
    Add-Check 'provenance.schema.exists' (Test-Path -LiteralPath $provenanceSchemaPath -PathType Leaf) $provenanceSchemaPath
    if (Test-Path -LiteralPath $provenanceSchemaPath -PathType Leaf) {
        $provenanceSchema = Read-Json $provenanceSchemaPath
        Add-Check 'provenance.schema.migration_branches' (@($provenanceSchema.oneOf).Count -eq 2) 'schema must preserve legacy v1 and attributed v2 branches'
    }
    $provenanceVersion = if (Has-Property $provenance 'schema_version') { [int]$provenance.schema_version } else { 1 }
    if ($provenanceVersion -lt 2) {
        Add-Check 'provenance.legacy_not_proven' ([string]$provenance.unattended_verdict -eq 'not_proven') 'legacy provenance remains readable but cannot prove unattended execution'
    } else {
        foreach ($field in @('project_id', 'run_id', 'git_base_commit', 'started_at_utc', 'completed_at_utc', 'provenance_scope', 'execution_unattended_verdict', 'source_authoring_verdict', 'input_manifest', 'models', 'agents', 'task_definitions', 'skills', 'tool_versions', 'file_attribution')) {
            Add-Check "provenance.required.$field" (Has-Property $provenance $field) 'provenance v2 field required'
        }
        Add-Check 'provenance.project_id' ([string]$provenance.project_id -eq $projectId) "$($provenance.project_id) / $projectId"
        Add-Check 'provenance.git_base_commit' ([string]$provenance.git_base_commit -eq $expectedCommit) "$($provenance.git_base_commit) / $expectedCommit"
        Add-Check 'provenance.time_order' ([DateTime]$provenance.completed_at_utc -ge [DateTime]$provenance.started_at_utc) "$($provenance.started_at_utc) -> $($provenance.completed_at_utc)"
        Add-Check 'provenance.models' (@($provenance.models).Count -gt 0 -and @($provenance.models | Where-Object { [string]::IsNullOrWhiteSpace([string]$_.model) }).Count -eq 0) "count=$(@($provenance.models).Count)"
        Add-Check 'provenance.agents' (@($provenance.agents).Count -gt 0 -and @($provenance.agents | Where-Object { [string]::IsNullOrWhiteSpace([string]$_.agent_id) }).Count -eq 0) "count=$(@($provenance.agents).Count)"
        Add-Check 'provenance.task_definitions' (@($provenance.task_definitions).Count -gt 0 -and @($provenance.task_definitions | Where-Object { [string]::IsNullOrWhiteSpace([string]$_.task_id) -or [string]::IsNullOrWhiteSpace([string]$_.agent_id) }).Count -eq 0) "count=$(@($provenance.task_definitions).Count)"
        Add-Check 'provenance.skill_versions' (@($provenance.skills).Count -gt 0 -and @($provenance.skills | Where-Object { [string]::IsNullOrWhiteSpace([string]$_.version) }).Count -eq 0) "count=$(@($provenance.skills).Count)"
        Add-Check 'provenance.tool_versions' (@($provenance.tool_versions).Count -gt 0 -and @($provenance.tool_versions | Where-Object { [string]::IsNullOrWhiteSpace([string]$_.version) }).Count -eq 0) "count=$(@($provenance.tool_versions).Count)"
        $provenanceAgentIds = @($provenance.agents | ForEach-Object { [string]$_.agent_id } | Sort-Object -Unique)
        $provenanceTaskIds = @($provenance.task_definitions | ForEach-Object { [string]$_.task_id } | Sort-Object -Unique)
        $sourceAuthoringTaskIds = @($provenance.task_definitions | Where-Object { @('source_authoring', 'source_generation') -contains [string]$_.task_kind } | ForEach-Object { [string]$_.task_id } | Sort-Object -Unique)
        foreach ($task in @($provenance.task_definitions)) {
            Add-Check "provenance.task.$($task.task_id).agent" ($provenanceAgentIds -contains [string]$task.agent_id) ([string]$task.agent_id)
        }

        $inputBinding = $provenance.input_manifest
        foreach ($field in @('artifact_ref', 'digest', 'source_commit', 'freshness')) {
            Add-Check "provenance.input_manifest.$field" (Has-Property $inputBinding $field) 'artifact binding field required'
        }
        if (Has-Property $inputBinding 'artifact_ref') {
            try {
                $boundInputPath = Resolve-RepoPath ([string]$inputBinding.artifact_ref)
                Add-Check 'provenance.input_manifest.path' (Test-Path -LiteralPath $boundInputPath -PathType Leaf) ([string]$inputBinding.artifact_ref)
                if (Test-Path -LiteralPath $boundInputPath -PathType Leaf) {
                    Add-Check 'provenance.input_manifest.digest' ((Get-NormalizedSha256 $boundInputPath) -eq [string]$inputBinding.digest.value) ([string]$inputBinding.artifact_ref)
                }
            } catch { Add-Check 'provenance.input_manifest.path' $false $_.Exception.Message }
        }

        $allowedKinds = @('agent_generated', 'agent_modified', 'pre_existing_user_change', 'human_intervention_detected')
        $attributionRecords = @($provenance.file_attribution.records)
        Add-Check 'provenance.file_attribution.records' ($attributionRecords.Count -gt 0) "count=$($attributionRecords.Count)"
        $humanInterventionCount = 0
        $sourceAuthoringRecordCount = 0
        $attributedPaths = New-Object System.Collections.Generic.List[string]
        foreach ($record in $attributionRecords) {
            $pathRef = [string]$record.path
            $attributedPaths.Add($pathRef)
            $kind = [string]$record.attribution_kind
            Add-Check "provenance.file.$pathRef.kind" ($allowedKinds -contains $kind) $kind
            Add-Check "provenance.file.$pathRef.before_present" (Has-Property $record 'before_sha256') 'before_sha256 must be present and may be null for a generated file'
            Add-Check "provenance.file.$pathRef.after_shape" ([string]$record.after_sha256 -match '^[0-9a-f]{64}$') ([string]$record.after_sha256)
            if (@('agent_generated', 'agent_modified') -contains $kind) {
                Add-Check "provenance.file.$pathRef.agent_id" (-not [string]::IsNullOrWhiteSpace([string]$record.agent_id)) ([string]$record.agent_id)
                Add-Check "provenance.file.$pathRef.task_id" (-not [string]::IsNullOrWhiteSpace([string]$record.task_id)) ([string]$record.task_id)
                Add-Check "provenance.file.$pathRef.agent_known" ($provenanceAgentIds -contains [string]$record.agent_id) ([string]$record.agent_id)
                Add-Check "provenance.file.$pathRef.task_known" ($provenanceTaskIds -contains [string]$record.task_id) ([string]$record.task_id)
            }
            if ($kind -eq 'human_intervention_detected') { $humanInterventionCount++ }
            if (@('agent_generated', 'agent_modified') -contains $kind -and $pathRef.Replace('\', '/') -match '/source/.+' -and $sourceAuthoringTaskIds -contains [string]$record.task_id) { $sourceAuthoringRecordCount++ }
            try {
                $attributedPath = Resolve-RepoPath $pathRef
                $exists = Test-Path -LiteralPath $attributedPath -PathType Leaf
                Add-Check "provenance.file.$pathRef.exists" $exists $pathRef
                if ($exists) { Add-Check "provenance.file.$pathRef.after_digest" ((Get-NormalizedSha256 $attributedPath) -eq [string]$record.after_sha256) $pathRef }
            } catch { Add-Check "provenance.file.$pathRef.path" $false $_.Exception.Message }
        }
        Add-Check 'provenance.file_attribution.path_uniqueness' (@($attributedPaths | Sort-Object -Unique).Count -eq $attributedPaths.Count) "count=$($attributedPaths.Count)"
        Add-Check 'provenance.file_attribution.human_count' ([int]$provenance.file_attribution.summary.human_intervention_detected -eq $humanInterventionCount) "computed=$humanInterventionCount"
        Add-Check 'provenance.file_attribution.human_flag' ([bool]$provenance.file_attribution.human_intervention_detected -eq ($humanInterventionCount -gt 0)) "computed=$($humanInterventionCount -gt 0)"
        $expectedExecutionVerdict = if ($humanInterventionCount -gt 0) { 'human_intervention_detected' } else { 'proven' }
        Add-Check 'provenance.execution_unattended_verdict' ([string]$provenance.execution_unattended_verdict -eq $expectedExecutionVerdict) "expected=$expectedExecutionVerdict; actual=$($provenance.execution_unattended_verdict)"
        $expectedSourceAuthoringVerdict = if ($sourceAuthoringRecordCount -gt 0 -and @('source_generation', 'source_authoring_and_delivery_fixture_materialization') -contains [string]$provenance.provenance_scope) { 'proven' } else { 'not_proven' }
        Add-Check 'provenance.source_authoring_verdict' ([string]$provenance.source_authoring_verdict -eq $expectedSourceAuthoringVerdict) "expected=$expectedSourceAuthoringVerdict; actual=$($provenance.source_authoring_verdict)"
        $expectedUnattendedVerdict = if ($humanInterventionCount -gt 0) { 'human_intervention_detected' } elseif ($expectedSourceAuthoringVerdict -eq 'proven') { 'proven' } else { 'not_proven' }
        Add-Check 'provenance.unattended_verdict' ([string]$provenance.unattended_verdict -eq $expectedUnattendedVerdict) "expected=$expectedUnattendedVerdict; actual=$($provenance.unattended_verdict)"
        Add-Check 'provenance.evidence_envelopes' (@($provenance.file_attribution.evidence_envelopes).Count -eq 2) 'provenance and delivery manifest must be identified as recursive evidence envelopes'
    }
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
    $anomalyDocument = $fixtures['anomalies']
    $anomalies = @($anomalyDocument.records)
    $retryThreshold = if ((Has-Property $anomalyDocument 'thresholds') -and (Has-Property $anomalyDocument.thresholds 'retry_count')) { [int]$anomalyDocument.thresholds.retry_count } else { 3 }
    $durationThresholdMs = if ((Has-Property $anomalyDocument 'thresholds') -and (Has-Property $anomalyDocument.thresholds 'duration_ms')) { [int64]$anomalyDocument.thresholds.duration_ms } else { 60000 }
    Add-Check 'anomalies.schema_version' ([int]$anomalyDocument.schema_version -ge 2) ([string]$anomalyDocument.schema_version)
    Add-Check 'anomalies.thresholds.retry_count' ($retryThreshold -gt 0) ([string]$retryThreshold)
    Add-Check 'anomalies.thresholds.duration_ms' ($durationThresholdMs -gt 0) ([string]$durationThresholdMs)
    $eventById = @{}
    if ($fixtures.ContainsKey('agent_events')) {
        foreach ($event in @($fixtures['agent_events'].records)) { $eventById[[string]$event.event_id] = $event }
    }
    $correctionRecords = if ($fixtures.ContainsKey('corrections')) { @($fixtures['corrections'].records) } else { @() }
    $gapIds = @($anomalies | Where-Object { Has-Property $_ 'gap_id' } | ForEach-Object { [string]$_.gap_id } | Sort-Object -Unique)
    $resultGaps = if ($null -ne $result) { @($result.known_gaps | ForEach-Object { [string]$_.id } | Sort-Object -Unique) } else { @() }
    Add-Check 'anomalies.all_result_gaps_preserved' (@($resultGaps | Where-Object { $gapIds -notcontains $_ }).Count -eq 0) "result_gaps=$($resultGaps.Count); anomaly_gaps=$($gapIds.Count)"
    foreach ($record in $anomalies) {
        $id = [string]$record.anomaly_id
        Add-Check "anomalies.$id.event_refs" ((Has-Property $record 'event_refs') -and @($record.event_refs).Count -gt 0) 'event_refs required'
        Add-Check "anomalies.$id.evidence_paths" ((Has-Property $record 'evidence_paths') -and @($record.evidence_paths).Count -gt 0) 'evidence_paths required'
        foreach ($eventRef in @($record.event_refs)) { Add-Check "anomalies.$id.event.$eventRef" ($eventIds -contains [string]$eventRef) ([string]$eventRef) }
        foreach ($field in @('root_cause', 'correction', 'affected_files', 'verification_result', 'long_search_evaluation')) {
            Add-Check "anomalies.$id.$field" (Has-Property $record $field) 'anomaly completeness field required'
        }
        $retryCandidates = @()
        if (Has-Property $record 'retry_count') { $retryCandidates += [int]$record.retry_count }
        if (Has-Property $record 'historical_retry_count') { $retryCandidates += [int]$record.historical_retry_count }
        $effectiveRetryCount = if ($retryCandidates.Count -gt 0) { [int](($retryCandidates | Measure-Object -Maximum).Maximum) } else { 0 }
        $eventDurationMs = [int64]0
        foreach ($eventRef in @($record.event_refs)) {
            if ($eventById.ContainsKey([string]$eventRef)) { $eventDurationMs += [int64]$eventById[[string]$eventRef].duration_ms }
        }
        $expectedLongSearch = $effectiveRetryCount -ge $retryThreshold -or $eventDurationMs -ge $durationThresholdMs
        Add-Check "anomalies.$id.long_search" ([bool]$record.long_search_or_trial_and_error -eq $expectedLongSearch) "expected=$expectedLongSearch; retry=$effectiveRetryCount/$retryThreshold; duration=$eventDurationMs/$durationThresholdMs"
        Add-Check "anomalies.$id.long_search_evaluation.retry_count" ([int]$record.long_search_evaluation.retry_count -eq $effectiveRetryCount) "expected=$effectiveRetryCount"
        Add-Check "anomalies.$id.long_search_evaluation.event_duration_ms" ([int64]$record.long_search_evaluation.event_duration_ms -eq $eventDurationMs) "expected=$eventDurationMs"
        Add-Check "anomalies.$id.root_cause.classification" ([string]$record.root_cause.classification -eq [string]$record.classification) ([string]$record.root_cause.classification)
        Add-Check "anomalies.$id.root_cause.summary" ([string]$record.root_cause.summary -eq [string]$record.summary) ([string]$record.root_cause.summary)
        $evidencePaths = @($record.evidence_paths | ForEach-Object { [string]$_ } | Sort-Object -Unique)
        $affectedFiles = @($record.affected_files | ForEach-Object { [string]$_ } | Sort-Object -Unique)
        Add-Check "anomalies.$id.affected_files_match" (($evidencePaths -join "`n") -eq ($affectedFiles -join "`n")) "evidence=$($evidencePaths -join ','); affected=$($affectedFiles -join ',')"
        $matchingCorrections = @($correctionRecords | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_.anomaly_id) -and [string]$_.anomaly_id -eq $id })
        $expectedCorrectionIds = @($matchingCorrections | ForEach-Object { [string]$_.correction_id } | Sort-Object -Unique)
        $recordedCorrectionIds = @($record.correction.correction_ids | ForEach-Object { [string]$_ } | Sort-Object -Unique)
        Add-Check "anomalies.$id.correction_ids" (($expectedCorrectionIds -join "`n") -eq ($recordedCorrectionIds -join "`n")) "expected=$($expectedCorrectionIds -join ','); actual=$($recordedCorrectionIds -join ',')"
        $expectedCorrectionStatus = if ($matchingCorrections.Count -gt 0) { 'recorded' } else { 'not_corrected' }
        Add-Check "anomalies.$id.correction.status" ([string]$record.correction.status -eq $expectedCorrectionStatus) "expected=$expectedCorrectionStatus; actual=$($record.correction.status)"
        if ($matchingCorrections.Count -eq 0) {
            Add-Check "anomalies.$id.correction.reason" (-not [string]::IsNullOrWhiteSpace([string]$record.correction.reason)) 'uncorrected anomaly requires a reason'
        }
        $verifiedCorrections = @($matchingCorrections | Where-Object { [string]$_.status -like 'verified*' })
        $expectedVerificationStatus = if ($matchingCorrections.Count -eq 0) { 'not_corrected' } elseif ($verifiedCorrections.Count -eq $matchingCorrections.Count) { 'verified' } else { 'recorded_not_verified' }
        Add-Check "anomalies.$id.verification_result.status" ([string]$record.verification_result.status -eq $expectedVerificationStatus) "expected=$expectedVerificationStatus; actual=$($record.verification_result.status)"
        $verificationCorrectionIds = @($record.verification_result.correction_ids | ForEach-Object { [string]$_ } | Sort-Object -Unique)
        Add-Check "anomalies.$id.verification_result.correction_ids" (($expectedCorrectionIds -join "`n") -eq ($verificationCorrectionIds -join "`n")) "expected=$($expectedCorrectionIds -join ','); actual=$($verificationCorrectionIds -join ',')"
    }
}

if ($fixtures.ContainsKey('corrections')) {
    $corrections = @($fixtures['corrections'].records)
    Add-Check 'corrections.count' ($corrections.Count -eq [int]$fixtures['corrections'].correction_count) "entries=$($corrections.Count); declared=$($fixtures['corrections'].correction_count)"
    foreach ($record in $corrections) {
        $id = [string]$record.correction_id
        Add-Check "corrections.$id.anomaly_id" (Has-Property $record 'anomaly_id') 'correction must retain anomaly_id, including explicit null'
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

if ($fixtures.ContainsKey('geometry_export')) {
    $geometryStatus = $fixtures['geometry_export']
    Add-Check 'geometry_export.status' (@('pass', 'blocked') -contains [string]$geometryStatus.status) ([string]$geometryStatus.status)
    Add-Check 'geometry_export.exit_code' (Has-Property $geometryStatus 'exit_code') 'exit_code required'
    if ([string]$geometryStatus.status -eq 'pass') {
        Add-Check 'geometry_export.fixture' ($fixtures.ContainsKey('geometry')) 'successful export requires a geometry fixture binding'
        if ($fixtures.ContainsKey('geometry')) {
            $geometry = $fixtures['geometry']
            Add-Check 'geometry.schema_version' ([int]$geometry.schema_version -ge 2) ([string]$geometry.schema_version)
            Add-Check 'geometry.artifact_kind' ([string]$geometry.artifact_kind -eq 'semantic_twin_geometry') ([string]$geometry.artifact_kind)
            Add-Check 'geometry.nodes_nonempty' (@($geometry.nodes).Count -gt 0) "count=$(@($geometry.nodes).Count)"
            Add-Check 'geometry.edges_nonempty' (@($geometry.edges).Count -gt 0) "count=$(@($geometry.edges).Count)"
            try {
                $statusGeometryPath = Resolve-RepoPath ([string]$geometryStatus.geometry_artifact)
                $fixtureGeometryPath = [System.IO.Path]::GetFullPath([string]$fixturePaths['geometry'])
                Add-Check 'geometry.status_binding' ($statusGeometryPath -eq $fixtureGeometryPath) ([string]$geometryStatus.geometry_artifact)
            } catch { Add-Check 'geometry.status_binding' $false $_.Exception.Message }
        }
    } else {
        Add-Check 'geometry_export.no_synthetic_geometry' (-not $fixtures.ContainsKey('geometry')) 'failed export must not publish an empty geometry fixture'
        Add-Check 'geometry_export.error_code' ([string]$geometryStatus.error_code -eq 'GEOMETRY_EXPORT_FAILED') ([string]$geometryStatus.error_code)
    }
}

if ($fixtures.ContainsKey('wiring_point_checks')) {
    $wiring = $fixtures['wiring_point_checks']
    $points = @($wiring.points)
    $pointIds = @($points | ForEach-Object { [string]$_.point_id })
    Add-Check 'wiring.unique_points' (@($pointIds | Sort-Object -Unique).Count -eq $pointIds.Count) "count=$($pointIds.Count)"
    Add-Check 'wiring.summary.compiler_derived_points' ([int]$wiring.summary.compiler_derived_points -eq $points.Count) "derived=$($wiring.summary.compiler_derived_points); entries=$($points.Count)"
    foreach ($point in $points) {
        Add-Check "wiring.$($point.point_id).alias" (-not [string]::IsNullOrWhiteSpace([string]$point.alias)) ([string]$point.alias)
        Add-Check "wiring.$($point.point_id).terminal" (-not [string]::IsNullOrWhiteSpace([string]$point.device_terminal)) ([string]$point.device_terminal)
        if ([string]$point.direction -eq 'output') { Add-Check "wiring.$($point.point_id).safe_state" (-not [string]::IsNullOrWhiteSpace([string]$point.safe_state)) ([string]$point.safe_state) }
    }
    $wiringVersion = if (Has-Property $wiring 'schema_version') { [int]$wiring.schema_version } else { 1 }
    if ($wiringVersion -ge 2) {
        foreach ($field in @('diagnostics', 'validation_summary', 'evidence_sources')) {
            Add-Check "wiring.required.$field" (Has-Property $wiring $field) 'wiring schema v2 field required'
        }
        $sourceTexts = New-Object System.Collections.Generic.List[string]
        $compilerIrPath = $null
        foreach ($category in @('controller', 'devices', 'connections', 'compiler_ir')) {
            $bindings = @($wiring.evidence_sources.$category)
            Add-Check "wiring.evidence_sources.$category" ($bindings.Count -gt 0) "count=$($bindings.Count)"
            foreach ($binding in $bindings) {
                try {
                    $sourcePath = Resolve-RepoPath ([string]$binding.artifact_ref)
                    $sourceExists = Test-Path -LiteralPath $sourcePath -PathType Leaf
                    Add-Check "wiring.evidence_sources.$category.$($binding.artifact_ref)" $sourceExists ([string]$binding.artifact_ref)
                    if ($sourceExists -and $category -ne 'compiler_ir') { $sourceTexts.Add([System.IO.File]::ReadAllText($sourcePath)) }
                    if ($sourceExists -and $category -eq 'compiler_ir') { $compilerIrPath = $sourcePath }
                } catch { Add-Check "wiring.evidence_sources.$category.path" $false $_.Exception.Message }
            }
        }
        $sourceCorpus = $sourceTexts.ToArray() -join "`n"
        $controllerIoByChannel = @{}
        foreach ($match in [regex]::Matches($sourceCorpus, '(?m)^\s*(input|output)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*((?:X|Y|AI|AO)[0-9]+)\s*\{([^}]*)\}')) {
            $body = [string]$match.Groups[4].Value
            $safeStateMatch = [regex]::Match($body, '(?:^|,)\s*safe_state\s*:\s*([A-Za-z0-9_.-]+)')
            $controllerIoByChannel[[string]$match.Groups[3].Value] = [ordered]@{
                alias = [string]$match.Groups[2].Value
                safe_state = if ($safeStateMatch.Success) { [string]$safeStateMatch.Groups[1].Value } else { $null }
            }
        }
        $derivedPoints = New-Object System.Collections.Generic.List[object]
        if ($null -ne $compilerIrPath) {
            try {
                $irDocument = Read-Json $compilerIrPath
                $irTopology = $irDocument.topology
                if ($null -eq $irTopology) { throw "Compiler IR has no topology object: $compilerIrPath" }
                $irEvidenceRef = $compilerIrPath.Substring($repoRoot.Length + 1).Replace('\', '/')
                foreach ($link in @($irTopology.links)) {
                    $direction = $null; $channel = $null; $deviceName = $null; $devicePort = $null
                    if ([string]$link.to -match '^(X|AI)[0-9]+$') {
                        $direction = 'input'; $channel = [string]$link.to; $deviceName = [string]$link.from
                        $devicePort = if ((Has-Property $link 'from_port') -and -not [string]::IsNullOrWhiteSpace([string]$link.from_port)) { [string]$link.from_port } else { 'self' }
                    } elseif ([string]$link.from -match '^(Y|AO)[0-9]+$') {
                        $direction = 'output'; $channel = [string]$link.from; $deviceName = [string]$link.to
                        $devicePort = if ((Has-Property $link 'to_port') -and -not [string]::IsNullOrWhiteSpace([string]$link.to_port)) { [string]$link.to_port } else { 'self' }
                    } else { continue }
                    $controllerIo = if ($controllerIoByChannel.ContainsKey($channel)) { $controllerIoByChannel[$channel] } else { $null }
                    $derivedPoints.Add([ordered]@{
                        wire_id = "wire:$channel`:$deviceName.$devicePort"
                        point_id = "plc_main.$channel"
                        alias = if ($null -ne $controllerIo) { [string]$controllerIo.alias } else { $null }
                        direction = $direction
                        device_terminal = "$deviceName.$devicePort"
                        signal_type = if ($channel -match '^(AI|AO)[0-9]+$') { 'analog' } else { 'digital' }
                        safe_state = if ($null -ne $controllerIo) { $controllerIo.safe_state } else { $null }
                        evidence_source = $irEvidenceRef
                    })
                }
            } catch { Add-Check 'wiring.compiler_ir.parse' $false $_.Exception.Message }
        }
        $recordedWireKeys = @($points | ForEach-Object { "$($_.wire_id)|$($_.point_id)|$($_.alias)|$($_.direction)|$($_.device_terminal)|$($_.signal_type)|$($_.safe_state)|$($_.evidence_source)" } | Sort-Object -Unique)
        $derivedWireKeys = @($derivedPoints | ForEach-Object { "$($_.wire_id)|$($_.point_id)|$($_.alias)|$($_.direction)|$($_.device_terminal)|$($_.signal_type)|$($_.safe_state)|$($_.evidence_source)" } | Sort-Object -Unique)
        Add-Check 'wiring.compiler_projection.complete' (($derivedWireKeys -join "`n") -eq ($recordedWireKeys -join "`n")) "derived=$($derivedWireKeys.Count); recorded=$($recordedWireKeys.Count)"
        $configPath = Join-Path $manifestDir 'delivery-project.config.json'
        $configDocument = Read-Json $configPath
        $expectedPoints = @($configDocument.wiring_points)
        Add-Check 'wiring.summary.declared_points' ([int]$wiring.summary.declared_points -eq $expectedPoints.Count) "declared=$($wiring.summary.declared_points); expected=$($expectedPoints.Count)"
        $expectedDiagnostics = New-Object System.Collections.Generic.List[object]
        foreach ($duplicate in @($expectedPoints | Group-Object point_id | Where-Object { $_.Count -gt 1 })) {
            $expectedDiagnostics.Add([ordered]@{ code = 'WIR-001'; point_id = [string]$duplicate.Name; kind = 'duplicate_channel' })
        }
        foreach ($duplicate in @($expectedPoints | Group-Object alias | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_.Name) -and $_.Count -gt 1 })) {
            $expectedDiagnostics.Add([ordered]@{ code = 'WIR-002'; point_id = [string]$duplicate.Group[0].point_id; kind = 'duplicate_alias' })
        }
        foreach ($duplicate in @($expectedPoints | Group-Object device_terminal | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_.Name) -and $_.Count -gt 1 })) {
            $expectedDiagnostics.Add([ordered]@{ code = 'WIR-003'; point_id = [string]$duplicate.Group[0].point_id; kind = 'duplicate_terminal' })
        }
        foreach ($point in $expectedPoints) {
            $pointId = [string]$point.point_id
            $alias = [string]$point.alias
            $terminal = [string]$point.device_terminal
            $direction = [string]$point.direction
            $directionMatches = ($direction -eq 'input' -and $pointId -match '^plc_main\.(X|AI)[0-9]+$') -or ($direction -eq 'output' -and $pointId -match '^plc_main\.(Y|AO)[0-9]+$')
            if (-not $directionMatches) { $expectedDiagnostics.Add([ordered]@{ code = 'WIR-004'; point_id = $pointId; kind = 'direction_mismatch' }) }
            $aliasRef = if ([string]::IsNullOrWhiteSpace($alias)) { $null } else { "plc_main.$alias" }
            $controllerRefs = @($pointId, $aliasRef | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) })
            if (@($controllerRefs | Where-Object { $sourceCorpus.Contains([string]$_) }).Count -eq 0) {
                $expectedDiagnostics.Add([ordered]@{ code = 'WIR-005'; point_id = $pointId; kind = 'unresolved_alias' })
            }
            if (@($derivedPoints | Where-Object { [string]$_.point_id -eq $pointId -and [string]$_.direction -eq $direction -and [string]$_.device_terminal -eq $terminal }).Count -eq 0) {
                $expectedDiagnostics.Add([ordered]@{ code = 'WIR-006'; point_id = $pointId; kind = 'unbound_port' })
            }
            if ($direction -eq 'output' -and [string]::IsNullOrWhiteSpace([string]$point.safe_state)) {
                $expectedDiagnostics.Add([ordered]@{ code = 'WIR-007'; point_id = $pointId; kind = 'missing_safe_state' })
            }
        }
        $recordedDiagnostics = @($wiring.diagnostics)
        $expectedKeys = @($expectedDiagnostics | ForEach-Object { "$($_.code)|$($_.point_id)" } | Sort-Object -Unique)
        $recordedKeys = @($recordedDiagnostics | ForEach-Object { "$($_.code)|$($_.point_id)" } | Sort-Object -Unique)
        Add-Check 'wiring.diagnostics.complete' (@($expectedKeys | Where-Object { $recordedKeys -notcontains $_ }).Count -eq 0) "expected=$($expectedKeys -join ',') recorded=$($recordedKeys -join ',')"
        Add-Check 'wiring.diagnostics.no_false_positive' (@($recordedKeys | Where-Object { $expectedKeys -notcontains $_ }).Count -eq 0) "expected=$($expectedKeys -join ',') recorded=$($recordedKeys -join ',')"
        Add-Check 'wiring.validation_summary.error_count' ([int]$wiring.validation_summary.error_count -eq $expectedKeys.Count) "expected=$($expectedKeys.Count); actual=$($wiring.validation_summary.error_count)"
        $expectedWiringStatus = if ($expectedKeys.Count -eq 0) { 'pass' } else { 'fail' }
        Add-Check 'wiring.validation_summary.status' ([string]$wiring.validation_summary.status -eq $expectedWiringStatus) "expected=$expectedWiringStatus; actual=$($wiring.validation_summary.status)"
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
