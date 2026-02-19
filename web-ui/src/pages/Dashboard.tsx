import React from 'react';
import { Card, Row, Col, Statistic, Tag, Button, Space, List, Typography } from 'antd';
import {
  PlayCircleOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  ClockCircleOutlined,
  WarningOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { runApi, alarmApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const { Title, Text } = Typography;

const Dashboard: React.FC = () => {
  const navigate = useNavigate();
  const { runMode, currentProject, alarmCount } = useAppStore();

  // 获取最近运行记录
  const { data: recentRuns, isLoading: runsLoading } = useQuery({
    queryKey: ['recentRuns'],
    queryFn: () => runApi.listRuns(5),
  });

  // 获取最新告警
  const { data: recentAlarms, isLoading: alarmsLoading } = useQuery({
    queryKey: ['recentAlarms'],
    queryFn: () => alarmApi.getAlarms({ limit: 5 }),
  });

  const latestRun = recentRuns?.data?.[0];

  const runModeLabel = {
    no_board: 'No-Board 模式',
    hil_board: 'HIL 模式',
    runtime_live: '实时运行',
  }[runMode];

  const statusIcon = {
    running: <ClockCircleOutlined style={{ color: '#1890ff' }} />,
    pass: <CheckCircleOutlined style={{ color: '#52c41a' }} />,
    fail: <CloseCircleOutlined style={{ color: '#f5222d' }} />,
  };

  return (
    <div>
      <Title level={2}>总览看板</Title>

      {/* 顶部统计卡片 */}
      <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
        <Col span={6}>
          <Card>
            <Statistic
              title="运行模式"
              value={runModeLabel}
              valueStyle={{ fontSize: '18px' }}
            />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic
              title="当前项目"
              value={currentProject || '未选择'}
              valueStyle={{ fontSize: '16px' }}
            />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic
              title="最新运行状态"
              value={latestRun?.status || '-'}
              prefix={latestRun ? statusIcon[latestRun.status] : null}
              valueStyle={{
                fontSize: '18px',
                color: latestRun?.status === 'pass' ? '#52c41a' : latestRun?.status === 'fail' ? '#f5222d' : '#1890ff',
              }}
            />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic
              title="告警数量"
              value={alarmCount.critical + alarmCount.warning + alarmCount.info}
              prefix={<WarningOutlined />}
              valueStyle={{
                fontSize: '18px',
                color: alarmCount.critical > 0 ? '#f5222d' : alarmCount.warning > 0 ? '#faad14' : '#52c41a',
              }}
            />
            <div style={{ marginTop: 8 }}>
              <Space size="small">
                <Tag color="error">严重: {alarmCount.critical}</Tag>
                <Tag color="warning">警告: {alarmCount.warning}</Tag>
                <Tag color="info">信息: {alarmCount.info}</Tag>
              </Space>
            </div>
          </Card>
        </Col>
      </Row>

      {/* 快速入口 */}
      <Card title="快速入口" style={{ marginBottom: 24 }}>
        <Space size="large">
          <Button
            type="primary"
            icon={<PlayCircleOutlined />}
            size="large"
            onClick={() => navigate('/run')}
          >
            运行门禁
          </Button>
          <Button
            icon={<ClockCircleOutlined />}
            size="large"
            onClick={() => navigate('/replay')}
          >
            Tick 回放
          </Button>
          <Button
            icon={<WarningOutlined />}
            size="large"
            onClick={() => navigate('/diagnosis')}
          >
            诊断中心
          </Button>
          <Button
            size="large"
            onClick={() => navigate('/audit')}
          >
            审计报告
          </Button>
        </Space>
      </Card>

      <Row gutter={[16, 16]}>
        {/* 最近运行记录 */}
        <Col span={12}>
          <Card
            title="最近运行记录"
            extra={<a onClick={() => navigate('/run')}>查看全部</a>}
            loading={runsLoading}
          >
            <List
              dataSource={recentRuns?.data || []}
              renderItem={(run) => (
                <List.Item
                  actions={[
                    <Tag color={run.status === 'pass' ? 'success' : run.status === 'fail' ? 'error' : 'processing'}>
                      {run.status}
                    </Tag>,
                  ]}
                >
                  <List.Item.Meta
                    avatar={statusIcon[run.status]}
                    title={`Run ${run.run_id.slice(0, 8)}`}
                    description={
                      <Space direction="vertical" size="small">
                        <Text type="secondary">触发人: {run.triggered_by}</Text>
                        <Text type="secondary">时间: {new Date(run.triggered_at).toLocaleString()}</Text>
                        {run.failure_summary && (
                          <Text type="danger">{run.failure_summary}</Text>
                        )}
                      </Space>
                    }
                  />
                </List.Item>
              )}
            />
          </Card>
        </Col>

        {/* 最新告警 */}
        <Col span={12}>
          <Card
            title="最新告警"
            extra={<a onClick={() => navigate('/diagnosis')}>查看全部</a>}
            loading={alarmsLoading}
          >
            <List
              dataSource={recentAlarms?.data || []}
              renderItem={(alarm: any) => (
                <List.Item
                  actions={[
                    <Tag
                      color={
                        alarm.severity === 'critical'
                          ? 'error'
                          : alarm.severity === 'warning'
                          ? 'warning'
                          : 'info'
                      }
                    >
                      {alarm.severity}
                    </Tag>,
                  ]}
                >
                  <List.Item.Meta
                    avatar={<WarningOutlined style={{ fontSize: 20 }} />}
                    title={alarm.alarm_id}
                    description={
                      <Space direction="vertical" size="small">
                        <Text type="secondary">
                          {new Date(alarm.first_seen_ms).toLocaleString()}
                        </Text>
                        {alarm.top_candidates?.[0] && (
                          <Text>{alarm.top_candidates[0].issue_code}</Text>
                        )}
                      </Space>
                    }
                  />
                </List.Item>
              )}
            />
          </Card>
        </Col>
      </Row>
    </div>
  );
};

export default Dashboard;
