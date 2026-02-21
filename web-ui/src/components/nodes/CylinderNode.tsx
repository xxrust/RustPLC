import React from 'react';
import type { NodeProps } from '@xyflow/react';
import type { NodeData } from '../../stores/topologyStore';
import NodePortHandles, { PortMetadataFallbackBadge } from './NodePortHandles';

const statusColor: Record<string, string> = {
  extended: '#00bcd4',
  retracted: '#a0a0a0',
  moving: '#722ed1',
  fault: '#f5222d',
  idle: '#4a4a4a',
};

const CylinderNode: React.FC<NodeProps> = ({ data, selected }) => {
  const d = data as NodeData;
  const status = d.status || 'retracted';
  const color = statusColor[status] || '#4a4a4a';
  const isExtended = status === 'extended';

  return (
    <div
      className="relative"
      style={{
        background: '#2d2d2d',
        border: `2px solid ${selected ? '#00bcd4' : '#3a3a3a'}`,
        borderRadius: 6,
        minWidth: 120,
        fontFamily: 'Inter, sans-serif',
        position: 'relative',
      }}
    >
      <PortMetadataFallbackBadge visible={Boolean(d.portContractFallback)} />
      {/* Header */}
      <div
        style={{
          background: '#1e1e1e',
          borderBottom: '1px solid #3a3a3a',
          padding: '4px 8px',
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          borderRadius: '4px 4px 0 0',
        }}
      >
        <div
          style={{
            width: 8,
            height: 8,
            borderRadius: '50%',
            background: color,
            boxShadow: `0 0 6px ${color}`,
          }}
        />
        <span style={{ color: '#e0e0e0', fontSize: 11, fontWeight: 600 }}>
          {d.label}
        </span>
      </div>

      {/* SVG Body */}
      <div style={{ padding: '8px 12px' }}>
        <svg width="96" height="40" viewBox="0 0 96 40">
          {/* Cylinder body */}
          <rect x="4" y="12" width="60" height="16" rx="2" fill="#3a3a3a" stroke="#5a5a5a" strokeWidth="1" />
          {/* Piston rod */}
          <rect
            x={isExtended ? 64 : 44}
            y="17"
            width={isExtended ? 28 : 8}
            height="6"
            rx="1"
            fill={color}
            style={{ transition: 'all 0.3s ease' }}
          />
          {/* End cap */}
          <rect x="0" y="10" width="6" height="20" rx="2" fill="#5a5a5a" />
          {/* Piston head */}
          <rect
            x={isExtended ? 60 : 40}
            y="14"
            width="6"
            height="12"
            rx="1"
            fill={color}
            style={{ transition: 'all 0.3s ease' }}
          />
        </svg>
        <div style={{ color: '#a0a0a0', fontSize: 10, marginTop: 2, fontFamily: 'JetBrains Mono, monospace' }}>
          {status}
        </div>
      </div>

      <NodePortHandles ports={d.ports} />
    </div>
  );
};

export default CylinderNode;
