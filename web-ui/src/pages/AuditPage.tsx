import React, { useMemo, useState } from 'react';
import { Card, List, Space, Spin, Tag, Typography } from 'antd';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import RunReviewCockpit from '../components/review/RunReviewCockpit';
import { geometryApi, runApi, traceApi } from '../services/api';
import { useAppStore } from '../stores/appStore';
import type { RunStatus } from '../types';
import { formatTimestamp } from '../utils/time';

const { Paragraph, Text, Title } = Typography;

const RUN_PRESETS: Record<
  string,
  { plcFile?: string; topologyFile?: string; scenarioFile?: string }
> = {
  demo: {
    plcFile: 'examples/demo.plc',
    scenarioFile: 'examples/demo.scenario.json',
  },
  component_model: {
    topologyFile: 'examples/component_model/topology.json',
    scenarioFile: 'examples/component_model/scenario_normal.json',
  },
  topology_perf_500: {
    plcFile: 'examples/topology_perf_500.plc',
    topologyFile: 'examples/topology_perf_500.topology.json',
    scenarioFile: 'examples/topology_perf_500.scenario.json',
  },
};

function pickDefaultRun(runs: RunStatus[], currentProject: string | null): string | null {
  return (
    runs.find((run) => runMatchesCurrentProject(run, currentProject))?.run_id ??
    runs.find((run) => run.status === 'fail')?.run_id ??
    runs.find((run) => run.status === 'running')?.run_id ??
    runs[0]?.run_id ??
    null
  );
}

const AuditPage: React.FC = () => {
  const { t } = useTranslation();
  const { currentProject } = useAppStore();
  const [requestedRunId, setRequestedRunId] = useState<string | null>(null);

  const { data: runsData, isLoading: isRunsLoading } = useQuery({
    queryKey: ['audit-runs'],
    queryFn: () => runApi.listRuns(20),
    refetchInterval: 5000,
  });

  const runs = useMemo(() => runsData?.data ?? [], [runsData?.data]);
  const selectedRunId = useMemo(
    () =>
      (requestedRunId && runs.some((run) => run.run_id === requestedRunId)
        ? requestedRunId
        : pickDefaultRun(runs, currentProject)),
    [currentProject, requestedRunId, runs]
  );

  const selectedRun = useMemo(
    () => runs.find((run) => run.run_id === selectedRunId),
    [runs, selectedRunId]
  );

  const { data: runStatusData, isLoading: isRunLoading } = useQuery({
    queryKey: ['audit-run-status', selectedRunId],
    queryFn: () => runApi.getRunStatus(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  const { data: geometryData, isLoading: isGeometryLoading } = useQuery({
    queryKey: ['audit-geometry', selectedRunId],
    queryFn: () => geometryApi.getGeometry(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  const { data: keypointsData, isLoading: isKeypointsLoading } = useQuery({
    queryKey: ['audit-keypoints', selectedRunId],
    queryFn: () => traceApi.getKeypoints(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  const { data: traceData, isLoading: isTraceLoading } = useQuery({
    queryKey: ['audit-trace', selectedRunId],
    queryFn: () => traceApi.getTrace(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  return (
    <div style={{ display: 'grid', gap: 24 }}>
      <div>
        <Title level={2} style={{ marginBottom: 8 }}>
          {t('auditPage.title')}
        </Title>
        <Paragraph style={{ color: '#94a3b8', marginBottom: 0 }}>
          {t('auditPage.intro')}
        </Paragraph>
      </div>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'minmax(320px, 0.95fr) minmax(0, 1.65fr)',
          gap: 16,
          alignItems: 'start',
        }}
      >
        <Card title={t('auditPage.recentRuns')} loading={isRunsLoading} styles={{ body: { padding: 0 } }}>
          <List
            dataSource={runs}
            locale={{ emptyText: t('auditPage.noRunsAvailable') }}
            renderItem={(run) => {
              const active = run.run_id === selectedRunId;
              return (
                <List.Item
                  style={{
                    padding: '14px 16px',
                    cursor: 'pointer',
                    background: active ? 'rgba(14,116,144,0.16)' : 'transparent',
                    borderLeft: active ? '3px solid #22d3ee' : '3px solid transparent',
                  }}
                  onClick={() => setRequestedRunId(run.run_id)}
                >
                  <div style={{ width: '100%' }}>
                    <div
                      style={{
                        display: 'flex',
                        justifyContent: 'space-between',
                        alignItems: 'center',
                        gap: 8,
                        flexWrap: 'wrap',
                      }}
                    >
                      <Text strong style={{ color: '#f8fafc' }}>
                        {run.run_id.slice(0, 12)}
                      </Text>
                      <Space wrap size={[6, 6]}>
                        <Tag color={run.status === 'pass' ? 'success' : run.status === 'fail' ? 'error' : 'processing'}>
                          {run.status.toUpperCase()}
                        </Tag>
                        {run.mode && <Tag>{run.mode}</Tag>}
                      </Space>
                    </div>
                    <div style={{ color: '#94a3b8', marginTop: 6 }}>
                      {formatTimestamp(run.triggered_at, run.triggered_at_ms)} | {run.triggered_by}
                    </div>
                    <div style={{ color: run.status === 'fail' ? '#fca5a5' : '#94a3b8', marginTop: 8 }}>
                      {run.failure_summary ??
                        (run.status === 'running'
                          ? t('auditPage.runningSummary')
                          : t('auditPage.noFailureSummary'))}
                    </div>
                  </div>
                </List.Item>
              );
            }}
          />
        </Card>

        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          {selectedRunId ? (
            <RunReviewCockpit
              run={runStatusData?.data ?? selectedRun}
              geometry={geometryData?.data}
              keypoints={keypointsData?.data}
              trace={traceData?.data}
              title={t('auditPage.focusTitle')}
              loading={isRunLoading || isGeometryLoading || isKeypointsLoading || isTraceLoading}
            />
          ) : (
            <Card>
              {isRunsLoading ? <Spin /> : <Text type="secondary">{t('auditPage.emptyPrompt')}</Text>}
            </Card>
          )}
        </Space>
      </div>
    </div>
  );
};

export default AuditPage;

function normalizePath(path?: string | null): string | undefined {
  return path?.replace(/\\/g, '/');
}

function runMatchesCurrentProject(
  run: RunStatus,
  currentProject: string | null
): boolean {
  if (!currentProject) {
    return false;
  }
  const preset = RUN_PRESETS[currentProject];
  if (!preset) {
    return false;
  }

  return (
    normalizePath(run.plc_file) === preset.plcFile ||
    normalizePath(run.topology_file) === preset.topologyFile ||
    normalizePath(run.scenario_file) === preset.scenarioFile
  );
}
