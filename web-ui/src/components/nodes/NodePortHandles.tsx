import React from 'react';
import { Handle, Position } from '@xyflow/react';
import type { DevicePortMetadata } from '../../types';

const HANDLE_SIZE = 8;

const PORT_COLORS: Record<string, string> = {
  digital: '#00bcd4',
  analog: '#faad14',
  pneumatic: '#52c41a',
  logical: '#722ed1',
  generic: '#a0a0a0',
};

const NodePortHandles: React.FC<{ ports?: DevicePortMetadata[] }> = ({ ports }) => {
  const sourcePorts = (ports || []).filter(
    (port) => port.role === 'producer' || port.role === 'bidirectional'
  );
  const targetPorts = (ports || []).filter(
    (port) => port.role === 'consumer' || port.role === 'bidirectional'
  );

  return (
    <>
      {targetPorts.map((port, index) => (
        <Handle
          key={`target-${port.id}`}
          type="target"
          id={port.id}
          position={Position.Left}
          style={buildHandleStyle(port.type, index, targetPorts.length)}
        />
      ))}
      {sourcePorts.map((port, index) => (
        <Handle
          key={`source-${port.id}`}
          type="source"
          id={port.id}
          position={Position.Right}
          style={buildHandleStyle(port.type, index, sourcePorts.length)}
        />
      ))}
    </>
  );
};

export default NodePortHandles;

export const PortMetadataFallbackBadge: React.FC<{ visible?: boolean }> = ({
  visible = false,
}) => {
  if (!visible) {
    return null;
  }
  return (
    <div
      title="Port metadata missing, using fallback contract"
      style={{
        position: 'absolute',
        top: 4,
        right: 4,
        fontSize: 11,
        color: '#faad14',
        lineHeight: 1,
      }}
    >
      ⚠
    </div>
  );
};

function buildHandleStyle(type: DevicePortMetadata['type'], index: number, total: number) {
  const top = `${Math.round(((index + 1) / (total + 1)) * 100)}%`;
  return {
    background: PORT_COLORS[type] || PORT_COLORS.generic,
    width: HANDLE_SIZE,
    height: HANDLE_SIZE,
    top,
    transform: 'translateY(-50%)',
    border: '1px solid #1e1e1e',
  };
}
