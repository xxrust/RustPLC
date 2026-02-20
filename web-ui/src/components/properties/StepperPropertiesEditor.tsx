import React, { useState } from 'react';
import { stepperSchema, type StepperData } from '../../schemas/nodeSchemas';

interface StepperPropertiesEditorProps {
  nodeId: string;
  data: any;
  onUpdate: (nodeId: string, data: Partial<any>) => void;
}

const StepperPropertiesEditor: React.FC<StepperPropertiesEditorProps> = ({
  nodeId,
  data,
  onUpdate,
}) => {
  const [formData, setFormData] = useState<StepperData>({
    label: data.label || '',
    direction: data.direction || 'stopped',
    enable: data.enable || false,
    position: data.position || 0,
    steps_per_rev: data.steps_per_rev || 200,
  });
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [isDirty, setIsDirty] = useState(false);

  const handleChange = (field: keyof StepperData, value: any) => {
    setFormData({ ...formData, [field]: value });
    setIsDirty(true);
    if (errors[field]) {
      setErrors({ ...errors, [field]: '' });
    }
  };

  const handleSave = () => {
    const result = stepperSchema.safeParse(formData);
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
      direction: data.direction || 'stopped',
      enable: data.enable || false,
      position: data.position || 0,
      steps_per_rev: data.steps_per_rev || 200,
    });
    setIsDirty(false);
    setErrors({});
  };

  return (
    <div style={{ padding: 16, color: '#e0e0e0' }}>
      <h3 style={{ margin: '0 0 16px 0', fontSize: 14, fontWeight: 600, color: '#00bcd4' }}>
        Stepper Motor Properties
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
          Direction
        </label>
        <select
          value={formData.direction}
          onChange={(e) => handleChange('direction', e.target.value)}
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
          <option value="forward">Forward</option>
          <option value="reverse">Reverse</option>
          <option value="stopped">Stopped</option>
        </select>
      </div>

      <div style={{ marginBottom: 12 }}>
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 11, color: '#a0a0a0' }}>
          <input
            type="checkbox"
            checked={formData.enable || false}
            onChange={(e) => handleChange('enable', e.target.checked)}
            style={{ width: 14, height: 14 }}
          />
          Enable
        </label>
      </div>

      <div style={{ marginBottom: 12 }}>
        <label style={{ display: 'block', fontSize: 11, marginBottom: 4, color: '#a0a0a0' }}>
          Position (steps)
        </label>
        <input
          type="number"
          value={formData.position}
          onChange={(e) => handleChange('position', parseFloat(e.target.value))}
          style={{
            width: '100%',
            padding: '6px 8px',
            background: '#1e1e1e',
            border: `1px solid ${errors.position ? '#f5222d' : '#3a3a3a'}`,
            borderRadius: 4,
            color: '#e0e0e0',
            fontSize: 12,
          }}
        />
        {errors.position && (
          <div style={{ color: '#f5222d', fontSize: 10, marginTop: 2 }}>{errors.position}</div>
        )}
      </div>

      <div style={{ marginBottom: 12 }}>
        <label style={{ display: 'block', fontSize: 11, marginBottom: 4, color: '#a0a0a0' }}>
          Steps per Revolution
        </label>
        <input
          type="number"
          value={formData.steps_per_rev}
          onChange={(e) => handleChange('steps_per_rev', parseFloat(e.target.value))}
          style={{
            width: '100%',
            padding: '6px 8px',
            background: '#1e1e1e',
            border: `1px solid ${errors.steps_per_rev ? '#f5222d' : '#3a3a3a'}`,
            borderRadius: 4,
            color: '#e0e0e0',
            fontSize: 12,
          }}
        />
        {errors.steps_per_rev && (
          <div style={{ color: '#f5222d', fontSize: 10, marginTop: 2 }}>{errors.steps_per_rev}</div>
        )}
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

export default StepperPropertiesEditor;
