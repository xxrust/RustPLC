export type EvidenceState =
  | 'authored'
  | 'derived'
  | 'verified'
  | 'observed'
  | 'warning'
  | 'blocked'
  | 'missing'
  | 'stale';

export type ResponsibilityState =
  | 'agent_running'
  | 'agent_complete'
  | 'human_action_required'
  | 'human_confirmed'
  | 'release_approved';

export type DeliveryLayer = 'module' | 'station' | 'line';

export interface DeliveryProjectSummary {
  project_id: string;
  name?: string;
  delivery_layer: DeliveryLayer;
  source_commit: string;
  source_entry?: string;
  status?: EvidenceState;
  responsibility_state?: ResponsibilityState;
  updated_at?: string;
  stale?: boolean;
  blocker_count?: number;
}

export interface DeliveryProjectDetail extends DeliveryProjectSummary {
  system_contract?: string;
  architecture?: string;
  artifact_roots?: Record<string, string>;
  required_artifacts?: Array<{
    id: string;
    label: string;
    path?: string;
    status: EvidenceState;
  }>;
  human_holds?: HumanHold[];
  release_verdict?: string;
}

export interface HumanHold {
  hold_id: string;
  label: string;
  role?: string;
  status: 'pending' | 'stale' | 'blocked' | 'confirmed' | 'rejected';
  signed_by?: string;
  signed_at?: string;
  reason?: string;
  blocker_ids?: string[];
}

export interface AuthenticatedUser {
  id: string;
  name: string;
  role:
    | 'engineer'
    | 'electrical_engineer'
    | 'commissioning_engineer'
    | 'safety_reviewer'
    | 'release_approver'
    | 'admin';
}

export interface HoldSignature {
  schema_version: number;
  signature_id: string;
  project_id: string;
  hold_id: string;
  hold_type: string;
  user: AuthenticatedUser;
  source_commit: string;
  evidence_digests: Record<string, string>;
  decision: 'approve' | 'reject';
  comment?: string;
  signed_at: string;
  signed_at_ms: number;
}

export interface HoldSignatureView extends HoldSignature {
  stale: boolean;
}

export interface HoldSignatureContext {
  schema_version: number;
  project_id: string;
  source_commit: string;
  current_evidence_digests: Record<string, string>;
  signatures: HoldSignatureView[];
}

export interface SignHoldRequest {
  hold_type: string;
  source_commit: string;
  evidence_digests: Record<string, string>;
  decision: 'approve' | 'reject';
  comment?: string;
}

export interface AgentRunEvent {
  event_id?: string;
  timestamp?: string;
  task?: string;
  agent?: string;
  tool?: string;
  duration_ms?: number;
  result?: string;
  status?: EvidenceState | 'running' | 'complete' | 'failed';
  artifact_ref?: string;
}

export interface AgentAnomaly {
  anomaly_id?: string;
  code?: string;
  summary: string;
  root_cause?: string;
  correction?: string;
  affected_files?: string[];
  status?: EvidenceState;
  retry_count?: number;
  long_search_or_trial_and_error?: boolean;
}

export interface AgentRun {
  run_id: string;
  status?: 'running' | 'complete' | 'failed' | 'blocked';
  started_at?: string;
  completed_at?: string;
  source_commit?: string;
  model?: string;
  unattended_verdict?: string;
  input_manifest_digest?: string;
  events?: AgentRunEvent[];
  anomalies?: AgentAnomaly[];
  corrections?: AgentAnomaly[];
}

export interface WiringPoint {
  point_id: string;
  controller?: string;
  channel?: string;
  alias?: string;
  direction?: string;
  device_terminal?: string;
  signal_type?: string;
  safe_state?: string;
  wire_id?: string;
  evidence_source?: string;
  compiler_status?: EvidenceState;
  point_check_status?: EvidenceState | 'pending';
  note?: string;
}

export type PointObservationStatus = 'pass' | 'fail' | 'blocked';

export interface PointMeasurement {
  value: string;
  unit?: string;
  instrument_id?: string;
}

export interface RecordPointObservationRequest {
  status: PointObservationStatus;
  measurement?: PointMeasurement;
  photo_upload_id?: string;
  trace_ref?: string;
  note?: string;
}

export interface PointObservation {
  schema_version: number;
  observation_id: string;
  project_id: string;
  point_id: string;
  status: PointObservationStatus;
  measurement?: PointMeasurement;
  photo_upload_id?: string;
  trace_ref?: string;
  trace_sha256?: string;
  note?: string;
  user: AuthenticatedUser;
  source_commit: string;
  observed_at: string;
  observed_at_ms: number;
  prior_evidence_digest_set_sha256: string;
  deep_link?: Record<string, unknown>;
}

