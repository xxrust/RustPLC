import React from 'react';
import { Handle, Position } from '@xyflow/react';
import type { NodeProps } from '@xyflow/react';
import type { NodeData } from '../../stores/topologyStore';

const SensorNode: React.FC<NodeProps> = ({ data, selected }) => {
  const d = data as NodeData;
  const isOn = d.status === 'on' || d.value === true;
  const isFault = d.status === 'fault';
  const ledColor = isFault ? '#f5222d' : isOn ? '#52c41a' : '#4a4a4a';

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
          {/* Sensor body */}
          <rect x="8" y="8" width="60" height="24" rx="12" fill="#3a3a3a" stroke="#5a5a5a" strokeWidth="1" />
          {/* LED indicator */}
          <circle cx="38" cy="20" r="8" fill={ledColor} style={{ filter: isOn ? `drop-shadow(0 0 4px ${ledColor})` : 'none', transition: 'all 0.2s' }} />
          {/* Detection beam */}
          {isOn && <line x1="68" y1="20" x2="76" y2="20" stroke={ledColor} strokeWidth="2" strokeDasharray="2,2" />}
        </svg>
        <div style={{ color: '#a0a0a0', fontSize: 10, marginTop: 2, fontFamily: 'JetBrains Mono, monospace' }}>
          {d.status || 'off'}
        </div>
      </div>
      <Handle type="target" position={Position.Left} style={{ background: '#00bcd4', width: 8, height: 8 }} />
      <Handle type="source" position={Position.Right} style={{ background: '#00bcd4', width: 8, height: 8 }} />
    </div>
  );
};

export default SensorNode;
