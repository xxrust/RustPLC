import React, { useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Collapse,
  Form,
  Input,
  Select,
  Space,
  Spin,
  Table,
  Tag,
  Typography,
} from 'antd';
import { PlayCircleOutlined, ReloadOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import RunReviewCockpit from '../components/review/RunReviewCockpit';
import { geometryApi, projectApi, runApi, traceApi } from '../services/api';
import { useAppStore } from '../stores/appStore';
import type { RunStatus } from '../types';
import { formatTimestamp } from '../utils/time';

const { Paragraph, Text, Title } = Typography;

type RunTriggerMode = 'no_board_gate' | 'component_sim';

interface RunTriggerFormValues {
  mode: RunTriggerMode;
  plcFile?: string;
  topologyFile?: string;
  scenarioFile?: string;
}

interface ProjectRecord {
  id: string;
  name: string;
  path: string;
  type: string;
}

interface ResolvedRunTargets {
  projectLabel: string;
  sourcePath?: string;
  plcFile?: string;
  topologyFile?: string;
  scenarioFile?: string;
  canRunPlc: boolean;
  canRunTopology: boolean;
  plcDisabledReason?: QuickRunDisabledReason;
  topologyDisabledReason?: QuickRunDisabledReason;
  manualDefaults: RunTriggerFormValues;
}

type QuickRunDisabledReason =
  | 'no_project'
  | 'local_buffer'
  | 'missing_plc'
  | 'missing_topology'
  | 'plc_only_project'
  | 'missing_scenario';

const RUN_PRESETS: Record<
  string,
  Partial<Pick<ResolvedRunTargets, 'plcFile' | 'topologyFile' | 'scenarioFile'>>
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

function localizeRunStatus(status: string, t: (key: string) => string): string {
  const map: Record<string, string> = {
    running: 'run.statusRunning',
    pass: 'run.statusPass',
    fail: 'run.statusFail',
  };
  return map[status] ? t(map[status]) : status;
}

function localizeTriggeredBy(value: string | undefined, t: (key: string) => string): string {
  if (value === 'web-user') return t('run.triggeredByWebUser');
  return value || '-';
}

function describeReviewSignal(run: RunStatus): string {
  if (run.status === 'fail') return run.failure_summary ?? 'FAILED_NO_SUMMARY';
  if (run.status === 'running') return run.failure_summary ?? 'RUNNING_NO_SUMMARY';
  return 'PASSED_READY_FOR_REVIEW';
}

function normalizePath(path?: string | null): string | undefined {
  return path?.replace(/\\/g, '/');
}

function hasFilesystemPath(path?: string): boolean {
  return Boolean(path && /[\\/]/.test(path));
}

function resolveRunTargets(
  currentProject: string | null,
  currentProjectPath: string | null,
  currentProjectContent: string | null,
  currentProjectRecord?: ProjectRecord
): ResolvedRunTargets {
  const preset = currentProject ? RUN_PRESETS[currentProject] : undefined;
  const sourcePath = normalizePath(currentProjectPath ?? currentProjectRecord?.path);
  const isLocalBuffer = Boolean(currentProjectContent && !hasFilesystemPath(sourcePath));

  const plcFile =
    preset?.plcFile ??
    (currentProjectRecord?.type === 'plc' && hasFilesystemPath(sourcePath) ? sourcePath : undefined);
  const topologyFile =
    preset?.topologyFile ??
    (currentProjectRecord?.type === 'component_topology' && hasFilesystemPath(sourcePath)
      ? sourcePath
      : undefined);
  const scenarioFile = preset?.scenarioFile;

  let plcDisabledReason: QuickRunDisabledReason | undefined;
  if (!currentProject) {
    plcDisabledReason = 'no_project';
  } else if (isLocalBuffer) {
    plcDisabledReason = 'local_buffer';
  } else if (!plcFile) {
    plcDisabledReason = 'missing_plc';
  } else if (!scenarioFile) {
    plcDisabledReason = 'missing_scenario';
  }

  let topologyDisabledReason: QuickRunDisabledReason | undefined;
  if (!currentProject) {
    topologyDisabledReason = 'no_project';
  } else if (isLocalBuffer) {
    topologyDisabledReason = 'local_buffer';
  } else if (plcFile && !topologyFile) {
    topologyDisabledReason = 'plc_only_project';
  } else if (!topologyFile) {
    topologyDisabledReason = 'missing_topology';
  } else if (!scenarioFile) {
    topologyDisabledReason = 'missing_scenario';
  }

  const prefersTopology =
    currentProjectRecord?.type === 'component_topology' || currentProject === 'component_model';

  return {
    projectLabel: currentProject ?? '-',
    sourcePath,
    plcFile,
    topologyFile,
    scenarioFile,
    canRunPlc: !plcDisabledReason,
    canRunTopology: !topologyDisabledReason,
    plcDisabledReason,
    topologyDisabledReason,
    manualDefaults: {
      mode: prefersTopology ? 'component_sim' : 'no_board_gate',
      plcFile,
      topologyFile,
      scenarioFile,
    },
  };
}

function localizeQuickRunDisabledReason(
  reason: QuickRunDisabledReason | undefined,
  t: (key: string) => string
): string | null {
  if (!reason) return null;
  const map: Record<QuickRunDisabledReason, string> = {
    no_project: 'run.quickDisabledNoProject',
    local_buffer: 'run.quickDisabledLocalBuffer',
    missing_plc: 'run.quickDisabledMissingPlc',
    missing_topology: 'run.quickDisabledMissingTopology',
    plc_only_project: 'run.quickDisabledPlcOnlyProject',
    missing_scenario: 'run.quickDisabledMissingScenario',
  };
  return t(map[reason]);
}

const RunPage: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const { currentProject, currentProjectPath, currentProjectContent } = useAppStore();

  const { data: runsData, isLoading, refetch } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runApi.listRuns(20),
    refetchInterval: 5000,
  });

  const { data: projectsData } = useQuery({
    queryKey: ['projects'],
    queryFn: () => projectApi.listProjects(),
  });

  const currentProjectRecord = useMemo(() => {
    const projects: ProjectRecord[] = projectsData?.data?.projects || [];
    return projects.find((project) => project.id === currentProject);
  }, [currentProject, projectsData?.data?.projects]);

  const runTargets = useMemo(
    () =>
      resolveRunTargets(
        currentProject,
        currentProjectPath,
        currentProjectContent,
        currentProjectRecord
      ),
    [currentProject, currentProjectContent, currentProjectPath, currentProjectRecord]
  );

  const showPlcQuickAction =
    currentProjectRecord?.type === 'plc' || Boolean(runTargets.plcFile || runTargets.canRunPlc);
  const showTopologyQuickAction =
    currentProjectRecord?.type === 'component_topology' ||
    Boolean(runTargets.topologyFile || runTargets.canRunTopology);
  const topologyUnavailableForCurrentProject =
    !showTopologyQuickAction &&
    Boolean(currentProject) &&
    (runTargets.topologyDisabledReason === 'plc_only_project' ||
      runTargets.topologyDisabledReason === 'missing_topology');

  React.useEffect(() => {
    if (!selectedRunId && runsData?.data?.length) {
      setSelectedRunId(runsData.data[0].run_id);
    }
  }, [runsData?.data, selectedRunId]);

  React.useEffect(() => {
    form.setFieldsValue(runTargets.manualDefaults);
  }, [form, runTargets.manualDefaults]);

  const triggerMutation = useMutation({
    mutationFn: (values: RunTriggerFormValues) => {
      if (values.mode === 'component_sim') {
        return runApi.triggerComponentSim(values.topologyFile || '', values.scenarioFile || '');
      }
      return runApi.triggerNoBoard(values.plcFile || '', values.scenarioFile || '');
    },
    onSuccess: (response) => {
      setSelectedRunId(response.data.run_id);
      refetch();
    },
  });

  const handleQuickTrigger = (mode: RunTriggerMode) => {
    if (mode === 'component_sim') {
      triggerMutation.mutate({
        mode,
        topologyFile: runTargets.topologyFile,
        scenarioFile: runTargets.scenarioFile,
      });
      return;
    }

    triggerMutation.mutate({
      mode,
      plcFile: runTargets.plcFile,
      scenarioFile: runTargets.scenarioFile,
    });
  };

  const columns = [
    {
      title: t('run.runId'),
      dataIndex: 'run_id',
      key: 'run_id',
      render: (id: string) => <Text code>{id.slice(0, 12)}</Text>,
    },
    {
      title: t('run.status'),
      dataIndex: 'status',
      key: 'status',
      render: (status: string) => {
        const colorMap = { running: 'processing', pass: 'success', fail: 'error' };
        return <Tag color={colorMap[status as keyof typeof colorMap]}>{localizeRunStatus(status, t)}</Tag>;
      },
    },
    {
      title: t('run.runMode'),
      dataIndex: 'mode',
      key: 'mode',
      render: (mode?: string) => {
        if (mode === 'component_sim') return <Tag color="purple">{t('run.modeComponent')}</Tag>;
        if (mode === 'no_board_gate') return <Tag color="blue">{t('run.modeNoBoard')}</Tag>;
        return mode || '-';
      },
    },
    {
      title: t('run.triggeredBy'),
      dataIndex: 'triggered_by',
      key: 'triggered_by',
      render: (value?: string) => localizeTriggeredBy(value, t),
    },
    {
      title: t('run.triggeredAt'),
      dataIndex: 'triggered_at',
      key: 'triggered_at',
      render: (_time: string, record: RunStatus) =>
        formatTimestamp(record.triggered_at, record.triggered_at_ms),
    },
    {
      title: t('run.reviewSignal'),
      dataIndex: 'failure_summary',
      key: 'failure_summary',
      render: (_summary: string | undefined, record: RunStatus) => {
        const signal = describeReviewSignal(record);
        const text =
          signal === 'FAILED_NO_SUMMARY'
            ? t('run.reviewSignalFailedNoSummary')
            : signal === 'RUNNING_NO_SUMMARY'
              ? t('run.reviewSignalRunning')
              : signal === 'PASSED_READY_FOR_REVIEW'
                ? t('run.reviewSignalPassed')
                : signal;
        return <Text type={record.status === 'fail' ? 'danger' : 'secondary'}>{text}</Text>;
      },
    },
    {
      title: t('run.actions'),
      key: 'action',
      render: (_: unknown, record: RunStatus) => (
        <Button size="small" type="primary" onClick={() => setSelectedRunId(record.run_id)}>
          {t('run.review')}
        </Button>
      ),
    },
  ];

  return (
    <div style={{ display: 'grid', gap: 24 }}>
      <div>
        <Title level={2} style={{ marginBottom: 8 }}>
          {t('run.title')}
        </Title>
        <Paragraph style={{ color: '#94a3b8', marginBottom: 0 }}>
          {t('run.intro')}
        </Paragraph>
      </div>

      <Card title={t('run.currentProjectCardTitle')}>
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          <Alert
            type="info"
            showIcon
            message={t('run.currentProjectSummary', { project: runTargets.projectLabel })}
            description={t('run.currentProjectHint')}
          />

          <div
            style={{
              display: 'grid',
              gap: 16,
              gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
            }}
          >
            {showPlcQuickAction && (
              <Card size="small" title={t('run.modeNoBoard')}>
                <Space direction="vertical" size="middle" style={{ width: '100%' }}>
                  <Paragraph style={{ marginBottom: 0 }}>{t('run.quickPlcDesc')}</Paragraph>
                  <div style={{ display: 'grid', gap: 8 }}>
                    <Text>
                      {t('run.sourcePath')}
                      :{' '}
                      <Text code>{runTargets.plcFile || t('common.noneSelected')}</Text>
                    </Text>
                    <Text>
                      {t('run.scenarioFile')}
                      :{' '}
                      <Text code>{runTargets.scenarioFile || t('common.noneSelected')}</Text>
                    </Text>
                  </div>
                  {runTargets.plcDisabledReason && (
                    <Alert
                      type="warning"
                      showIcon
                      message={localizeQuickRunDisabledReason(runTargets.plcDisabledReason, t)}
                    />
                  )}
                  <Button
                    type="primary"
                    icon={<PlayCircleOutlined />}
                    loading={triggerMutation.isPending}
                    disabled={!runTargets.canRunPlc}
                    onClick={() => handleQuickTrigger('no_board_gate')}
                  >
                    {t('run.runCurrentPlc')}
                  </Button>
                </Space>
              </Card>
            )}

            {showTopologyQuickAction && (
              <Card size="small" title={t('run.modeComponent')}>
                <Space direction="vertical" size="middle" style={{ width: '100%' }}>
                  <Paragraph style={{ marginBottom: 0 }}>{t('run.quickTopologyDesc')}</Paragraph>
                  <div style={{ display: 'grid', gap: 8 }}>
                    <Text>
                      {t('run.sourcePath')}
                      :{' '}
                      <Text code>{runTargets.topologyFile || t('common.noneSelected')}</Text>
                    </Text>
                    <Text>
                      {t('run.scenarioFile')}
                      :{' '}
                      <Text code>{runTargets.scenarioFile || t('common.noneSelected')}</Text>
                    </Text>
                  </div>
                  {runTargets.topologyDisabledReason && (
                    <Alert
                      type="warning"
                      showIcon
                      message={localizeQuickRunDisabledReason(runTargets.topologyDisabledReason, t)}
                    />
                  )}
                  <Button
                    type="primary"
                    icon={<PlayCircleOutlined />}
                    loading={triggerMutation.isPending}
                    disabled={!runTargets.canRunTopology}
                    onClick={() => handleQuickTrigger('component_sim')}
                  >
                    {t('run.runCurrentTopology')}
                  </Button>
                </Space>
              </Card>
            )}
          </div>

          {topologyUnavailableForCurrentProject && (
            <Alert
              type="info"
              showIcon
              message={t('run.topologyUnavailableForCurrentProject')}
            />
          )}

          <div style={{ display: 'grid', gap: 8 }}>
            <Text type="secondary">
              {t('run.currentProject')}
              : <Text strong>{runTargets.projectLabel}</Text>
            </Text>
            {runTargets.sourcePath && (
              <Text type="secondary">
                {t('run.sourcePath')}
                : <Text code>{runTargets.sourcePath}</Text>
              </Text>
            )}
          </div>

          {triggerMutation.isSuccess && (
            <Alert
              message={t('run.requestSubmitted')}
              description={t('run.requestSubmittedDesc', { runId: triggerMutation.data?.data.run_id })}
              type="info"
              showIcon
              closable
            />
          )}
          {triggerMutation.isError && (
            <Alert
              message={t('run.triggerFailed')}
              description={triggerMutation.error?.message}
              type="error"
              showIcon
              closable
            />
          )}
        </Space>
      </Card>

      <Card>
        <Collapse
          items={[
            {
              key: 'advanced',
              label: t('run.advancedTitle'),
              children: (
                <Space direction="vertical" size="middle" style={{ width: '100%' }}>
                  <Paragraph style={{ color: '#94a3b8', marginBottom: 0 }}>
                    {t('run.advancedIntro')}
                  </Paragraph>
                  <Text type="secondary">{t('run.formPrefilled')}</Text>
                  <Form
                    form={form}
                    layout="vertical"
                    onFinish={(values) => triggerMutation.mutate(values)}
                  >
                    <Form.Item
                      name="mode"
                      label={t('run.runMode')}
                      rules={[{ required: true, message: t('run.runModeRequired') }]}
                      style={{ width: 360 }}
                    >
                      <Select
                        options={[
                          { label: t('run.modeComponent'), value: 'component_sim' },
                          { label: t('run.modeNoBoard'), value: 'no_board_gate' },
                        ]}
                      />
                    </Form.Item>

                    <Form.Item noStyle shouldUpdate={(prev, next) => prev.mode !== next.mode}>
                      {({ getFieldValue }) =>
                        getFieldValue('mode') === 'component_sim' ? (
                          <Form.Item
                            name="topologyFile"
                            label={t('run.topologyFile')}
                            rules={[{ required: true, message: t('run.topologyFileRequired') }]}
                            style={{ width: 680 }}
                          >
                            <Input placeholder={t('run.topologyPlaceholder')} />
                          </Form.Item>
                        ) : (
                          <Form.Item
                            name="plcFile"
                            label={t('run.plcFile')}
                            rules={[{ required: true, message: t('run.plcFileRequired') }]}
                            style={{ width: 680 }}
                          >
                            <Input placeholder={t('run.plcPlaceholder')} />
                          </Form.Item>
                        )
                      }
                    </Form.Item>

                    <Form.Item
                      name="scenarioFile"
                      label={t('run.scenarioFile')}
                      rules={[{ required: true, message: t('run.scenarioFileRequired') }]}
                      style={{ width: 680 }}
                    >
                      <Input placeholder={t('run.scenarioPlaceholder')} />
                    </Form.Item>

                    <Form.Item style={{ marginBottom: 0 }}>
                      <Button
                        type="primary"
                        htmlType="submit"
                        icon={<PlayCircleOutlined />}
                        loading={triggerMutation.isPending}
                      >
                        {t('run.submitRunRequest')}
                      </Button>
                    </Form.Item>
                  </Form>
                </Space>
              ),
            },
          ]}
        />
      </Card>

      <Card
        title={t('run.runHistory')}
        extra={
          <Button icon={<ReloadOutlined />} onClick={() => refetch()}>
            {t('run.refresh')}
          </Button>
        }
      >
        <Table
          dataSource={runsData?.data || []}
          columns={columns}
          rowKey="run_id"
          loading={isLoading}
          locale={{ emptyText: t('run.noRunsAvailable') }}
          pagination={{ pageSize: 10 }}
          rowClassName={(record) => (record.run_id === selectedRunId ? 'ant-table-row-selected' : '')}
        />
      </Card>

      {selectedRunId ? (
        <Card title={t('run.reviewFocus', { runId: selectedRunId.slice(0, 12) })}>
          <RunDetails runId={selectedRunId} />
        </Card>
      ) : (
        <Card>
          <Text type="secondary">{t('run.noRunsAvailable')}</Text>
        </Card>
      )}
    </div>
  );
};

