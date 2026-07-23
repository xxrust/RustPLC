import axios from 'axios';
import type {
  ComponentTopology,
  ComponentScenario,
  RunStatus,
  DiagnosisReport,
  TraceData,
  TraceKeypointArtifact,
  TimingReport,
  GeometryArtifactResponse,
  PlcDiagnosticsResponse,
  DslCapabilitiesReport,
  FlowchartGeneratePlcRequest,
  FlowchartGeneratePlcResponse,
  PlcLanguageSnapshot,
  AlarmEvent,
} from '../types';
import type {
  AgentRun,
  DeliveryProjectDetail,
  DeliveryProjectSummary,
  AuthenticatedUser,
  EvidenceRecord,
  EvidenceUpload,
  HoldProjection,
  HoldSignature,
  HoldSignatureContext,
  PhysicalEvidenceProjection,
  RecordPointObservationRequest,
  ReleaseProjection,
  SignHoldRequest,
  PointObservation,
  VerificationStage,
  WiringPoint,
  WiringProjection,
  WorkspaceProblem,
  WorkspaceProblemsProjection,
  WorkspaceTest,
  WorkspaceTestSource,
  WorkspaceTestsProjection,
} from '../types/workbench';

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || '/api';
const WS_BASE_URL = import.meta.env.VITE_WS_BASE_URL || deriveWebSocketBaseUrl(API_BASE_URL);

