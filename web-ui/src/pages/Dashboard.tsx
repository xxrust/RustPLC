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
import { useTranslation } from 'react-i18next';
import { runApi, alarmApi } from '../services/api';
import { useAppStore } from '../stores/appStore';
import { formatTimestamp } from '../utils/time';

const { Title, Text } = Typography;

const Dashboard: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { runMode, currentProject, alarmCount } = useAppStore();

  const { data: recentRuns, isLoading: runsLoading } = useQuery({
    queryKey: ['recentRuns'],
    queryFn: () => runApi.listRuns(5),
  });

  const { data: recentAlarms, isLoading: alarmsLoading } = useQuery({
    queryKey: ['recentAlarms'],
    queryFn: () => alarmApi.getAlarms({ limit: 5 }),
  });

  const latestRun = recentRuns?.data?.[0];

  const statusIcon = {
    running: <ClockCircleOutlined style={{ color: '#1890ff' }} />,
    pass: <CheckCircleOutlined style={{ color: '#52c41a' }} />,
    fail: <CloseCircleOutlined style={{ color: '#f5222d' }} />,
  };

  return (
    <div>
      <Title level={2}>{t('dashboard.title')}</Title>

      <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
        <Col span={6}>
          <Card>
            <Statistic title={t('dashboard.runMode')} value={t(`runMode.${runMode}`)} valueStyle={{ fontSize: '18px' }} />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic title={t('dashboard.currentProject')} value={currentProject || t('idde.noProjectSelected')} valueStyle={{ fontSize: '16px' }} />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic
              title={t('dashboard.latestRunStatus')}
              value={latestRun?.status || '-'}
              prefix={latestRun ? statusIcon[latestRun.status as keyof typeof statusIcon] : null}
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
              title={t('dashboard.alarmCount')}
              value={alarmCount.critical + alarmCount.warning + alarmCount.info}
              prefix={<WarningOutlined />}
              valueStyle={{
                fontSize: '18px',
                color: alarmCount.critical > 0 ? '#f5222d' : alarmCount.warning > 0 ? '#faad14' : '#52c41a',
              }}
            />
            <div style={{ marginTop: 8 }}>
              <Space size="small">
                <Tag color="error">{t('statusBar.critical')}: {alarmCount.critical}</Tag>
                <Tag color="warning">{t('statusBar.warning')}: {alarmCount.warning}</Tag>
                <Tag color="info">{t('statusBar.info')}: {alarmCount.info}</Tag>
              </Space>
            </div>
          </Card>
        </Col>
      </Row>

      <Card title={t('dashboard.quickAccess')} style={{ marginBottom: 24 }}>
        <Space size="large">
          <Button type="primary" icon={<PlayCircleOutlined />} size="large" onClick={() => navigate('/run')}>
            {t('dashboard.runGate')}
          </Button>
          <Button icon={<ClockCircleOutlined />} size="large" onClick={() => navigate('/replay')}>
            {t('tabs.replay')}
          </Button>
          <Button icon={<WarningOutlined />} size="large" onClick={() => navigate('/diagnosis')}>
            {t('diagnosis.title')}
          </Button>
          <Button size="large" onClick={() => navigate('/audit')}>
            {t('dashboard.auditReport')}
          </Button>
        </Space>
      </Card>

      <Row gutter={[16, 16]}>
        <Col span={12}>
          <Card
            title={t('dashboard.recentRuns')}
            extra={<a onClick={() => navigate('/run')}>{t('dashboard.viewAll')}</a>}
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
                    avatar={statusIcon[run.status as keyof typeof statusIcon]}
                    title={`Run ${run.run_id.slice(0, 8)}`}
                    description={
                      <Space direction="vertical" size="small">
                        <Text type="secondary">{t('run.triggeredBy')}: {run.triggered_by}</Text>
                        <Text type="secondary">
                          {formatTimestamp(run.triggered_at, run.triggered_at_ms)}
                        </Text>
                        {run.failure_summary && <Text type="danger">{run.failure_summary}</Text>}
                      </Space>
                    }
                  />
                </List.Item>
              )}
            />
          </Card>
        </Col>

        <Col span={12}>
          <Card
            title={t('dashboard.recentAlarms')}
            extra={<a onClick={() => navigate('/diagnosis')}>{t('dashboard.viewAll')}</a>}
            loading={alarmsLoading}
          >
            <List
              dataSource={recentAlarms?.data || []}
              renderItem={(alarm: any) => (
                <List.Item
                  actions={[
                    <Tag color={alarm.severity === 'critical' ? 'error' : alarm.severity === 'warning' ? 'warning' : 'info'}>
                      {alarm.severity}
                    </Tag>,
                  ]}
                >
                  <List.Item.Meta
                    avatar={<WarningOutlined style={{ fontSize: 20 }} />}
                    title={alarm.alarm_id}
                    description={
                      <Space direction="vertical" size="small">
                        <Text type="secondary">{new Date(alarm.first_seen_ms).toLocaleString()}</Text>
                        {alarm.top_candidates?.[0] && <Text>{alarm.top_candidates[0].issue_code}</Text>}
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
