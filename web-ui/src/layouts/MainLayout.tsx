import React from 'react';
import { Layout, Menu, Badge, Tag, Space, Dropdown } from 'antd';
import {
  DashboardOutlined,
  ApartmentOutlined,
  ExperimentOutlined,
  PlayCircleOutlined,
  HistoryOutlined,
  WarningOutlined,
  AuditOutlined,
  UserOutlined,
  SaveOutlined,
} from '@ant-design/icons';
import { useNavigate, useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAppStore } from '../stores/appStore';
import { ProjectSelector } from '../components/ProjectSelector';
import type { MenuProps } from 'antd';

const { Header, Sider, Content } = Layout;

interface MainLayoutProps {
  children: React.ReactNode;
}

const MainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { runMode, currentUser, hasUnsavedChanges, alarmCount } = useAppStore();

  const totalAlarms = alarmCount.critical + alarmCount.warning;

  const menuItems: MenuProps['items'] = [
    { key: '/', icon: <DashboardOutlined />, label: t('dashboard.title') },
    { key: '/topology', icon: <ApartmentOutlined />, label: t('tabs.topology') },
    { key: '/scenario', icon: <ExperimentOutlined />, label: t('tabs.scenario') },
    { key: '/run', icon: <PlayCircleOutlined />, label: t('tabs.run') },
    { key: '/replay', icon: <HistoryOutlined />, label: t('tabs.replay') },
    {
      key: '/diagnosis',
      icon: <WarningOutlined />,
      label: totalAlarms > 0 ? (
        <Badge count={totalAlarms} offset={[10, 0]}>{t('tabs.diagnosis')}</Badge>
      ) : t('tabs.diagnosis'),
    },
    { key: '/audit', icon: <AuditOutlined />, label: t('tabs.audit') },
  ];

  const handleMenuClick: MenuProps['onClick'] = (e) => navigate(e.key);

  const runModeColor = { no_board: 'blue', hil_board: 'orange', runtime_live: 'green' }[runMode];

  const userMenuItems: MenuProps['items'] = [
    { key: 'profile', label: t('mainLayout.profile') },
    { key: 'settings', label: t('mainLayout.settings') },
    { type: 'divider' },
    { key: 'logout', label: t('mainLayout.logout'), danger: true },
  ];

  const totalAlarmsHeader = alarmCount.info + alarmCount.warning + alarmCount.critical;

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Header
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '0 24px',
          background: '#001529',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
          <h1 style={{ color: 'white', margin: 0, fontSize: '20px' }}>RustPLC</h1>
          <ProjectSelector />
          {hasUnsavedChanges && (
            <Tag icon={<SaveOutlined />} color="warning">{t('topBar.unsavedChanges')}</Tag>
          )}
        </div>

        <Space size="large">
          <Tag color={runModeColor}>{t(`runMode.${runMode}`)}</Tag>

          <Badge count={totalAlarmsHeader} overflowCount={99}>
            <WarningOutlined
              style={{ fontSize: '20px', color: 'white', cursor: 'pointer' }}
              onClick={() => navigate('/diagnosis')}
            />
          </Badge>

          <Dropdown menu={{ items: userMenuItems }} placement="bottomRight">
            <Space style={{ cursor: 'pointer', color: 'white' }}>
              <UserOutlined />
              <span>{currentUser?.name}</span>
              <Tag color="cyan">{currentUser?.role}</Tag>
            </Space>
          </Dropdown>
        </Space>
      </Header>

      <Layout>
        <Sider width={200} theme="light">
          <Menu
            mode="inline"
            selectedKeys={[location.pathname]}
            onClick={handleMenuClick}
            style={{ height: '100%', borderRight: 0 }}
            items={menuItems}
          />
        </Sider>

        <Layout style={{ padding: '24px' }}>
          <Content
            style={{
              padding: 24,
              margin: 0,
              minHeight: 280,
              background: '#fff',
              borderRadius: '8px',
            }}
          >
            {children}
          </Content>
        </Layout>
      </Layout>
    </Layout>
  );
};

export default MainLayout;