function deriveWebSocketBaseUrl(apiBaseUrl: string): string {
  if (/^https?:\/\//.test(apiBaseUrl)) {
    return apiBaseUrl.replace(/^http/, 'ws').replace(/\/api\/?$/, '');
  }
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}`;
}

export function buildWebSocketUrl(path: string): string {
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  return `${WS_BASE_URL}${normalizedPath}`;
}

const apiClient = axios.create({
  baseURL: API_BASE_URL,
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json',
  },
});

// Project API
export const projectApi = {
  listProjects: () => apiClient.get('/projects'),

  getProjectSource: (id: string) =>
    apiClient.get<{ id: string; path: string; content: string }>(`/projects/${id}/source`),
};

function listFromPayload<T>(payload: unknown, keys: string[]): T[] {
  if (Array.isArray(payload)) return payload as T[];
  if (!payload || typeof payload !== 'object') return [];
  const record = payload as Record<string, unknown>;
  for (const key of keys) {
    if (Array.isArray(record[key])) return record[key] as T[];
  }
  return [];
}

function recordOf(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' ? value as Record<string, unknown> : {};
}

function readableRecord(value: unknown, preferredKeys: string[]): string | undefined {
  if (typeof value === 'string') return value || undefined;
  const record = recordOf(value);
  const parts = preferredKeys.flatMap((key) => {
    const field = record[key];
    if (Array.isArray(field)) return field.map(String).filter(Boolean);
    return field === undefined || field === null || field === '' ? [] : [String(field)];
  });
  return parts.length > 0 ? parts.join(' / ') : undefined;
}

function stringList(value: unknown): string[] | undefined {
  return Array.isArray(value) ? value.map(String).filter(Boolean) : undefined;
}

function workspaceProblemSeverity(value: unknown): WorkspaceProblem['severity'] {
  const severity = String(value ?? 'info').toLowerCase();
  if (severity === 'warn') return 'warning';
  if (severity === 'error' || severity === 'warning' || severity === 'blocked') return severity;
  return 'info';
}

function normalizeTestSource(value: unknown): WorkspaceTestSource {
  const source = recordOf(value);
  const freshness = recordOf(source.freshness);
  const runs = Array.isArray(freshness.runs) ? freshness.runs.map((runValue) => {
    const run = recordOf(runValue);
    const runFreshness = recordOf(run.freshness);
    return {
      run_id: String(run.run_id ?? '') || undefined,
      freshness: {
        state: String(runFreshness.state ?? '') || undefined,
        bindings: Array.isArray(runFreshness.bindings)
          ? runFreshness.bindings.map((bindingValue) => {
              const binding = recordOf(bindingValue);
              return {
                name: String(binding.name ?? '') || undefined,
                state: String(binding.state ?? '') || undefined,
                artifact: binding.artifact === null ? null : String(binding.artifact ?? '') || undefined,
                expected_sha256: binding.expected_sha256 === null ? null : String(binding.expected_sha256 ?? '') || undefined,
                actual_sha256: binding.actual_sha256 === null ? null : String(binding.actual_sha256 ?? '') || undefined,
              };
            })
          : [],
      },
    };
  }) : [];
  return {
    project_id: String(source.project_id ?? '') || undefined,
    execution_source: String(source.execution_source ?? 'unknown'),
    status: String(source.status ?? 'unknown'),
    test_count: Number(source.test_count ?? 0) || 0,
    freshness: {
      state: String(freshness.state ?? '') || undefined,
      error_code: String(freshness.error_code ?? '') || undefined,
      reason: String(freshness.reason ?? '') || undefined,
      runs,
    },
  };
}

function evidenceState(value: unknown): EvidenceRecord['evidence_state'] {
  const status = String(value ?? 'derived').toLowerCase();
  if (status.includes('not_exercised') || status.includes('blocked') || status === 'failed') return 'blocked';
  if (status.includes('corrected_with')) return 'warning';
  if (status === 'corrected') return 'verified';
  if (status.includes('blocker') || status.includes('warning')) return 'warning';
  if (status.includes('observed')) return 'observed';
  if (status.includes('verified') || status === 'pass' || status === 'passed') return 'verified';
  if (status === 'authored') return 'authored';
  if (status === 'stale') return 'stale';
  return 'derived';
}

function normalizeRun(value: unknown): AgentRun {
  const run = recordOf(value);
  const documents = recordOf(run.documents);
  const provenance = recordOf(documents.provenance);
  const anomalies = listFromPayload<Record<string, unknown>>(documents.anomalies, ['records']);
  const corrections = listFromPayload<Record<string, unknown>>(documents.corrections, ['records']);
  const eventRecords = listFromPayload<Record<string, unknown>>(documents.agent_events, ['records']);
  const models = Array.isArray(provenance.models) ? provenance.models.map(recordOf) : [];
  const agents = Array.isArray(provenance.agents) ? provenance.agents.map(recordOf) : [];
  const attribution = recordOf(run.attribution);
  const attributionRecords = Array.isArray(attribution.records) ? attribution.records.map(recordOf) : [];
  const inputManifest = recordOf(documents.input_manifest);
  const digest = recordOf(inputManifest.digest);
  const statusRecord = recordOf(run.evidence_status);
  const statusState = evidenceState(statusRecord.state ?? run.reported_status);
  const startedAt = String(run.started_at ?? provenance.started_at_utc ?? '');
  const completedAt = String(run.completed_at ?? provenance.completed_at_utc ?? '');
  const duration = Number(run.elapsed_ms ?? provenance.elapsed_ms ?? 0);
  const fallbackEvent = startedAt || completedAt ? [{
    event_id: `${String(run.run_id)}:execution`,
    timestamp: startedAt || completedAt,
    task: 'Unattended delivery generation',
    agent: String(agents[0]?.agent_id ?? 'recorded agent team'),
    tool: 'agent harness',
    duration_ms: Number.isFinite(duration) ? duration : undefined,
    result: String(provenance.unattended_reason ?? run.reported_status ?? 'Recorded run'),
    status: statusState,
  }] : [];
  const events = eventRecords.length > 0 ? eventRecords.map((item) => {
    const artifactRefs = Array.isArray(item.artifact_refs) ? item.artifact_refs : [];
    return {
      event_id: String(item.event_id ?? ''),
      timestamp: String(item.started_at ?? item.timestamp ?? '') || undefined,
      task: String(item.task ?? item.phase ?? 'Unlabeled task'),
      agent: String(item.agent_id ?? item.agent ?? 'Agent identity not recorded'),
      tool: String(item.tool ?? 'Tool not recorded'),
      duration_ms: Number(item.duration_ms ?? 0) || undefined,
      result: String(item.result ?? item.action ?? 'Recorded event'),
      status: evidenceState(item.result ?? item.status),
      artifact_ref: artifactRefs.length > 0 ? String(artifactRefs[0]) : undefined,
      artifact_refs: artifactRefs.map(String),
    };
  }) : fallbackEvent;
  return {
    run_id: String(run.run_id ?? 'unknown'),
    status: statusState === 'blocked' ? 'blocked' : statusState === 'verified' ? 'complete' : undefined,
    started_at: startedAt || undefined,
    completed_at: completedAt || undefined,
    source_commit: String(run.source_commit ?? provenance.source_commit ?? '') || undefined,
    model: String(run.model ?? models[0]?.model ?? '') || undefined,
    unattended_verdict: String(run.unattended_verdict ?? provenance.unattended_verdict ?? '') || undefined,
    input_manifest_digest: String(run.input_manifest_digest ?? digest.value ?? inputManifest.sha256 ?? '') || undefined,
    events,
    attribution: attribution.unattended_verdict ? {
      provenance_scope: String(attribution.provenance_scope ?? '') || undefined,
      unattended_verdict: String(attribution.unattended_verdict),
      execution_unattended_verdict: String(attribution.execution_unattended_verdict ?? '') || undefined,
      source_authoring_verdict: String(attribution.source_authoring_verdict ?? '') || undefined,
      source_authoring_record_count: Number(attribution.source_authoring_record_count ?? 0),
      reason: String(attribution.reason ?? '') || undefined,
      human_intervention_detected: Boolean(attribution.human_intervention_detected),
      validation_issues: Array.isArray(attribution.validation_issues)
        ? attribution.validation_issues.map(String)
        : [],
      records: attributionRecords.map((item) => ({
        path: String(item.path ?? 'unknown'),
        before_sha256: item.before_sha256 === null ? null : String(item.before_sha256 ?? '') || undefined,
        after_sha256: String(item.after_sha256 ?? '') || undefined,
        current_sha256: String(item.current_sha256 ?? '') || undefined,
        recorded_attribution_kind: String(item.recorded_attribution_kind ?? '') || undefined,
        attribution_kind: String(item.attribution_kind ?? 'unattributed_change'),
        agent_id: String(item.agent_id ?? '') || undefined,
        task_id: String(item.task_id ?? '') || undefined,
        event_id: String(item.event_id ?? '') || undefined,
        current_state: String(item.current_state ?? '') || undefined,
      })),
    } : undefined,
    anomalies: anomalies.map((item) => ({
      anomaly_id: String(item.anomaly_id ?? ''),
      code: String(item.gap_id ?? item.anomaly_id ?? ''),
      summary: String(item.summary ?? 'Anomaly record'),
      root_cause: readableRecord(item.root_cause, ['classification', 'summary'])
        ?? (String(item.classification ?? '') || undefined),
      correction: readableRecord(item.correction, ['status', 'summaries', 'reason']),
      verification_result: readableRecord(item.verification_result, ['status', 'evidence_refs']),
      status: evidenceState(item.status),
      retry_count: Math.max(Number(item.retry_count ?? 0), Number(item.historical_retry_count ?? 0)) || undefined,
      long_search_or_trial_and_error: Boolean(item.long_search_or_trial_and_error),
      affected_files: stringList(item.affected_files) ?? stringList(item.evidence_paths),
    })),
    corrections: corrections.map((item) => ({
      anomaly_id: String(item.correction_id ?? ''),
      code: String(item.correction_id ?? ''),
      summary: String(item.summary ?? 'Correction record'),
      correction: String(item.summary ?? ''),
      status: evidenceState(item.status),
    })),
  };
}

export const deliveryProjectApi = {
  listProjects: async (): Promise<DeliveryProjectSummary[]> => {
    const { data } = await apiClient.get<unknown>('/delivery-projects');
    return listFromPayload<DeliveryProjectSummary>(data, ['projects', 'items']);
  },
  getProject: async (projectId: string): Promise<DeliveryProjectDetail> => {
    const { data } = await apiClient.get<DeliveryProjectDetail>(
      `/delivery-projects/${encodeURIComponent(projectId)}`
    );
    return data;
  },
  listRuns: async (projectId: string): Promise<AgentRun[]> => {
    const { data } = await apiClient.get<unknown>(
      `/delivery-projects/${encodeURIComponent(projectId)}/runs`
    );
    return listFromPayload<unknown>(data, ['runs', 'items']).map(normalizeRun);
  },
  getRun: async (projectId: string, runId: string): Promise<AgentRun> => {
    const { data } = await apiClient.get<unknown>(
      `/delivery-projects/${encodeURIComponent(projectId)}/runs/${encodeURIComponent(runId)}`
    );
    return normalizeRun(data);
  },
  getWiring: async (projectId: string): Promise<WiringProjection> => {
    const { data } = await apiClient.get<unknown>(
      `/delivery-projects/${encodeURIComponent(projectId)}/wiring`
    );
    const payload = recordOf(data);
    return {
      points: listFromPayload<WiringPoint>(data, ['points', 'wiring', 'items']),
      diagnostics: listFromPayload<Record<string, unknown>>(payload.diagnostics, ['items']).map((item) => ({
        code: String(item.code ?? 'WIRING_DIAGNOSTIC'),
        kind: String(item.kind ?? 'wiring_error'),
        point_id: String(item.point_id ?? '') || undefined,
        severity: String(item.severity ?? 'error') as 'error' | 'warning' | 'blocked',
        message: String(item.message ?? 'Wiring validation failed'),
      })),
    };
  },
  getPhysicalEvidence: async (projectId: string): Promise<PhysicalEvidenceProjection> => {
    const { data } = await apiClient.get<PhysicalEvidenceProjection>(
      `/delivery-projects/${encodeURIComponent(projectId)}/physical-evidence`
    );
    return data;
  },
  getHoldProjection: async (projectId: string): Promise<HoldProjection> => {
    const { data } = await apiClient.get<HoldProjection>(
      `/delivery-projects/${encodeURIComponent(projectId)}/holds`
    );
    return data;
  },
  getReleaseProjection: async (projectId: string): Promise<ReleaseProjection> => {
    const { data } = await apiClient.get<ReleaseProjection>(
      `/delivery-projects/${encodeURIComponent(projectId)}/release`
    );
    return data;
  },
  uploadPointPhoto: async (projectId: string, pointId: string, file: File): Promise<EvidenceUpload> => {
    const { data } = await apiClient.post<EvidenceUpload>(
      `/delivery-projects/${encodeURIComponent(projectId)}/evidence/uploads/${encodeURIComponent(file.name)}`,
      file,
      {
        headers: {
          'Content-Type': file.type || 'application/octet-stream',
          'x-evidence-kind': 'photo',
          'x-semantic-object-kind': 'wiring_point',
          'x-semantic-object-id': pointId,
        },
      }
    );
    return data;
  },
  recordPointObservation: async (projectId: string, pointId: string, request: RecordPointObservationRequest): Promise<PointObservation> => {
    const { data } = await apiClient.post<PointObservation>(
      `/delivery-projects/${encodeURIComponent(projectId)}/wiring/points/${encodeURIComponent(pointId)}/observations`,
      request
    );
    return data;
  },
  getArtifactText: async (artifactRef: string): Promise<string> => {
    const normalized = artifactRef.replace(/\\/g, '/').replace(/^\/?artifacts\//, '').replace(/^\//, '');
    const encodedPath = normalized.split('/').map(encodeURIComponent).join('/');
    const { data } = await apiClient.get<string>(`/artifacts/${encodedPath}`, {
      responseType: 'text',
      transformResponse: [(value) => value],
    });
    return data;
  },
  getVerification: async (projectId: string): Promise<VerificationStage[]> => {
    const { data } = await apiClient.get<unknown>(
      `/delivery-projects/${encodeURIComponent(projectId)}/verification`
    );
    return listFromPayload<VerificationStage>(data, ['stages', 'verification', 'items']);
  },
  getEvidence: async (projectId: string): Promise<EvidenceRecord[]> => {
    const { data } = await apiClient.get<unknown>(
      `/delivery-projects/${encodeURIComponent(projectId)}/evidence`
    );
    return listFromPayload<EvidenceRecord>(data, ['evidence', 'items']);
  },
  getGeometry: async (projectId: string): Promise<GeometryArtifactResponse> => {
    const { data } = await apiClient.get<GeometryArtifactResponse>(
      `/delivery-projects/${encodeURIComponent(projectId)}/geometry`
    );
    return data;
  },
  getWorkspaceProblems: async (): Promise<WorkspaceProblemsProjection> => {
    const { data } = await apiClient.get<unknown>('/workspace/problems');
    const payload = recordOf(data);
    const problems = listFromPayload<Record<string, unknown>>(data, ['problems', 'items']).map((problem, index) => ({
      id: String(problem.id ?? problem.code ?? `problem-${index}`),
      project_id: problem.project_id ? String(problem.project_id) : undefined,
      severity: workspaceProblemSeverity(problem.severity),
      stage: problem.stage ? String(problem.stage) : undefined,
      code: problem.code ? String(problem.code) : undefined,
      message: String(problem.message ?? 'Compiler evidence problem'),
      source_ref: String(problem.source_ref ?? recordOf(problem.artifact).path ?? '') || undefined,
      source_commit: String(problem.source_commit ?? '') || undefined,
      artifact_ref: String(problem.artifact_ref ?? recordOf(problem.artifact).path ?? '') || undefined,
      semantic_object: problem.semantic_object
        ? {
            kind: String(recordOf(problem.semantic_object).kind ?? '') || undefined,
            id: String(recordOf(problem.semantic_object).id ?? '') || undefined,
          }
        : undefined,
      deep_link: problem.deep_link ? recordOf(problem.deep_link) : undefined,
      line: Number(problem.line ?? recordOf(problem.location).line ?? 0) || undefined,
      column: Number(problem.column ?? recordOf(problem.location).column ?? 0) || undefined,
    }));
    return {
      schema_version: Number(payload.schema_version ?? 0) || undefined,
      count: Number(payload.count ?? problems.length),
      partial: Boolean(payload.partial),
      problems,
    };
  },
  getWorkspaceTests: async (): Promise<WorkspaceTestsProjection> => {
    const { data } = await apiClient.get<unknown>('/workspace/tests');
    const payload = recordOf(data);
    const tests = listFromPayload<Record<string, unknown>>(data, ['tests', 'items']).map((test, index) => {
      const state = evidenceState(test.status ?? test.reported_status);
      return {
        id: String(test.id ?? `${test.project_id ?? 'workspace'}:${test.name ?? index}`),
        project_id: test.project_id ? String(test.project_id) : undefined,
        suite: test.suite ? String(test.suite) : undefined,
        execution_source: String(test.execution_source ?? '') || undefined,
        test_scope: String(test.test_scope ?? '') || undefined,
        name: String(test.name ?? 'Unnamed test'),
        status: state === 'verified' ? 'pass' : state === 'blocked' ? 'blocked' : state === 'warning' ? 'fail' : 'skipped',
        duration_ms: Number(test.duration_ms ?? test.elapsed_ms ?? 0) || undefined,
        artifact_ref: String(recordOf(recordOf(test.provenance).artifact).path ?? '') || undefined,
      } satisfies WorkspaceTest;
    });
    return {
      schema_version: Number(payload.schema_version ?? 0) || undefined,
      count: Number(payload.count ?? tests.length),
      partial: Boolean(payload.partial),
      boundary: String(payload.boundary ?? '') || undefined,
      sources: Array.isArray(payload.sources) ? payload.sources.map(normalizeTestSource) : [],
      tests,
    };
  },
  getSignatures: async (projectId: string): Promise<HoldSignatureContext> => {
    const { data } = await apiClient.get<HoldSignatureContext>(
      `/delivery-projects/${encodeURIComponent(projectId)}/holds/signatures`
    );
    return data;
  },
  signHold: async (projectId: string, holdId: string, request: SignHoldRequest): Promise<HoldSignature> => {
    const { data } = await apiClient.post<HoldSignature>(
      `/delivery-projects/${encodeURIComponent(projectId)}/holds/${encodeURIComponent(holdId)}/sign`,
      request
    );
    return data;
  },
};

export const plcApi = {
  getDiagnostics: (content: string) =>
    apiClient.post<PlcDiagnosticsResponse>('/plc/diagnostics', { content }),

  getLanguageSnapshot: (content: string) =>
    apiClient.post<PlcLanguageSnapshot>('/plc/language', { content }),
};

export const flowchartApi = {
  generatePlc: (request: FlowchartGeneratePlcRequest) =>
    apiClient.post<FlowchartGeneratePlcResponse>('/flowchart/generate-plc', request),
};

// 请求拦截器：添加 JWT token
// DSL capability API
export const dslApi = {
  getCapabilities: () => apiClient.get<DslCapabilitiesReport>('/dsl/capabilities'),
};

apiClient.interceptors.request.use((config) => {
  const token = localStorage.getItem('auth_token');
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

// 响应拦截器：统一错误处理
apiClient.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      // Token 过期，跳转登录
      localStorage.removeItem('auth_token');
      window.location.href = '/login';
    }
    return Promise.reject(error);
  }
);

// 拓扑相关 API
export const topologyApi = {
  getTopology: (id: string) =>
    apiClient.get<ComponentTopology>(`/topology/${id}`),

  parsePlc: (content: string) =>
    apiClient.post<ComponentTopology>('/topology/parse-plc', { content }),

  validateTopology: (topology: ComponentTopology) =>
    apiClient.post<{ valid: boolean; errors: string[] }>('/topology/validate', topology),

  saveTopology: (id: string, topology: ComponentTopology) =>
    apiClient.put(`/topology/${id}`, topology),
};

// 场景相关 API
export const scenarioApi = {
  getScenario: (id: string) =>
    apiClient.get<ComponentScenario>(`/scenario/${id}`),

  validateScenario: (scenario: ComponentScenario) =>
    apiClient.post<{ valid: boolean; errors: string[] }>('/scenario/validate', scenario),

  saveScenario: (id: string, scenario: ComponentScenario) =>
    apiClient.put(`/scenario/${id}`, scenario),
};

// 运行相关 API
export const runApi = {
  triggerNoBoard: (plcFile: string, scenarioFile: string) =>
    apiClient.post<{ run_id: string }>('/run/no-board-gate', {
      plc_file: plcFile,
      scenario_file: scenarioFile,
    }),

  triggerComponentSim: (topologyFile: string, scenarioFile: string) =>
    apiClient.post<{ run_id: string }>('/run/no-board-gate', {
      topology_file: topologyFile,
      scenario_file: scenarioFile,
      mode: 'component_sim',
    }),

  getRunStatus: (runId: string) =>
    apiClient.get<RunStatus>(`/run/${runId}/status`),

  listRuns: (limit = 20) =>
    apiClient.get<RunStatus[]>('/run/list', { params: { limit } }),
};

export const geometryApi = {
  getGeometry: (runId: string) =>
    apiClient.get<GeometryArtifactResponse>(`/geometry/${runId}`),
};

// Trace 相关 API
export const traceApi = {
  getTrace: (runId: string) =>
    apiClient.get<TraceData>(`/trace/${runId}`),

  getKeypoints: (runId: string) =>
    apiClient.get<TraceKeypointArtifact>(`/trace/${runId}/keypoints`),

  getTickRange: (runId: string, startTick: number, endTick: number) =>
    apiClient.get<TraceData>(`/trace/${runId}/range`, {
      params: { start: startTick, end: endTick },
    }),
};

// 诊断相关 API
export const diagnosisApi = {
  getDiagnosisReport: (runId: string) =>
    apiClient.get<DiagnosisReport>(`/diagnosis/${runId}`),

  getTimingReport: (runId: string) =>
    apiClient.get<TimingReport>(`/timing/${runId}`),
};

// 告警相关 API
export const alarmApi = {
  getAlarms: (params?: { severity?: string; limit?: number }) =>
    apiClient.get<AlarmEvent[]>('/alarms', { params }),

  acknowledgeAlarm: (alarmId: string, comment: string) =>
    apiClient.post(`/alarms/${alarmId}/ack`, { comment }),
};

// 仿真注入相关 API
export const simulationApi = {
  injectEvent: (target: string, eventType: string, value: boolean, triggeredBy: string) =>
    apiClient.post('/simulation/inject-event', {
      target,
      event_type: eventType,
      value,
      triggered_by: triggeredBy,
    }),

  injectFault: (target: string, faultKind: string, durationMs: number | undefined, triggeredBy: string) =>
    apiClient.post('/simulation/inject-fault', {
      target,
      fault_kind: faultKind,
      duration_ms: durationMs,
      triggered_by: triggeredBy,
    }),

  clearFaults: (target: string, triggeredBy: string) =>
    apiClient.post('/simulation/clear-faults', {
      target,
      triggered_by: triggeredBy,
    }),
};

// 认证相关 API
export const authApi = {
  login: (username: string, password: string) =>
    apiClient.post<{ token: string; expires_at_ms: number; user: AuthenticatedUser }>('/auth/login', { username, password }),

  logout: () =>
    apiClient.post('/auth/logout'),

  getCurrentUser: () =>
    apiClient.get<AuthenticatedUser>('/auth/me'),
};

export default apiClient;
