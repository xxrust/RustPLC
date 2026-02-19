import React, { useState } from 'react';
import { Card, Button, Form, Input, Space, Table, Tag, Typography, Alert, Spin } from 'antd';
import { PlayCircleOutlined, ReloadOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { runApi } from '../services/api';
import type { RunStatus } from '../types';

const { Title, Text } = Typography;

const RunPage: React.FC = () => {
  const [form] = Form.useForm();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  // 获取运行列表
  const { data: runsData, isLoading, refetch } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runApi.listRuns(20),
    refetchInterval: 5000, // 每5秒刷新
  });

  // 触发运行
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
      title: '运行 ID',
      dataIndex: 'run_id',
      key: 'run_id',
      render: (id: string) => (
        <Text code>{id.slice(0, 12)}</Text>
      ),
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      render: (status: string) => {
        const colorMap = {
          running: 'processing',
          pass: 'success',
          fail: 'error',
        };
        return <Tag color={colorMap[status as keyof typeof colorMap]}>{status.toUpperCase()}</Tag>;
      },
    },
    {
      title: '触发人',
      dataIndex: 'triggered_by',
      key: 'triggered_by',
    },
    {
      title: '触发时间',
      dataIndex: 'triggered_at',
      key: 'triggered_at',
      render: (time: string) => new Date(time).toLocaleString(),
    },
    {
      title: '失败原因',
      dataIndex: 'failure_summary',
      key: 'failure_summary',
      render: (summary?: string) => summary ? <Text type="danger">{summary}</Text> : '-',
    },
    {
      title: '操作',
      key: 'action',
      render: (_: any, record: RunStatus) => (
        <Space>
          <Button size="small" onClick={() => setSelectedRunId(record.run_id)}>
            查看详情
          </Button>
          {record.artifacts?.trace && (
            <Button size="small" type="link" href={record.artifacts.trace} target="_blank">
              Trace
            </Button>
          )}
          {record.artifacts?.diagnosis && (
            <Button size="small" type="link" href={record.artifacts.diagnosis} target="_blank">
              诊断
            </Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Title level={2}>运行监控</Title>

      {/* 触发运行表单 */}
      <Card title="触发 No-Board Gate" style={{ marginBottom: 24 }}>
        <Form
          form={form}
          layout="inline"
          onFinish={handleTrigger}
          initialValues={{
            plcFile: 'examples/demo.plc',
            scenarioFile: 'examples/demo_scenario.yaml',
          }}
        >
          <Form.Item
            name="plcFile"
            label="PLC 文件"
            rules={[{ required: true, message: '请输入 PLC 文件路径' }]}
            style={{ width: 300 }}
          >
            <Input placeholder="examples/demo.plc" />
          </Form.Item>

          <Form.Item
            name="scenarioFile"
            label="场景文件"
            rules={[{ required: true, message: '请输入场景文件路径' }]}
            style={{ width: 300 }}
          >
            <Input placeholder="examples/demo_scenario.yaml" />
          </Form.Item>

          <Form.Item>
            <Button
              type="primary"
              htmlType="submit"
              icon={<PlayCircleOutlined />}
              loading={triggerMutation.isPending}
            >
              运行
            </Button>
          </Form.Item>
        </Form>

        {triggerMutation.isSuccess && (
          <Alert
            message="运行已触发"
            description={`运行 ID: ${triggerMutation.data?.data.run_id}`}
            type="success"
            showIcon
            closable
            style={{ marginTop: 16 }}
          />
        )}

        {triggerMutation.isError && (
          <Alert
            message="运行失败"
            description={triggerMutation.error?.message}
            type="error"
            showIcon
            closable
            style={{ marginTop: 16 }}
          />
        )}
      </Card>

      {/* 运行列表 */}
      <Card
        title="运行记录"
        extra={
          <Button icon={<ReloadOutlined />} onClick={() => refetch()}>
            刷新
          </Button>
        }
      >
        <Table
          dataSource={runsData?.data || []}
          columns={columns}
          rowKey="run_id"
          loading={isLoading}
          pagination={{ pageSize: 10 }}
          rowClassName={(record) =>
            record.run_id === selectedRunId ? 'ant-table-row-selected' : ''
          }
        />
      </Card>

      {/* 运行详情 */}
      {selectedRunId && (
        <Card title={`运行详情: ${selectedRunId.slice(0, 12)}`} style={{ marginTop: 24 }}>
          <RunDetails runId={selectedRunId} />
        </Card>
      )}
    </div>
  );
};

// 运行详情组件
const RunDetails: React.FC<{ runId: string }> = ({ runId }) => {
  const { data, isLoading } = useQuery({
    queryKey: ['runStatus', runId],
    queryFn: () => runApi.getRunStatus(runId),
    refetchInterval: (query) => {
      const status = query.state.data?.data.status;
      return status === 'running' ? 2000 : false;
    },
  });

  if (isLoading) {
    return <Spin />;
  }

  const run = data?.data;

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="large">
      <div>
        <Text strong>状态: </Text>
        <Tag color={run?.status === 'pass' ? 'success' : run?.status === 'fail' ? 'error' : 'processing'}>
          {run?.status?.toUpperCase()}
        </Tag>
      </div>

      <div>
        <Text strong>触发人: </Text>
        <Text>{run?.triggered_by}</Text>
      </div>

      <div>
        <Text strong>触发时间: </Text>
        <Text>{run?.triggered_at ? new Date(run.triggered_at).toLocaleString() : '-'}</Text>
      </div>

      {run?.failure_summary && (
        <Alert message="失败原因" description={run.failure_summary} type="error" showIcon />
      )}

      {run?.artifacts && (
        <div>
          <Text strong>工件:</Text>
          <ul>
            {run.artifacts.trace && <li><a href={run.artifacts.trace} target="_blank">Trace 数据</a></li>}
            {run.artifacts.diff && <li><a href={run.artifacts.diff} target="_blank">Diff 报告</a></li>}
            {run.artifacts.timing && <li><a href={run.artifacts.timing} target="_blank">时序报告</a></li>}
            {run.artifacts.diagnosis && <li><a href={run.artifacts.diagnosis} target="_blank">诊断报告</a></li>}
          </ul>
        </div>
      )}
    </Space>
  );
};

export default RunPage;
