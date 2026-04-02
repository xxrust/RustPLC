param(
    [Parameter(Mandatory = $true)]
    [string]$RepoRoot,

    [Parameter(Mandatory = $true)]
    [string]$TargetSkillPath,

    [string]$Task,
    [string]$TaskFile,
    [ValidateSet("amp", "claude", "codex")]
    [string]$Tool = "codex",
    [int]$MaxIterations = 5,
    [int]$IterationTimeoutSeconds = 1800,
    [int]$MaxIdleIterations = 2,
    [string]$StateFile,
    [string]$ProgressFile,
    [string]$LogDir,
    [string]$TaskLabel,
    [switch]$ResetState
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (($Task -and $TaskFile) -or (-not $Task -and -not $TaskFile)) {
    throw "Provide exactly one of -Task or -TaskFile."
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$runnerPath = Join-Path $scriptDir "flywheel_runner.py"
$resolvedTarget = (Resolve-Path $TargetSkillPath).Path
$targetConfigDir = Join-Path $resolvedTarget ".skill_flywheel"
if (-not $LogDir) {
    $LogDir = Join-Path $targetConfigDir "runner_logs"
}
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$sessionLog = Join-Path $LogDir ("session_{0}.log" -f (Get-Date -Format "yyyyMMdd_HHmmss"))
if ($StateFile) {
    $statePath = $StateFile
} else {
    $statePath = Join-Path $targetConfigDir "runner_state.json"
}

function Write-SessionLine {
    param([string]$Message)
    $line = "[{0}] {1}" -f (Get-Date -Format "s"), $Message
    $line | Tee-Object -FilePath $sessionLog -Append
}

function Get-RunnerArgs {
    param([int]$RunnerIterations)
    $argsList = @(
        $runnerPath,
        "--repo-root", $RepoRoot,
        "--target-skill-path", $TargetSkillPath,
        "--tool", $Tool,
        "--max-iterations", $RunnerIterations,
        "--iteration-timeout-seconds", $IterationTimeoutSeconds,
        "--max-idle-iterations", $MaxIdleIterations
    )

    if ($Task) {
        $argsList += @("--task", $Task)
    } else {
        $argsList += @("--task-file", $TaskFile)
    }
    if ($StateFile) {
        $argsList += @("--state-file", $StateFile)
    }
    if ($ProgressFile) {
        $argsList += @("--progress-file", $ProgressFile)
    }
    if ($LogDir) {
        $argsList += @("--log-dir", $LogDir)
    }
    if ($TaskLabel) {
        $argsList += @("--task-label", $TaskLabel)
    }
    if ($ResetState -and $RunnerIterations -eq 1 -and $script:CurrentIteration -eq 1) {
        $argsList += "--reset-state"
    }
    return $argsList
}

Write-SessionLine "Starting flywheel shell loop. Tool=$Tool MaxIterations=$MaxIterations"

for ($script:CurrentIteration = 1; $script:CurrentIteration -le $MaxIterations; $script:CurrentIteration++) {
    Write-SessionLine ("Flywheel Iteration {0} of {1}" -f $script:CurrentIteration, $MaxIterations)
    $runnerArgs = Get-RunnerArgs -RunnerIterations 1
    & python @runnerArgs 2>&1 | Tee-Object -FilePath $sessionLog -Append
    $exitCode = $LASTEXITCODE

    if (-not (Test-Path $statePath)) {
        Write-SessionLine "runner_state.json is missing. Stopping."
        exit 1
    }

    $state = Get-Content $statePath -Encoding UTF8 | ConvertFrom-Json
    $status = [string]$state.status
    $continueNext = $true
    if ($null -ne $state.continue_next_iteration) {
        $continueNext = [bool]$state.continue_next_iteration
    }
    Write-SessionLine ("State status={0} continue_next_iteration={1} last_cycle={2}" -f $status, $continueNext, $state.last_cycle)

    if ($status -in @("complete", "completed", "done", "stop", "stopped") -or -not $continueNext) {
        Write-SessionLine "Stop condition reached."
        exit 0
    }

    if ($status -eq "blocked") {
        Write-SessionLine "Blocked condition reached."
        exit 1
    }

    if ($exitCode -ne 0) {
        Write-SessionLine ("Runner exited with code {0}; continue only if state still requests it." -f $exitCode)
    }

    Start-Sleep -Seconds 2
}

Write-SessionLine ("Reached max shell iterations: {0}" -f $MaxIterations)
exit 0
