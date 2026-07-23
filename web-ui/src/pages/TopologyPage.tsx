import React from 'react';
import { Card, Button, Empty, Form, Input, Space, Tabs, Typography, message } from 'antd';
import { SaveOutlined, CheckOutlined, FolderOpenOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { topologyApi } from '../services/api';
import { DEFAULT_PROJECT_ID, useAppStore } from '../stores/appStore';
import type { ComponentTopology } from '../types';

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

const TopologyPage: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const { currentProject, setCurrentProject } = useAppStore();
  const selectedId = currentProject || '';

  const { data: topologyData } = useQuery({
    queryKey: ['topology', selectedId],
    queryFn: () => topologyApi.getTopology(selectedId),
    enabled: Boolean(selectedId),
  });

  const validateMutation = useMutation({
    mutationFn: (topology: ComponentTopology) => topologyApi.validateTopology(topology),
    onSuccess: (response) => {
      if (response.data.valid) {
        message.success(t('topologyPage.validateSuccess'));
      } else {
        message.error(`${t('topologyPage.validateFailed')}: ${response.data.errors.join(', ')}`);
      }
    },
  });

  const saveMutation = useMutation({
    mutationFn: (values: { id: string; topology: ComponentTopology }) =>
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

  const handleLoadExample = () => {
    const exampleId = DEFAULT_PROJECT_ID;
    setCurrentProject(exampleId, null, null);
    message.success(t('topologyPage.exampleLoaded'));
  };

  React.useEffect(() => {
    if (topologyData?.data) {
      const data = topologyData.data as unknown as Record<string, unknown>;
      const content = typeof data.content === 'string' ? data.content : JSON.stringify(data, null, 2);
      form.setFieldsValue({ topology: content });
    } else {
      form.resetFields();
    }
  }, [topologyData, form]);

  return (
    <div style={{ display: 'grid', gap: 24 }}>
      <div>
        <Title level={2} style={{ marginBottom: 8 }}>
          {t('topologyPage.title')}
        </Title>
        <Paragraph style={{ color: '#94a3b8', marginBottom: 0 }}>
          {t('topologyPage.intro')}
        </Paragraph>
      </div>

      <Card>
        <Space>
          <Text>{t('common.currentProject')}:</Text>
          <Text strong>{selectedId || t('common.noneSelected')}</Text>
          <Button icon={<FolderOpenOutlined />} onClick={handleLoadExample}>
            {t('topologyPage.loadExample')}
          </Button>
          <Button
            type="primary"
            icon={<CheckOutlined />}
            onClick={handleValidate}
            loading={validateMutation.isPending}
            disabled={!selectedId}
          >
            {t('topologyPage.validate')}
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
            defaultActiveKey="source"
            items={[
              {
                key: 'source',
                label: t('topologyPage.sourceTab'),
                children: (
                  <Form form={form} layout="vertical">
                    <Form.Item name="topology" label={t('topologyPage.sourceLabel')}>
                      <TextArea
                        rows={25}
                        style={{ fontFamily: 'monospace', fontSize: '13px' }}
                        placeholder={t('topologyPage.placeholder')}
                      />
                    </Form.Item>
                  </Form>
                ),
              },
              {
                key: 'visual',
                label: t('topologyPage.visualEditor'),
                children: (
                  <div style={{ padding: '24px', textAlign: 'center', minHeight: '240px' }}>
                    <Text type="secondary" style={{ fontSize: '16px' }}>
                      {t('topologyPage.visualEditorUnavailable')}
                    </Text>
                    <br />
                    <br />
                    <Text type="secondary">{t('topologyPage.visualEditorUseSource')}</Text>
                  </div>
                ),
              },
            ]}
          />
        ) : (
          <Empty description={t('topologyPage.emptyPrompt')} image={Empty.PRESENTED_IMAGE_SIMPLE}>
            <Space direction="vertical" size={8} style={{ alignItems: 'center' }}>
              <Text type="secondary">{t('topologyPage.emptyActionHint')}</Text>
              <Button type="primary" icon={<FolderOpenOutlined />} onClick={handleLoadExample}>
                {t('topologyPage.loadExample')}
              </Button>
            </Space>
          </Empty>
        )}
      </Card>
    </div>
  );
};

export default TopologyPage;