const RunDetails: React.FC<{ runId: string }> = ({ runId }) => {
  const { t } = useTranslation();
  const { data: runStatusData, isLoading } = useQuery({
    queryKey: ['runStatus', runId],
    queryFn: () => runApi.getRunStatus(runId),
    refetchInterval: (query) => {
      const status = query.state.data?.data.status;
      return status === 'running' ? 2000 : false;
    },
  });

  const run = runStatusData?.data;

  const { data: geometryData, isLoading: isGeometryLoading } = useQuery({
    queryKey: ['geometry', runId, run?.artifacts?.geometry ?? 'missing'],
    queryFn: () => geometryApi.getGeometry(runId),
    enabled: Boolean(runId),
    refetchInterval: run?.status === 'running' || !run?.artifacts?.geometry ? 2000 : false,
  });

  const { data: keypointsData, isLoading: isKeypointsLoading } = useQuery({
    queryKey: ['trace-keypoints', runId],
    queryFn: () => traceApi.getKeypoints(runId),
    enabled: Boolean(runId),
    refetchInterval: run?.status === 'running' ? 2000 : false,
  });

  const { data: traceData, isLoading: isTraceLoading } = useQuery({
    queryKey: ['trace', runId],
    queryFn: () => traceApi.getTrace(runId),
    enabled: Boolean(runId),
    refetchInterval: run?.status === 'running' ? 2000 : false,
  });

  if (isLoading) {
    return <Spin />;
  }

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <RunReviewCockpit
        run={run}
        geometry={geometryData?.data}
        keypoints={keypointsData?.data}
        trace={traceData?.data}
        title={t('run.reviewTitle')}
        loading={isGeometryLoading || isKeypointsLoading || isTraceLoading}
      />
    </Space>
  );
};

export default RunPage;
