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
} from '../types';

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || '/api';

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
};

// 请求拦截器：添加 JWT token
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
    apiClient.get('/alarms', { params }),

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
    apiClient.post('/auth/login', { username, password }),

  logout: () =>
    apiClient.post('/auth/logout'),

  getCurrentUser: () =>
    apiClient.get('/auth/me'),
};

export default apiClient;
