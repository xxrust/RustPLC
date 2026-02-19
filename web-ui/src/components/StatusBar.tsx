import React from 'react';
import { useAppStore } from '../stores/appStore';

const StatusBar: React.FC = () => {
  const { alarmCount, runMode } = useAppStore();

  const connectionStatus = 'connected'; // TODO: real WS status

  return (
    <div
      style={{
        height: 32,
        background: '#1a1a1a',
        borderTop: '1px solid #3a3a3a',
        display: 'flex',
        alignItems: 'center',
        padding: '0 16px',
        gap: 16,
        flexShrink: 0,
        fontSize: 11,
        fontFamily: 'JetBrains Mono, monospace',
      }}
    >
      {/* Connection status */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <div
          style={{
            width: 6,
            height: 6,
            borderRadius: '50%',
            background: connectionStatus === 'connected' ? '#52c41a' : '#f5222d',
          }}
        />
        <span style={{ color: '#a0a0a0' }}>
          {connectionStatus === 'connected' ? 'Connected' : 'Disconnected'}
        </span>
      </div>

      <span style={{ color: '#3a3a3a' }}>|</span>

      {/* Alarm counts */}
      <div style={{ display: 'flex', gap: 8 }}>
        {alarmCount.critical > 0 && (
          <span style={{ color: '#f5222d' }}>● {alarmCount.critical} critical</span>
        )}
        {alarmCount.warning > 0 && (
          <span style={{ color: '#faad14' }}>● {alarmCount.warning} warning</span>
        )}
        {alarmCount.info > 0 && (
          <span style={{ color: '#1890ff' }}>● {alarmCount.info} info</span>
        )}
        {alarmCount.critical === 0 && alarmCount.warning === 0 && alarmCount.info === 0 && (
          <span style={{ color: '#4a4a4a' }}>No alarms</span>
        )}
      </div>

      <span style={{ color: '#3a3a3a' }}>|</span>

      {/* Run mode */}
      <span style={{ color: '#a0a0a0' }}>Mode: <span style={{ color: '#e0e0e0' }}>{runMode}</span></span>

      {/* Right side */}
      <div style={{ marginLeft: 'auto', color: '#4a4a4a' }}>
        RustPLC IDDE v1.0
      </div>
    </div>
  );
};

export default StatusBar;
