import React, { useState, useEffect } from 'react';
import { Card, Button, Space, Form, Input, message, Typography, Tabs } from 'antd';
import { SaveOutlined, CheckOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { scenarioApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const { Title, Text } = Typography;
const { TextArea } = Input;

const ScenarioPage: React.FC = () => {
  const [form] = Form.useForm();
  const { currentProject } = useAppStore();
  const [selectedId, setSelectedId] = useState<string>(currentProject || 'two_cylinder');

  // Update selectedId when currentProject changes
  useEffect(() => {
    if (currentProject) {
      setSelectedId(currentProject);
    }
  }, [currentProject]);

  // 获取场景
  const { data: scenarioData } = useQuery({
    queryKey: ['scenario', selectedId],
    queryFn: () => scenarioApi.getScenario(selectedId),
    enabled: !!selectedId,
  });

  // 验证场景
  const validateMutation = useMutation({
    mutationFn: (scenario: any) => scenarioApi.validateScenario(scenario),
    onSuccess: (response) => {
      if (response.data.valid) {
        message.success('场景验证通过');
      } else {
        message.error(`验证失败: ${response.data.errors.join(', ')}`);
      }
    },
  });

  // 保存场景
  const saveMutation = useMutation({
    mutationFn: (values: { id: string; scenario: any }) =>
      scenarioApi.saveScenario(values.id, values.scenario),
    onSuccess: () => {
      message.success('场景已保存');
    },
  });

  const handleValidate = () => {
    const scenario = form.getFieldValue('scenario');
    try {
      const parsed = typeof scenario === 'string' ? JSON.parse(scenario) : scenario;
      validateMutation.mutate(parsed);
    } catch (error) {
      message.error('JSON 格式错误');
    }
  };

  const handleSave = () => {
    const scenario = form.getFieldValue('scenario');
    try {
      const parsed = typeof scenario === 'string' ? JSON.parse(scenario) : scenario;
      saveMutation.mutate({ id: selectedId, scenario: parsed });
    } catch (error) {
      message.error('JSON 格式错误');
    }
  };

  React.useEffect(() => {
    if (scenarioData?.data) {
      const data = scenarioData.data as any;
      // If it has content field, show it directly
      if (data.content) {
        form.setFieldsValue({ scenario: data.content });
      } else {
        form.setFieldsValue({
          scenario: JSON.stringify(data, null, 2),
        });
      }
    }
  }, [scenarioData, form]);

  return (
    <div>
      <Title level={2}>场景管理器</Title>

      <Card style={{ marginBottom: 24 }}>
        <Space>
          <Text>当前项目:</Text>
          <Text strong>{selectedId}</Text>

          <Button
            type="primary"
            icon={<CheckOutlined />}
            onClick={handleValidate}
            loading={validateMutation.isPending}
          >
            验证
          </Button>

          <Button
            type="primary"
            icon={<SaveOutlined />}
            onClick={handleSave}
            loading={saveMutation.isPending}
          >
            保存
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
                  <Form.Item name="scenario" label={`场景文件: examples/${selectedId}_scenario.yaml`}>
                    <TextArea
                      rows={25}
                      style={{ fontFamily: 'monospace', fontSize: '13px' }}
                      placeholder="场景 YAML 或 JSON..."
                    />
                  </Form.Item>
                </Form>
              ),
            },
            {
              key: 'visual',
              label: '可视化编辑（开发中）',
              children: (
                <div style={{ padding: '24px', textAlign: 'center', minHeight: '400px' }}>
                  <Text type="secondary" style={{ fontSize: '16px' }}>
                    可视化场景编辑器开发中...
                  </Text>
                  <br />
                  <br />
                  <Text type="secondary">
                    功能规划：时间线编辑、事件拖拽、故障注入配置
                  </Text>
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
