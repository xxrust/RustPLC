import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { NodeData } from '../../stores/topologyStore';

interface GenericPropertiesEditorProps {
  nodeId: string;
  data: NodeData;
  onUpdate: (nodeId: string, data: Partial<NodeData>) => void;
}

const GenericPropertiesEditor: React.FC<GenericPropertiesEditorProps> = ({
  nodeId,
  data,
  onUpdate,
}) => {
  const { t } = useTranslation();
  const [formData, setFormData] = useState<Partial<NodeData>>({ ...data });
  const [jsonError, setJsonError] = useState<string>('');
  const [isDirty, setIsDirty] = useState(false);

  const handleFieldChange = (key: string, value: unknown) => {
    setFormData({ ...formData, [key]: value });
    setIsDirty(true);
  };

  const handleAddField = () => {
    const newKey = `field_${Object.keys(formData).length}`;
    setFormData({ ...formData, [newKey]: '' });
    setIsDirty(true);
  };

  const handleRemoveField = (key: string) => {
    const newData = { ...formData };
    delete newData[key];
    setFormData(newData);
    setIsDirty(true);
  };

  const handleSave = () => {
    if (!formData.label || formData.label.trim() === '') {
      setJsonError('Label is required');
      return;
    }

    onUpdate(nodeId, formData);
    setIsDirty(false);
    setJsonError('');
  };

  const handleRevert = () => {
    setFormData({ ...data });
    setIsDirty(false);
    setJsonError('');
  };

  return (
    <div style={{ padding: 16, color: '#e0e0e0' }}>
      <h3 style={{ margin: '0 0 16px 0', fontSize: 14, fontWeight: 600, color: '#00bcd4' }}>
        {t('properties.genericTitle')}
      </h3>

      <div style={{ marginBottom: 12, padding: 8, background: '#1e1e1e', borderRadius: 4, border: '1px solid #3a3a3a' }}>
        <div style={{ fontSize: 10, color: '#a0a0a0', marginBottom: 8 }}>
          {t('properties.keyValueEditor')}
        </div>
        {Object.entries(formData).map(([key, value]) => (
          <div key={key} style={{ display: 'flex', gap: 8, marginBottom: 8, alignItems: 'center' }}>
            <input
              type="text"
              value={key}
              disabled
              style={{
                flex: 1,
                padding: '4px 6px',
                background: '#2d2d2d',
                border: '1px solid #3a3a3a',
                borderRadius: 3,
                color: '#a0a0a0',
                fontSize: 11,
              }}
            />
            <input
              type="text"
              value={typeof value === 'object' ? JSON.stringify(value) : String(value)}
              onChange={(e) => {
                try {
                  const parsed = JSON.parse(e.target.value);
                  handleFieldChange(key, parsed);
                } catch {
                  handleFieldChange(key, e.target.value);
                }
              }}
              style={{
                flex: 2,
                padding: '4px 6px',
                background: '#1e1e1e',
                border: '1px solid #3a3a3a',
                borderRadius: 3,
                color: '#e0e0e0',
                fontSize: 11,
              }}
            />
            {key !== 'label' && key !== 'type' && (
              <button
                onClick={() => handleRemoveField(key)}
                style={{
                  padding: '4px 8px',
                  background: '#3a3a3a',
                  border: 'none',
                  borderRadius: 3,
                  color: '#f5222d',
                  fontSize: 11,
                  cursor: 'pointer',
                }}
              >
                ×
              </button>
            )}
          </div>
        ))}
        <button
          onClick={handleAddField}
          style={{
            width: '100%',
            padding: '4px 8px',
            background: '#3a3a3a',
            border: '1px dashed #5a5a5a',
            borderRadius: 3,
            color: '#a0a0a0',
            fontSize: 11,
            cursor: 'pointer',
            marginTop: 4,
          }}
        >
          {t('properties.addField')}
        </button>
      </div>

      {jsonError && (
        <div style={{ color: '#f5222d', fontSize: 10, marginBottom: 12, padding: 8, background: '#f5222d22', borderRadius: 4 }}>
          {jsonError}
        </div>
      )}

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
            {t('properties.save')}
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
            {t('properties.revert')}
          </button>
        </div>
      )}
    </div>
  );
};

export default GenericPropertiesEditor;
