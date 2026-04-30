import React, { useState, useCallback, useEffect, useRef } from 'react';
import type { Node } from '@xyflow/react';
import { useTranslation } from 'react-i18next';
import TopBar from '../components/TopBar';
import StatusBar from '../components/StatusBar';
import ComponentLibrary from '../components/ComponentLibrary';
import PropertiesPanel from '../components/PropertiesPanel';
import TopologyCanvas from '../components/canvas/TopologyCanvas';
import RunPage from '../pages/RunPage';
import DiagnosisPage from '../pages/DiagnosisPage';
import ScenarioPage from '../pages/ScenarioPage';
import ReplayPage from '../pages/ReplayPage';
import AuditPage from '../pages/AuditPage';
import { useTopologyStore } from '../stores/topologyStore';
import { useAppStore } from '../stores/appStore';
import { topologyApi } from '../services/api';
import type { NodeData } from '../stores/topologyStore';
import type { DevicePortMetadata } from '../types';
import { normalizeDeviceTags } from '../utils/deviceTags';
import { getDefaultPortsForNodeType, getEdgeSignalLabel } from '../utils/portContract';

interface Tab {
  id: string;
  label: string;
  view: 'topology' | 'replay' | 'scenario' | 'run' | 'diagnosis' | 'audit';
  dirty?: boolean;
}

let tabCounter = 1;

const SIDEBAR_WIDTH = 280;
const PANEL_WIDTH = 320;
const SHARED_TOPOLOGY_VIEWS: Tab['view'][] = ['scenario', 'run', 'diagnosis', 'replay', 'audit'];

