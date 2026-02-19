import React from 'react';
import { Handle, Position } from '@xyflow/react';
import type { NodeProps } from '@xyflow/react';
import type { NodeData } from '../../stores/topologyStore';

const StepperNode: React.FC<NodeProps> = ({ data, selected }) => {
  const d = data as NodeData;
  const isRunning = d.status === 'running' || d.status === 'forward' || d.status === 'backward';
  const isFault = d.status === 'fault';
  const color = isFault ? '#f5222d' : isRunning ? '#722ed1' : '#a0a0a0';
  const isForward = d.status === 'forward';

  return (
    <div
      style={{
        background: '#2d2d2d',
        border: `2px solid ${selected ? '#00bcd4' : '#3a3a3a'}`,
        borderRadius: 6,
        minWidth: 110,
      }}
    >
      <div style={{ background: '#1e1e1e', borderBottom: '1px solid #3a3a3a', padding: '4px 8px', display: 'flex', alignItems: 'center', gap: 6, borderRadius: '4px 4px 0 0' }}>
        <div style={{ width: 8, height: 8, borderRadius: '50%', background: color, boxShadow: `0 0 6px ${color}` }} />
        <span style={{ color: '#e0e0e0', fontSize: 11, fontWeight: 600 }}>{d.label}</span>
      </div>
      <div style={{ padding: '8px 12px' }}>
        <svg width="86" height="40" viewBox="0 0 86 40">
          {/* Motor body */}
          <rect x="18" y="8" width="50" height="24" rx="4" fill="#3a3a3a" stroke="#5a5a5a" strokeWidth="1" />
          {/* Shaft */}
          <rect x="68" y="17" width="18" height="6" rx="2" fill="#5a5a5a" />
          {/* Rotor circle */}
          <circle cx="43" cy="20" r="8" fill="#2a2a2a" stroke={color} strokeWidth="1.5" />
          {/* Direction arrow */}
          {isRunning && (
            <path
              d={isForward ? 'M38,20 L48,20 M44,16 L48,20 L44,24' : 'M48,20 L38,20 M42,16 L38,20 L42,24'}
              stroke={color}
              strokeWidth="1.5"
              fill="none"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          )}
          {/* Mounting feet */}
          <rect x="22" y="30" width="8" height="4" rx="1" fill="#4a4a4a" />
          <rect x="56" y="30" width="8" height="4" rx="1" fill="#4a4a4a" />
        </svg>
        <div style={{ color: '#a0a0a0', fontSize: 10, marginTop: 2, fontFamily: 'JetBrains Mono, monospace' }}>
          {d.status || 'idle'}
        </div>
      </div>
      <Handle type="target" position={Position.Left} style={{ background: '#00bcd4', width: 8, height: 8 }} />
      <Handle type="source" position={Position.Right} style={{ background: '#00bcd4', width: 8, height: 8 }} />
    </div>
  );
};

export default StepperNode;
