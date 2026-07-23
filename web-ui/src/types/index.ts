// 基于后端 JSON 工件的类型定义

export type RunMode = 'no_board' | 'hil_board' | 'runtime_live';

export type UserRole =
  | 'operator'
  | 'engineer'
  | 'electrical_engineer'
  | 'commissioning_engineer'
  | 'safety_reviewer'
  | 'release_approver'
  | 'auditor'
  | 'admin';

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
  acknowledged?: boolean;
}

export interface VerificationSummary {
  safety: { status: 'pass' | 'fail'; details: string };
  liveness: { status: 'pass' | 'fail'; details: string };
  timing: { status: 'pass' | 'fail'; details: string };
  causality: { status: 'pass' | 'fail'; details: string };
}

export type PlcDiagnosticSeverity = 'error' | 'warning' | 'info';

export interface PlcDiagnosticIssue {
  severity: PlcDiagnosticSeverity;
  stage: string;
  message: string;
  line: number;
  column: number;
  code?: string;
  suggestion?: string;
}

export interface PlcDiagnosticsSummary {
  topology_devices: number;
  tasks: number;
  states: number;
  transitions: number;
  constraints: number;
  verification_warnings: number;
}

export interface PlcDiagnosticsResponse {
  valid: boolean;
  stage: string;
  errors: string[];
  issues: PlcDiagnosticIssue[];
  summary: PlcDiagnosticsSummary;
}

export type PlcSymbolKind = 'device' | 'task' | 'step' | 'variable' | 'resource' | 'workpiece';

export interface PlcLanguageSymbol {
  name: string;
  qualified_name: string;
  kind: PlcSymbolKind;
  line: number;
  detail: string;
  documentation: string;
}

export interface PlcLanguageCompletion {
  label: string;
  kind: string;
  detail?: string;
  documentation?: string;
  insert_text?: string;
  snippet: boolean;
}

export interface PlcLanguageSnapshot {
  symbols: PlcLanguageSymbol[];
  completions: PlcLanguageCompletion[];
}

export interface PlcRealtimeAnalysisResponse {
  request_id?: number;
  diagnostics: PlcDiagnosticsResponse;
  language: PlcLanguageSnapshot;
}

export interface DslCapabilityEntry {
  id: string;
  status: string;
  layer: string;
  summary: string;
  evidence: string[];
}

export interface DslTemplateAsset {
  id: string;
  status: string;
  summary: string;
  paths: string[];
}

export interface DslUnsupportedFeature {
  id: string;
  status: string;
  reason: string;
  required_contract: string;
}

export interface DslCapabilitiesReport {
  schema_version: number;
  command: string;
  output: string;
  parser_contract: string;
  supported_features: DslCapabilityEntry[];
  template_assets: DslTemplateAsset[];
  unsupported_features: DslUnsupportedFeature[];
}

export interface CollabEvent {
  room: string;
  kind: 'hello' | 'edit' | 'cursor' | 'comment' | 'error' | string;
  client_id: string;
  user_name?: string;
  content?: string;
  revision?: number;
  cursor_line?: number;
  cursor_column?: number;
  comment?: string;
  at_ms: number;
}