const IDDELayout: React.FC = () => {
  const { t } = useTranslation();
  const [tabs, setTabs] = useState<Tab[]>([
    { id: 'topology-1', label: t('tabs.topology'), view: 'topology' },
    { id: 'scenario-1', label: t('tabs.scenario'), view: 'scenario' },
    { id: 'run-1', label: t('tabs.run'), view: 'run' },
    { id: 'diagnosis-1', label: t('tabs.diagnosis'), view: 'diagnosis' },
    { id: 'replay-1', label: t('tabs.replay'), view: 'replay' },
    { id: 'audit-1', label: t('tabs.audit'), view: 'audit' },
  ]);
  const [activeTabId, setActiveTabId] = useState('topology-1');
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(false);

  const { setNodes, setEdges, hasUnsavedChanges } = useTopologyStore();
  const { currentProject, currentProjectContent } = useAppStore();

  const dragTypeRef = useRef<{ type: string; label: string } | null>(null);

  // Warn on navigation with unsaved changes
  useEffect(() => {
    const handleBeforeUnload = (e: BeforeUnloadEvent) => {
      if (hasUnsavedChanges) {
        e.preventDefault();
        e.returnValue = '';
      }
    };

    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => window.removeEventListener('beforeunload', handleBeforeUnload);
  }, [hasUnsavedChanges]);

  // Load topology from API (or parse local PLC content)
  useEffect(() => {
    const projectId = currentProject;
    let cancelled = false;

    const applyTopology = (data: any) => {
      const { nodes, edges } = toCanvasTopology(data);
      if (!cancelled) {
        setNodes(nodes);
        setEdges(edges);
      }
    };

    const clearTopology = () => {
      if (!cancelled) {
        setNodes([]);
        setEdges([]);
      }
    };

    const loadTopology = async () => {
      if (!projectId) {
        clearTopology();
        return;
      }

      try {
        if (currentProjectContent) {
          const parsed = await topologyApi.parsePlc(currentProjectContent);
          applyTopology(parsed.data);
          return;
        }

        const res = await topologyApi.getTopology(projectId);
        const data = res.data as any;

        if (data.components && Array.isArray(data.components)) {
          applyTopology(data);
          return;
        }

        if (typeof data.content === 'string') {
          const parsed = await topologyApi.parsePlc(data.content);
          applyTopology(parsed.data);
          return;
        }

        clearTopology();
      } catch {
        clearTopology();
      }
    };

    loadTopology();

    return () => {
      cancelled = true;
    };
  }, [currentProject, currentProjectContent, setNodes, setEdges]);

  const activeTab = tabs.find((t) => t.id === activeTabId);
  const showEditorChrome = activeTab?.view === 'topology';
  const showSharedTopologyWorkspace = Boolean(
    activeTab && SHARED_TOPOLOGY_VIEWS.includes(activeTab.view)
  );

  const handleTabClick = (id: string) => setActiveTabId(id);

  const handleTabClose = (id: string) => {
    const remaining = tabs.filter((t) => t.id !== id);
    if (remaining.length === 0) {
      const newTab: Tab = { id: `topology-${++tabCounter}`, label: t('tabs.topology'), view: 'topology' };
      setTabs([newTab]);
      setActiveTabId(newTab.id);
    } else {
      setTabs(remaining);
      if (activeTabId === id) setActiveTabId(remaining[remaining.length - 1].id);
    }
  };

  const handleNewTab = (view: Tab['view'], label: string) => {
    const newTab: Tab = { id: `${view}-${++tabCounter}`, label, view };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(newTab.id);
  };

  // Drag-and-drop from component library
  const handleDragStart = useCallback((type: string, label: string) => {
    dragTypeRef.current = { type, label };
  }, []);

  const handleCanvasDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      if (!dragTypeRef.current) return;
      const { type, label } = dragTypeRef.current;
      const rect = e.currentTarget.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const y = e.clientY - rect.top;
      const newNode: Node<NodeData> = {
        id: `${type}-${Date.now()}`,
        type,
        position: { x, y },
        data: { label: `${label}_${Date.now() % 1000}`, type, status: 'idle' },
      };
      useTopologyStore.getState().addNode(newNode);
      dragTypeRef.current = null;
    },
    []
  );

  const handleCanvasDragOver = (e: React.DragEvent) => e.preventDefault();

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: '#1a1a1a', overflow: 'hidden' }}>
      <TopBar
        tabs={tabs}
        activeTabId={activeTabId}
        onTabClick={handleTabClick}
        onTabClose={handleTabClose}
        onNewTab={handleNewTab}
      />

      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        {/* Left sidebar */}
        {showEditorChrome && !leftCollapsed && (
          <div
            style={{
              width: SIDEBAR_WIDTH,
              background: '#2d2d2d',
              borderRight: '1px solid #3a3a3a',
              display: 'flex',
              flexDirection: 'column',
              flexShrink: 0,
              overflow: 'hidden',
            }}
          >
            <div
              style={{
                padding: '8px 12px',
                borderBottom: '1px solid #3a3a3a',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <span style={{ color: '#a0a0a0', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.08em' }}>
                {t('componentLibrary.title')}
              </span>
              <button
                onClick={() => setLeftCollapsed(true)}
                style={{ background: 'none', border: 'none', color: '#5a5a5a', cursor: 'pointer', fontSize: 14 }}
              >
                ‹
              </button>
            </div>
            <ComponentLibrary onDragStart={handleDragStart} />
          </div>
        )}

        {/* Collapsed left toggle */}
        {showEditorChrome && leftCollapsed && (
          <button
            onClick={() => setLeftCollapsed(false)}
            style={{
              width: 20,
              background: '#2d2d2d',
              border: 'none',
              borderRight: '1px solid #3a3a3a',
              color: '#5a5a5a',
              cursor: 'pointer',
              fontSize: 12,
              flexShrink: 0,
            }}
            title={t('idde.showSidebar')}
          >
            ›
          </button>
        )}

        {/* Main content area */}
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          {/* Canvas / view area */}
          {(activeTab?.view === 'topology' || !activeTab) ? (
            <>
              <div
                style={{ flex: 1, overflow: 'hidden', position: 'relative' }}
                onDrop={handleCanvasDrop}
                onDragOver={handleCanvasDragOver}
              >
                <ViewContent view={activeTab?.view || 'topology'} />
              </div>
            </>
          ) : showSharedTopologyWorkspace ? (
            <div
              style={{
                flex: 1,
                display: 'grid',
                gridTemplateColumns: 'minmax(520px, 1.25fr) minmax(420px, 0.95fr)',
                overflow: 'hidden',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  minWidth: 0,
                  borderRight: '1px solid #2d2d2d',
                  background: '#161616',
                }}
              >
                <div
                  style={{
                    padding: '10px 14px',
                    borderBottom: '1px solid #2d2d2d',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                  }}
                >
                  <span style={{ color: '#e5e7eb', fontSize: 13, fontWeight: 600 }}>
                    {t('idde.sharedTopologyTitle')}
                  </span>
                  <span style={{ color: '#94a3b8', fontSize: 12 }}>
                    {t('idde.sharedTopologyHint')}
                  </span>
                </div>
                <div style={{ flex: 1, minHeight: 0, position: 'relative' }}>
                  <TopologyCanvas readOnly />
                </div>
              </div>

              <div style={{ flex: 1, overflowY: 'auto', background: '#1e1e1e' }}>
                <ViewContent view={activeTab.view} embedded />
              </div>
            </div>
          ) : (
            <div style={{ flex: 1, overflowY: 'auto', background: '#1e1e1e' }}>
              <ViewContent view={activeTab.view} />
            </div>
          )}
        </div>

        {/* Right properties panel */}
        {showEditorChrome && !rightCollapsed && (
          <div
            style={{
              width: PANEL_WIDTH,
              background: '#2d2d2d',
              borderLeft: '1px solid #3a3a3a',
              display: 'flex',
              flexDirection: 'column',
              flexShrink: 0,
              overflow: 'hidden',
            }}
          >
            <div
              style={{
                padding: '8px 12px',
                borderBottom: '1px solid #3a3a3a',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <button
                onClick={() => setRightCollapsed(true)}
                style={{ background: 'none', border: 'none', color: '#5a5a5a', cursor: 'pointer', fontSize: 14 }}
              >
                ›
              </button>
              <span style={{ color: '#a0a0a0', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.08em' }}>
                {t('properties.title')}
              </span>
            </div>
            <div style={{ flex: 1, overflowY: 'auto' }}>
              <PropertiesPanel />
            </div>
          </div>
        )}

        {/* Collapsed right toggle */}
        {showEditorChrome && rightCollapsed && (
          <button
            onClick={() => setRightCollapsed(false)}
            style={{
              width: 20,
              background: '#2d2d2d',
              border: 'none',
              borderLeft: '1px solid #3a3a3a',
              color: '#5a5a5a',
              cursor: 'pointer',
              fontSize: 12,
              flexShrink: 0,
            }}
            title={t('idde.showProperties')}
          >
            ‹
          </button>
        )}
      </div>

      <StatusBar />
    </div>
  );
};

// View content dispatcher
const ViewContent: React.FC<{ view: Tab['view']; embedded?: boolean }> = ({
  view,
  embedded = false,
}) => {
  const pageStyle = { padding: embedded ? 20 : 24 };

  switch (view) {
    case 'topology':
      return <TopologyCanvas />;
    case 'scenario':
      return <div style={pageStyle}><ScenarioPage /></div>;
    case 'run':
      return <div style={pageStyle}><RunPage /></div>;
    case 'diagnosis':
      return <div style={pageStyle}><DiagnosisPage /></div>;
    case 'audit':
      return <div style={pageStyle}><AuditPage /></div>;
    case 'replay':
      return <div style={pageStyle}><ReplayPage /></div>;
    default:
      return null;
  }
};

export default IDDELayout;

// ── Helpers ──────────────────────────────────────────────────────────────────

function toCanvasTopology(data: any): { nodes: Node<NodeData>[]; edges: Array<{ id: string; source: string; target: string }> } {
  const edges = (data.connections || []).map((conn: any, i: number) => {
    const fromEndpoint = parseEndpoint(conn.from);
    const toEndpoint = parseEndpoint(conn.to);
    const sourceHandle = normalizeHandleId(conn.from_port ?? fromEndpoint.portId);
    const targetHandle = normalizeHandleId(conn.to_port ?? toEndpoint.portId);
    const edge: any = {
      id: `e-${i}`,
      source: fromEndpoint.nodeId,
      target: toEndpoint.nodeId,
      data:
        typeof conn.relation === 'string' && conn.relation
          ? { relation: conn.relation }
          : undefined,
    };
    if (sourceHandle) {
      edge.sourceHandle = sourceHandle;
    }
    if (targetHandle) {
      edge.targetHandle = targetHandle;
    }
    edge.label = getEdgeSignalLabel(sourceHandle, targetHandle, conn.signal);
    return edge;
  });

  const inferredPortsByNode = buildInferredPortsByNode(data.components || [], edges);

  const nodes: Node<NodeData>[] = (data.components || []).map((comp: any, i: number) => {
    const nodeType = mapComponentType(
      comp.component_id || comp.type || 'generic',
      comp.params?.device_type,
      comp.params?.endpoint_kind
    );
    const explicitPorts = Array.isArray(comp.params?.ports) ? comp.params.ports : [];
    const inferredPorts = inferredPortsByNode.get(comp.id) || [];

    return {
      id: comp.id,
      type: nodeType,
      position: comp.position || { x: 150 + (i % 3) * 240, y: 100 + Math.floor(i / 3) * 180 },
      data: {
        label: comp.id,
        type: nodeType,
        status: 'idle',
        ...comp.params,
        ports: mergePorts(explicitPorts, inferredPorts),
        tags: normalizeDeviceTags(comp.params?.tags),
      },
    };
  });
  return { nodes, edges };
}

function parseEndpoint(raw: string): { nodeId: string; portId?: string } {
  if (!raw) {
    return { nodeId: raw };
  }
  const idx = raw.indexOf('.');
  if (idx < 0) {
    return { nodeId: raw };
  }
  const nodeId = raw.slice(0, idx);
  const portId = raw.slice(idx + 1).trim();
  return {
    nodeId,
    portId: portId || undefined,
  };
}

function normalizeHandleId(raw: unknown): string | undefined {
  if (typeof raw !== 'string') {
    return undefined;
  }
  const trimmed = raw.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function mapComponentType(raw: string, deviceType?: string, endpointKind?: string): string {
  if (endpointKind === 'controller_port') {
    if (deviceType?.toLowerCase().includes('input')) return 'input_terminal';
    if (deviceType?.toLowerCase().includes('output')) return 'output_terminal';
  }

  // 优先根据 device_type 判断
  if (deviceType) {
    const dt = deviceType.toLowerCase();
    if (dt === 'digital_input') return 'input_terminal';
    if (dt === 'digital_output') return 'output_terminal';
    if (dt === 'analog_input') return 'input_terminal';
    if (dt === 'analog_output') return 'output_terminal';
  }

  // 回退到 component_id 判断
  const t = raw.toLowerCase();
  if (t.includes('cylinder')) return 'cylinder';
  if (t.includes('sensor')) return 'sensor';
  if (t.includes('switch')) return 'switch';
  if (t.includes('plc') || t.includes('controller')) return 'generic';
  if (t.includes('stepper') || t.includes('motor')) return 'stepper_pd';
  return 'generic';
}

function buildInferredPortsByNode(
  components: any[],
  edges: Array<{ source: string; target: string; sourceHandle?: string; targetHandle?: string }>
): Map<string, DevicePortMetadata[]> {
  const componentById = new Map(
    components.map((comp) => [
      comp.id,
      mapComponentType(
        comp.component_id || comp.type || 'generic',
        comp.params?.device_type,
        comp.params?.endpoint_kind
      ),
    ])
  );
  const portMap = new Map<string, Map<string, DevicePortMetadata>>();

  const ensurePort = (nodeId: string, portId: string, role: 'producer' | 'consumer') => {
    const nodeType = componentById.get(nodeId);
    const defaultPorts = getDefaultPortsForNodeType(nodeType);
    const defaultPort = defaultPorts.find((port) => port.id === portId);
    const inferredType = defaultPort?.type || inferPortType(portId);

    let nodePorts = portMap.get(nodeId);
    if (!nodePorts) {
      nodePorts = new Map<string, DevicePortMetadata>();
      portMap.set(nodeId, nodePorts);
    }

    const existing = nodePorts.get(portId);
    if (existing) {
      nodePorts.set(portId, {
        ...existing,
        type: existing.type === 'generic' ? inferredType : existing.type,
        role:
          existing.role === role || existing.role === 'bidirectional'
            ? existing.role
            : 'bidirectional',
      });
      return;
    }

    nodePorts.set(portId, {
      id: portId,
      type: inferredType,
      role,
    });
  };

  edges.forEach((edge) => {
    if (edge.sourceHandle) {
      ensurePort(edge.source, edge.sourceHandle, 'producer');
    }
    if (edge.targetHandle) {
      ensurePort(edge.target, edge.targetHandle, 'consumer');
    }
  });

  return new Map(
    Array.from(portMap.entries()).map(([nodeId, ports]) => [
      nodeId,
      Array.from(ports.values()),
    ])
  );
}

function mergePorts(
  explicitPorts: DevicePortMetadata[],
  inferredPorts: DevicePortMetadata[]
): DevicePortMetadata[] {
  if (explicitPorts.length === 0) {
    return inferredPorts;
  }

  const merged = new Map<string, DevicePortMetadata>();
  explicitPorts.forEach((port) => merged.set(port.id, { ...port }));

  inferredPorts.forEach((port) => {
    const existing = merged.get(port.id);
    if (!existing) {
      merged.set(port.id, { ...port });
      return;
    }
    merged.set(port.id, {
      id: existing.id,
      type: existing.type === 'generic' ? port.type : existing.type,
      role:
        existing.role === port.role || existing.role === 'bidirectional'
          ? existing.role
          : 'bidirectional',
    });
  });

  return Array.from(merged.values());
}

function inferPortType(portId: string): DevicePortMetadata['type'] {
  const id = portId.toLowerCase();
  if (/^(ai|ao)\d+$/.test(id) || id.includes('analog') || id.includes('freq') || id.includes('pressure')) {
    return 'analog';
  }
  if (
    id.includes('extended') ||
    id.includes('retracted') ||
    id.includes('fault') ||
    id.includes('ready') ||
    id.includes('position') ||
    id.includes('running') ||
    id.includes('home') ||
    id.includes('target')
  ) {
    return 'logical';
  }
  return 'digital';
}

