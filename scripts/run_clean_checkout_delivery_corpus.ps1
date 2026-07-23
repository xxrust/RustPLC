[CmdletBinding()]
param(
    [string]$RunId,
    [string]$SourceCommit = 'HEAD'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$sourceRepo = Split-Path -Parent $PSScriptRoot
if (-not $RunId) { $RunId = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') }

if ($RunId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$') {
    [ordered]@{
        schema_version = 1
        command = 'run_clean_checkout_delivery_corpus'
        run_id = $RunId
        ok = $false
        status = 'fail'
        error_code = 'CLEAN_CHECKOUT_RUN_ID_INVALID'
        operation = 'validate_run_id'
        current_step = 'validate_run_id'
        message = 'RunId must use 1-64 ASCII letters, digits, dots, underscores, or hyphens and must start with a letter or digit.'
        evidence = @()
        remediation = @('Choose a filesystem-safe RunId such as 20260724-160000.')
    } | ConvertTo-Json -Depth 10
    exit 19
}

$evidenceRoot = Join-Path $sourceRepo 'out/clean-checkout'
$runRoot = Join-Path $evidenceRoot $RunId
$checkoutRoot = Join-Path $runRoot 'repo'
$logsRoot = Join-Path $runRoot 'logs'
$resultPath = Join-Path $runRoot 'result.json'
$orchestratorSnapshotPath = Join-Path $runRoot 'orchestrator.snapshot.ps1'
$corpusRunId = 'cc'
$longestProjectedArtifact = Join-Path $checkoutRoot "out/delivery-project-corpus/$corpusRunId/specimens/station.dual-slot-shuttle-press-cell/runs/$corpusRunId/project-check/no_board_gate/artifacts/virtual_board_meta.json"

$exitCodes = @{
    CLEAN_CHECKOUT_RUN_ID_INVALID = 19
    CLEAN_CHECKOUT_SOURCE_NOT_GIT = 20
    CLEAN_CHECKOUT_RUN_ALREADY_EXISTS = 21
    CLEAN_CHECKOUT_SOURCE_COMMIT_UNRESOLVED = 22
    CLEAN_CHECKOUT_PREFLIGHT_TOOL_MISSING = 23
    CLEAN_CHECKOUT_CLONE_FAILED = 24
    CLEAN_CHECKOUT_COMMIT_MISMATCH = 25
    CLEAN_CHECKOUT_DIRTY_BEFORE_RUN = 26
    CLEAN_CHECKOUT_REQUIRED_INPUT_MISSING = 27
    CLEAN_CHECKOUT_CORPUS_FAILED = 28
    CLEAN_CHECKOUT_CORPUS_RESULT_MISSING = 29
    CLEAN_CHECKOUT_CORPUS_RESULT_INVALID = 30
    CLEAN_CHECKOUT_CORPUS_PROVENANCE_MISMATCH = 31
    CLEAN_CHECKOUT_DIRTY_AFTER_RUN = 32
    CLEAN_CHECKOUT_PATH_BUDGET_EXCEEDED = 33
    CLEAN_CHECKOUT_INTERNAL_ERROR = 99
}

if ([System.IO.Path]::GetFullPath($longestProjectedArtifact).Length -gt 240) {
    [ordered]@{
        schema_version = 1
        command = 'run_clean_checkout_delivery_corpus'
        run_id = $RunId
        ok = $false
        status = 'fail'
        error_code = 'CLEAN_CHECKOUT_PATH_BUDGET_EXCEEDED'
        operation = 'validate_path_budget'
        current_step = 'validate_path_budget'
        message = "Projected artifact path exceeds the 240-character Windows safety budget: $longestProjectedArtifact"
        evidence = @($longestProjectedArtifact)
        remediation = @('Choose a shorter outer RunId or move the repository to a shorter filesystem path.')
    } | ConvertTo-Json -Depth 10
    exit $exitCodes.CLEAN_CHECKOUT_PATH_BUDGET_EXCEEDED
}

function Write-Json {
    param(
        [Parameter(Mandatory)]$Value,
        [Parameter(Mandatory)][string]$Path
    )

    $parent = Split-Path -Parent $Path
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    $json = $Value | ConvertTo-Json -Depth 50
    [System.IO.File]::WriteAllText(
        $Path,
        $json + [Environment]::NewLine,
        (New-Object System.Text.UTF8Encoding($false))
    )
}

function Read-Json {
    param([Parameter(Mandatory)][string]$Path)
    return Get-Content -Raw -Encoding UTF8 -LiteralPath $Path | ConvertFrom-Json
}

function Artifact-Ref {
    param([Parameter(Mandatory)][string]$Path)

    $resolved = [System.IO.Path]::GetFullPath($Path)
    $prefix = [System.IO.Path]::GetFullPath($runRoot).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "artifact path escapes run root: $resolved"
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

function New-RunException {
    param(
        [Parameter(Mandatory)][string]$ErrorCode,
        [Parameter(Mandatory)][string]$Operation,
        [Parameter(Mandatory)][string]$Message,
        [string[]]$Evidence = @(),
        [string[]]$Remediation = @()
    )

    $exception = New-Object System.Exception($Message)
    $exception.Data['error_code'] = $ErrorCode
    $exception.Data['operation'] = $Operation
    $exception.Data['evidence'] = @($Evidence)
    $exception.Data['remediation'] = @($Remediation)
    return $exception
}

function Invoke-CapturedCommand {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Command,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [Parameter(Mandatory)][string]$StdoutPath,
        [Parameter(Mandatory)][string]$StderrPath
    )

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StdoutPath) | Out-Null
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $exitCode = Invoke-NativeProcess $Command $Arguments $WorkingDirectory $StdoutPath $StderrPath
    }
    finally {
        $watch.Stop()
    }

    return [ordered]@{
        name = $Name
        status = if ($exitCode -eq 0) { 'pass' } else { 'fail' }
        exit_code = [int]$exitCode
        elapsed_ms = [int64]$watch.ElapsedMilliseconds
        stdout_ref = Artifact-Ref $StdoutPath
        stderr_ref = Artifact-Ref $StderrPath
        artifact_refs = @()
    }
}

