import React, { useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import type { NodeProps } from '@xyflow/react';
import { useTranslation } from 'react-i18next';
import type { NodeData } from '../../stores/topologyStore';
import { useTopologyStore } from '../../stores/topologyStore';
import { useAppStore } from '../../stores/appStore';
import { simulationApi } from '../../services/api';

const SensorNode: React.FC<NodeProps> = ({ data, selected, id }) => {
  const { t } = useTranslation();
  const d = data as NodeData;
  const isOn = d.status === 'on' || d.value === true;
  const isFault = d.status === 'fault';
  const ledColor = isFault ? '#f5222d' : isOn ? '#52c41a' : '#4a4a4a';
  const detectsLabel = d.detects as string | undefined;

  const { updateNodeData } = useTopologyStore();
  const { runMode, currentUser } = useAppStore();
  const showControls = runMode === 'no_board';
  const [loading, setLoading] = useState(false);

  const handleToggle = async (e: React.MouseEvent) => {
    e.stopPropagation();
    const newStatus = isOn ? 'off' : 'on';
    const newValue = !isOn;

    try {
      setLoading(true);
      await simulationApi.injectEvent(id, 'sensor', newValue, currentUser?.name || 'unknown');
      updateNodeData(id, { status: newStatus, value: newValue });
    } catch (error) {
      console.error('Failed to inject sensor event:', error);
      alert(t('notifications.toggleFailed'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        background: '#2d2d2d',
        border: `2px solid ${selected ? '#00bcd4' : '#3a3a3a'}`,
        borderRadius: 6,
        minWidth: 100,
      }}
    >
      <div style={{ background: '#1e1e1e', borderBottom: '1px solid #3a3a3a', padding: '4px 8px', display: 'flex', alignItems: 'center', gap: 6, borderRadius: '4px 4px 0 0' }}>
        <div style={{ width: 8, height: 8, borderRadius: '50%', background: ledColor, boxShadow: `0 0 6px ${ledColor}` }} />
        <span style={{ color: '#e0e0e0', fontSize: 11, fontWeight: 600 }}>{d.label}</span>
      </div>
      <div style={{ padding: '8px 12px' }}>
        <svg width="76" height="40" viewBox="0 0 76 40">
          <rect x="8" y="8" width="60" height="24" rx="12" fill="#3a3a3a" stroke="#5a5a5a" strokeWidth="1" />
          <circle cx="38" cy="20" r="8" fill={ledColor} style={{ filter: isOn ? `drop-shadow(0 0 4px ${ledColor})` : 'none', transition: 'all 0.2s' }} />
          {isOn && <line x1="68" y1="20" x2="76" y2="20" stroke={ledColor} strokeWidth="2" strokeDasharray="2,2" />}
        </svg>
        <div style={{ color: '#a0a0a0', fontSize: 10, marginTop: 2, fontFamily: 'JetBrains Mono, monospace' }}>
          {d.status ? t(`properties.status${d.status.charAt(0).toUpperCase() + d.status.slice(1)}`, d.status) : t('properties.statusOff')}
        </div>
        {detectsLabel && (
          <div style={{ color: '#8ad7e0', fontSize: 9, marginTop: 2, fontFamily: 'JetBrains Mono, monospace' }}>
            {t('properties.detects')}: {detectsLabel}
          </div>
        )}
        {showControls && (
          <button
            className="nodrag"
            onClick={handleToggle}
            disabled={loading}
            style={{
              marginTop: 4,
              width: '100%',
              padding: '2px 6px',
              background: isOn ? '#52c41a' : '#3a3a3a',
              border: '1px solid #5a5a5a',
              borderRadius: 3,
              color: '#e0e0e0',
              fontSize: 10,
              cursor: loading ? 'wait' : 'pointer',
              fontFamily: 'JetBrains Mono, monospace',
              opacity: loading ? 0.6 : 1,
            }}
          >
            {loading ? '...' : isOn ? t('properties.statusOn') : t('properties.statusOff')}
          </button>
        )}
      </div>
      <Handle type="target" id="in" position={Position.Left} style={{ background: '#00bcd4', width: 8, height: 8 }} />
      <Handle type="source" id="state" position={Position.Right} style={{ background: '#00bcd4', width: 8, height: 8 }} />
    </div>
  );
};

export default SensorNode;
