import React, { useCallback, useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ReactFlow,
  Background,
  Controls,
  ControlButton,
  MiniMap,
  BackgroundVariant,
  SelectionMode,
  addEdge,
  useReactFlow,
  type Node,
} from '@xyflow/react';
import type { Connection } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { useTopologyStore } from '../../stores/topologyStore';
import { useAppStore } from '../../stores/appStore';
import { simulationApi } from '../../services/api';
import CylinderNode from '../nodes/CylinderNode';
import SensorNode from '../nodes/SensorNode';
import SwitchNode from '../nodes/SwitchNode';
import StepperNode from '../nodes/StepperNode';
import GenericNode from '../nodes/GenericNode';
import InputTerminalNode from '../nodes/InputTerminalNode';
import OutputTerminalNode from '../nodes/OutputTerminalNode';
import ContextMenu, { type MenuItem } from '../ContextMenu';

const nodeTypes = {
  cylinder: CylinderNode,
  sensor: SensorNode,
  switch: SwitchNode,
  stepper_pd: StepperNode,
  stepper: StepperNode,
  generic: GenericNode,
  input_terminal: InputTerminalNode,
  output_terminal: OutputTerminalNode,
};

const TopologyCanvas: React.FC = () => {
  const { t } = useTranslation();
  const {
    nodes,
    edges,
    onNodesChange,
    onEdgesChange,
    setEdges,
    setSelectedNodeId,
    updateNodeData,
    deleteNode,
    deleteEdge,
  } = useTopologyStore();

  const { currentUser } = useAppStore();
  const [isInteractive, setIsInteractive] = useState(true);

  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    node: Node;
  } | null>(null);

  // Delete key handler
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Delete') {
        const selectedNodes = nodes.filter(n => n.selected);
        const selectedEdges = edges.filter(e => e.selected);

        if (selectedNodes.length > 0) {
          // Check for safety-critical nodes
          const hasCritical = selectedNodes.some(n =>
            ['cylinder', 'stepper_pd'].includes(n.type || '')
          );

          if (hasCritical) {
            const confirmed = window.confirm(t('contextMenu.deleteConfirm'));
            if (confirmed) {
              selectedNodes.forEach(n => deleteNode(n.id));
            }
          } else {
            selectedNodes.forEach(n => deleteNode(n.id));
          }
        }

        if (selectedEdges.length > 0) {
          selectedEdges.forEach(e => deleteEdge(e.id));
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [nodes, edges, deleteNode, deleteEdge, t]);

  const onSelectionChange = useCallback(
    ({ nodes: selectedNodes }: { nodes: any[] }) => {
      setSelectedNodeId(selectedNodes.length === 1 ? selectedNodes[0].id : null);
    },
    [setSelectedNodeId]
  );

  const onConnect = useCallback(
    (connection: Connection) => {
      setEdges(addEdge({ ...connection, id: `e-${Date.now()}` }, edges));
    },
    [edges, setEdges]
  );

  const onNodeContextMenu = useCallback(
    (event: React.MouseEvent, node: Node) => {
      event.preventDefault();
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        node,
      });
    },
    []
  );

  const getFaultMenuItems = (node: Node): MenuItem[] => {
    const nodeType = node.type || 'generic';
    const items: MenuItem[] = [];

    // Fault injection options based on node type
    switch (nodeType) {
      case 'cylinder':
        items.push(
          {
            label: t('contextMenu.injectJammed'),
            onClick: () => injectFault(node.id, 'jammed'),
            badge: 'native',
            danger: true,
          },
          {
            label: t('contextMenu.injectMotionTimeout'),
            onClick: () => injectFault(node.id, 'motion_timeout'),
            badge: 'native',
            danger: true,
          }
        );
        break;
      case 'sensor':
        items.push(
          {
            label: t('contextMenu.injectStuckOn'),
            onClick: () => injectFault(node.id, 'stuck_on'),
            badge: 'native',
            danger: true,
          },
          {
            label: t('contextMenu.injectStuckOff'),
            onClick: () => injectFault(node.id, 'stuck_off'),
            badge: 'native',
            danger: true,
          },
          {
            label: t('contextMenu.injectChatter'),
            onClick: () => injectFault(node.id, 'chatter'),
            badge: 'native',
            danger: true,
          }
        );
        break;
      case 'switch':
        items.push(
          {
            label: t('contextMenu.injectStuckOn'),
            onClick: () => injectFault(node.id, 'stuck_on'),
            badge: 'native',
            danger: true,
          },
          {
            label: t('contextMenu.injectStuckOff'),
            onClick: () => injectFault(node.id, 'stuck_off'),
            badge: 'native',
            danger: true,
          }
        );
        break;
      case 'stepper':
      case 'stepper_pd':
        items.push(
          {
            label: t('contextMenu.injectLostStep'),
            onClick: () => injectFault(node.id, 'lost_step'),
            badge: 'native',
            danger: true,
          },
          {
            label: t('contextMenu.injectStall'),
            onClick: () => injectFault(node.id, 'stall'),
            badge: 'native',
            danger: true,
          },
          {
            label: t('contextMenu.injectDirectionReversed'),
            onClick: () => injectFault(node.id, 'direction_reversed'),
            badge: 'native',
            danger: true,
          }
        );
        break;
    }

    if (items.length > 0) {
      items.push({
        label: '─────────',
        onClick: () => {},
        disabled: true,
      });
    }

    items.push(
      {
        label: t('contextMenu.clearFaults'),
        onClick: () => clearFaults(node.id),
      },
      {
        label: '─────────',
        onClick: () => {},
        disabled: true,
      },
      {
        label: t('contextMenu.deleteNode'),
        onClick: () => {
          const isCritical = ['cylinder', 'stepper_pd'].includes(nodeType);
          if (isCritical) {
            const confirmed = window.confirm(t('contextMenu.deleteConfirm'));
            if (confirmed) {
              deleteNode(node.id);
              setContextMenu(null);
            }
          } else {
            deleteNode(node.id);
            setContextMenu(null);
          }
        },
        danger: true,
      }
    );

    return items;
  };

  const injectFault = async (nodeId: string, faultType: string) => {
    try {
      await simulationApi.injectFault(
        nodeId,
        faultType,
        undefined,
        currentUser?.name || 'unknown'
      );
      updateNodeData(nodeId, { status: 'fault', faultType });
      console.log(`Injected fault ${faultType} to node ${nodeId}`);
    } catch (error) {
      console.error('Failed to inject fault:', error);
      alert(t('notifications.injectFailed'));
    }
  };

  const clearFaults = async (nodeId: string) => {
    try {
      await simulationApi.clearFaults(nodeId, currentUser?.name || 'unknown');
      updateNodeData(nodeId, { status: 'idle', faultType: undefined });
      console.log(`Cleared faults for node ${nodeId}`);
    } catch (error) {
      console.error('Failed to clear faults:', error);
      alert(t('notifications.clearFailed'));
    }
  };

  return (
    <div style={{ width: '100%', height: '100%' }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onSelectionChange={onSelectionChange}
        onNodeContextMenu={onNodeContextMenu}
        nodeTypes={nodeTypes}
        selectionMode={SelectionMode.Partial}
        nodesDraggable={isInteractive}
        nodesConnectable={isInteractive}
        elementsSelectable={isInteractive}
        fitView
        style={{ background: '#1e1e1e' }}
        proOptions={{ hideAttribution: true }}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={20}
          size={1}
          color="#2a2a2a"
        />
        <Controls
          showZoom={false}
          showFitView={false}
          showInteractive={false}
          style={{
            background: '#2d2d2d',
            border: '1px solid #3a3a3a',
            borderRadius: 6,
          }}
        >
          <CanvasControls isInteractive={isInteractive} onToggleInteractive={() => setIsInteractive(v => !v)} />
        </Controls>
        <MiniMap
          style={{
            background: '#1a1a1a',
            border: '1px solid #3a3a3a',
          }}
          nodeColor={(node) => {
            const statusColors: Record<string, string> = {
              extended: '#00bcd4',
              on: '#52c41a',
              fault: '#f5222d',
              running: '#722ed1',
            };
            const d = node.data as any;
            return statusColors[d?.status] || '#4a4a4a';
          }}
          maskColor="rgba(0,0,0,0.4)"
        />
      </ReactFlow>
      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={getFaultMenuItems(contextMenu.node)}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
};

export default TopologyCanvas;

const CanvasControls: React.FC<{ isInteractive: boolean; onToggleInteractive: () => void }> = ({
  isInteractive,
  onToggleInteractive,
}) => {
  const { t } = useTranslation();
  const { zoomIn, zoomOut, fitView } = useReactFlow();
  return (
    <>
      <ControlButton onClick={() => zoomIn()} title={t('canvas.zoomIn')}>+</ControlButton>
      <ControlButton onClick={() => zoomOut()} title={t('canvas.zoomOut')}>−</ControlButton>
      <ControlButton onClick={() => fitView()} title={t('canvas.fitView')}>⊡</ControlButton>
      <ControlButton
        onClick={onToggleInteractive}
        title={isInteractive ? t('canvas.lockView') : t('canvas.unlockView')}
      >
        {isInteractive ? '🔓' : '🔒'}
      </ControlButton>
    </>
  );
};
