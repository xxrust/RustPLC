import React, { useState } from 'react';
import { Card, Button, Space, Table, Tag, Typography, Modal, message } from 'antd';
import { WarningOutlined, CheckCircleOutlined, CloseCircleOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { alarmApi } from '../services/api';
import type { DiagnosisCandidate } from '../types';

const { Title, Text, Paragraph } = Typography;

const DiagnosisPage: React.FC = () => {
  const [selectedAlarm, setSelectedAlarm] = useState<any | null>(null);
  const [detailsVisible, setDetailsVisible] = useState(false);

  // 获取告警列表
  const { data: alarmsData, isLoading } = useQuery({
    queryKey: ['alarms'],
    queryFn: () => alarmApi.getAlarms({ limit: 50 }),
    refetchInterval: 5000,
  });

  const handleViewDetails = (alarm: any) => {
    setSelectedAlarm(alarm);
    setDetailsVisible(true);
  };

  const handleAcknowledge = async (alarmId: string) => {
    try {
      await alarmApi.acknowledgeAlarm(alarmId, '已确认');
      message.success('告警已确认');
    } catch (error) {
      message.error('确认失败');
    }
  };

  const columns = [
    {
      title: '严重程度',
      dataIndex: 'severity',
      key: 'severity',
      render: (severity: string) => {
        const config = {
          critical: { color: 'error', icon: <CloseCircleOutlined /> },
          warning: { color: 'warning', icon: <WarningOutlined /> },
          info: { color: 'info', icon: <CheckCircleOutlined /> },
        };
        const { color, icon } = config[severity as keyof typeof config] || config.info;
        return (
          <Tag color={color} icon={icon}>
            {severity.toUpperCase()}
          </Tag>
        );
      },
      filters: [
        { text: 'Critical', value: 'critical' },
        { text: 'Warning', value: 'warning' },
        { text: 'Info', value: 'info' },
      ],
      onFilter: (value: any, record: any) => record.severity === value,
    },
    {
      title: '告警 ID',
      dataIndex: 'alarm_id',
      key: 'alarm_id',
      render: (id: string) => <Text code>{id}</Text>,
    },
    {
      title: '首次发现',
      dataIndex: 'first_seen_ms',
      key: 'first_seen_ms',
      render: (ms: number) => new Date(ms).toLocaleString(),
      sorter: (a: any, b: any) => a.first_seen_ms - b.first_seen_ms,
    },
    {
      title: '场景/配方',
      dataIndex: 'scenario_or_recipe_id',
      key: 'scenario_or_recipe_id',
    },
    {
      title: '证据来源',
      dataIndex: 'evidence_source',
      key: 'evidence_source',
      render: (source: string) => {
        const colorMap = {
          no_board: 'blue',
          hil_board: 'orange',
          runtime_live: 'green',
          mixed: 'purple',
        };
        return <Tag color={colorMap[source as keyof typeof colorMap]}>{source}</Tag>;
      },
    },
    {
      title: '操作',
      key: 'action',
      render: (_: any, record: any) => (
        <Space>
          <Button size="small" onClick={() => handleViewDetails(record)}>
            查看详情
          </Button>
          <Button size="small" type="primary" onClick={() => handleAcknowledge(record.alarm_id)}>
            确认
          </Button>
        </Space>
      ),
    },
  ];

  // 按严重程度统计
  const alarms = alarmsData?.data || [];
  const stats = {
    critical: alarms.filter((a: any) => a.severity === 'critical').length,
    warning: alarms.filter((a: any) => a.severity === 'warning').length,
    info: alarms.filter((a: any) => a.severity === 'info').length,
  };

  return (
    <div>
      <Title level={2}>诊断中心</Title>

      {/* 统计卡片 */}
      <Space style={{ marginBottom: 24, width: '100%' }} size="large">
        <Card>
          <Tag color="error" icon={<CloseCircleOutlined />}>
            严重: {stats.critical}
          </Tag>
        </Card>
        <Card>
          <Tag color="warning" icon={<WarningOutlined />}>
            警告: {stats.warning}
          </Tag>
        </Card>
        <Card>
          <Tag color="info" icon={<CheckCircleOutlined />}>
            信息: {stats.info}
          </Tag>
        </Card>
      </Space>

      {/* 告警列表 */}
      <Card title="告警列表">
        <Table
          dataSource={alarms}
          columns={columns}
          rowKey="alarm_id"
          loading={isLoading}
          pagination={{ pageSize: 20 }}
        />
      </Card>

      {/* 详情弹窗 */}
      <Modal
        title={`告警详情: ${selectedAlarm?.alarm_id}`}
        open={detailsVisible}
        onCancel={() => setDetailsVisible(false)}
        footer={[
          <Button key="close" onClick={() => setDetailsVisible(false)}>
            关闭
          </Button>,
          <Button
            key="ack"
            type="primary"
            onClick={() => {
              handleAcknowledge(selectedAlarm?.alarm_id);
              setDetailsVisible(false);
            }}
          >
            确认告警
          </Button>,
        ]}
        width={800}
      >
        {selectedAlarm && <AlarmDetails alarm={selectedAlarm} />}
      </Modal>
    </div>
  );
};

// 告警详情组件
const AlarmDetails: React.FC<{ alarm: any }> = ({ alarm }) => {
  return (
    <Space direction="vertical" style={{ width: '100%' }} size="large">
      <div>
        <Text strong>严重程度: </Text>
        <Tag
          color={
            alarm.severity === 'critical'
              ? 'error'
              : alarm.severity === 'warning'
              ? 'warning'
              : 'info'
          }
        >
          {alarm.severity.toUpperCase()}
        </Tag>
      </div>

      <div>
        <Text strong>首次发现: </Text>
        <Text>{new Date(alarm.first_seen_ms).toLocaleString()}</Text>
      </div>

      <div>
        <Text strong>场景/配方 ID: </Text>
        <Text code>{alarm.scenario_or_recipe_id}</Text>
      </div>

      <div>
        <Text strong>证据来源: </Text>
        <Tag>{alarm.evidence_source}</Tag>
      </div>

      <div>
        <Text strong>证据引用: </Text>
        <Text code>{alarm.evidence_ref}</Text>
      </div>

      {/* 诊断候选项 */}
      {alarm.top_candidates && alarm.top_candidates.length > 0 && (
        <div>
          <Title level={5}>诊断候选项</Title>
          {alarm.top_candidates.map((candidate: DiagnosisCandidate, index: number) => (
            <Card key={index} size="small" style={{ marginBottom: 16 }}>
              <Space direction="vertical" style={{ width: '100%' }}>
                <div>
                  <Text strong>问题代码: </Text>
                  <Text code>{candidate.issue_code}</Text>
                  <Tag style={{ marginLeft: 8 }}>排名: {candidate.rank}</Tag>
                  <Tag color="blue">置信度: {(candidate.confidence * 100).toFixed(0)}%</Tag>
                </div>

                <div>
                  <Text strong>类别: </Text>
                  <Tag>{candidate.category}</Tag>
                </div>

                <div>
                  <Text strong>证据:</Text>
                  <ul>
                    {candidate.evidence.map((e, i) => (
                      <li key={i}>{e}</li>
                    ))}
                  </ul>
                </div>

                <div>
                  <Text strong>建议修复: </Text>
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
