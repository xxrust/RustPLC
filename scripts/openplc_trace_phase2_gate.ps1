param(
    [string]$FixtureDir,
    [string]$OutDir
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir "..")) | Select-Object -ExpandProperty Path

if ([string]::IsNullOrWhiteSpace($FixtureDir)) {
    $FixtureDir = Join-Path $repoRoot "examples/openplc_trace_phase2"
}
if ([string]::IsNullOrWhiteSpace($OutDir)) {
    $OutDir = Join-Path $repoRoot "out/openplc_trace_phase2"
}

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

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

$pythonCmd = Get-PythonCommand

function Invoke-Phase2Case {
    param(
        [Parameter(Mandatory = $true)][string]$CaseName,
        [Parameter(Mandatory = $true)][string]$Vars,
        [Parameter(Mandatory = $true)][string]$Mapping
    )

    $rawCsv = Join-Path $FixtureDir ("{0}.openplc_raw.csv" -f $CaseName)
    $silNorm = Join-Path $FixtureDir ("{0}.sil.normalized.jsonl" -f $CaseName)
    $openPlcNorm = Join-Path $OutDir ("{0}.openplc.normalized.jsonl" -f $CaseName)
    $report = Join-Path $OutDir ("{0}.trace_compare.report.json" -f $CaseName)

    Write-Host "[OpenPLC-Phase2] Normalize raw CSV: $CaseName"
    & $pythonCmd (Join-Path $repoRoot "scripts/openplc_trace.py") normalize-modbus `
        --raw $rawCsv `
        --mapping $Mapping `
        --tick-ms 10 `
        --out $openPlcNorm
    if ($LASTEXITCODE -ne 0) {
        throw "normalize-modbus failed for $CaseName"
    }

    Write-Host "[OpenPLC-Phase2] Compare SIL vs OpenPLC: $CaseName"
    & (Join-Path $scriptDir "openplc_trace_gate.ps1") `
        -Sil $silNorm `
        -OpenPlc $openPlcNorm `
        -Vars $Vars `
        -TickTolerance 1 `
        -MinPassRate 0.95 `
        -Out $report
    if ($LASTEXITCODE -ne 0) {
        throw "trace compare gate failed for $CaseName"
    }
}

Invoke-Phase2Case -CaseName "two_cylinder" -Vars "_state,valve_a,valve_b" -Mapping (Join-Path $repoRoot "scenarios/openplc_trace_map.two_cylinder.json")
Invoke-Phase2Case -CaseName "assembly_station" -Vars "_state,motor_left,motor_right" -Mapping (Join-Path $repoRoot "scenarios/openplc_trace_map.assembly_station.json")

Write-Host "[OpenPLC-Phase2] OK - core scenario trace gates passed"
