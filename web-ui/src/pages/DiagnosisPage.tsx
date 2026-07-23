import React, { useState } from 'react';
import { Card, Button, Space, Table, Tag, Typography, Modal, message } from 'antd';
import type { TableColumnsType } from 'antd';
import { WarningOutlined, CheckCircleOutlined, CloseCircleOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { alarmApi } from '../services/api';
import type { AlarmEvent, DiagnosisCandidate, EvidenceSource } from '../types';

const { Title, Text, Paragraph } = Typography;

const DiagnosisPage: React.FC = () => {
  const { t } = useTranslation();
  const [selectedAlarm, setSelectedAlarm] = useState<AlarmEvent | null>(null);
  const [detailsVisible, setDetailsVisible] = useState(false);

  const { data: alarmsData, isLoading } = useQuery({
    queryKey: ['alarms'],
    queryFn: () => alarmApi.getAlarms({ limit: 50 }),
    refetchInterval: 5000,
  });

  const handleViewDetails = (alarm: AlarmEvent) => {
    setSelectedAlarm(alarm);
    setDetailsVisible(true);
  };

  const handleAcknowledge = async (alarmId: string) => {
    try {
      await alarmApi.acknowledgeAlarm(alarmId, t('diagnosis.acknowledged'));
      message.success(t('diagnosis.ackSuccess'));
    } catch {
      message.error(t('diagnosis.ackFailed'));
    }
  };

  const columns: TableColumnsType<AlarmEvent> = [
    {
      title: t('diagnosis.severity'),
      dataIndex: 'severity',
      key: 'severity',
      render: (severity: string) => {
        const config = {
          critical: { color: 'error', icon: <CloseCircleOutlined /> },
          warning: { color: 'warning', icon: <WarningOutlined /> },
          info: { color: 'info', icon: <CheckCircleOutlined /> },
        };
        const { color, icon } = config[severity as keyof typeof config] || config.info;
        return <Tag color={color} icon={icon}>{severity.toUpperCase()}</Tag>;
      },
      filters: [
        { text: t('statusBar.critical'), value: 'critical' },
        { text: t('statusBar.warning'), value: 'warning' },
        { text: t('statusBar.info'), value: 'info' },
      ],
      onFilter: (value, record) => record.severity === value,
    },
    {
      title: t('diagnosis.alarmId'),
      dataIndex: 'alarm_id',
      key: 'alarm_id',
      render: (id: string) => <Text code>{id}</Text>,
    },
    {
      title: t('diagnosis.firstSeen'),
      dataIndex: 'first_seen_ms',
      key: 'first_seen_ms',
      render: (ms: number) => new Date(ms).toLocaleString(),
      sorter: (a, b) => a.first_seen_ms - b.first_seen_ms,
    },
    {
      title: t('diagnosis.scenario'),
      dataIndex: 'scenario_or_recipe_id',
      key: 'scenario_or_recipe_id',
    },
    {
      title: t('diagnosis.evidenceSource'),
      dataIndex: 'evidence_source',
      key: 'evidence_source',
      render: (source: EvidenceSource) => {
        const colorMap: Record<EvidenceSource, string> = {
          no_board: 'blue',
          hil_board: 'orange',
          runtime_live: 'green',
          mixed: 'purple',
        };
        return <Tag color={colorMap[source]}>{source}</Tag>;
      },
    },
    {
      title: t('diagnosis.actions'),
      key: 'action',
      render: (_: unknown, record) => (
        <Space>
          <Button size="small" onClick={() => handleViewDetails(record)}>{t('diagnosis.viewDetails')}</Button>
          <Button size="small" type="primary" onClick={() => handleAcknowledge(record.alarm_id)}>{t('diagnosis.acknowledge')}</Button>
        </Space>
      ),
    },
  ];

  const alarms = alarmsData?.data ?? [];
  const stats = {
    critical: alarms.filter((alarm) => alarm.severity === 'critical').length,
    warning: alarms.filter((alarm) => alarm.severity === 'warning').length,
    info: alarms.filter((alarm) => alarm.severity === 'info').length,
  };

  return (
    <div>
      <Title level={2}>{t('diagnosis.title')}</Title>

      <Space style={{ marginBottom: 24, width: '100%' }} size="large">
        <Card>
          <Tag color="error" icon={<CloseCircleOutlined />}>{t('statusBar.critical')}: {stats.critical}</Tag>
        </Card>
        <Card>
          <Tag color="warning" icon={<WarningOutlined />}>{t('statusBar.warning')}: {stats.warning}</Tag>
        </Card>
        <Card>
          <Tag color="info" icon={<CheckCircleOutlined />}>{t('statusBar.info')}: {stats.info}</Tag>
        </Card>
      </Space>

      <Card title={t('diagnosis.alarmList')}>
        <Table
          dataSource={alarms}
          columns={columns}
          rowKey="alarm_id"
          loading={isLoading}
          locale={{ emptyText: t('diagnosis.noData') }}
          pagination={{ pageSize: 20 }}
        />
      </Card>

      <Modal
        title={`${t('diagnosis.alarmDetails')}: ${selectedAlarm?.alarm_id}`}
        open={detailsVisible}
        onCancel={() => setDetailsVisible(false)}
        footer={[
          <Button key="close" onClick={() => setDetailsVisible(false)}>{t('common.cancel')}</Button>,
          <Button
            key="ack"
            type="primary"
            disabled={!selectedAlarm}
            onClick={() => {
              if (selectedAlarm) {
                void handleAcknowledge(selectedAlarm.alarm_id);
              }
              setDetailsVisible(false);
            }}
          >
            {t('diagnosis.acknowledgeAlarm')}
          </Button>,
        ]}
        width={800}
      >
        {selectedAlarm && <AlarmDetails alarm={selectedAlarm} />}
      </Modal>
    </div>
  );
};

const AlarmDetails: React.FC<{ alarm: AlarmEvent }> = ({ alarm }) => {
  const { t } = useTranslation();
  return (
    <Space direction="vertical" style={{ width: '100%' }} size="large">
      <div>
        <Text strong>{t('diagnosis.severity')}: </Text>
        <Tag color={alarm.severity === 'critical' ? 'error' : alarm.severity === 'warning' ? 'warning' : 'info'}>
          {alarm.severity.toUpperCase()}
        </Tag>
      </div>
      <div>
        <Text strong>{t('diagnosis.firstSeen')}: </Text>
        <Text>{new Date(alarm.first_seen_ms).toLocaleString()}</Text>
      </div>
      <div>
        <Text strong>{t('diagnosis.scenario')}: </Text>
        <Text code>{alarm.scenario_or_recipe_id}</Text>
      </div>
      <div>
        <Text strong>{t('diagnosis.evidenceSource')}: </Text>
        <Tag>{alarm.evidence_source}</Tag>
      </div>
      <div>
        <Text strong>{t('diagnosis.evidenceRef')}: </Text>
        <Text code>{alarm.evidence_ref}</Text>
      </div>
      {alarm.top_candidates && alarm.top_candidates.length > 0 && (
        <div>
          <Title level={5}>{t('diagnosis.candidates')}</Title>
          {alarm.top_candidates.map((candidate: DiagnosisCandidate, index: number) => (
            <Card key={index} size="small" style={{ marginBottom: 16 }}>
              <Space direction="vertical" style={{ width: '100%' }}>
                <div>
                  <Text strong>{t('diagnosis.issueCode')}: </Text>
                  <Text code>{candidate.issue_code}</Text>
                  <Tag style={{ marginLeft: 8 }}>{t('diagnosis.rank')}: {candidate.rank}</Tag>
                  <Tag color="blue">{t('diagnosis.confidence')}: {(candidate.confidence * 100).toFixed(0)}%</Tag>
                </div>
                <div>
                  <Text strong>{t('diagnosis.category')}: </Text>
                  <Tag>{candidate.category}</Tag>
                </div>
                <div>
                  <Text strong>{t('diagnosis.evidence')}:</Text>
                  <ul>{candidate.evidence.map((e, i) => <li key={i}>{e}</li>)}</ul>
                </div>
                <div>
                  <Text strong>{t('diagnosis.suggestedFix')}: </Text>
                  <Paragraph>{candidate.suggested_fix}</Paragraph>
                </div>
              </Space>
            </Card>
          ))}
        </div>
      )}
    </Space>
  );
};

export default DiagnosisPage;
