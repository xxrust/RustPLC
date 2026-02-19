import React, { useState } from 'react';
import { Modal, Select, Button, Space, Typography, message } from 'antd';
import { FolderOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { projectApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const { Text } = Typography;
const { Option } = Select;

interface Project {
  id: string;
  name: string;
  path: string;
  type: string;
}

export const ProjectSelector: React.FC = () => {
  const [visible, setVisible] = useState(false);
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const { currentProject, setCurrentProject } = useAppStore();

  const { data: projectsData, isLoading } = useQuery({
    queryKey: ['projects'],
    queryFn: () => projectApi.listProjects(),
  });

  const projects: Project[] = projectsData?.data?.projects || [];

  const handleOpen = () => {
    setSelectedProject(currentProject);
    setVisible(true);
  };

  const handleOk = () => {
    if (selectedProject) {
      setCurrentProject(selectedProject);
      message.success(`已切换到项目: ${selectedProject}`);
      setVisible(false);
    }
  };

  const handleCancel = () => {
    setVisible(false);
  };

  return (
    <>
      <Button icon={<FolderOutlined />} onClick={handleOpen}>
        {currentProject || '选择项目'}
      </Button>

      <Modal
        title="选择项目"
        open={visible}
        onOk={handleOk}
        onCancel={handleCancel}
        okText="确定"
        cancelText="取消"
      >
        <Space direction="vertical" style={{ width: '100%' }} size="large">
          <div>
            <Text>当前项目: </Text>
            <Text strong>{currentProject || '未选择'}</Text>
          </div>

          <div>
            <Text>选择新项目:</Text>
            <Select
              style={{ width: '100%', marginTop: 8 }}
              placeholder="选择一个 PLC 项目"
              value={selectedProject}
              onChange={setSelectedProject}
              loading={isLoading}
              showSearch
              filterOption={(input, option) =>
                (option?.label as string)?.toLowerCase().includes(input.toLowerCase())
              }
            >
              {projects.map((project) => (
                <Option key={project.id} value={project.id}>
                  {project.name}
                </Option>
              ))}
            </Select>
          </div>

          {selectedProject && (
            <div>
              <Text type="secondary">
                路径: {projects.find((p) => p.id === selectedProject)?.path}
              </Text>
            </div>
          )}
        </Space>
      </Modal>
    </>
  );
};
