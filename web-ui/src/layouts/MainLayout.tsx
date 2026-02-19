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
import { useAppStore } from '../stores/appStore';
import { ProjectSelector } from '../components/ProjectSelector';
import type { MenuProps } from 'antd';

const { Header, Sider, Content } = Layout;

interface MainLayoutProps {
  children: React.ReactNode;
}

const MainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  const navigate = useNavigate();
  const location = useLocation();
  const {
    runMode,
    currentUser,
    hasUnsavedChanges,
    alarmCount,
  } = useAppStore();

  const totalAlarms = alarmCount.critical + alarmCount.warning;

  const menuItems: MenuProps['items'] = [
    {
      key: '/',
      icon: <DashboardOutlined />,
      label: '总览',
    },
    {
      key: '/topology',
      icon: <ApartmentOutlined />,
      label: '拓扑',
    },
    {
      key: '/scenario',
      icon: <ExperimentOutlined />,
      label: '场景',
    },
    {
      key: '/run',
      icon: <PlayCircleOutlined />,
      label: '运行',
    },
    {
      key: '/replay',
      icon: <HistoryOutlined />,
      label: '回放',
    },
    {
      key: '/diagnosis',
      icon: <WarningOutlined />,
      label: totalAlarms > 0 ? (
        <Badge count={totalAlarms} offset={[10, 0]}>
          诊断
        </Badge>
      ) : '诊断',
    },
    {
      key: '/audit',
      icon: <AuditOutlined />,
      label: '审计',
    },
  ];

  const handleMenuClick: MenuProps['onClick'] = (e) => {
    navigate(e.key);
  };

  const runModeColor = {
    no_board: 'blue',
    hil_board: 'orange',
    runtime_live: 'green',
  }[runMode];

  const runModeLabel = {
    no_board: 'No-Board',
    hil_board: 'HIL',
    runtime_live: 'Live',
  }[runMode];

  const userMenuItems: MenuProps['items'] = [
    {
      key: 'profile',
      label: '个人信息',
    },
    {
      key: 'settings',
      label: '设置',
    },
    {
      type: 'divider',
    },
    {
      key: 'logout',
      label: '退出登录',
      danger: true,
    },
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
          <h1 style={{ color: 'white', margin: 0, fontSize: '20px' }}>
            RustPLC Web
          </h1>
          <ProjectSelector />
          {hasUnsavedChanges && (
            <Tag icon={<SaveOutlined />} color="warning">
              未保存
            </Tag>
          )}
        </div>

        <Space size="large">
          <Tag color={runModeColor}>{runModeLabel}</Tag>

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
