[CmdletBinding()]
param(
    [string]$RunId,
    [string]$RepeatOf
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $RunId) { $RunId = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') }
$runRoot = Join-Path $repoRoot "out/delivery-project-corpus/$RunId"
if (Test-Path -LiteralPath $runRoot) {
    throw "corpus run already exists; choose a new RunId: $runRoot"
}
$specimenRoot = Join-Path $runRoot 'specimens'
$projectOutputRoot = Join-Path $runRoot 'projects'
New-Item -ItemType Directory -Force -Path $specimenRoot, $projectOutputRoot | Out-Null

function Read-Json {
    param([Parameter(Mandatory)][string]$Path)
    return Get-Content -Raw -Encoding UTF8 $Path | ConvertFrom-Json
}

function Write-Json {
    param([Parameter(Mandatory)]$Value, [Parameter(Mandatory)][string]$Path)
    $parent = Split-Path -Parent $Path
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    $json = $Value | ConvertTo-Json -Depth 50
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, (New-Object System.Text.UTF8Encoding($false)))
}

function Repo-Ref {
    param([Parameter(Mandatory)][string]$Path)
    $resolved = [System.IO.Path]::GetFullPath($Path)
    $prefix = $repoRoot.TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "path escapes repository root: $resolved"
    }
    return $resolved.Substring($prefix.Length).Replace('\', '/')
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

function Invoke-CapturedCommand {
    param(
        [Parameter(Mandatory)][string]$Command,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$StdoutPath,
        [Parameter(Mandatory)][string]$StderrPath
    )
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StdoutPath), (Split-Path -Parent $StderrPath) | Out-Null
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $exitCode = Invoke-NativeProcess $Command $Arguments $repoRoot $StdoutPath $StderrPath
    }
    finally {
        $watch.Stop()
    }
    return [ordered]@{ exit_code = $exitCode; elapsed_ms = [int64]$watch.ElapsedMilliseconds }
}

