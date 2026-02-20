import React, { useState, useEffect } from 'react';
import { Card, Button, Space, Form, Input, message, Typography, Tabs } from 'antd';
import { SaveOutlined, CheckOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { scenarioApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const { Title, Text } = Typography;
const { TextArea } = Input;

const ScenarioPage: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const { currentProject } = useAppStore();
  const [selectedId, setSelectedId] = useState<string>(currentProject || 'two_cylinder');

  useEffect(() => {
    if (currentProject) setSelectedId(currentProject);
  }, [currentProject]);

  const { data: scenarioData } = useQuery({
    queryKey: ['scenario', selectedId],
    queryFn: () => scenarioApi.getScenario(selectedId),
    enabled: !!selectedId,
  });

  const validateMutation = useMutation({
    mutationFn: (scenario: any) => scenarioApi.validateScenario(scenario),
    onSuccess: (response) => {
      if (response.data.valid) {
        message.success(t('scenarioPage.validateSuccess'));
      } else {
        message.error(`${t('scenarioPage.validateFailed')}: ${response.data.errors.join(', ')}`);
      }
    },
  });

  const saveMutation = useMutation({
    mutationFn: (values: { id: string; scenario: any }) =>
      scenarioApi.saveScenario(values.id, values.scenario),
    onSuccess: () => {
      message.success(t('scenarioPage.saveSuccess'));
    },
  });

  const handleValidate = () => {
    const scenario = form.getFieldValue('scenario');
    try {
      const parsed = typeof scenario === 'string' ? JSON.parse(scenario) : scenario;
      validateMutation.mutate(parsed);
    } catch {
      message.error(t('scenarioPage.jsonError'));
    }
  };

  const handleSave = () => {
    const scenario = form.getFieldValue('scenario');
    try {
      const parsed = typeof scenario === 'string' ? JSON.parse(scenario) : scenario;
      saveMutation.mutate({ id: selectedId, scenario: parsed });
    } catch {
      message.error(t('scenarioPage.jsonError'));
    }
  };

  React.useEffect(() => {
    if (scenarioData?.data) {
      const data = scenarioData.data as any;
      form.setFieldsValue({ scenario: data.content ?? JSON.stringify(data, null, 2) });
    }
  }, [scenarioData, form]);

  return (
    <div>
      <Title level={2}>{t('scenarioPage.title')}</Title>

      <Card style={{ marginBottom: 24 }}>
        <Space>
          <Text>{t('dashboard.currentProject')}:</Text>
          <Text strong>{selectedId}</Text>
          <Button type="primary" icon={<CheckOutlined />} onClick={handleValidate} loading={validateMutation.isPending}>
            {t('scenarioPage.validate')}
          </Button>
          <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={saveMutation.isPending}>
            {t('properties.save')}
          </Button>
        </Space>
      </Card>

      <Card>
        <Tabs
          defaultActiveKey="yaml"
          items={[
            {
              key: 'yaml',
              label: 'YAML/JSON',
              children: (
                <Form form={form} layout="vertical">
                  <Form.Item name="scenario" label={`${t('scenarioPage.scenarioFile')}: examples/${selectedId}_scenario.yaml`}>
                    <TextArea rows={25} style={{ fontFamily: 'monospace', fontSize: '13px' }} placeholder={t('scenarioPage.placeholder')} />
                  </Form.Item>
                </Form>
              ),
            },
            {
              key: 'visual',
              label: t('scenarioPage.visualEditor'),
              children: (
                <div style={{ padding: '24px', textAlign: 'center', minHeight: '400px' }}>
                  <Text type="secondary" style={{ fontSize: '16px' }}>{t('scenarioPage.visualEditorWip')}</Text>
                  <br /><br />
                  <Text type="secondary">{t('scenarioPage.visualEditorPlan')}</Text>
                </div>
              ),
            },
          ]}
        />
      </Card>
    </div>
  );
};

export default ScenarioPage;
