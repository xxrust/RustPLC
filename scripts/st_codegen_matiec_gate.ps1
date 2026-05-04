param(
    [string]$OutDir
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir "..")) | Select-Object -ExpandProperty Path

if ([string]::IsNullOrWhiteSpace($OutDir)) {
    $OutDir = Join-Path $repoRoot "out/st_codegen_matiec"
}

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

if (-not (Get-Command iec2c -ErrorAction SilentlyContinue)) {
    Write-Error "[ST-MATIEC] iec2c not found in PATH. Install MATIEC first."
    exit 1
}

function Invoke-GenerateAndCompile {
    param(
        [Parameter(Mandatory = $true)][string]$PlcPath,
        [Parameter(Mandatory = $true)][string]$Stem
    )

    $stFile = Join-Path $OutDir ("{0}.st" -f $Stem)

    Write-Host "[ST-MATIEC] Generate ST: $PlcPath -> $stFile"
    Push-Location $repoRoot
    try {
        & cargo run --release --bin rust_plc -- gen-st $PlcPath --out $stFile --program-name Main
        if ($LASTEXITCODE -ne 0) {
            throw "cargo gen-st failed for $PlcPath"
        }
    }
    finally {
        Pop-Location
    }

    Write-Host "[ST-MATIEC] Compile ST with iec2c: $Stem.st"
    Push-Location $OutDir
    try {
        & iec2c ("{0}.st" -f $Stem)
        if ($LASTEXITCODE -ne 0) {
            throw "iec2c compile failed for $Stem.st"
        }
    }
    finally {
        Pop-Location
    }
}

Invoke-GenerateAndCompile -PlcPath "examples/project_scaffold_demo/plc/main.plc" -Stem "project_scaffold_demo"
Invoke-GenerateAndCompile -PlcPath "examples/dual_axis_platform.plc" -Stem "dual_axis_platform"

Write-Host "[ST-MATIEC] OK - generated ST files compile with MATIEC"