function Get-NativeOutput {
    param(
        [Parameter(Mandatory)][string]$Command,
        [Parameter(Mandatory)][string[]]$Arguments
    )

    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = @(& $Command @Arguments 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    return [ordered]@{
        exit_code = [int]$exitCode
        output = ($output | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
    }
}

function Get-GitStatusLines {
    param(
        [Parameter(Mandatory)][string]$Repository,
        [ValidateSet('all', 'normal')][string]$UntrackedFiles = 'all'
    )

    $status = Get-NativeOutput 'git' @('-C', $Repository, 'status', '--porcelain=v1', "--untracked-files=$UntrackedFiles")
    if ($status.exit_code -ne 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_INTERNAL_ERROR' `
            -Operation 'git_status' `
            -Message "git status failed for $Repository" `
            -Remediation @('Inspect the git status command and repository integrity.'))
    }
    if ([string]::IsNullOrWhiteSpace($status.output)) { return @() }
    return @($status.output -split "`r?`n" | Where-Object { $_ })
}

function Get-ToolVersion {
    param(
        [Parameter(Mandatory)][string]$Command,
        [Parameter(Mandatory)][string[]]$Arguments
    )

    $tool = Get-Command $Command -ErrorAction SilentlyContinue
    if ($null -eq $tool) { return $null }
    $version = Get-NativeOutput $Command $Arguments
    if ($version.exit_code -ne 0) { return $null }
    return $version.output.Trim()
}

function Assert-RequiredTrackedInputs {
    param([Parameter(Mandatory)][string]$Repository)

    $required = @(
        'Cargo.toml',
        'Cargo.lock',
        'src/main.rs',
        'scripts/run_clean_checkout_delivery_corpus.ps1',
        'scripts/run_delivery_project_corpus.ps1',
        'scripts/materialize_delivery_project_fixture.ps1',
        'scripts/validate_delivery_project_fixture.ps1',
        'delivery-projects/schema/delivery-project.schema.json',
        'delivery-projects/schema/run-provenance.schema.json',
        'delivery-projects/module.axis-move-blocking-baseline/delivery-project.config.json',
        'delivery-projects/station.dual-slot-shuttle-press-cell/delivery-project.config.json',
        'delivery-projects/line.three-station-assembly/delivery-project.config.json'
    )

    $missing = New-Object System.Collections.Generic.List[string]
    foreach ($path in $required) {
        $check = Get-NativeOutput 'git' @('-C', $Repository, 'ls-files', '--error-unmatch', '--', $path)
        if ($check.exit_code -ne 0) { $missing.Add($path) }
    }
    if ($missing.Count -gt 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_REQUIRED_INPUT_MISSING' `
            -Operation 'tracked_input_preflight' `
            -Message "Required committed inputs are missing: $($missing -join ', ')" `
            -Remediation @('Commit every required runner, schema, project config, and compiler input before rerunning.'))
    }
    return $required
}

