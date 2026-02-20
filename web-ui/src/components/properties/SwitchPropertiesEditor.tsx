import React, { useState } from 'react';
import { switchSchema, type SwitchData } from '../../schemas/nodeSchemas';

interface SwitchPropertiesEditorProps {
  nodeId: string;
  data: any;
  onUpdate: (nodeId: string, data: Partial<any>) => void;
}

const SwitchPropertiesEditor: React.FC<SwitchPropertiesEditorProps> = ({
  nodeId,
  data,
  onUpdate,
}) => {
  const [formData, setFormData] = useState<SwitchData>({
    label: data.label || '',
    status: data.status || 'open',
    value: data.value || false,
  });
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [isDirty, setIsDirty] = useState(false);

  const handleChange = (field: keyof SwitchData, value: any) => {
    setFormData({ ...formData, [field]: value });
    setIsDirty(true);
    if (errors[field]) {
      setErrors({ ...errors, [field]: '' });
    }
  };

  const handleSave = () => {
    const result = switchSchema.safeParse(formData);
    if (!result.success) {
      const newErrors: Record<string, string> = {};
      result.error.issues.forEach((err) => {
        if (err.path[0]) {
          newErrors[err.path[0] as string] = err.message;
        }
      });
      setErrors(newErrors);
      return;
    }

    onUpdate(nodeId, formData);
    setIsDirty(false);
    setErrors({});
  };

  const handleRevert = () => {
    setFormData({
      label: data.label || '',
      status: data.status || 'open',
      value: data.value || false,
    });
    setIsDirty(false);
    setErrors({});
  };

  return (
    <div style={{ padding: 16, color: '#e0e0e0' }}>
      <h3 style={{ margin: '0 0 16px 0', fontSize: 14, fontWeight: 600, color: '#00bcd4' }}>
        Switch Properties
      </h3>

      <div style={{ marginBottom: 12 }}>
        <label style={{ display: 'block', fontSize: 11, marginBottom: 4, color: '#a0a0a0' }}>
          Label
        </label>
        <input
          type="text"
          value={formData.label}
          onChange={(e) => handleChange('label', e.target.value)}
          style={{
            width: '100%',
            padding: '6px 8px',
            background: '#1e1e1e',
            border: `1px solid ${errors.label ? '#f5222d' : '#3a3a3a'}`,
            borderRadius: 4,
            color: '#e0e0e0',
            fontSize: 12,
          }}
        />
        {errors.label && (
          <div style={{ color: '#f5222d', fontSize: 10, marginTop: 2 }}>{errors.label}</div>
        )}
      </div>

      <div style={{ marginBottom: 12 }}>
        <label style={{ display: 'block', fontSize: 11, marginBottom: 4, color: '#a0a0a0' }}>
          Status
        </label>
        <select
          value={formData.status}
          onChange={(e) => handleChange('status', e.target.value)}
          style={{
            width: '100%',
            padding: '6px 8px',
            background: '#1e1e1e',
            border: '1px solid #3a3a3a',
            borderRadius: 4,
            color: '#e0e0e0',
            fontSize: 12,
          }}
        >
          <option value="open">Open</option>
          <option value="closed">Closed</option>
          <option value="fault">Fault</option>
        </select>
      </div>

      <div style={{ marginBottom: 12 }}>
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 11, color: '#a0a0a0' }}>
          <input
            type="checkbox"
            checked={formData.value || false}
            onChange={(e) => handleChange('value', e.target.checked)}
            style={{ width: 14, height: 14 }}
          />
          Value (Boolean)
        </label>
      </div>

      {isDirty && (
        <div style={{ display: 'flex', gap: 8, marginTop: 16 }}>
          <button
            onClick={handleSave}
            style={{
              flex: 1,
              padding: '6px 12px',
              background: '#00bcd4',
              border: 'none',
              borderRadius: 4,
              color: '#1e1e1e',
              fontSize: 12,
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            Save
          </button>
          <button
            onClick={handleRevert}
            style={{
              flex: 1,
              padding: '6px 12px',
              background: '#3a3a3a',
              border: 'none',
              borderRadius: 4,
              color: '#e0e0e0',
              fontSize: 12,
              cursor: 'pointer',
            }}
          >
            Revert
          </button>
        </div>
      )}
    </div>
  );
};

export default SwitchPropertiesEditor;
