// 基于后端 JSON 工件的类型定义

export type RunMode = 'no_board' | 'hil_board' | 'runtime_live';

export type UserRole = 'operator' | 'engineer' | 'auditor' | 'admin';

export type AlarmSeverity = 'info' | 'warning' | 'critical';

export type EvidenceSource = 'no_board' | 'hil_board' | 'runtime_live' | 'mixed';

export type DiagnosisCategory =
  | 'expected_input_never_changed'
  | 'actuator_command_missing'
  | 'interlock_or_requires_blocked'
  | 'mapping_or_alias_mismatch'
  | 'timeout_budget_too_short';

export interface DiagnosisCandidate {
  issue_code: string;
  category: DiagnosisCategory;
  rank: number;
  confidence: number;
  evidence: string[];
  suggested_fix: string;
  evidence_source: EvidenceSource;
}

export interface DiagnosisReport {
  schema_version: number;
  candidates: DiagnosisCandidate[];
  anchor?: {
    kind: 'timeout' | 'first_trace_mismatch';
    tick?: number;
    trace_index?: number;
    detail: string;
  };
}

export interface AlarmEvent {
  alarm_id: string;
  severity: AlarmSeverity;
  first_seen_ms: number;
  top_candidates: DiagnosisCandidate[];
  evidence_ref: string;
  evidence_source: EvidenceSource;
  scenario_or_recipe_id: string;
}

export interface VerificationSummary {
  safety: { status: 'pass' | 'fail'; details: string };
  liveness: { status: 'pass' | 'fail'; details: string };
  timing: { status: 'pass' | 'fail'; details: string };
  causality: { status: 'pass' | 'fail'; details: string };
}

export interface RunStatus {
  run_id: string;
  status: 'running' | 'pass' | 'fail';
  mode?: 'no_board_gate' | 'component_sim' | string;
  triggered_by: string;
  triggered_at: string;
  artifacts: {
    trace?: string;
    diff?: string;
    timing?: string;
    diagnosis?: string;
    geometry?: string;
  };
  failure_summary?: string;
}

export type GeometryViewKind = 'constellation' | 'orbit' | 'evidence';

export type GeometryLaneKind = 'topology' | 'task' | 'evidence';

export type GeometryNodeKind =
  | 'device'
  | 'task'
  | 'step'
  | 'semantic_resource'
  | 'claim_source'
  | 'timing_rule'
  | 'causality_chain'
  | 'workpiece_site'
  | 'workpiece_holder'
  | 'workpiece_carrier'
  | 'external_reference';

export type GeometryEdgeKind =
  | 'contains'
  | 'topology_link'
  | 'transition'
  | 'resource_claim'
  | 'timing_scope'
  | 'causality';

export type GeometryEvidenceStatus =
  | 'authored'
  | 'derived'
  | 'verified'
  | 'observed'
  | 'warning'
  | 'blocked';

export interface GeometrySummary {
  task_count: number;
  step_count: number;
  transition_count: number;
  device_count: number;
  resource_count: number;
  timing_rule_count: number;
  causality_chain_count: number;
  observed_transition_count: number;
  intent_mismatch_count: number;
}

export interface GeometryLane {
  id: string;
  kind: GeometryLaneKind;
  label: string;
  position: number;
}

export interface GeometryNode {
  id: string;
  kind: GeometryNodeKind;
  label: string;
  lane_id: string;
  views: GeometryViewKind[];
  evidence_status: GeometryEvidenceStatus;
  attributes: Record<string, string>;
}

export interface GeometryEdge {
  id: string;
  kind: GeometryEdgeKind;
  from: string;
  to: string;
  label: string;
  views: GeometryViewKind[];
  evidence_status: GeometryEvidenceStatus;
  attributes: Record<string, string>;
}

export interface GeometryObservedTransition {
  tick: number;
  task_index: number;
  from_step: number;
  to_step: number;
  reason: string;
  task_name?: string;
  from_state?: string;
  to_state?: string;
}

export interface GeometryTraceOverlay {
  observed_transition_count: number;
  resolution: string;
  transitions: GeometryObservedTransition[];
}

export interface GeometryIntentOverlay {
  verdict: 'pass' | 'warn' | 'fail' | string;
  primary_mismatch_kind?: string;
  blocker_kind?: string;
  mismatch_count: number;
  warnings: string[];
  mismatches: Array<Record<string, unknown>>;
}

export interface GeometryArtifact {
  schema_version: number;
  artifact_kind: 'semantic_twin_geometry' | string;
  source_path: string;
  summary: GeometrySummary;
  lanes: GeometryLane[];
  nodes: GeometryNode[];
  edges: GeometryEdge[];
  overlays: {
    trace?: GeometryTraceOverlay;
    intent?: GeometryIntentOverlay;
  };
}

export interface GeometryArtifactMissing {
  schema_version: number;
  artifact_kind: 'semantic_twin_geometry' | string;
  status: 'missing';
}

export type GeometryArtifactResponse = GeometryArtifact | GeometryArtifactMissing;

export interface DeviceTags {
  functional_group: string[];
  danger_level: string[];
  location_group: string[];
}

export const TOPOLOGY_TAGS_SCHEMA_VERSION = 1;

export type TagDimension = keyof DeviceTags;

export type PortSignalType =
  | 'digital'
  | 'analog'
  | 'pneumatic'
  | 'logical'
  | 'generic';

export type DevicePortRole = 'producer' | 'consumer' | 'bidirectional';

export interface DevicePortMetadata {
  id: string;
  type: PortSignalType;
  role: DevicePortRole;
}

export interface TopologyComponentParams extends Record<string, unknown> {
  purpose?: string;
  tags?: DeviceTags;
  ports?: DevicePortMetadata[];
  endpoint_kind?: 'controller_port' | 'controller_device' | 'process_device';
}

export interface ComponentTopology {
  schema_version: number;
  tags_schema_version?: number;
  component_library: {
    schema_version: number;
    components: Array<{
      id: string;
      name: string;
      type: string;
      params: TopologyComponentParams;
    }>;
  };
  components: Array<{
    id: string;
    component_id: string;
    params: TopologyComponentParams;
  }>;
  connections: Array<{
    from: string;
    to: string;
    relation?: string;
    signal?: string;
    from_port?: string;
    to_port?: string;
  }>;
}

export interface ComponentScenario {
  schema_version: number;
  tick_ms: number;
  duration_ms: number;
  switch_events: Array<{
    at_ms: number;
    target: string;
    value: boolean;
  }>;
  sensor_events: Array<{
    at_ms: number;
    target: string;
    value: boolean;
  }>;
  component_faults: Array<{
    at_ms: number;
    target: string;
    fault_kind: string;
    duration_ms?: number;
  }>;
}

export interface TickSnapshot {
  tick: number;
  digital_inputs: boolean[];
  analog_inputs: number[];
  digital_outputs: boolean[];
  analog_outputs: number[];
  component_states?: Record<string, any>;
}

export interface TraceData {
  schema_version: number;
  tick_ms: number;
  ticks: TickSnapshot[];
}

export interface TimingReport {
  schema_version: number;
  tick_ms: number;
  total_ticks: number;
  statistics: {
    p50_exec_us: number;
    p95_exec_us: number;
    p99_exec_us: number;
    max_exec_us: number;
    overrun_count: number;
  };
}

export interface GateResult {
  status: 'pass' | 'fail';
  trace_match: boolean;
  timing_pass: boolean;
  diagnosis_report?: DiagnosisReport;
  artifacts: {
    sil_trace: string;
    board_trace: string;
    diff_report: string;
    timing_report: string;
  };
}
