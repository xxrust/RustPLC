import React, { useState } from 'react';
import { Alert, Button, Card, Form, Input, Select, Space, Spin, Table, Tag, Typography } from 'antd';
import { PlayCircleOutlined, ReloadOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import RunReviewCockpit from '../components/review/RunReviewCockpit';
import { geometryApi, runApi, traceApi } from '../services/api';
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

const RunPage: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  const { data: runsData, isLoading, refetch } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runApi.listRuns(20),
    refetchInterval: 5000,
  });

  React.useEffect(() => {
    if (!selectedRunId && runsData?.data?.length) {
      setSelectedRunId(runsData.data[0].run_id);
    }
  }, [runsData?.data, selectedRunId]);

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

      <Card title={t('run.triggerCardTitle')}>
        <Paragraph style={{ color: '#94a3b8' }}>
          {t('run.triggerHint')}
        </Paragraph>

        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => triggerMutation.mutate(values)}
          initialValues={{ mode: 'component_sim' }}
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

        {triggerMutation.isSuccess && (
          <Alert
            message={t('run.requestSubmitted')}
            description={t('run.requestSubmittedDesc', { runId: triggerMutation.data?.data.run_id })}
            type="info"
            showIcon
            closable
            style={{ marginTop: 16 }}
          />
        )}
        {triggerMutation.isError && (
          <Alert
            message={t('run.triggerFailed')}
            description={triggerMutation.error?.message}
            type="error"
            showIcon
            closable
            style={{ marginTop: 16 }}
          />
        )}
      </Card>
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
