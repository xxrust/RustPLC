import React from 'react';
import type { NodeProps } from '@xyflow/react';
import type { NodeData } from '../../stores/topologyStore';
import NodePortHandles, { PortMetadataFallbackBadge } from './NodePortHandles';

const GenericNode: React.FC<NodeProps> = ({ data, selected }) => {
  const d = data as NodeData;
  const statusColors: Record<string, string> = {
    active: '#52c41a',
    inactive: '#4a4a4a',
    fault: '#f5222d',
    warning: '#faad14',
    running: '#722ed1',
  };
  const color = statusColors[d.status || 'inactive'] || '#4a4a4a';

  return (
    <div
      style={{
        background: '#2d2d2d',
        border: `2px solid ${selected ? '#00bcd4' : '#3a3a3a'}`,
        borderRadius: 6,
        minWidth: 100,
        position: 'relative',
      }}
    >
      <PortMetadataFallbackBadge visible={Boolean(d.portContractFallback)} />
      <div style={{ background: '#1e1e1e', borderBottom: '1px solid #3a3a3a', padding: '4px 8px', display: 'flex', alignItems: 'center', gap: 6, borderRadius: '4px 4px 0 0' }}>
        <div style={{ width: 8, height: 8, borderRadius: '50%', background: color, boxShadow: `0 0 6px ${color}` }} />
        <span style={{ color: '#e0e0e0', fontSize: 11, fontWeight: 600 }}>{d.label}</span>
      </div>
      <div style={{ padding: '8px 12px' }}>
        <div style={{ color: '#a0a0a0', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' }}>
          {d.device_type || d.type || 'generic'}
        </div>
        <div style={{ color: '#a0a0a0', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' }}>
          {d.status || 'idle'}
        </div>
      </div>
      <NodePortHandles ports={d.ports} />
    </div>
  );
};

export default GenericNode;