$started = [DateTime]::UtcNow
$sourceCommit = (& git -C $repoRoot rev-parse HEAD).Trim()
$gitStatusLines = @(& git -C $repoRoot status --porcelain=v1)
$changedPaths = @($gitStatusLines | ForEach-Object { if ($_.Length -gt 3) { $_.Substring(3).Replace('\', '/') } })

$thresholds = New-Object System.Collections.Generic.List[object]
function Add-Threshold {
    param([string]$Name, [bool]$Ok, [string]$Detail)
    $thresholds.Add([ordered]@{ name = $Name; ok = $Ok; detail = $Detail })
}

$buildStdout = Join-Path $runRoot 'build/cargo-build.stdout.log'
$buildStderr = Join-Path $runRoot 'build/cargo-build.stderr.log'
$build = Invoke-CapturedCommand 'cargo' @('build', '--bin', 'rust_plc') $buildStdout $buildStderr
Add-Threshold 'current_compiler_build_pass' ($build.exit_code -eq 0) "exit_code=$($build.exit_code)"

$configFiles = @(
    Get-ChildItem -LiteralPath (Join-Path $repoRoot 'delivery-projects') -Recurse -Filter delivery-project.config.json -File |
        Sort-Object FullName
)
$configs = @($configFiles | ForEach-Object { Read-Json $_.FullName })
$projectIds = @($configs | ForEach-Object { [string]$_.project_id })
$layers = @($configs | ForEach-Object { [string]$_.delivery_layer })

Add-Threshold 'project_count_exactly_three' ($configFiles.Count -eq 3) "count=$($configFiles.Count)"
Add-Threshold 'project_ids_unique' (@($projectIds | Sort-Object -Unique).Count -eq $projectIds.Count) ($projectIds -join ',')
foreach ($layer in @('module', 'station', 'line')) {
    Add-Threshold "layer.$layer.exactly_one" (@($layers | Where-Object { $_ -eq $layer }).Count -eq 1) ($layers -join ',')
}

$canonicalRoots = @($configFiles | ForEach-Object { $_.Directory.FullName.TrimEnd('\') })
for ($left = 0; $left -lt $canonicalRoots.Count; $left++) {
    for ($right = $left + 1; $right -lt $canonicalRoots.Count; $right++) {
        $leftPrefix = $canonicalRoots[$left] + '\'
        $rightPrefix = $canonicalRoots[$right] + '\'
        $overlap = $canonicalRoots[$left].StartsWith($rightPrefix, [System.StringComparison]::OrdinalIgnoreCase) -or $canonicalRoots[$right].StartsWith($leftPrefix, [System.StringComparison]::OrdinalIgnoreCase)
        Add-Threshold "independence.root.$left.$right" (-not $overlap) "$($canonicalRoots[$left]) / $($canonicalRoots[$right])"
    }
}

foreach ($configFile in $configFiles) {
    $projectRoot = $configFile.Directory.FullName
    $projectId = [string](Read-Json $configFile.FullName).project_id
    $otherRootNames = @($configFiles | Where-Object { $_.Directory.FullName -ne $projectRoot } | ForEach-Object { $_.Directory.Name })
    $sourceText = @(
        Get-ChildItem -LiteralPath (Join-Path $projectRoot 'source') -Recurse -File -ErrorAction SilentlyContinue |
            ForEach-Object { Get-Content -Raw -Encoding UTF8 $_.FullName }
    ) -join "`n"
    foreach ($otherName in $otherRootNames) {
        Add-Threshold "independence.source_ref.$projectId.$otherName" (-not $sourceText.Contains("delivery-projects/$otherName")) 'cross-project authored source references are forbidden'
    }
}

$projectResults = New-Object System.Collections.Generic.List[object]
foreach ($configFile in $configFiles) {
    $config = Read-Json $configFile.FullName
    $canonicalRoot = $configFile.Directory.FullName
    $projectId = [string]$config.project_id
    $safeId = $projectId.Replace('.', '_')
    $specimenPath = Join-Path $specimenRoot $configFile.Directory.Name
    New-Item -ItemType Directory -Force -Path $specimenPath | Out-Null
    Copy-Item -LiteralPath $configFile.FullName -Destination (Join-Path $specimenPath 'delivery-project.config.json') -Force
    Copy-Item -LiteralPath (Join-Path $canonicalRoot 'source') -Destination $specimenPath -Recurse -Force
    $reviewSource = Join-Path $canonicalRoot 'review'
    if (Test-Path -LiteralPath $reviewSource -PathType Container) {
        Copy-Item -LiteralPath $reviewSource -Destination $specimenPath -Recurse -Force
    }

    $projectLogRoot = Join-Path $projectOutputRoot $safeId
    $materializeStdout = Join-Path $projectLogRoot 'materializer.stdout.log'
    $materializeStderr = Join-Path $projectLogRoot 'materializer.stderr.log'
    if ($build.exit_code -eq 0) {
        $materialize = Invoke-CapturedCommand 'powershell' @(
            '-NoProfile', '-ExecutionPolicy', 'Bypass',
            '-File', (Join-Path $PSScriptRoot 'materialize_delivery_project_fixture.ps1'),
            '-ProjectRoot', (Repo-Ref $specimenPath),
            '-RunId', $RunId
        ) $materializeStdout $materializeStderr
    } else {
        [System.IO.File]::WriteAllText($materializeStderr, 'Skipped because cargo build failed.' + [Environment]::NewLine)
        $materialize = [ordered]@{ exit_code = 1; elapsed_ms = 0 }
    }

    $manifestPath = Join-Path $specimenPath 'delivery-project.json'
    $validationPath = Join-Path $projectLogRoot 'validation-result.json'
    $validatorStdout = Join-Path $projectLogRoot 'validator.stdout.log'
    $validatorStderr = Join-Path $projectLogRoot 'validator.stderr.log'
    if ($materialize.exit_code -eq 0 -and (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        $validationRun = Invoke-CapturedCommand 'powershell' @(
            '-NoProfile', '-ExecutionPolicy', 'Bypass',
            '-File', (Join-Path $PSScriptRoot 'validate_delivery_project_fixture.ps1'),
            '-ManifestPath', $manifestPath,
            '-OutputPath', $validationPath,
            '-RegistryRoot', $specimenRoot
        ) $validatorStdout $validatorStderr
    } else {
        [System.IO.File]::WriteAllText($validatorStderr, 'Skipped because materialization failed.' + [Environment]::NewLine)
        $validationRun = [ordered]@{ exit_code = 1; elapsed_ms = 0 }
    }

    $validation = if (Test-Path -LiteralPath $validationPath -PathType Leaf) { Read-Json $validationPath } else { $null }
    $manifest = if (Test-Path -LiteralPath $manifestPath -PathType Leaf) { Read-Json $manifestPath } else { $null }
    $projectRun = $null
    $projectRunPath = $null
    if ($null -ne $manifest) {
        $projectRunPath = Join-Path $specimenPath ([string]$manifest.fixtures.api_run_result.fixture_ref).Replace('/', '\')
        if (Test-Path -LiteralPath $projectRunPath -PathType Leaf) { $projectRun = Read-Json $projectRunPath }
    }

    $stepNames = if ($null -ne $projectRun) { @($projectRun.steps | ForEach-Object { [string]$_.name }) } else { @() }
    $projectResults.Add([ordered]@{
        project_id = $projectId
        delivery_layer = [string]$config.delivery_layer
        project_run_id = if ($null -ne $projectRun) { [string]$projectRun.run_id } else { $null }
        materializer_status = if ($materialize.exit_code -eq 0) { 'pass' } else { 'fail' }
        validation_status = if ($validationRun.exit_code -eq 0 -and $null -ne $validation) { [string]$validation.status } else { 'fail' }
        harness_status = if ($null -ne $projectRun) { [string]$projectRun.harness_status } else { 'missing' }
        delivery_status = if ($null -ne $projectRun) { [string]$projectRun.delivery_status } else { 'missing' }
        acceptance_pass = if ($null -ne $projectRun) { [int]$projectRun.status.acceptance_pass } else { 0 }
        acceptance_blocked = if ($null -ne $projectRun) { [int]$projectRun.status.acceptance_blocked } else { 0 }
        acceptance_fail = if ($null -ne $projectRun) { [int]$projectRun.status.acceptance_fail } else { 0 }
        input_digest = if ($null -ne $projectRun) { [string]$projectRun.digests.input_set_sha256 } else { $null }
        post_run_input_digest = if ($null -ne $projectRun) { [string]$projectRun.digests.post_run_input_set_sha256 } else { $null }
        source_digest = if ($null -ne $projectRun) { [string]$projectRun.digests.source_set_sha256 } else { $null }
        input_changed_during_run = if ($null -ne $projectRun) { [bool]$projectRun.status.input_changed_during_run } else { $true }
        step_names = $stepNames
        event_count = if ($null -ne $validation) { [int]$validation.facts.event_count } else { 0 }
        check_count = if ($null -ne $validation) { [int]$validation.check_count } else { 0 }
        error_count = if ($null -ne $validation) { [int]$validation.error_count } else { 1 }
        elapsed_ms = [int64]($materialize.elapsed_ms + $validationRun.elapsed_ms)
        specimen_ref = Repo-Ref $specimenPath
        manifest_ref = if ($null -ne $manifest) { Repo-Ref $manifestPath } else { $null }
        project_result_ref = if ($null -ne $projectRunPath) { Repo-Ref $projectRunPath } else { $null }
        validation_result_ref = if (Test-Path -LiteralPath $validationPath) { Repo-Ref $validationPath } else { $null }
        materializer_stdout = Repo-Ref $materializeStdout
        materializer_stderr = Repo-Ref $materializeStderr
        validator_stdout = Repo-Ref $validatorStdout
        validator_stderr = Repo-Ref $validatorStderr
    })
}

Add-Threshold 'all_materializers_pass' (@($projectResults | Where-Object { $_.materializer_status -ne 'pass' }).Count -eq 0) ($projectResults.materializer_status -join ',')
Add-Threshold 'all_fixture_validators_pass' (@($projectResults | Where-Object { $_.validation_status -ne 'pass' }).Count -eq 0) ($projectResults.validation_status -join ',')
Add-Threshold 'all_harness_status_pass' (@($projectResults | Where-Object { $_.harness_status -ne 'pass' }).Count -eq 0) ($projectResults.harness_status -join ',')
Add-Threshold 'all_project_runs_same_run' (@($projectResults | Where-Object { $_.project_run_id -ne $RunId }).Count -eq 0) ($projectResults.project_run_id -join ',')
Add-Threshold 'all_inputs_stable_during_run' (@($projectResults | Where-Object { $_.input_changed_during_run }).Count -eq 0) ($projectResults.input_changed_during_run -join ',')
Add-Threshold 'event_streams_non_coarse' (@($projectResults | Where-Object { $_.event_count -lt 4 }).Count -eq 0) ($projectResults.event_count -join ',')

$repeatability = [ordered]@{
    status = 'not_proven'
    baseline_ref = $null
    compared_fields = @('project_id', 'materializer_status', 'validation_status', 'harness_status', 'delivery_status', 'acceptance_pass', 'acceptance_blocked', 'acceptance_fail', 'input_digest', 'step_names')
    differences = @()
}
if ($RepeatOf) {
    $baselinePath = if ([System.IO.Path]::IsPathRooted($RepeatOf)) { $RepeatOf } else { Join-Path $repoRoot $RepeatOf.Replace('/', '\') }
    if (Test-Path -LiteralPath $baselinePath -PathType Leaf) {
        $baseline = Read-Json $baselinePath
        $differences = New-Object System.Collections.Generic.List[string]
        if ([string]$baseline.source_commit -ne $sourceCommit) {
            $differences.Add("source_commit: $($baseline.source_commit) -> $sourceCommit")
        }
        foreach ($current in $projectResults) {
            $prior = @($baseline.project_results | Where-Object { $_.project_id -eq $current.project_id })
            if ($prior.Count -ne 1) { $differences.Add("missing baseline project: $($current.project_id)"); continue }
            foreach ($field in @('materializer_status', 'validation_status', 'harness_status', 'delivery_status', 'acceptance_pass', 'acceptance_blocked', 'acceptance_fail', 'input_digest')) {
                if ([string]$current.$field -ne [string]$prior[0].$field) {
                    $differences.Add("$($current.project_id).${field}: $($prior[0].$field) -> $($current.$field)")
                }
            }
            $currentSteps = @($current.step_names) -join '|'
            $priorSteps = @($prior[0].step_names) -join '|'
            if ($currentSteps -ne $priorSteps) {
                $differences.Add("$($current.project_id).step_names: $priorSteps -> $currentSteps")
            }
        }
        $repeatability.status = if ($differences.Count -eq 0) { 'pass' } else { 'fail' }
        $repeatability.baseline_ref = Repo-Ref $baselinePath
        $repeatability.differences = $differences.ToArray()
    } else {
        $repeatability.status = 'fail'
        $repeatability.differences = @("baseline does not exist: $RepeatOf")
    }
}
Add-Threshold 'repeatability_when_requested' (-not $RepeatOf -or $repeatability.status -eq 'pass') ([string]$repeatability.status)

$anomalies = @(
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-001'; classification = 'subagent_definition_drift'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'Definition v1 replaced the existing canonical project and conflated harness validity with delivery readiness.'; correction_id = 'CORPUS-COR-001' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-002'; classification = 'path_normalization'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'Materializer used Resolve-Path for future output artifacts and failed before compilation.'; correction_id = 'CORPUS-COR-002' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-003'; classification = 'powershell_native_stderr'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'PowerShell 5 promoted normal native stderr progress output to a terminating error under Stop preference.'; correction_id = 'CORPUS-COR-003' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-004'; classification = 'powershell_parameter_contract'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'The event helper incorrectly required a non-empty artifact array for scenario validation.'; correction_id = 'CORPUS-COR-004' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-005'; classification = 'canonical_example_metadata'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'Axis baseline failed SEM-107 because device purpose metadata was missing.'; correction_id = 'CORPUS-COR-005' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-006'; classification = 'canonical_example_safety'; status = 'corrected'; retry_count = 2; long_search_or_trial_and_error = $false; summary = 'Axis baseline first used the wrong run state and then required an explicit enable state before motion.'; correction_id = 'CORPUS-COR-006' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-007'; classification = 'runtime_mapping'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'Axis project-check could not resolve a physical output until plc_main.Y0 was mapped to axis_x.enable.'; correction_id = 'CORPUS-COR-007' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-008'; classification = 'scenario_runtime_code_gap'; status = 'failed'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'Three-station scenario-doctor and no-board-gate reject semantic sensor guard expressions; line delivery remains fail.'; correction_id = $null },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-009'; classification = 'multi_agent_route_interruption'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'Earlier cross-agent redirection interrupted definition and review branches and created conflicting intermediate scopes.'; correction_id = 'CORPUS-COR-008' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-010'; classification = 'powershell_interpolation'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'The first corpus runner parse failed because a field interpolation was parsed as a drive-qualified variable.'; correction_id = 'CORPUS-COR-009' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-011'; classification = 'false_freshness_claim'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'The first corpus runner validated fixed project artifacts and labeled the projection same-run without re-executing compile and project-check.'; correction_id = 'CORPUS-COR-010' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-012'; classification = 'repeatability_digest_design'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'The input-manifest digest included run metadata and could not serve as a stable cross-run source digest.'; correction_id = 'CORPUS-COR-011' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-013'; classification = 'validator_registry_coupling'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'The validator hard-coded the canonical delivery-projects registry and rejected valid run-specific specimen manifests.'; correction_id = 'CORPUS-COR-012' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-014'; classification = 'historical_retry_projection'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'Station historical retry counts were projected as current-run retries and made chronology validation demand synthetic events.'; correction_id = 'CORPUS-COR-013' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-015'; classification = 'incomplete_input_digest'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'The stable digest covered source files but omitted the config that selects scenarios, accepted product gaps, and acceptance metadata.'; correction_id = 'CORPUS-COR-014' },
    [ordered]@{ anomaly_id = 'CORPUS-ANOM-016'; classification = 'windows_path_budget'; status = 'corrected'; retry_count = 1; long_search_or_trial_and_error = $false; summary = 'The clean-checkout runner repeated a long outer RunId inside corpus and project run directories, pushing module and station artifacts beyond the traditional Windows MAX_PATH boundary.'; correction_id = 'CORPUS-COR-015' }
)
$corrections = @(
    [ordered]@{ correction_id = 'CORPUS-COR-001'; status = 'verified'; summary = 'Definition v2 preserves the station fixture, adds module and line layers, and separates harness_status from delivery_status.' },
    [ordered]@{ correction_id = 'CORPUS-COR-002'; status = 'verified'; summary = 'Replaced Resolve-Path with workspace-contained GetFullPath for future artifacts.' },
    [ordered]@{ correction_id = 'CORPUS-COR-003'; status = 'verified'; summary = 'Scoped ErrorActionPreference around native process execution and retained exit codes and logs.' },
    [ordered]@{ correction_id = 'CORPUS-COR-004'; status = 'verified'; summary = 'Made event artifact lists optional while preserving the required empty array.' },
    [ordered]@{ correction_id = 'CORPUS-COR-005'; status = 'verified'; summary = 'Added purpose metadata in the project-local axis snapshot.' },
    [ordered]@{ correction_id = 'CORPUS-COR-006'; status = 'verified'; summary = 'Added set axis_x.enable on before the blocking move.' },
    [ordered]@{ correction_id = 'CORPUS-COR-007'; status = 'verified'; summary = 'Added plc_main and the Y0 to axis-enable topology relation; project-check then passed.' },
    [ordered]@{ correction_id = 'CORPUS-COR-008'; status = 'verified'; summary = 'Stopped cross-branch implementation edits; definition and execution roles now have bounded tasks.' },
    [ordered]@{ correction_id = 'CORPUS-COR-009'; status = 'verified'; summary = 'Delimited the interpolated field variable; the runner parses and executes.' },
    [ordered]@{ correction_id = 'CORPUS-COR-010'; status = 'verified'; summary = 'Each corpus run now copies three read-only specimens, materializes current-run artifacts, and validates the generated manifests.' },
    [ordered]@{ correction_id = 'CORPUS-COR-011'; status = 'verified'; summary = 'Materializer now records a stable source-set digest and rechecks it after pipeline execution.' },
    [ordered]@{ correction_id = 'CORPUS-COR-012'; status = 'verified'; summary = 'Validator now accepts an explicit repository-contained RegistryRoot; corpus runs pass their specimen registry.' },
    [ordered]@{ correction_id = 'CORPUS-COR-013'; status = 'verified'; summary = 'Station records current-run retry_count separately from historical_retry_count while preserving the seven-attempt finding.' },
    [ordered]@{ correction_id = 'CORPUS-COR-014'; status = 'verified'; summary = 'Repeatability now compares input_set_sha256 over config, source, and review while preserving a separate source_set_sha256.' },
    [ordered]@{ correction_id = 'CORPUS-COR-015'; status = 'verified'; summary = 'The clean runner now uses the short internal run id cc and rejects projected artifact paths beyond a 240-character Windows safety budget before cloning.' }
)

$completed = [DateTime]::UtcNow
$harnessStatus = if (@($thresholds | Where-Object { -not $_.ok }).Count -eq 0) { 'pass' } else { 'fail' }
$deliveryStatuses = @($projectResults | ForEach-Object { [string]$_.delivery_status })
$deliveryStatus = if ($deliveryStatuses -contains 'fail') { 'fail' } elseif ($deliveryStatuses -contains 'blocked') { 'blocked' } else { 'pass' }
$allValidatorsPass = @($projectResults | Where-Object { $_.validation_status -ne 'pass' }).Count -eq 0
$resultPath = Join-Path $runRoot 'corpus-result.json'
$result = [ordered]@{
    schema_version = 2
    command = 'run_delivery_project_corpus'
    pipeline_mode = 'materialize_current_run_specimens'
    corpus_run_id = $RunId
    harness_status = $harnessStatus
    delivery_status = $deliveryStatus
    freshness = 'same_run'
    error_code = if ($harnessStatus -eq 'pass') { $null } else { 'DELIVERY_CORPUS_VALIDATION_FAILED' }
    source_commit = $sourceCommit
    dirty_worktree = $changedPaths.Count -gt 0
    changed_paths = $changedPaths
    started_at_utc = $started.ToString('o')
    completed_at_utc = $completed.ToString('o')
    elapsed_ms = [int64]($completed - $started).TotalMilliseconds
    build = [ordered]@{
        status = if ($build.exit_code -eq 0) { 'pass' } else { 'fail' }
        exit_code = $build.exit_code
        elapsed_ms = $build.elapsed_ms
        stdout_log = Repo-Ref $buildStdout
        stderr_log = Repo-Ref $buildStderr
    }
    specimen_root = Repo-Ref $specimenRoot
    project_count = $projectResults.Count
    project_results = $projectResults.ToArray()
    thresholds = $thresholds.ToArray()
    delivery_summary = [ordered]@{
        pass = @($projectResults | Where-Object { $_.delivery_status -eq 'pass' }).Count
        blocked = @($projectResults | Where-Object { $_.delivery_status -eq 'blocked' }).Count
        fail = @($projectResults | Where-Object { $_.delivery_status -eq 'fail' }).Count
    }
    fresh_checkout = [ordered]@{
        actual_clean_clone = 'not_proven'
        repository_local_ownership_proxy = if ($allValidatorsPass) { 'pass' } else { 'fail' }
        reason = 'This run proves repository-local specimen ownership and ignored-out independence. It does not claim an actual clean clone.'
    }
    repeatability = $repeatability
    anomalies = $anomalies
    corrections = $corrections
}
Write-Json $result $resultPath
$result | ConvertTo-Json -Depth 50
if ($harnessStatus -ne 'pass') { exit 1 }
