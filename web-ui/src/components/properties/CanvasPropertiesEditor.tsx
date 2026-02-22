import React from 'react';
import { useTranslation } from 'react-i18next';
import { useTopologyStore } from '../../stores/topologyStore';
import TagBatchEditor from './TagBatchEditor';
import TagVisualizationPanel from './TagVisualizationPanel';

const CanvasPropertiesEditor: React.FC = () => {
  const { t } = useTranslation();
  const { nodes, edges } = useTopologyStore();

  const nodeTypeCounts = nodes.reduce((acc, node) => {
    const type = node.type || 'generic';
    acc[type] = (acc[type] || 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  return (
    <div style={{ padding: 16, color: '#e0e0e0' }}>
      <h3 style={{ margin: '0 0 16px 0', fontSize: 14, fontWeight: 600, color: '#00bcd4' }}>
        {t('properties.canvasTitle')}
      </h3>

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>{t('properties.statistics')}</div>
        <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
            <span style={{ fontSize: 12, color: '#a0a0a0' }}>{t('properties.totalNodes')}:</span>
            <span style={{ fontSize: 12, fontWeight: 600 }}>{nodes.length}</span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
            <span style={{ fontSize: 12, color: '#a0a0a0' }}>{t('properties.totalEdges')}:</span>
            <span style={{ fontSize: 12, fontWeight: 600 }}>{edges.length}</span>
          </div>
        </div>
      </div>

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>{t('properties.nodeTypes')}</div>
        <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12 }}>
          {Object.entries(nodeTypeCounts).map(([type, count]) => (
            <div key={type} style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
              <span style={{ fontSize: 12, color: '#a0a0a0', textTransform: 'capitalize' }}>
                {t(`componentLibrary.${type}`, type.replace('_', ' '))}:
              </span>
              <span style={{ fontSize: 12, fontWeight: 600 }}>{count}</span>
            </div>
          ))}
          {Object.keys(nodeTypeCounts).length === 0 && (
            <div style={{ fontSize: 12, color: '#5a5a5a', textAlign: 'center' }}>
              {t('properties.noNodes')}
            </div>
          )}
        </div>
      </div>

      <TagVisualizationPanel />

      <TagBatchEditor />

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>{t('properties.instructions')}</div>
        <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12, fontSize: 11, lineHeight: 1.6 }}>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            {t('properties.instructionDrag')}
          </p>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            {t('properties.instructionConnect')}
          </p>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            {t('properties.instructionSelect')}
          </p>
          <p style={{ margin: '0 0 8px 0', color: '#a0a0a0' }}>
            {t('properties.instructionDelete')}
          </p>
          <p style={{ margin: '0', color: '#a0a0a0' }}>
            {t('properties.instructionRightClick')}
          </p>
        </div>
      </div>
    </div>
  );
};

export default CanvasPropertiesEditor;
