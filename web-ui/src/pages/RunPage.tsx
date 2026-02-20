import React, { useState } from 'react';
import { Card, Button, Form, Input, Space, Table, Tag, Typography, Alert, Spin } from 'antd';
import { PlayCircleOutlined, ReloadOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { runApi } from '../services/api';
import type { RunStatus } from '../types';

const { Title, Text } = Typography;

const RunPage: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  const { data: runsData, isLoading, refetch } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runApi.listRuns(20),
    refetchInterval: 5000,
  });

  const triggerMutation = useMutation({
    mutationFn: (values: { plcFile: string; scenarioFile: string }) =>
      runApi.triggerNoBoard(values.plcFile, values.scenarioFile),
    onSuccess: (response) => {
      setSelectedRunId(response.data.run_id);
      refetch();
    },
  });

  const handleTrigger = (values: any) => {
    triggerMutation.mutate(values);
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
        return <Tag color={colorMap[status as keyof typeof colorMap]}>{status.toUpperCase()}</Tag>;
      },
    },
    {
      title: t('run.triggeredBy'),
      dataIndex: 'triggered_by',
      key: 'triggered_by',
    },
    {
      title: t('run.triggeredAt'),
      dataIndex: 'triggered_at',
      key: 'triggered_at',
      render: (time: string) => new Date(time).toLocaleString(),
    },
    {
      title: t('run.failureSummary'),
      dataIndex: 'failure_summary',
      key: 'failure_summary',
      render: (summary?: string) => summary ? <Text type="danger">{summary}</Text> : '-',
    },
    {
      title: t('run.actions'),
      key: 'action',
      render: (_: any, record: RunStatus) => (
        <Space>
          <Button size="small" onClick={() => setSelectedRunId(record.run_id)}>{t('run.viewDetails')}</Button>
          {record.artifacts?.trace && (
            <Button size="small" type="link" href={record.artifacts.trace} target="_blank">Trace</Button>
          )}
          {record.artifacts?.diagnosis && (
            <Button size="small" type="link" href={record.artifacts.diagnosis} target="_blank">{t('run.diagnosis')}</Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Title level={2}>{t('run.title')}</Title>

      <Card title={t('run.triggerGate')} style={{ marginBottom: 24 }}>
        <Form
          form={form}
          layout="inline"
          onFinish={handleTrigger}
          initialValues={{ plcFile: 'examples/demo.plc', scenarioFile: 'examples/demo_scenario.yaml' }}
        >
          <Form.Item
            name="plcFile"
            label={t('run.plcFile')}
            rules={[{ required: true, message: t('run.plcFileRequired') }]}
            style={{ width: 300 }}
          >
            <Input placeholder="examples/demo.plc" />
          </Form.Item>
          <Form.Item
            name="scenarioFile"
            label={t('run.scenarioFile')}
            rules={[{ required: true, message: t('run.scenarioFileRequired') }]}
            style={{ width: 300 }}
          >
            <Input placeholder="examples/demo_scenario.yaml" />
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit" icon={<PlayCircleOutlined />} loading={triggerMutation.isPending}>
              {t('run.run')}
            </Button>
          </Form.Item>
        </Form>

        {triggerMutation.isSuccess && (
          <Alert
            message={t('run.triggered')}
            description={`${t('run.runId')}: ${triggerMutation.data?.data.run_id}`}
            type="success"
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

      <Card
        title={t('run.runHistory')}
        extra={<Button icon={<ReloadOutlined />} onClick={() => refetch()}>{t('run.refresh')}</Button>}
      >
        <Table
          dataSource={runsData?.data || []}
          columns={columns}
          rowKey="run_id"
          loading={isLoading}
          pagination={{ pageSize: 10 }}
          rowClassName={(record) => record.run_id === selectedRunId ? 'ant-table-row-selected' : ''}
        />
      </Card>

      {selectedRunId && (
        <Card title={`${t('run.runDetails')}: ${selectedRunId.slice(0, 12)}`} style={{ marginTop: 24 }}>
          <RunDetails runId={selectedRunId} />
        </Card>
      )}
    </div>
  );
};

const RunDetails: React.FC<{ runId: string }> = ({ runId }) => {
  const { t } = useTranslation();
  const { data, isLoading } = useQuery({
    queryKey: ['runStatus', runId],
    queryFn: () => runApi.getRunStatus(runId),
    refetchInterval: (query) => {
      const status = query.state.data?.data.status;
      return status === 'running' ? 2000 : false;
    },
  });

  if (isLoading) return <Spin />;

  const run = data?.data;

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="large">
      <div>
        <Text strong>{t('run.status')}: </Text>
        <Tag color={run?.status === 'pass' ? 'success' : run?.status === 'fail' ? 'error' : 'processing'}>
          {run?.status?.toUpperCase()}
        </Tag>
      </div>
      <div>
        <Text strong>{t('run.triggeredBy')}: </Text>
        <Text>{run?.triggered_by}</Text>
      </div>
      <div>
        <Text strong>{t('run.triggeredAt')}: </Text>
        <Text>{run?.triggered_at ? new Date(run.triggered_at).toLocaleString() : '-'}</Text>
      </div>
      {run?.failure_summary && (
        <Alert message={t('run.failureSummary')} description={run.failure_summary} type="error" showIcon />
      )}
      {run?.artifacts && (
        <div>
          <Text strong>{t('run.artifacts')}:</Text>
          <ul>
            {run.artifacts.trace && <li><a href={run.artifacts.trace} target="_blank">{t('run.traceData')}</a></li>}
            {run.artifacts.diff && <li><a href={run.artifacts.diff} target="_blank">{t('run.diffReport')}</a></li>}
            {run.artifacts.timing && <li><a href={run.artifacts.timing} target="_blank">{t('run.timingReport')}</a></li>}
            {run.artifacts.diagnosis && <li><a href={run.artifacts.diagnosis} target="_blank">{t('run.diagnosisReport')}</a></li>}
          </ul>
        </div>
      )}
    </Space>
  );
};

export default RunPage;
