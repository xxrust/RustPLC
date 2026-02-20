import React, { useState, useEffect } from 'react';
import { Card, Button, Space, Form, Input, message, Typography, Tabs } from 'antd';
import { SaveOutlined, CheckOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { topologyApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const { Title, Text } = Typography;
const { TextArea } = Input;

const TopologyPage: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const { currentProject } = useAppStore();
  const [selectedId, setSelectedId] = useState<string>(currentProject || 'two_cylinder');

  useEffect(() => {
    if (currentProject) setSelectedId(currentProject);
  }, [currentProject]);

  const { data: topologyData } = useQuery({
    queryKey: ['topology', selectedId],
    queryFn: () => topologyApi.getTopology(selectedId),
    enabled: !!selectedId,
  });

  const validateMutation = useMutation({
    mutationFn: (topology: any) => topologyApi.validateTopology(topology),
    onSuccess: (response) => {
      if (response.data.valid) {
        message.success(t('topologyPage.validateSuccess'));
      } else {
        message.error(`${t('topologyPage.validateFailed')}: ${response.data.errors.join(', ')}`);
      }
    },
  });

  const saveMutation = useMutation({
    mutationFn: (values: { id: string; topology: any }) =>
      topologyApi.saveTopology(values.id, values.topology),
    onSuccess: () => {
      message.success(t('topologyPage.saveSuccess'));
    },
  });

  const handleValidate = () => {
    const topology = form.getFieldValue('topology');
    try {
      const parsed = typeof topology === 'string' ? JSON.parse(topology) : topology;
      validateMutation.mutate(parsed);
    } catch {
      message.error(t('topologyPage.jsonError'));
    }
  };

  const handleSave = () => {
    const topology = form.getFieldValue('topology');
    try {
      const parsed = typeof topology === 'string' ? JSON.parse(topology) : topology;
      saveMutation.mutate({ id: selectedId, topology: parsed });
    } catch {
      message.error(t('topologyPage.jsonError'));
    }
  };

  React.useEffect(() => {
    if (topologyData?.data) {
      const data = topologyData.data as any;
      form.setFieldsValue({ topology: data.content ?? JSON.stringify(data, null, 2) });
    }
  }, [topologyData, form]);

  return (
    <div>
      <Title level={2}>{t('topologyPage.title')}</Title>

      <Card style={{ marginBottom: 24 }}>
        <Space>
          <Text>{t('dashboard.currentProject')}:</Text>
          <Text strong>{selectedId}</Text>
          <Button type="primary" icon={<CheckOutlined />} onClick={handleValidate} loading={validateMutation.isPending}>
            {t('topologyPage.validate')}
          </Button>
          <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={saveMutation.isPending}>
            {t('properties.save')}
          </Button>
        </Space>
      </Card>

      <Card>
        <Tabs
          defaultActiveKey="plc"
          items={[
            {
              key: 'plc',
              label: t('topologyPage.plcCode'),
              children: (
                <Form form={form} layout="vertical">
                  <Form.Item name="topology" label={`${t('topologyPage.plcFile')}: examples/${selectedId}.plc`}>
                    <TextArea rows={25} style={{ fontFamily: 'monospace', fontSize: '13px' }} placeholder={t('topologyPage.placeholder')} />
                  </Form.Item>
                </Form>
              ),
            },
            {
              key: 'visual',
              label: t('topologyPage.visualEditor'),
              children: (
                <div style={{ padding: '24px', textAlign: 'center', minHeight: '400px' }}>
                  <Text type="secondary" style={{ fontSize: '16px' }}>{t('topologyPage.visualEditorWip')}</Text>
                  <br /><br />
                  <Text type="secondary">{t('topologyPage.visualEditorPlan')}</Text>
                </div>
              ),
            },
          ]}
        />
      </Card>
    </div>
  );
};

export default TopologyPage;
