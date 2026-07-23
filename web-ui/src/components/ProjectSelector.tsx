import React, { useMemo, useRef, useState } from 'react';
import { Modal, Select, Space, Typography, message, Divider, Button } from 'antd';
import { FolderOutlined, FileOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { projectApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const { Text } = Typography;
const { Option, OptGroup } = Select;

interface Project {
  id: string;
  name: string;
  path: string;
  type: string;
  category?: string;
  summary?: string;
  scenario_path?: string;
}

export const ProjectSelector: React.FC = () => {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const [isSwitching, setIsSwitching] = useState(false);
  const { currentProject, setCurrentProject } = useAppStore();
  const fileInputRef = useRef<HTMLInputElement>(null);

  const { data: projectsData, isLoading } = useQuery({
    queryKey: ['projects'],
    queryFn: () => projectApi.listProjects(),
    enabled: visible,
  });

  const projects: Project[] = useMemo(() => projectsData?.data?.projects || [], [projectsData?.data?.projects]);
  const projectsByCategory = useMemo(() => {
    return projects.reduce<Record<string, Project[]>>((groups, project) => {
      const category = project.category || t('projectSelector.uncategorized');
      groups[category] = groups[category] || [];
      groups[category].push(project);
      return groups;
    }, {});
  }, [projects, t]);

  const handleOpen = () => {
    setSelectedProject(currentProject);
    setVisible(true);
  };

  const handleOk = async () => {
    if (selectedProject) {
      const proj = projects.find((p) => p.id === selectedProject);
      try {
        setIsSwitching(true);
        if (proj?.type === 'plc') {
          const source = await projectApi.getProjectSource(proj.id);
          setCurrentProject(proj.id, source.data.path, source.data.content);
        } else {
          setCurrentProject(selectedProject, proj?.path ?? null, null);
        }
        message.success(`${t('projectSelector.switched')}: ${selectedProject}`);
        setVisible(false);
      } catch {
        message.error(t('projectSelector.loadTemplateFailed'));
      } finally {
        setIsSwitching(false);
      }
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
      const path = typeof (file as File & { path?: unknown }).path === 'string'
        ? (file as File & { path: string }).path
        : file.name;
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
        confirmLoading={isSwitching}
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
              {t('projectSelector.templateLibrary')}
            </Text>
          </Divider>

          {/* Server project list */}
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t('projectSelector.current')}: <Text strong>{currentProject || t('idde.noProjectSelected')}</Text>
            </Text>
            <Text type="secondary" style={{ display: 'block', fontSize: 11, marginTop: 4 }}>
              {t('projectSelector.templateCount', { count: projects.length })}
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
              {Object.entries(projectsByCategory).map(([category, categoryProjects]) => (
                <OptGroup key={category} label={category}>
                  {categoryProjects.map((project) => (
                    <Option
                      key={project.id}
                      value={project.id}
                      label={`${project.name} ${project.path} ${project.summary ?? ''}`}
                    >
                      <div style={{ display: 'grid', gap: 2 }}>
                        <Space size={6}>
                          <Text strong>{project.name}</Text>
                          <Text type="secondary" style={{ fontSize: 11 }}>
                            {project.type}
                          </Text>
                        </Space>
                        <Text type="secondary" style={{ fontSize: 11 }}>
                          {project.summary || project.path}
                        </Text>
                      </div>
                    </Option>
                  ))}
                </OptGroup>
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