if (Test-Path -LiteralPath $runRoot) {
    $collision = [ordered]@{
        schema_version = 1
        command = 'run_clean_checkout_delivery_corpus'
        run_id = $RunId
        ok = $false
        status = 'fail'
        error_code = 'CLEAN_CHECKOUT_RUN_ALREADY_EXISTS'
        operation = 'create_run_root'
        current_step = 'create_run_root'
        message = "Run root already exists: $runRoot"
        evidence = @($runRoot)
        remediation = @('Choose a new RunId; existing run evidence is immutable.')
    }
    $collision | ConvertTo-Json -Depth 10
    exit $exitCodes.CLEAN_CHECKOUT_RUN_ALREADY_EXISTS
}

New-Item -ItemType Directory -Force -Path $runRoot, $logsRoot | Out-Null
Copy-Item -LiteralPath $PSCommandPath -Destination $orchestratorSnapshotPath

$started = [DateTime]::UtcNow
$steps = New-Object System.Collections.Generic.List[object]
$currentStep = 'preflight'
$resolvedCommit = $null
$sourceChangedPaths = @()
$preRunStatus = @()
$postRunStatus = @()
$requiredTrackedInputs = @()
$corpusResult = $null
$corpusResultPath = Join-Path $checkoutRoot "out/delivery-project-corpus/$corpusRunId/corpus-result.json"
$toolVersions = [ordered]@{}
$result = $null
$finalExitCode = 0
$trackedFileCount = 0

