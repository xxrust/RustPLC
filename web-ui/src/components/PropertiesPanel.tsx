import React from 'react';
import { useTopologyStore } from '../stores/topologyStore';

const PropertiesPanel: React.FC = () => {
  const { nodes, selectedNodeId, updateNodeData } = useTopologyStore();
  const selectedNode = nodes.find((n: { id: string }) => n.id === selectedNodeId);

  if (!selectedNode) {
    return (
      <div style={{ padding: 16 }}>
        <div style={{ color: '#a0a0a0', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 12 }}>
          Canvas Properties
        </div>
        <div style={{ color: '#a0a0a0', fontSize: 12 }}>
          Click a node to inspect its properties.
        </div>
        <div style={{ marginTop: 16, borderTop: '1px solid #3a3a3a', paddingTop: 16 }}>
          <div style={{ color: '#a0a0a0', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 8 }}>
            Topology Info
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <Row label="Nodes" value={String(nodes.length)} />
          </div>
        </div>
      </div>
    );
  }

  const d = selectedNode.data as any;

  return (
    <div style={{ padding: 16 }}>
      <div style={{ color: '#a0a0a0', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 12 }}>
        {selectedNode.type} Properties
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <Field
          label="Label"
          value={d.label || ''}
          onChange={(v) => updateNodeData(selectedNode.id, { label: v })}
        />
        <Field
          label="Status"
          value={d.status || ''}
          onChange={(v) => updateNodeData(selectedNode.id, { status: v })}
        />
        {d.response_time !== undefined && (
          <Field
            label="Response Time (ms)"
            value={String(d.response_time)}
            onChange={(v) => updateNodeData(selectedNode.id, { response_time: Number(v) })}
            type="number"
          />
        )}
        {d.value !== undefined && (
          <Row label="Value" value={String(d.value)} />
        )}
      </div>

      <div style={{ marginTop: 16, borderTop: '1px solid #3a3a3a', paddingTop: 16 }}>
        <div style={{ color: '#a0a0a0', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 8 }}>
          Node Info
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <Row label="ID" value={selectedNode.id} mono />
          <Row label="Type" value={selectedNode.type || 'generic'} />
          <Row label="Position" value={`${Math.round(selectedNode.position.x)}, ${Math.round(selectedNode.position.y)}`} mono />
        </div>
      </div>
    </div>
  );
};

const Row: React.FC<{ label: string; value: string; mono?: boolean }> = ({ label, value, mono }) => (
  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
    <span style={{ color: '#a0a0a0', fontSize: 11 }}>{label}</span>
    <span style={{ color: '#e0e0e0', fontSize: 11, fontFamily: mono ? 'JetBrains Mono, monospace' : undefined }}>
      {value}
    </span>
  </div>
);

const Field: React.FC<{
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
}> = ({ label, value, onChange, type = 'text' }) => (
  <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
    <label style={{ color: '#a0a0a0', fontSize: 11 }}>{label}</label>
    <input
      type={type}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      style={{
        background: '#1e1e1e',
        border: '1px solid #3a3a3a',
        borderRadius: 4,
        color: '#e0e0e0',
        padding: '4px 8px',
        fontSize: 12,
        fontFamily: 'JetBrains Mono, monospace',
        outline: 'none',
        width: '100%',
      }}
      onFocus={(e) => (e.target.style.borderColor = '#00bcd4')}
      onBlur={(e) => (e.target.style.borderColor = '#3a3a3a')}
    />
  </div>
);

export default PropertiesPanel;