export interface RunStatus {
  run_id: string;
  status: 'running' | 'pass' | 'fail';
  mode?: 'no_board_gate' | 'component_sim' | string;
  triggered_by: string;
  triggered_at: string;
  triggered_at_ms?: number;
  plc_file?: string;
  scenario_file?: string;
  topology_file?: string;
  tick_ms?: number;
  artifacts: {
    trace?: string;
    diff?: string;
    timing?: string;
    diagnosis?: string;
    geometry?: string;
    keypoints?: string;
    fault_audit?: string;
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
  attributes?: Record<string, string>;
}

export interface GeometryEdge {
  id: string;
  kind: GeometryEdgeKind;
  from: string;
  to: string;
  label: string;
  views: GeometryViewKind[];
  evidence_status: GeometryEvidenceStatus;
  attributes?: Record<string, string>;
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

export interface GeometryNarrativeDeviceRef {
  device_id: string;
  label: string;
  kind: string;
}

export interface GeometryNarrativeDeviceChain {
  source_kind: string;
  explanation: string;
  command_devices: GeometryNarrativeDeviceRef[];
  actuator_devices: GeometryNarrativeDeviceRef[];
  feedback_devices: GeometryNarrativeDeviceRef[];
  io_devices: GeometryNarrativeDeviceRef[];
  evidence_chain_ids: string[];
}

export interface GeometryNarrativeAction {
  kind: string;
  label: string;
  target_device_id?: string;
  target_port?: string;
}

export interface GeometryNarrativeTransition {
  transition_id: string;
  to_step_id: string;
  to_step_label: string;
  guard_kind: string;
  guard_label: string;
  timers: string[];
  actions: GeometryNarrativeAction[];
  effects: string[];
  observed: boolean;
}

export interface GeometryNarrativeTransitionRef {
  transition_id: string;
  guard_kind: string;
  guard_label: string;
  to_step_id: string;
  to_step_label: string;
}

export interface GeometryNarrativeBlockingPoint {
  step_id: string;
  step_label: string;
  release_transitions: GeometryNarrativeTransitionRef[];
  timeout_transitions: GeometryNarrativeTransitionRef[];
}

export interface GeometryNarrativeExit {
  from_step_id: string;
  from_step_label: string;
  via: GeometryNarrativeTransitionRef;
}

export interface GeometryNarrativeCoverage {
  uncovered_step_count: number;
  trace_available: boolean;
  intent_available: boolean;
}

export interface GeometryNarrativeStep {
  step_id: string;
  label: string;
  index: number;
  is_initial: boolean;
  is_current: boolean;
  incoming_transition_ids: string[];
  outgoing: GeometryNarrativeTransition[];
  device_chains: GeometryNarrativeDeviceChain[];
  evidence_chain_ids: string[];
  evidence_reasons: string[];
}

export interface GeometryNarrativeTask {
  task_id: string;
  label: string;
  entry_step_id: string;
  current_step_id: string;
  blocking_state: string;
  pending_actions: string[];
  main_path_step_ids: string[];
  blocking_points: GeometryNarrativeBlockingPoint[];
  fault_exits: GeometryNarrativeExit[];
  coverage: GeometryNarrativeCoverage;
  steps: GeometryNarrativeStep[];
}

export interface GeometryNarrative {
  tasks: GeometryNarrativeTask[];
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
  narrative?: GeometryNarrative;
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
  component_states?: Record<string, unknown>;
}

export interface TraceData {
  schema_version: number;
  tick_ms: number;
  ticks: TickSnapshot[];
}

export interface TraceKeypoint {
  tick: number;
  at_ms: number;
  category: string;
  source: string;
  label: string;
}

export interface TraceKeypointArtifact {
  schema_version: number;
  tick_ms: number;
  keypoints: TraceKeypoint[];
}

export interface FlowchartSystemContractSummary {
  path: string;
  excerpt: string;
  byte_count: number;
}

export interface FlowchartStepSummary {
  task_name: string;
  step_name: string;
  source: string;
  line: number;
  generated: boolean;
  statements: string[];
}

export interface FlowchartEdgeSummary {
  from_task: string;
  from_step: string;
  to_task: string;
  to_step: string;
  label: string;
  guard: string;
  actions: string[];
  effects: string[];
}

export interface FlowchartTaskDiagram {
  task_name: string;
  steps: FlowchartStepSummary[];
  transitions: FlowchartEdgeSummary[];
}

export interface FlowchartTopologySummary {
  device_count: number;
  link_count: number;
  station_count: number;
  handshake_count: number;
  transfer_point_count: number;
  devices?: string[];
  variables: string[];
  workpiece_sites: string[];
  workpiece_holders: string[];
  workpiece_types: string[];
  stations: string[];
  handshakes: string[];
  transfer_points: string[];
  links: string[];
}

export interface FlowchartArtifact {
  schema_version: number;
  source_plc: string;
  title: string;
  system_contract?: FlowchartSystemContractSummary | null;
  tasks: FlowchartTaskDiagram[];
  topology: FlowchartTopologySummary;
}

export interface FlowchartEditorStep {
  id: string;
  label?: string;
  action?: string;
  delay_ms?: number;
}

export interface FlowchartEditorTransition {
  from: string;
  to: string;
  guard?: string;
}

export interface FlowchartGeneratePlcRequest {
  project_id?: string | null;
  task_name: string;
  steps: FlowchartEditorStep[];
  transitions: FlowchartEditorTransition[];
}

export interface FlowchartGeneratePlcResponse {
  source: string;
  valid: boolean;
  diagnostics: PlcDiagnosticsResponse;
  normalized_task_name: string;
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