try {
    $currentStep = 'source_repository_preflight'
    $insideWorkTree = Get-NativeOutput 'git' @('-C', $sourceRepo, 'rev-parse', '--is-inside-work-tree')
    if ($insideWorkTree.exit_code -ne 0 -or $insideWorkTree.output.Trim() -ne 'true') {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_SOURCE_NOT_GIT' `
            -Operation $currentStep `
            -Message "Source path is not a git worktree: $sourceRepo" `
            -Remediation @('Run this script from a committed RustPLC repository checkout.'))
    }

    foreach ($command in @('git', 'cargo', 'rustc', 'powershell')) {
        if ($null -eq (Get-Command $command -ErrorAction SilentlyContinue)) {
            throw (New-RunException `
                -ErrorCode 'CLEAN_CHECKOUT_PREFLIGHT_TOOL_MISSING' `
                -Operation $currentStep `
                -Message "Required tool is unavailable on PATH: $command" `
                -Remediation @("Install $command and make it available on PATH."))
        }
    }

    $toolVersions.git = Get-ToolVersion 'git' @('--version')
    $toolVersions.cargo = Get-ToolVersion 'cargo' @('--version')
    $toolVersions.rustc = Get-ToolVersion 'rustc' @('--version')
    $toolVersions.powershell = $PSVersionTable.PSVersion.ToString()

    $commitResolution = Get-NativeOutput 'git' @('-C', $sourceRepo, 'rev-parse', '--verify', "$SourceCommit`^{commit}")
    if ($commitResolution.exit_code -ne 0 -or [string]::IsNullOrWhiteSpace($commitResolution.output)) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_SOURCE_COMMIT_UNRESOLVED' `
            -Operation $currentStep `
            -Message "Cannot resolve source commit: $SourceCommit" `
            -Remediation @('Pass an existing commit, tag, or branch name.'))
    }
    $resolvedCommit = $commitResolution.output.Trim()
    $sourceChangedPaths = @(Get-GitStatusLines $sourceRepo -UntrackedFiles normal)
    $sourcePreflightPath = Join-Path $runRoot 'source-preflight.json'
    Write-Json ([ordered]@{
        status = 'pass'
        source_repository = [System.IO.Path]::GetFullPath($sourceRepo)
        requested_ref = $SourceCommit
        resolved_commit = $resolvedCommit
        source_worktree_dirty = $sourceChangedPaths.Count -gt 0
        source_changed_paths = $sourceChangedPaths
        tool_versions = $toolVersions
    }) $sourcePreflightPath
    $steps.Add([ordered]@{
        name = $currentStep
        status = 'pass'
        exit_code = 0
        elapsed_ms = [int64]0
        stdout_ref = $null
        stderr_ref = $null
        artifact_refs = @((Artifact-Ref $sourcePreflightPath))
    })

    $currentStep = 'clone_committed_repository'
    $clone = Invoke-CapturedCommand `
        -Name $currentStep `
        -Command 'git' `
        -Arguments @('clone', '--no-local', '--no-checkout', '--quiet', '--', $sourceRepo, $checkoutRoot) `
        -WorkingDirectory $runRoot `
        -StdoutPath (Join-Path $logsRoot 'clone.stdout.log') `
        -StderrPath (Join-Path $logsRoot 'clone.stderr.log')
    $steps.Add($clone)
    if ($clone.exit_code -ne 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_CLONE_FAILED' `
            -Operation $currentStep `
            -Message "git clone failed with exit code $($clone.exit_code)" `
            -Evidence @($clone.stdout_ref, $clone.stderr_ref) `
            -Remediation @('Inspect clone logs and confirm the source repository is readable.'))
    }

    $currentStep = 'checkout_requested_commit'
    $checkout = Invoke-CapturedCommand `
        -Name $currentStep `
        -Command 'git' `
        -Arguments @('-C', $checkoutRoot, 'checkout', '--detach', '--quiet', $resolvedCommit) `
        -WorkingDirectory $runRoot `
        -StdoutPath (Join-Path $logsRoot 'checkout.stdout.log') `
        -StderrPath (Join-Path $logsRoot 'checkout.stderr.log')
    $steps.Add($checkout)
    if ($checkout.exit_code -ne 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_CLONE_FAILED' `
            -Operation $currentStep `
            -Message "git checkout failed with exit code $($checkout.exit_code)" `
            -Evidence @($checkout.stdout_ref, $checkout.stderr_ref) `
            -Remediation @('Inspect checkout logs and verify the requested commit exists in the clone.'))
    }

    $actualHead = Get-NativeOutput 'git' @('-C', $checkoutRoot, 'rev-parse', 'HEAD')
    if ($actualHead.exit_code -ne 0 -or $actualHead.output.Trim() -ne $resolvedCommit) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_COMMIT_MISMATCH' `
            -Operation $currentStep `
            -Message "Clone HEAD does not match requested commit $resolvedCommit" `
            -Remediation @('Discard this run and inspect clone/checkout behavior.'))
    }

    $currentStep = 'clean_checkout_preflight'
    $preRunStatus = @(Get-GitStatusLines $checkoutRoot)
    if ($preRunStatus.Count -gt 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_DIRTY_BEFORE_RUN' `
            -Operation $currentStep `
            -Message "Fresh clone is dirty before execution: $($preRunStatus -join '; ')" `
            -Remediation @('Inspect checkout filters, line-ending configuration, and generated files.'))
    }
    $requiredTrackedInputs = @(Assert-RequiredTrackedInputs $checkoutRoot)

    $trackedCountResult = Get-NativeOutput 'git' @('-C', $checkoutRoot, 'ls-files')
    if ($trackedCountResult.exit_code -ne 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_INTERNAL_ERROR' `
            -Operation $currentStep `
            -Message 'Unable to enumerate tracked files in the clean clone.' `
            -Remediation @('Inspect clone integrity and git availability.'))
    }
    $trackedFileCount = @($trackedCountResult.output -split "`r?`n" | Where-Object { $_ }).Count
    $checkoutPreflightPath = Join-Path $runRoot 'checkout-preflight.json'
    Write-Json ([ordered]@{
        status = 'pass'
        checkout_kind = 'git_clone_no_local_detached'
        checkout_ref = Artifact-Ref $checkoutRoot
        head_commit = $resolvedCommit
        git_status_porcelain = $preRunStatus
        clean = $preRunStatus.Count -eq 0
        tracked_file_count = $trackedFileCount
        required_tracked_inputs = $requiredTrackedInputs
        untracked_dependency_check = 'pass'
    }) $checkoutPreflightPath
    $steps.Add([ordered]@{
        name = $currentStep
        status = 'pass'
        exit_code = 0
        elapsed_ms = [int64]0
        stdout_ref = $null
        stderr_ref = $null
        artifact_refs = @((Artifact-Ref $checkoutPreflightPath))
    })

    $currentStep = 'run_delivery_project_corpus'
    $savedCargoTargetDir = $env:CARGO_TARGET_DIR
    $savedRustcWrapper = $env:RUSTC_WRAPPER
    $savedRustFlags = $env:RUSTFLAGS
    try {
        $env:CARGO_TARGET_DIR = $null
        $env:RUSTC_WRAPPER = $null
        $env:RUSTFLAGS = $null
        $corpus = Invoke-CapturedCommand `
            -Name $currentStep `
            -Command 'powershell' `
            -Arguments @(
                '-NoProfile', '-ExecutionPolicy', 'Bypass',
                '-File', (Join-Path $checkoutRoot 'scripts/run_delivery_project_corpus.ps1'),
                '-RunId', $corpusRunId
            ) `
            -WorkingDirectory $checkoutRoot `
            -StdoutPath (Join-Path $logsRoot 'corpus.stdout.log') `
            -StderrPath (Join-Path $logsRoot 'corpus.stderr.log')
    }
    finally {
        $env:CARGO_TARGET_DIR = $savedCargoTargetDir
        $env:RUSTC_WRAPPER = $savedRustcWrapper
        $env:RUSTFLAGS = $savedRustFlags
    }
    if (Test-Path -LiteralPath $corpusResultPath -PathType Leaf) {
        $corpus.artifact_refs = @((Artifact-Ref $corpusResultPath))
    }
    $steps.Add($corpus)
    if ($corpus.exit_code -ne 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_CORPUS_FAILED' `
            -Operation $currentStep `
            -Message "Delivery corpus runner failed with exit code $($corpus.exit_code)" `
            -Evidence @($corpus.stdout_ref, $corpus.stderr_ref) `
            -Remediation @('Inspect the corpus logs and its same-run artifacts; do not relabel a project or harness failure.'))
    }
    if (-not (Test-Path -LiteralPath $corpusResultPath -PathType Leaf)) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_CORPUS_RESULT_MISSING' `
            -Operation $currentStep `
            -Message "Corpus runner did not produce $corpusResultPath" `
            -Evidence @($corpus.stdout_ref, $corpus.stderr_ref) `
            -Remediation @('Inspect the corpus runner output contract.'))
    }

    try {
        $corpusResult = Read-Json $corpusResultPath
    }
    catch {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_CORPUS_RESULT_INVALID' `
            -Operation $currentStep `
            -Message "Corpus result is not valid JSON: $($_.Exception.Message)" `
            -Evidence @((Artifact-Ref $corpusResultPath)) `
            -Remediation @('Repair the corpus result serializer or schema before rerunning.'))
    }

    $provenanceProblems = New-Object System.Collections.Generic.List[string]
    if ([string]$corpusResult.source_commit -ne $resolvedCommit) { $provenanceProblems.Add('source_commit mismatch') }
    if ([bool]$corpusResult.dirty_worktree) { $provenanceProblems.Add('corpus reported dirty_worktree=true') }
    if (@($corpusResult.changed_paths).Count -ne 0) { $provenanceProblems.Add('corpus reported changed paths') }
    if ([string]$corpusResult.harness_status -ne 'pass') { $provenanceProblems.Add('harness_status is not pass') }
    if ([string]$corpusResult.freshness -ne 'same_run') { $provenanceProblems.Add('freshness is not same_run') }
    if ([int]$corpusResult.project_count -ne 3) { $provenanceProblems.Add('project_count is not 3') }
    foreach ($project in @($corpusResult.project_results)) {
        if ([string]$project.project_run_id -ne $corpusRunId) {
            $provenanceProblems.Add("project run mismatch: $($project.project_id)")
        }
    }
    if ($provenanceProblems.Count -gt 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_CORPUS_PROVENANCE_MISMATCH' `
            -Operation 'validate_corpus_provenance' `
            -Message ($provenanceProblems -join '; ') `
            -Evidence @((Artifact-Ref $corpusResultPath)) `
            -Remediation @('Treat the clean-checkout proof as failed and repair the mismatched provenance field.'))
    }

    $currentStep = 'clean_checkout_postflight'
    $postRunStatus = @(Get-GitStatusLines $checkoutRoot)
    if ($postRunStatus.Count -gt 0) {
        throw (New-RunException `
            -ErrorCode 'CLEAN_CHECKOUT_DIRTY_AFTER_RUN' `
            -Operation $currentStep `
            -Message "Corpus execution changed tracked or untracked non-ignored files: $($postRunStatus -join '; ')" `
            -Evidence @((Artifact-Ref $corpusResultPath)) `
            -Remediation @('Move generated files under ignored run-specific output roots or restore input immutability.'))
    }
    $checkoutPostflightPath = Join-Path $runRoot 'checkout-postflight.json'
    Write-Json ([ordered]@{
        status = 'pass'
        head_commit = $resolvedCommit
        git_status_porcelain = $postRunStatus
        clean = $postRunStatus.Count -eq 0
        corpus_result_ref = Artifact-Ref $corpusResultPath
    }) $checkoutPostflightPath
    $steps.Add([ordered]@{
        name = $currentStep
        status = 'pass'
        exit_code = 0
        elapsed_ms = [int64]0
        stdout_ref = $null
        stderr_ref = $null
        artifact_refs = @((Artifact-Ref $checkoutPostflightPath), (Artifact-Ref $corpusResultPath))
    })

    $completed = [DateTime]::UtcNow
    $corpusCheckCount = [int](@($corpusResult.project_results | ForEach-Object { [int]$_.check_count } | Measure-Object -Sum).Sum)
    $corpusErrorCount = [int](@($corpusResult.project_results | ForEach-Object { [int]$_.error_count } | Measure-Object -Sum).Sum)
    $result = [ordered]@{
        schema_version = 1
        command = 'run_clean_checkout_delivery_corpus'
        run_id = $RunId
        ok = $true
        status = 'pass'
        error_code = $null
        operation = 'complete'
        current_step = 'complete'
        message = 'The committed RustPLC delivery corpus passed from an isolated clean clone.'
        started_at_utc = $started.ToString('o')
        completed_at_utc = $completed.ToString('o')
        elapsed_ms = [int64]($completed - $started).TotalMilliseconds
        source = [ordered]@{
            repository = [System.IO.Path]::GetFullPath($sourceRepo)
            requested_ref = $SourceCommit
            commit = $resolvedCommit
            source_worktree_dirty = $sourceChangedPaths.Count -gt 0
            source_changed_paths = $sourceChangedPaths
        }
        orchestrator = [ordered]@{
            script = 'scripts/run_clean_checkout_delivery_corpus.ps1'
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $orchestratorSnapshotPath).Hash.ToLowerInvariant()
            snapshot_ref = Artifact-Ref $orchestratorSnapshotPath
        }
        checkout = [ordered]@{
            kind = 'git_clone_no_local_detached'
            checkout_ref = Artifact-Ref $checkoutRoot
            head_commit = $resolvedCommit
            clean_before_run = $preRunStatus.Count -eq 0
            clean_after_run = $postRunStatus.Count -eq 0
            untracked_dependency_check = 'pass'
            tracked_file_count = $trackedFileCount
            required_tracked_inputs = $requiredTrackedInputs
        }
        fresh_checkout = [ordered]@{
            status = 'pass'
            actual_clean_clone = $true
            requested_commit_checked_out = $true
            clean_before_run = $true
            clean_after_run = $true
            committed_corpus_runner_executed = $true
            corpus_harness_status = [string]$corpusResult.harness_status
        }
        tool_versions = $toolVersions
        environment = [ordered]@{
            cargo_target_dir_cleared_for_run = $true
            rustc_wrapper_cleared_for_run = $true
            rustflags_cleared_for_run = $true
        }
        steps = $steps.ToArray()
        corpus = [ordered]@{
            run_id = $corpusRunId
            result_ref = Artifact-Ref $corpusResultPath
            source_commit = [string]$corpusResult.source_commit
            harness_status = [string]$corpusResult.harness_status
            delivery_status = [string]$corpusResult.delivery_status
            project_count = [int]$corpusResult.project_count
            check_count = $corpusCheckCount
            error_count = $corpusErrorCount
            delivery_summary = $corpusResult.delivery_summary
            repeatability = $corpusResult.repeatability
        }
        artifact_refs = @(
            Artifact-Ref $orchestratorSnapshotPath
            Artifact-Ref $sourcePreflightPath
            Artifact-Ref $checkoutPreflightPath
            Artifact-Ref $checkoutPostflightPath
            Artifact-Ref $corpusResultPath
            'logs/clone.stdout.log'
            'logs/clone.stderr.log'
            'logs/checkout.stdout.log'
            'logs/checkout.stderr.log'
            'logs/corpus.stdout.log'
            'logs/corpus.stderr.log'
        )
        evidence = @((Artifact-Ref $corpusResultPath))
        remediation = @()
    }
}
catch {
    $completed = [DateTime]::UtcNow
    $errorCode = if ($_.Exception.Data.Contains('error_code')) {
        [string]$_.Exception.Data['error_code']
    } else {
        'CLEAN_CHECKOUT_INTERNAL_ERROR'
    }
    $operation = if ($_.Exception.Data.Contains('operation')) {
        [string]$_.Exception.Data['operation']
    } else {
        $currentStep
    }
    $evidence = if ($_.Exception.Data.Contains('evidence')) {
        $rawEvidence = $_.Exception.Data['evidence']
        if ($null -eq $rawEvidence) { @() } else { @($rawEvidence | ForEach-Object { [string]$_ }) }
    } else {
        @()
    }
    $remediation = if ($_.Exception.Data.Contains('remediation')) {
        $rawRemediation = $_.Exception.Data['remediation']
        if ($null -eq $rawRemediation) { @() } else { @($rawRemediation | ForEach-Object { [string]$_ }) }
    } else {
        @('Inspect result.json, step logs, and the failing operation before retrying.')
    }
    $finalExitCode = if ($exitCodes.ContainsKey($errorCode)) { [int]$exitCodes[$errorCode] } else { 99 }
    $result = [ordered]@{
        schema_version = 1
        command = 'run_clean_checkout_delivery_corpus'
        run_id = $RunId
        ok = $false
        status = 'fail'
        error_code = $errorCode
        operation = $operation
        current_step = $currentStep
        message = $_.Exception.Message
        started_at_utc = $started.ToString('o')
        completed_at_utc = $completed.ToString('o')
        elapsed_ms = [int64]($completed - $started).TotalMilliseconds
        source = [ordered]@{
            repository = [System.IO.Path]::GetFullPath($sourceRepo)
            requested_ref = $SourceCommit
            commit = $resolvedCommit
            source_worktree_dirty = $sourceChangedPaths.Count -gt 0
            source_changed_paths = $sourceChangedPaths
        }
        orchestrator = [ordered]@{
            script = 'scripts/run_clean_checkout_delivery_corpus.ps1'
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $orchestratorSnapshotPath).Hash.ToLowerInvariant()
            snapshot_ref = Artifact-Ref $orchestratorSnapshotPath
        }
        checkout = [ordered]@{
            kind = 'git_clone_no_local_detached'
            checkout_ref = if (Test-Path -LiteralPath $checkoutRoot) { Artifact-Ref $checkoutRoot } else { $null }
            clean_before_run = if ($preRunStatus.Count -eq 0 -and (Test-Path -LiteralPath $checkoutRoot)) { $true } else { $false }
            clean_after_run = if ($postRunStatus.Count -eq 0 -and $currentStep -eq 'clean_checkout_postflight') { $true } else { $null }
            untracked_dependency_check = 'fail'
            required_tracked_inputs = $requiredTrackedInputs
        }
        fresh_checkout = [ordered]@{
            status = 'fail'
            actual_clean_clone = Test-Path -LiteralPath $checkoutRoot
            requested_commit_checked_out = $null -ne $resolvedCommit
            clean_before_run = if ($preRunStatus.Count -eq 0 -and (Test-Path -LiteralPath $checkoutRoot)) { $true } else { $false }
            clean_after_run = if ($postRunStatus.Count -eq 0 -and $currentStep -eq 'clean_checkout_postflight') { $true } else { $null }
            committed_corpus_runner_executed = $null -ne $corpusResult
            corpus_harness_status = if ($null -ne $corpusResult) { [string]$corpusResult.harness_status } else { $null }
        }
        tool_versions = $toolVersions
        steps = $steps.ToArray()
        corpus = if ($null -ne $corpusResult) {
            [ordered]@{
                run_id = $corpusRunId
                result_ref = Artifact-Ref $corpusResultPath
                source_commit = [string]$corpusResult.source_commit
                harness_status = [string]$corpusResult.harness_status
                delivery_status = [string]$corpusResult.delivery_status
            }
        } else {
            $null
        }
        evidence = @($evidence)
        remediation = @($remediation)
    }
}

Write-Json $result $resultPath
$result | ConvertTo-Json -Depth 50
exit $finalExitCode
