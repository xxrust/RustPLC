param(
    [string]$Sil,
    [string]$OpenPlc,
    [string]$Out,
    [string]$Vars = "_state,valve_a,valve_b",
    [int]$TickTolerance = 1,
    [double]$MinPassRate = 0.95
)

$ErrorActionPreference = "Stop"

function Get-PythonCommand {
    $python3 = Get-Command python3 -ErrorAction SilentlyContinue
    if ($python3) {
        return "python3"
    }

    $python = Get-Command python -ErrorAction SilentlyContinue
    if ($python) {
        return "python"
    }

    throw "python3/python not found in PATH"
}

if ([string]::IsNullOrWhiteSpace($Sil) -or [string]::IsNullOrWhiteSpace($OpenPlc) -or [string]::IsNullOrWhiteSpace($Out)) {
    Write-Error "Missing required args: -Sil/-OpenPlc/-Out"
    exit 2
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir "..")) | Select-Object -ExpandProperty Path
$pythonCmd = Get-PythonCommand

& $pythonCmd (Join-Path $repoRoot "scripts/openplc_trace.py") compare `
    --sil $Sil `
    --openplc $OpenPlc `
    --vars $Vars `
    --tick-tolerance $TickTolerance `
    --min-pass-rate $MinPassRate `
    --out $Out

if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
