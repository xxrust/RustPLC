[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$projectRoot = Join-Path $repoRoot 'delivery-projects/station.dual-slot-shuttle-press-cell'
$runRoot = Join-Path $projectRoot 'runs/20260723-172826'
$sourceCommit = '0474cfb54809189bd42055f58b8a565a1b6498a8'

function Read-Json {
    param([string]$Path)
    return Get-Content -Raw -Encoding UTF8 $Path | ConvertFrom-Json
}

function Write-Json {
    param($Value, [string]$Path)
    $json = $Value | ConvertTo-Json -Depth 40
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, (New-Object System.Text.UTF8Encoding($false)))
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

function Repo-Ref {
    param([string]$Path)
    return ([System.IO.Path]::GetFullPath($Path)).Substring($repoRoot.Length + 1).Replace('\', '/')
}

function New-Binding {
    param([string]$Path, [string]$Status, [string]$Basis)
    return [ordered]@{
        artifact_ref = Repo-Ref $Path
        digest = [ordered]@{ algorithm = 'sha256'; value = Get-NormalizedSha256 $Path; normalization = 'utf8_lf' }
        source_commit = $sourceCommit
        freshness = [ordered]@{ status = $Status; basis = $Basis }
    }
}

function New-Fixture {
    param([string]$Path)
    return [ordered]@{
        fixture_ref = ([System.IO.Path]::GetFullPath($Path)).Substring($projectRoot.Length + 1).Replace('\', '/')
        digest = [ordered]@{ algorithm = 'sha256'; value = Get-NormalizedSha256 $Path; normalization = 'utf8_lf' }
    }
}

$sourceFiles = @(
    Get-ChildItem -LiteralPath (Join-Path $projectRoot 'source') -Recurse -File |
        Sort-Object FullName |
        ForEach-Object {
            [ordered]@{
                path = $_.FullName.Substring($projectRoot.Length + 1).Replace('\', '/')
                size_bytes = $_.Length
                sha256 = Get-NormalizedSha256 $_.FullName
            }
        }
)
$inputManifestPath = Join-Path $runRoot 'input-manifest.json'
$changedPaths = @(& git -C $repoRoot status --porcelain=v1 | ForEach-Object { if ($_.Length -gt 3) { $_.Substring(3).Replace('\', '/') } })
$inputManifest = [ordered]@{
    schema_version = 1
    project_id = 'station.dual_slot_shuttle_press_cell'
    run_id = '20260723-172826'
    source_commit = $sourceCommit
    dirty_worktree = $true
    changed_paths = $changedPaths
    runner_version = 'delivery-corpus-v1'
    compiler_version = '0.1.0'
    file_count = $sourceFiles.Count
    implementation_file_count = $sourceFiles.Count
    missing_required = @()
    path_uniqueness_required = $true
    digest_policy = 'sha256 over UTF-8 text with CRLF and CR normalized to LF'
    files = $sourceFiles
}
Write-Json $inputManifest $inputManifestPath

$legacyResultPath = Join-Path $runRoot 'legacy-authoritative-result.json'
$resultPath = Join-Path $runRoot 'result.json'
$result = Read-Json $resultPath
$result | Add-Member -NotePropertyName project_id -NotePropertyValue 'station.dual_slot_shuttle_press_cell' -Force
$result | Add-Member -NotePropertyName harness_status -NotePropertyValue 'pass' -Force
$result | Add-Member -NotePropertyName delivery_status -NotePropertyValue 'blocked' -Force
$result | Add-Member -NotePropertyName freshness -NotePropertyValue 'same_run' -Force
$result | Add-Member -NotePropertyName error_code -NotePropertyValue $null -Force
$result.imported_from = New-Binding $legacyResultPath 'current_for_import' 'Committed copy of the original authoritative harness result; historical out paths are retained only inside this raw record.'
$result.artifact_root = Repo-Ref $runRoot
$result.inputs.manifest = Repo-Ref $inputManifestPath
$result.inputs.file_count = $sourceFiles.Count
$result.digests.input_manifest_sha256 = Get-NormalizedSha256 $inputManifestPath
Write-Json $result $resultPath

$provenancePath = Join-Path $runRoot 'provenance.json'
$provenance = Read-Json $provenancePath
$provenance.artifact_ref = Repo-Ref $legacyResultPath
$provenance.digest.value = Get-NormalizedSha256 $legacyResultPath
$provenance.freshness.status = 'current_for_import'
$provenance.freshness.basis = 'The original result is committed inside the delivery fixture; unattended execution remains not_proven.'
$provenance | Add-Member -NotePropertyName event_stream -NotePropertyValue (Repo-Ref (Join-Path $runRoot 'agent-events.json')) -Force
Write-Json $provenance $provenancePath

$anomaliesPath = Join-Path $runRoot 'anomalies.json'
$anomalies = Read-Json $anomaliesPath
$anomalies.evidence_sources.agent_b_execution = New-Binding (Join-Path $runRoot 'review/agent-b-execution.md') 'reviewed_post_run' 'Committed original execution log with seven compile-guided attempts.'
$anomalies.evidence_sources.corrections_log = New-Binding (Join-Path $runRoot 'review/corrections.md') 'reviewed_post_run' 'Committed correction ledger.'
$anomalies.evidence_sources.authoritative_result = New-Binding $resultPath 'same_run' 'Normalized project-local result preserving blocked delivery status.'
foreach ($record in $anomalies.records) {
    $record | Add-Member -NotePropertyName event_refs -NotePropertyValue @('EVT-009') -Force
    $record | Add-Member -NotePropertyName evidence_paths -NotePropertyValue @((Repo-Ref $anomaliesPath)) -Force
}
$anomalies.records[0].event_refs = @('EVT-002', 'EVT-003', 'EVT-004', 'EVT-005', 'EVT-006', 'EVT-007', 'EVT-008')
$anomalies.records[0].evidence_paths = @((Repo-Ref (Join-Path $runRoot 'review/agent-b-execution.md')))
Write-Json $anomalies $anomaliesPath

$correctionsPath = Join-Path $runRoot 'corrections.json'
$corrections = Read-Json $correctionsPath
$corrections.artifact_ref = Repo-Ref (Join-Path $runRoot 'review/corrections.md')
$corrections.digest.value = Get-NormalizedSha256 (Join-Path $runRoot 'review/corrections.md')
$corrections | Add-Member -NotePropertyName source_commit -NotePropertyValue $sourceCommit -Force
Write-Json $corrections $correctionsPath

$compilerStagesPath = Join-Path $runRoot 'compiler-stages.json'
$compilerStages = Read-Json $compilerStagesPath
$compileReportPath = Join-Path $runRoot 'compile/verification_report.json'
$irPath = Join-Path $runRoot 'compile/ir_bundle.json'
$projectCheckPath = Join-Path $runRoot 'project-check/project_check_report.json'
foreach ($stage in $compilerStages.stages) {
    $target = $resultPath
    if (@('Parser', 'Safety', 'Liveness', 'Causality') -contains [string]$stage.stage) { $target = $compileReportPath }
    if (@('AST', 'Semantic', 'IR') -contains [string]$stage.stage) { $target = $irPath }
    if ([string]$stage.stage -eq 'Process Model Check') { $target = $projectCheckPath }
    $stage.evidence = New-Binding $target 'same_run' 'Committed project-local evidence for this compiler stage.'
}
Write-Json $compilerStages $compilerStagesPath

$wiringPath = Join-Path $projectRoot 'wiring/point-checks.json'
$wiring = Read-Json $wiringPath
$wiring.evidence_sources.controller = New-Binding (Join-Path $projectRoot 'source/00_topology/controller.plc') 'input_snapshot' 'Project-local controller source.'
$wiring.evidence_sources.devices = New-Binding (Join-Path $projectRoot 'source/00_topology/devices.plc') 'input_snapshot' 'Project-local device source.'
$wiring.evidence_sources.connections = New-Binding (Join-Path $projectRoot 'source/00_topology/connections.plc') 'input_snapshot' 'Project-local topology relations.'
Write-Json $wiring $wiringPath

$holdsPath = Join-Path $projectRoot 'release/human-holds.json'
$holds = Read-Json $holdsPath
$holds.artifact_ref = Repo-Ref $resultPath
$holds.digest.value = Get-NormalizedSha256 $resultPath
$holds.freshness.basis = 'Release holds are derived from the committed blocked result; no signatures are synthesized.'
Write-Json $holds $holdsPath

$manifestPath = Join-Path $projectRoot 'delivery-project.json'
$manifest = Read-Json $manifestPath
$manifest.artifact_roots.execution = 'runs/20260723-172826'
$manifest.evidence_bindings.source_entry = New-Binding (Join-Path $projectRoot 'source/rustplc.bundle.toml') 'input_snapshot' 'Project-local bundle in the committed source snapshot.'
$manifest.evidence_bindings.system_contract = New-Binding (Join-Path $projectRoot 'source/plc/main.system.md') 'input_snapshot' 'Project-local system contract.'
$manifest.evidence_bindings.authoritative_result = New-Binding $resultPath 'same_run' 'Normalized committed result for the final corrected harness run.'
$manifest.evidence_bindings.reviewed_completeness = New-Binding (Join-Path $projectRoot 'review/completeness.md') 'reviewed_post_run' 'Committed bounded completeness review.'
$manifest.fixtures.api_run_result = New-Fixture $resultPath
$manifest.fixtures.provenance = New-Fixture $provenancePath
$manifest.fixtures.input_manifest = New-Fixture $inputManifestPath
$manifest.fixtures.anomalies = New-Fixture $anomaliesPath
$manifest.fixtures.corrections = New-Fixture $correctionsPath
$manifest.fixtures.compiler_stages = New-Fixture $compilerStagesPath
$manifest.fixtures | Add-Member -NotePropertyName agent_events -NotePropertyValue (New-Fixture (Join-Path $runRoot 'agent-events.json')) -Force
$manifest.fixtures.wiring_point_checks = New-Fixture $wiringPath
$manifest.fixtures.human_holds = New-Fixture $holdsPath
Write-Json $manifest $manifestPath

[ordered]@{
    schema_version = 1
    command = 'normalize_legacy_station_delivery_fixture'
    status = 'pass'
    project_id = [string]$manifest.project_id
    source_file_count = $sourceFiles.Count
    manifest = Repo-Ref $manifestPath
} | ConvertTo-Json -Depth 8
