import React from 'react';
import { Card, Button, Empty, Form, Input, Space, Tabs, Typography, message } from 'antd';
import { SaveOutlined, CheckOutlined, FolderOpenOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { scenarioApi } from '../services/api';
import { DEFAULT_PROJECT_ID, useAppStore } from '../stores/appStore';
import type { ComponentScenario } from '../types';

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

const ScenarioPage: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const { currentProject, setCurrentProject } = useAppStore();
  const selectedId = currentProject || '';

  const { data: scenarioData } = useQuery({
    queryKey: ['scenario', selectedId],
    queryFn: () => scenarioApi.getScenario(selectedId),
    enabled: Boolean(selectedId),
  });

  const validateMutation = useMutation({
    mutationFn: (scenario: ComponentScenario) => scenarioApi.validateScenario(scenario),
    onSuccess: (response) => {
      if (response.data.valid) {
        message.success(t('scenarioPage.validateSuccess'));
      } else {
        message.error(`${t('scenarioPage.validateFailed')}: ${response.data.errors.join(', ')}`);
      }
    },
  });

  const saveMutation = useMutation({
    mutationFn: (values: { id: string; scenario: ComponentScenario }) =>
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

  const handleLoadExample = () => {
    const exampleId = DEFAULT_PROJECT_ID;
    setCurrentProject(exampleId, null, null);
    message.success(t('scenarioPage.exampleLoaded'));
  };

  React.useEffect(() => {
    if (scenarioData?.data) {
      const data = scenarioData.data as unknown as Record<string, unknown>;
      const content = typeof data.content === 'string' ? data.content : JSON.stringify(data, null, 2);
      form.setFieldsValue({ scenario: content });
    } else {
      form.resetFields();
    }
  }, [scenarioData, form]);

  return (
    <div style={{ display: 'grid', gap: 24 }}>
      <div>
        <Title level={2} style={{ marginBottom: 8 }}>
          {t('scenarioPage.title')}
        </Title>
        <Paragraph style={{ color: '#94a3b8', marginBottom: 0 }}>
          {t('scenarioPage.intro')}
        </Paragraph>
      </div>

      <Card>
        <Space>
          <Text>{t('common.currentProject')}:</Text>
          <Text strong>{selectedId || t('common.noneSelected')}</Text>
          <Button icon={<FolderOpenOutlined />} onClick={handleLoadExample}>
            {t('scenarioPage.loadExample')}
          </Button>
          <Button
            type="primary"
            icon={<CheckOutlined />}
            onClick={handleValidate}
            loading={validateMutation.isPending}
            disabled={!selectedId}
          >
            {t('scenarioPage.validate')}
          </Button>
          <Button
            type="primary"
            icon={<SaveOutlined />}
            onClick={handleSave}
            loading={saveMutation.isPending}
            disabled={!selectedId}
          >
            {t('properties.save')}
          </Button>
        </Space>
      </Card>

      <Card>
        {selectedId ? (
          <Tabs
            defaultActiveKey="yaml"
            items={[
              {
                key: 'yaml',
                label: t('scenarioPage.sourceTab'),
                children: (
                  <Form form={form} layout="vertical">
                    <Form.Item name="scenario" label={t('scenarioPage.sourceLabel')}>
                      <TextArea rows={25} style={{ fontFamily: 'monospace', fontSize: '13px' }} placeholder={t('scenarioPage.placeholder')} />
                    </Form.Item>
                  </Form>
                ),
              },
              {
                key: 'visual',
                label: t('scenarioPage.visualEditor'),
                children: (
                  <div style={{ padding: '24px', textAlign: 'center', minHeight: '240px' }}>
                    <Text type="secondary" style={{ fontSize: '16px' }}>
                      {t('scenarioPage.visualEditorUnavailable')}
                    </Text>
                    <br />
                    <br />
                    <Text type="secondary">{t('scenarioPage.visualEditorUseSource')}</Text>
                  </div>
                ),
              },
            ]}
          />
        ) : (
          <Empty description={t('scenarioPage.emptyPrompt')} image={Empty.PRESENTED_IMAGE_SIMPLE}>
            <Space direction="vertical" size={8} style={{ alignItems: 'center' }}>
              <Text type="secondary">{t('scenarioPage.emptyActionHint')}</Text>
              <Button type="primary" icon={<FolderOpenOutlined />} onClick={handleLoadExample}>
                {t('scenarioPage.loadExample')}
              </Button>
            </Space>
          </Empty>
        )}
      </Card>
    </div>
  );
};

export default ScenarioPage;
