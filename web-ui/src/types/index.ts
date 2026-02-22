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
  triggered_by: string;
  triggered_at: string;
  artifacts: {
    trace?: string;
    diff?: string;
    timing?: string;
    diagnosis?: string;
  };
  failure_summary?: string;
}

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
  tags?: DeviceTags;
  ports?: DevicePortMetadata[];
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
