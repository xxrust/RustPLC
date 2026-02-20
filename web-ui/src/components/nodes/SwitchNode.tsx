import React, { useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import type { NodeProps } from '@xyflow/react';
import { useTranslation } from 'react-i18next';
import type { NodeData } from '../../stores/topologyStore';
import { useTopologyStore } from '../../stores/topologyStore';
import { useAppStore } from '../../stores/appStore';
import { simulationApi } from '../../services/api';

const SwitchNode: React.FC<NodeProps> = ({ data, selected, id }) => {
  const { t } = useTranslation();
  const d = data as NodeData;
  const isClosed = d.status === 'closed' || d.value === true;
  const color = isClosed ? '#52c41a' : '#a0a0a0';

  const { updateNodeData } = useTopologyStore();
  const { runMode, currentUser } = useAppStore();
  const showControls = runMode === 'no_board';
  const [loading, setLoading] = useState(false);

  const handleToggle = async (e: React.MouseEvent) => {
    e.stopPropagation();
    const newStatus = isClosed ? 'open' : 'closed';
    const newValue = !isClosed;

    try {
      setLoading(true);
      await simulationApi.injectEvent(id, 'switch', newValue, currentUser?.name || 'unknown');
      updateNodeData(id, { status: newStatus, value: newValue });
    } catch (error) {
      console.error('Failed to inject switch event:', error);
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
        <div style={{ width: 8, height: 8, borderRadius: '50%', background: color, boxShadow: `0 0 6px ${color}` }} />
        <span style={{ color: '#e0e0e0', fontSize: 11, fontWeight: 600 }}>{d.label}</span>
      </div>
      <div style={{ padding: '8px 12px' }}>
        <svg width="76" height="40" viewBox="0 0 76 40">
          <line x1="0" y1="20" x2="20" y2="20" stroke="#5a5a5a" strokeWidth="2" />
          <line x1="56" y1="20" x2="76" y2="20" stroke="#5a5a5a" strokeWidth="2" />
          <circle cx="20" cy="20" r="3" fill="#5a5a5a" />
          <circle cx="56" cy="20" r="3" fill="#5a5a5a" />
          <line
            x1="20" y1="20"
            x2="56" y2={isClosed ? 20 : 10}
            stroke={color}
            strokeWidth="2.5"
            style={{ transition: 'all 0.2s' }}
          />
        </svg>
        <div style={{ color: '#a0a0a0', fontSize: 10, marginTop: 2, fontFamily: 'JetBrains Mono, monospace' }}>
          {isClosed ? t('properties.statusClosed') : t('properties.statusOpen')}
        </div>
        {showControls && (
          <button
            className="nodrag"
            onClick={handleToggle}
            disabled={loading}
            style={{
              marginTop: 4,
              width: '100%',
              padding: '2px 6px',
              background: isClosed ? '#52c41a' : '#3a3a3a',
              border: '1px solid #5a5a5a',
              borderRadius: 3,
              color: '#e0e0e0',
              fontSize: 10,
              cursor: loading ? 'wait' : 'pointer',
              fontFamily: 'JetBrains Mono, monospace',
              opacity: loading ? 0.6 : 1,
            }}
          >
            {loading ? '...' : isClosed ? t('properties.statusClosed') : t('properties.statusOpen')}
          </button>
        )}
      </div>
      <Handle type="target" position={Position.Left} style={{ background: '#00bcd4', width: 8, height: 8 }} />
      <Handle type="source" position={Position.Right} style={{ background: '#00bcd4', width: 8, height: 8 }} />
    </div>
  );
};

export default SwitchNode;
