import React, { useState, useEffect } from 'react';
import { Card, Button, Space, Form, Input, message, Typography, Tabs } from 'antd';
import { SaveOutlined, CheckOutlined } from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { topologyApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const { Title, Text } = Typography;
const { TextArea } = Input;

const TopologyPage: React.FC = () => {
  const [form] = Form.useForm();
  const { currentProject } = useAppStore();
  const [selectedId, setSelectedId] = useState<string>(currentProject || 'two_cylinder');

  // Update selectedId when currentProject changes
  useEffect(() => {
    if (currentProject) {
      setSelectedId(currentProject);
    }
  }, [currentProject]);

  // 获取拓扑
  const { data: topologyData } = useQuery({
    queryKey: ['topology', selectedId],
    queryFn: () => topologyApi.getTopology(selectedId),
    enabled: !!selectedId,
  });

  // 验证拓扑
  const validateMutation = useMutation({
    mutationFn: (topology: any) => topologyApi.validateTopology(topology),
    onSuccess: (response) => {
      if (response.data.valid) {
        message.success('拓扑验证通过');
      } else {
        message.error(`验证失败: ${response.data.errors.join(', ')}`);
      }
    },
  });

  // 保存拓扑
  const saveMutation = useMutation({
    mutationFn: (values: { id: string; topology: any }) =>
      topologyApi.saveTopology(values.id, values.topology),
    onSuccess: () => {
      message.success('拓扑已保存');
    },
  });

  const handleValidate = () => {
    const topology = form.getFieldValue('topology');
    try {
      const parsed = typeof topology === 'string' ? JSON.parse(topology) : topology;
      validateMutation.mutate(parsed);
    } catch (error) {
      message.error('JSON 格式错误');
    }
  };

  const handleSave = () => {
    const topology = form.getFieldValue('topology');
    try {
      const parsed = typeof topology === 'string' ? JSON.parse(topology) : topology;
      saveMutation.mutate({ id: selectedId, topology: parsed });
    } catch (error) {
      message.error('JSON 格式错误');
    }
  };

  React.useEffect(() => {
    if (topologyData?.data) {
      const data = topologyData.data as any;
      // If it's a PLC file, show the content
      if (data.content) {
        form.setFieldsValue({ topology: data.content });
      } else {
        form.setFieldsValue({
          topology: JSON.stringify(data, null, 2),
        });
      }
    }
  }, [topologyData, form]);

  return (
    <div>
      <Title level={2}>拓扑编辑器</Title>

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
          defaultActiveKey="plc"
          items={[
            {
              key: 'plc',
              label: 'PLC 代码',
              children: (
                <Form form={form} layout="vertical">
                  <Form.Item name="topology" label={`PLC 文件: examples/${selectedId}.plc`}>
                    <TextArea
                      rows={25}
                      style={{ fontFamily: 'monospace', fontSize: '13px' }}
                      placeholder="PLC 代码..."
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
                    可视化拓扑编辑器开发中...
                  </Text>
                  <br />
                  <br />
                  <Text type="secondary">
                    功能规划：拖拽组件、连线编辑、属性配置、实时验证
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

export default TopologyPage;
