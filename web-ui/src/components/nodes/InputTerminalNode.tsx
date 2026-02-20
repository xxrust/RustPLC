import React from 'react';
import { Handle, Position } from '@xyflow/react';
import type { NodeProps } from '@xyflow/react';
import type { NodeData } from '../../stores/topologyStore';

const InputTerminalNode: React.FC<NodeProps> = ({ data, selected }) => {
  const d = data as NodeData;
  const isActive = d.status === 'on' || d.value === true;
  const color = isActive ? '#52c41a' : '#4a4a4a';

  return (
    <div
      style={{
        background: '#2d2d2d',
        border: `2px solid ${selected ? '#00bcd4' : '#3a3a3a'}`,
        borderRadius: 6,
        minWidth: 80,
      }}
    >
      <div style={{ background: '#1e1e1e', borderBottom: '1px solid #3a3a3a', padding: '4px 8px', display: 'flex', alignItems: 'center', gap: 6, borderRadius: '4px 4px 0 0' }}>
        <div style={{ width: 8, height: 8, borderRadius: '50%', background: color, boxShadow: `0 0 6px ${color}` }} />
        <span style={{ color: '#e0e0e0', fontSize: 11, fontWeight: 600 }}>{d.label}</span>
      </div>
      <div style={{ padding: '8px 12px' }}>
        <svg width="56" height="32" viewBox="0 0 56 32">
          {/* Terminal block */}
          <rect x="4" y="8" width="48" height="16" rx="2" fill="#3a3a3a" stroke="#5a5a5a" strokeWidth="1" />
          {/* LED indicator */}
          <circle cx="28" cy="16" r="4" fill={color} style={{ filter: isActive ? `drop-shadow(0 0 3px ${color})` : 'none' }} />
          {/* Output line */}
          {isActive && <line x1="52" y1="16" x2="56" y2="16" stroke={color} strokeWidth="2" />}
        </svg>
        <div style={{ color: '#a0a0a0', fontSize: 9, marginTop: 2, fontFamily: 'JetBrains Mono, monospace' }}>
          {isActive ? 'ON' : 'OFF'}
        </div>
      </div>
      {/* 输入端子只有左侧输入（从外部接收信号） */}
      <Handle type="target" id="in" position={Position.Left} style={{ background: '#00bcd4', width: 8, height: 8 }} />
    </div>
  );
};

export default InputTerminalNode;
