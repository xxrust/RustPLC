param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$RunnerArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $RunnerArgs -or $RunnerArgs.Count -eq 0) {
    throw "Pass the arguments for flywheel.ps1 after this script."
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$flywheelScript = Join-Path $scriptDir "flywheel.ps1"

function Get-ArgumentValue {
    param(
        [string[]]$ArgsList,
        [string]$Name
    )
    for ($i = 0; $i -lt $ArgsList.Count; $i++) {
        if ($ArgsList[$i] -eq $Name -and $i + 1 -lt $ArgsList.Count) {
            return $ArgsList[$i + 1]
        }
    }
    return $null
}

$targetSkillPath = Get-ArgumentValue -ArgsList $RunnerArgs -Name "-TargetSkillPath"
$explicitLogDir = Get-ArgumentValue -ArgsList $RunnerArgs -Name "-LogDir"

if ($explicitLogDir) {
    $sessionDir = $explicitLogDir
} elseif ($targetSkillPath) {
    $resolvedTarget = (Resolve-Path $targetSkillPath).Path
    $sessionDir = Join-Path $resolvedTarget ".skill_flywheel\runner_logs"
} else {
    $sessionDir = Join-Path (Get-Location) ".flywheel_logs"
}

New-Item -ItemType Directory -Force -Path $sessionDir | Out-Null
$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$stdoutLog = Join-Path $sessionDir "background_${timestamp}.out.log"
$stderrLog = Join-Path $sessionDir "background_${timestamp}.err.log"

$argList = @(
    "-NoProfile",
    "-ExecutionPolicy", "Bypass",
    "-File", $flywheelScript
) + $RunnerArgs

$process = Start-Process -FilePath "powershell.exe" `
    -ArgumentList $argList `
    -WindowStyle Hidden `
    -RedirectStandardOutput $stdoutLog `
    -RedirectStandardError $stderrLog `
    -PassThru

[pscustomobject]@{
    pid = $process.Id
    stdout_log = $stdoutLog
    stderr_log = $stderrLog
    script = $flywheelScript
} | ConvertTo-Json -Compress
