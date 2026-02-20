import React, { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useAppStore } from '../stores/appStore';
import { useAlarmWebSocket } from '../hooks/useAlarmWebSocket';
import { useAlarmPolling } from '../hooks/useAlarmPolling';

const StatusBar: React.FC = () => {
  const { t } = useTranslation();
  const { alarmCount, setAlarmCount, runMode } = useAppStore();
  const { connected, alarms: wsAlarms } = useAlarmWebSocket();
  const { alarms: polledAlarms } = useAlarmPolling(!connected);
  const alarms = connected ? wsAlarms : polledAlarms;

  const connectionStatus = connected ? t('statusBar.connectedWebSocket') : t('statusBar.connectedPolling');
  const statusColor = connected ? '#52c41a' : '#faad14';

  // Update alarm counts when alarms change
  useEffect(() => {
    const counts = alarms.reduce(
      (acc, alarm) => {
        if (!alarm.acknowledged) {
          acc[alarm.severity] += 1;
        }
        return acc;
      },
      { info: 0, warning: 0, critical: 0 }
    );
    setAlarmCount(counts);
  }, [alarms, setAlarmCount]);

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
            background: statusColor,
          }}
        />
        <span style={{ color: '#a0a0a0' }}>{connectionStatus}</span>
      </div>

      <span style={{ color: '#3a3a3a' }}>|</span>

      {/* Alarm counts */}
      <div style={{ display: 'flex', gap: 8 }}>
        {alarmCount.critical > 0 && (
          <span style={{ color: '#f5222d' }}>● {alarmCount.critical} {t('statusBar.critical')}</span>
        )}
        {alarmCount.warning > 0 && (
          <span style={{ color: '#faad14' }}>● {alarmCount.warning} {t('statusBar.warning')}</span>
        )}
        {alarmCount.info > 0 && (
          <span style={{ color: '#1890ff' }}>● {alarmCount.info} {t('statusBar.info')}</span>
        )}
        {alarmCount.critical === 0 && alarmCount.warning === 0 && alarmCount.info === 0 && (
          <span style={{ color: '#4a4a4a' }}>{t('statusBar.noAlarms')}</span>
        )}
      </div>

      <span style={{ color: '#3a3a3a' }}>|</span>

      {/* Run mode */}
      <span style={{ color: '#a0a0a0' }}>{t('statusBar.mode')}: <span style={{ color: '#e0e0e0' }}>{t(`runMode.${runMode}`)}</span></span>

      {/* Right side */}
      <div style={{ marginLeft: 'auto', color: '#4a4a4a' }}>
        {t('statusBar.version')}
      </div>
    </div>
  );
};

export default StatusBar;
