import React from 'react';
import { useTopologyStore } from '../../stores/topologyStore';

const CanvasPropertiesEditor: React.FC = () => {
  const { nodes, edges } = useTopologyStore();

  const nodeTypeCounts = nodes.reduce((acc, node) => {
    const type = node.type || 'generic';
    acc[type] = (acc[type] || 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  return (
    <div style={{ padding: 16, color: '#e0e0e0' }}>
      <h3 style={{ margin: '0 0 16px 0', fontSize: 14, fontWeight: 600, color: '#00bcd4' }}>
        Topology Overview
      </h3>

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>Statistics</div>
        <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
            <span style={{ fontSize: 12, color: '#a0a0a0' }}>Total Nodes:</span>
            <span style={{ fontSize: 12, fontWeight: 600 }}>{nodes.length}</span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
            <span style={{ fontSize: 12, color: '#a0a0a0' }}>Total Edges:</span>
            <span style={{ fontSize: 12, fontWeight: 600 }}>{edges.length}</span>
          </div>
        </div>
      </div>

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>Node Types</div>
        <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12 }}>
          {Object.entries(nodeTypeCounts).map(([type, count]) => (
            <div key={type} style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
              <span style={{ fontSize: 12, color: '#a0a0a0', textTransform: 'capitalize' }}>
                {type.replace('_', ' ')}:
              </span>
              <span style={{ fontSize: 12, fontWeight: 600 }}>{count}</span>
            </div>
          ))}
          {Object.keys(nodeTypeCounts).length === 0 && (
            <div style={{ fontSize: 12, color: '#5a5a5a', textAlign: 'center' }}>
              No nodes in topology
            </div>
          )}
        </div>
      </div>

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>Instructions</div>
        <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12, fontSize: 11, lineHeight: 1.6 }}>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            • Drag components from the library to add nodes
          </p>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            • Connect nodes by dragging from handles
          </p>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            • Select a node to edit its properties
          </p>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            • Press Delete to remove selected nodes/edges
          </p>
          <p style={{ margin: '0', color: '#a0a0a0' }}>
            • Right-click nodes for fault injection
          </p>
        </div>
      </div>
    </div>
  );
};

export default CanvasPropertiesEditor;