export interface EvidenceUpload {
  schema_version: number;
  upload_id: string;
  project_id: string;
  original_filename: string;
  artifact_ref: string;
  media_type: string;
  evidence_kind: 'photo' | 'trace' | 'measurement' | 'document' | 'other';
  semantic_object_kind?: string;
  semantic_object_id?: string;
  note?: string;
  size_bytes: number;
  sha256: string;
  user: AuthenticatedUser;
  source_commit: string;
  uploaded_at: string;
  uploaded_at_ms: number;
  deep_link?: Record<string, unknown>;
}

export interface PointCheckProjectionPoint {
  point_id: string;
  authored: WiringPoint;
  status: EvidenceState | 'pending';
  evidence_state: EvidenceState;
  responsibility_state: ResponsibilityState;
  latest_observation?: PointObservation;
  deep_link?: Record<string, unknown>;
}

export interface PointCheckProjection {
  summary: {
    declared_points: number;
    observed_points: number;
    blocked_points: number;
    remaining_points: number;
  };
  points: PointCheckProjectionPoint[];
}

export interface PhysicalEvidenceProjection {
  schema_version: number;
  project_id: string;
  point_checks: PointCheckProjection;
  observations: PointObservation[];
  uploads: EvidenceUpload[];
  provenance?: {
    observation_log?: string;
    upload_log?: string;
  };
}

export type HoldProjectionStatus = 'blocked' | 'human_action_required' | 'human_confirmed' | 'rejected' | 'stale';

export interface HoldProjectionItem {
  hold_id: string;
  required_role?: string;
  contract_status?: string;
  status: HoldProjectionStatus;
  reason?: string;
  blocker_ids?: string[];
  signature?: HoldSignatureView;
  stale_signature_present?: boolean;
  point_check_summary?: PointCheckProjection['summary'];
  deep_link?: Record<string, unknown>;
}

export interface HoldProjection {
  schema_version: number;
  project_id: string;
  source_commit: string;
  holds: HoldProjectionItem[];
  current_evidence_digest_set_sha256: string;
  provenance?: {
    manifest?: string;
    hold_contract?: string;
    signature_store?: string;
  };
}

export interface ReleaseProjection {
  schema_version: number;
  project_id: string;
  status: 'blocked' | 'human_action_required' | 'release_approved';
  delivery_status: string;
  delivery_status_gate: {
    status: 'current' | 'blocked';
    allowed_statuses: string[];
    error_code?: string;
  };
  holds: HoldProjectionItem[];
  prerequisites: HoldProjectionItem[];
  blocked_prerequisites: HoldProjectionItem[];
  release_signature?: HoldSignatureView;
  provenance?: HoldProjection['provenance'];
  deep_link?: Record<string, unknown>;
}

export interface VerificationStage {
  stage: string;
  status: EvidenceState;
  producer?: string;
  source_commit?: string;
  artifact_ref?: string;
  diagnostic_code?: string;
  message?: string;
  updated_at?: string;
}

export interface EvidenceRecord {
  evidence_id: string;
  label: string;
  evidence_state: EvidenceState;
  responsibility_state?: ResponsibilityState;
  producer?: string;
  timestamp?: string;
  source_commit?: string;
  artifact_ref?: string;
  digest?: string;
  stale?: boolean;
  blocker_reason?: string;
}

export interface WorkspaceProblem {
  id: string;
  project_id?: string;
  severity: 'error' | 'warning' | 'info' | 'blocked';
  stage?: string;
  code?: string;
  message: string;
  source_ref?: string;
  line?: number;
  column?: number;
}

export interface WorkspaceTest {
  id: string;
  project_id?: string;
  suite?: string;
  name: string;
  status: 'pass' | 'fail' | 'blocked' | 'skipped' | 'running';
  duration_ms?: number;
  artifact_ref?: string;
}

export type WorkbenchView =
  | 'overview'
  | 'agent-run'
  | 'wiring'
  | 'verification'
  | 'source'
  | 'topology'
  | 'run'
  | 'replay'
  | 'audit';

export interface WorkbenchTab {
  id: string;
  label: string;
  view: WorkbenchView;
  resource_id?: string;
  pinned?: boolean;
  group?: 'primary' | 'secondary';
}
