import React, { useState } from 'react';
import { useAppStore } from '../stores/appStore';
import { useTopologyStore } from '../stores/topologyStore';
import { topologyApi } from '../services/api';
import ValidationErrorPanel from './ValidationErrorPanel';
import type { ComponentTopology } from '../types';

interface Tab {
  id: string;
  label: string;
  view: 'topology' | 'replay' | 'scenario' | 'run' | 'diagnosis' | 'audit';
  dirty?: boolean;
}

interface TopBarProps {
  tabs: Tab[];
  activeTabId: string;
  onTabClick: (id: string) => void;
  onTabClose: (id: string) => void;
  onNewTab: (view: Tab['view'], label: string) => void;
}

const RUN_MODE_COLORS: Record<string, string> = {
  no_board: '#1890ff',
  hil_board: '#faad14',
  runtime_live: '#52c41a',
};

const RUN_MODE_LABELS: Record<string, string> = {
  no_board: 'No-Board',
  hil_board: 'HIL',
  runtime_live: 'Live',
};

const TopBar: React.FC<TopBarProps> = ({ tabs, activeTabId, onTabClick, onTabClose, onNewTab }) => {
  const { runMode, currentProject, hasUnsavedChanges, alarmCount, currentUser } = useAppStore();
  const { nodes, edges, setHasUnsavedChanges } = useTopologyStore();
  const [showNewTab, setShowNewTab] = useState(false);
  const [saving, setSaving] = useState(false);
  const [validationErrors, setValidationErrors] = useState<any[]>([]);

  const totalAlarms = alarmCount.critical + alarmCount.warning;

  const handleSave = async () => {
    if (!currentProject) {
      alert('No project selected');
      return;
    }

    try {
      setSaving(true);

      // Convert React Flow state to ComponentTopology format
      const topology: ComponentTopology = {
        schema_version: 1,
        component_library: { schema_version: 1, components: [] },
        components: nodes.map((n) => ({
          id: n.id,
          component_id: n.type || 'generic',
          params: n.data,
          position: n.position,
        })),
        connections: edges.map((e) => ({
          from: e.source,
          to: e.target,
        })),
      };

      // Validate via API
      const validation = await topologyApi.validateTopology(topology);
      if (!validation.data.valid) {
        setValidationErrors(
          validation.data.errors.map((err: string) => ({
            code: 'VALIDATION_ERROR',
            path: 'topology',
            message: err,
          }))
        );
        return;
      }

      // Save via API
      await topologyApi.saveTopology(currentProject, topology);
      setHasUnsavedChanges(false);
      alert('Topology saved successfully');
    } catch (error: any) {
      console.error('Failed to save topology:', error);
      alert(error.response?.data?.message || 'Failed to save topology');
    } finally {
      setSaving(false);
    }
  };

  const NEW_TAB_OPTIONS: Array<{ view: Tab['view']; label: string }> = [
    { view: 'topology', label: 'Topology' },
    { view: 'replay', label: 'Tick Replay' },
    { view: 'scenario', label: 'Scenario' },
    { view: 'run', label: 'Run & Gate' },
    { view: 'diagnosis', label: 'Diagnosis' },
    { view: 'audit', label: 'Audit' },
  ];

  return (
    <div
      style={{
        height: 56,
        background: '#2d2d2d',
        borderBottom: '1px solid #3a3a3a',
        display: 'flex',
        alignItems: 'stretch',
        flexShrink: 0,
        position: 'relative',
        zIndex: 10,
      }}
    >
      {/* Logo */}
      <div
        style={{
          padding: '0 16px',
          display: 'flex',
          alignItems: 'center',
          borderRight: '1px solid #3a3a3a',
          gap: 8,
          flexShrink: 0,
        }}
      >
        <span style={{ color: '#00bcd4', fontWeight: 700, fontSize: 15, letterSpacing: '-0.02em' }}>
          RustPLC
        </span>
        {currentProject && (
          <span style={{ color: '#a0a0a0', fontSize: 12 }}>/ {currentProject}</span>
        )}
        {hasUnsavedChanges && (
          <span style={{ color: '#faad14', fontSize: 14 }} title="Unsaved changes">●</span>
        )}
      </div>

      {/* Save button */}
      {hasUnsavedChanges && (
        <button
          onClick={handleSave}
          disabled={saving}
          style={{
            padding: '0 16px',
            background: saving ? '#3a3a3a' : '#00bcd4',
            border: 'none',
            borderRight: '1px solid #3a3a3a',
            color: saving ? '#5a5a5a' : '#1e1e1e',
            fontSize: 12,
            fontWeight: 600,
            cursor: saving ? 'wait' : 'pointer',
            display: 'flex',
            alignItems: 'center',
            gap: 6,
          }}
        >
          {saving ? '⏳ Saving...' : '💾 Save'}
        </button>
      )}

      {/* Tabs */}
      <div style={{ display: 'flex', alignItems: 'stretch', flex: 1, overflowX: 'auto' }}>
        {tabs.map((tab) => (
          <div
            key={tab.id}
            onClick={() => onTabClick(tab.id)}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              padding: '0 14px',
              cursor: 'pointer',
              borderRight: '1px solid #3a3a3a',
              borderBottom: tab.id === activeTabId ? '2px solid #00bcd4' : '2px solid transparent',
              background: tab.id === activeTabId ? '#1e1e1e' : 'transparent',
              color: tab.id === activeTabId ? '#e0e0e0' : '#a0a0a0',
              fontSize: 13,
              flexShrink: 0,
              userSelect: 'none',
            }}
          >
            {tab.dirty && <span style={{ color: '#faad14', fontSize: 10 }}>●</span>}
            <span>{tab.label}</span>
            <button
              onClick={(e) => { e.stopPropagation(); onTabClose(tab.id); }}
              style={{
                background: 'none',
                border: 'none',
                color: '#5a5a5a',
                cursor: 'pointer',
                padding: '0 2px',
                fontSize: 14,
                lineHeight: 1,
                display: 'flex',
                alignItems: 'center',
              }}
              onMouseEnter={(e) => (e.currentTarget.style.color = '#e0e0e0')}
              onMouseLeave={(e) => (e.currentTarget.style.color = '#5a5a5a')}
            >
              ×
            </button>
          </div>
        ))}

        {/* New tab button */}
        <div style={{ position: 'relative' }}>
          <button
            onClick={() => setShowNewTab(!showNewTab)}
            style={{
              background: 'none',
              border: 'none',
              color: '#a0a0a0',
              cursor: 'pointer',
              padding: '0 12px',
              height: '100%',
              fontSize: 18,
              display: 'flex',
              alignItems: 'center',
            }}
            title="New tab"
          >
            +
          </button>
          {showNewTab && (
            <div
              style={{
                position: 'absolute',
                top: '100%',
                left: 0,
                background: '#2d2d2d',
                border: '1px solid #3a3a3a',
                borderRadius: 6,
                zIndex: 100,
                minWidth: 160,
                boxShadow: '0 4px 16px rgba(0,0,0,0.4)',
              }}
            >
              {NEW_TAB_OPTIONS.map((opt) => (
                <button
                  key={opt.view}
                  onClick={() => { onNewTab(opt.view, opt.label); setShowNewTab(false); }}
                  style={{
                    display: 'block',
                    width: '100%',
                    background: 'none',
                    border: 'none',
                    color: '#e0e0e0',
                    padding: '8px 16px',
                    textAlign: 'left',
                    cursor: 'pointer',
                    fontSize: 13,
                  }}
                  onMouseEnter={(e) => (e.currentTarget.style.background = '#3a3a3a')}
                  onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Right side: run mode + alarms + user */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          padding: '0 16px',
          borderLeft: '1px solid #3a3a3a',
          flexShrink: 0,
        }}
      >
        {/* Run mode badge */}
        <div
          style={{
            background: RUN_MODE_COLORS[runMode] + '22',
            border: `1px solid ${RUN_MODE_COLORS[runMode]}`,
            borderRadius: 4,
            padding: '2px 8px',
            color: RUN_MODE_COLORS[runMode],
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: '0.04em',
          }}
        >
          {RUN_MODE_LABELS[runMode]}
        </div>

        {/* Alarm counter */}
        {totalAlarms > 0 && (
          <div
            style={{
              background: '#f5222d22',
              border: '1px solid #f5222d',
              borderRadius: 4,
              padding: '2px 8px',
              color: '#f5222d',
              fontSize: 11,
              fontWeight: 600,
              cursor: 'pointer',
            }}
            title={`${alarmCount.critical} critical, ${alarmCount.warning} warning`}
          >
            ⚠ {totalAlarms}
          </div>
        )}

        {/* User */}
        <div style={{ color: '#a0a0a0', fontSize: 12 }}>
          {currentUser?.name}
          <span style={{ color: '#4a4a4a', marginLeft: 4 }}>({currentUser?.role})</span>
        </div>
      </div>

      {/* Validation error panel */}
      {validationErrors.length > 0 && (
        <ValidationErrorPanel
          errors={validationErrors}
          onClose={() => setValidationErrors([])}
        />
      )}
    </div>
  );
};

export default TopBar;
