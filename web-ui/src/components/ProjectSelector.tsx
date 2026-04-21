import React, { useRef, useState } from 'react';
import { Modal, Select, Space, Typography, message, Divider, Button } from 'antd';
import { FolderOutlined, FileOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
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
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const { currentProject, setCurrentProject } = useAppStore();
  const fileInputRef = useRef<HTMLInputElement>(null);

  const { data: projectsData, isLoading } = useQuery({
    queryKey: ['projects'],
    queryFn: () => projectApi.listProjects(),
    enabled: visible,
  });

  const projects: Project[] = projectsData?.data?.projects || [];

  const handleOpen = () => {
    setSelectedProject(currentProject);
    setVisible(true);
  };

  const handleOk = () => {
    if (selectedProject) {
      const proj = projects.find((p) => p.id === selectedProject);
      setCurrentProject(selectedProject, proj?.path ?? null, null);
      message.success(`${t('projectSelector.switched')}: ${selectedProject}`);
      setVisible(false);
    }
  };

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    try {
      const content = await file.text();
      // Strip extension to use as project id/name
      const name = file.name.replace(/\.plc$/i, '');
      // webkitRelativePath is empty for single file pick; use file.name as path
      const path = (file as any).path || file.name;
      setCurrentProject(name, path, content);
      message.success(`${t('projectSelector.switched')}: ${name}`);
      setVisible(false);
    } catch {
      message.error(t('projectSelector.openFailed', { fileName: file.name }));
    } finally {
      // Reset so the same file can be re-selected
      e.target.value = '';
    }
  };

  return (
    <>
      <button
        onClick={handleOpen}
        title={t('topBar.clickToSwitchProject')}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 4,
          background: 'none',
          border: 'none',
          color: currentProject ? '#a0a0a0' : '#5a5a5a',
          fontSize: 12,
          cursor: 'pointer',
          padding: '2px 6px',
          borderRadius: 3,
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = '#3a3a3a')}
        onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
      >
        <FolderOutlined style={{ fontSize: 12 }} />
        / {currentProject || t('projectSelector.select')}
      </button>

      {/* Hidden file input */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".plc"
        style={{ display: 'none' }}
        onChange={handleFileSelect}
      />

      <Modal
        title={t('projectSelector.title')}
        open={visible}
        onOk={handleOk}
        onCancel={() => setVisible(false)}
        okText={t('common.confirm')}
        cancelText={t('common.cancel')}
        okButtonProps={{ disabled: !selectedProject }}
      >
        <Space direction="vertical" style={{ width: '100%' }} size="middle">
          {/* Local file picker */}
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t('projectSelector.openLocal')}
            </Text>
            <div style={{ marginTop: 8 }}>
              <Button
                icon={<FileOutlined />}
                onClick={() => fileInputRef.current?.click()}
                style={{ width: '100%' }}
              >
                {t('projectSelector.browseFile')}
              </Button>
            </div>
          </div>

          <Divider style={{ margin: '4px 0' }}>
            <Text type="secondary" style={{ fontSize: 11 }}>
              {t('projectSelector.orFromServer')}
            </Text>
          </Divider>

          {/* Server project list */}
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t('projectSelector.current')}: <Text strong>{currentProject || t('idde.noProjectSelected')}</Text>
            </Text>
            <Select
              style={{ width: '100%', marginTop: 8 }}
              placeholder={t('projectSelector.placeholder')}
              value={selectedProject}
              onChange={setSelectedProject}
              loading={isLoading}
              showSearch
              allowClear
              filterOption={(input, option) =>
                (option?.label as string)?.toLowerCase().includes(input.toLowerCase())
              }
            >
              {projects.map((project) => (
                <Option key={project.id} value={project.id} label={project.name}>
                  {project.name}
                  <Text type="secondary" style={{ fontSize: 11, marginLeft: 8 }}>
                    {project.path}
                  </Text>
                </Option>
              ))}
            </Select>
          </div>

          {selectedProject && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              {t('projectSelector.path')}: {projects.find((p) => p.id === selectedProject)?.path}
            </Text>
          )}
        </Space>
      </Modal>
    </>
  );
};
