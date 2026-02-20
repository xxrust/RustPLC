import React, { useState, useCallback, useEffect, useRef } from 'react';
import type { Node } from '@xyflow/react';
import TopBar from '../components/TopBar';
import StatusBar from '../components/StatusBar';
import ComponentLibrary from '../components/ComponentLibrary';
import PropertiesPanel from '../components/PropertiesPanel';
import TopologyCanvas from '../components/canvas/TopologyCanvas';
import TickTimeline from '../components/replay/TickTimeline';
import { useTopologyStore } from '../stores/topologyStore';
import { useReplayStore } from '../stores/replayStore';
import { useAppStore } from '../stores/appStore';
import { topologyApi, traceApi, runApi } from '../services/api';
import type { NodeData } from '../stores/topologyStore';

interface Tab {
  id: string;
  label: string;
  view: 'topology' | 'replay' | 'scenario' | 'run' | 'diagnosis' | 'audit';
  dirty?: boolean;
}

let tabCounter = 1;

const SIDEBAR_WIDTH = 280;
const PANEL_WIDTH = 320;

const IDDELayout: React.FC = () => {
  const [tabs, setTabs] = useState<Tab[]>([
    { id: 'topology-1', label: 'Topology', view: 'topology' },
  ]);
  const [activeTabId, setActiveTabId] = useState('topology-1');
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(false);

  const { setNodes, setEdges, hasUnsavedChanges } = useTopologyStore();
  const { setSnapshots } = useReplayStore();
  const { currentProject } = useAppStore();

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

  // Load topology from API (or fall back to demo data)
  useEffect(() => {
    const projectId = currentProject || 'two_cylinder';

    topologyApi.getTopology(projectId)
      .then((res) => {
        const data = res.data as any;
        // If it's a component topology JSON with components array
        if (data.components && Array.isArray(data.components)) {
          const nodes: Node<NodeData>[] = data.components.map((comp: any, i: number) => ({
            id: comp.id,
            type: mapComponentType(comp.component_id || comp.type || 'generic'),
            position: comp.position || { x: 150 + (i % 3) * 200, y: 100 + Math.floor(i / 3) * 160 },
            data: {
              label: comp.id,
              type: mapComponentType(comp.component_id || comp.type || 'generic'),
              status: 'idle',
              ...comp.params,
            },
          }));
          const edges = (data.connections || []).map((conn: any, i: number) => ({
            id: `e-${i}`,
            source: conn.from,
            target: conn.to,
          }));
          setNodes(nodes);
          setEdges(edges);
          return;
        }
        // Fall back to demo data
        loadDemoData(setNodes, setEdges);
      })
      .catch(() => loadDemoData(setNodes, setEdges));

    // Load latest run trace for replay
    runApi.listRuns(1)
      .then((res) => {
        const runs = res.data as any[];
        if (runs.length === 0) { loadDemoSnapshots(setSnapshots); return; }
        const latestRun = runs[0];
        return traceApi.getTrace(latestRun.run_id).then((traceRes) => {
          const trace = traceRes.data as any;
          if (!trace.ticks || trace.ticks.length === 0) { loadDemoSnapshots(setSnapshots); return; }
          const snapshots = trace.ticks.map((tick: any) => ({
            tick: tick.tick,
            components: tick.component_states || {},
            io: {
              di: tick.digital_inputs,
              do: tick.digital_outputs,
              ai: tick.analog_inputs,
              ao: tick.analog_outputs,
            },
            events: [],
          }));
          setSnapshots(snapshots);
        });
      })
      .catch(() => loadDemoSnapshots(setSnapshots));
  }, [currentProject, setNodes, setEdges, setSnapshots]);

  const activeTab = tabs.find((t) => t.id === activeTabId);

  const handleTabClick = (id: string) => setActiveTabId(id);

  const handleTabClose = (id: string) => {
    const remaining = tabs.filter((t) => t.id !== id);
    if (remaining.length === 0) {
      const newTab: Tab = { id: `topology-${++tabCounter}`, label: 'Topology', view: 'topology' };
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
        {!leftCollapsed && (
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
                Components
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
        {leftCollapsed && (
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
            title="Show sidebar"
          >
            ›
          </button>
        )}

        {/* Main content area */}
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          {/* Canvas / view area */}
          <div
            style={{ flex: 1, overflow: 'hidden', position: 'relative' }}
            onDrop={handleCanvasDrop}
            onDragOver={handleCanvasDragOver}
          >
            <ViewContent view={activeTab?.view || 'topology'} />
          </div>

          {/* Tick timeline (only in replay view) */}
          {activeTab?.view === 'replay' && <TickTimeline />}
        </div>

        {/* Right properties panel */}
        {!rightCollapsed && (
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
                Properties
              </span>
            </div>
            <div style={{ flex: 1, overflowY: 'auto' }}>
              <PropertiesPanel />
            </div>
          </div>
        )}

        {/* Collapsed right toggle */}
        {rightCollapsed && (
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
            title="Show properties"
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
const ViewContent: React.FC<{ view: Tab['view'] }> = ({ view }) => {
  const { snapshots, currentTick } = useReplayStore();
  const { nodes, updateNodeData } = useTopologyStore();

  // Sync replay tick → node data
  useEffect(() => {
    if (view !== 'replay') return;
    const snapshot = snapshots[currentTick];
    if (!snapshot) return;
    nodes.forEach((node) => {
      const comp = snapshot.components[node.id];
      if (comp) updateNodeData(node.id, comp);
    });
  }, [currentTick, snapshots, view, nodes, updateNodeData]);

  switch (view) {
    case 'topology':
    case 'replay':
      return <TopologyCanvas />;
    case 'scenario':
      return <PlaceholderView title="Scenario / Recipe" description="Scenario YAML editor and visual timeline — Phase 2" />;
    case 'run':
      return <PlaceholderView title="Run & Gate" description="Trigger no-board-gate, commissioning-run, trace-doctor — Phase 2" />;
    case 'diagnosis':
      return <PlaceholderView title="Alarm & Diagnosis" description="Real-time alarms and diagnosis report — Phase 2" />;
    case 'audit':
      return <PlaceholderView title="Audit & Reports" description="Audit log and report export — Phase 3" />;
    default:
      return null;
  }
};

const PlaceholderView: React.FC<{ title: string; description: string }> = ({ title, description }) => (
  <div
    style={{
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      height: '100%',
      gap: 12,
      background: '#1e1e1e',
    }}
  >
    <div style={{ color: '#e0e0e0', fontSize: 18, fontWeight: 600 }}>{title}</div>
    <div style={{ color: '#a0a0a0', fontSize: 13 }}>{description}</div>
  </div>
);

export default IDDELayout;

// ── Helpers ──────────────────────────────────────────────────────────────────

function mapComponentType(raw: string): string {
  const t = raw.toLowerCase();
  if (t.includes('cylinder')) return 'cylinder';
  if (t.includes('sensor')) return 'sensor';
  if (t.includes('switch')) return 'switch';
  if (t.includes('stepper') || t.includes('motor')) return 'stepper_pd';
  return 'generic';
}

function loadDemoData(
  setNodes: (nodes: Node<NodeData>[]) => void,
  setEdges: (edges: any[]) => void
) {
  setNodes([
    { id: 'cyl_a', type: 'cylinder', position: { x: 300, y: 150 }, data: { label: 'cyl_a', type: 'cylinder', status: 'retracted', response_time: 200 } },
    { id: 'cyl_b', type: 'cylinder', position: { x: 300, y: 300 }, data: { label: 'cyl_b', type: 'cylinder', status: 'extended', response_time: 200 } },
    { id: 'sensor_a1', type: 'sensor', position: { x: 100, y: 120 }, data: { label: 'sensor_a1', type: 'sensor', status: 'off' } },
    { id: 'sensor_a2', type: 'sensor', position: { x: 100, y: 200 }, data: { label: 'sensor_a2', type: 'sensor', status: 'on' } },
    { id: 'sensor_b1', type: 'sensor', position: { x: 100, y: 280 }, data: { label: 'sensor_b1', type: 'sensor', status: 'off' } },
    { id: 'sensor_b2', type: 'sensor', position: { x: 100, y: 360 }, data: { label: 'sensor_b2', type: 'sensor', status: 'on' } },
  ]);
  setEdges([
    { id: 'e1', source: 'sensor_a1', target: 'cyl_a' },
    { id: 'e2', source: 'sensor_a2', target: 'cyl_a' },
    { id: 'e3', source: 'sensor_b1', target: 'cyl_b' },
    { id: 'e4', source: 'sensor_b2', target: 'cyl_b' },
  ]);
}

function loadDemoSnapshots(setSnapshots: (s: any[]) => void) {
  setSnapshots(Array.from({ length: 100 }, (_, i) => ({
    tick: i,
    components: {
      cyl_a: { status: i < 30 ? 'retracted' : i < 60 ? 'extended' : 'retracted' },
      cyl_b: { status: i < 50 ? 'extended' : 'retracted' },
      sensor_a1: { status: i >= 30 && i < 60 ? 'on' : 'off' },
      sensor_a2: { status: i < 30 ? 'on' : 'off' },
      sensor_b1: { status: i >= 50 ? 'on' : 'off' },
      sensor_b2: { status: i < 50 ? 'on' : 'off' },
    },
    events: i === 30
      ? [{ type: 'info' as const, message: 'cyl_a extended' }]
      : i === 75
      ? [{ type: 'error' as const, message: 'timeout detected' }]
      : [],
  })));
}
