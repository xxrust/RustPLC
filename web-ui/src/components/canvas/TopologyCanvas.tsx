import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
  type Edge,
  type Node,
  type ReactFlowInstance,
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
import type { NodeData } from '../../stores/topologyStore';
import { CanvasInteractionContext } from './CanvasInteractionContext';
import {
  buildTagGroupColorMap,
  getPrimaryTagValue,
  resolveLocationFocusNodeIds,
  resolveTagFilterNodeIds,
} from '../../utils/tagVisualization';
import {
  canPortConsume,
  canPortProduce,
  findPortById,
  getEdgeSignalLabel,
  isPortTypeCompatible,
  isPortTypeCompatibleForRelation,
} from '../../utils/portContract';
import type { DevicePortMetadata } from '../../types';

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

interface PortResolution {
  handleId?: string;
  port?: DevicePortMetadata;
  usedFallbackContract: boolean;
  inferredHandle: boolean;
}

interface TopologyCanvasProps {
  readOnly?: boolean;
  showReadOnlyNotice?: boolean;
}

const TopologyCanvas: React.FC<TopologyCanvasProps> = ({ readOnly = false, showReadOnlyNotice = true }) => {
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
    tagFilter,
    tagGrouping,
    locationFocus,
  } = useTopologyStore();

  const { currentUser } = useAppStore();
  const liveSimulationEnabled = false;
  const [isInteractive, setIsInteractive] = useState(true);
  const reactFlowRef = useRef<ReactFlowInstance<Node, Edge> | null>(null);

  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    node: Node;
  } | null>(null);
  const [connectionWarning, setConnectionWarning] = useState<string | null>(null);

  const nodesById = useMemo(
    () => new Map(nodes.map((node) => [node.id, node])),
    [nodes]
  );

  const fallbackPortNodeCount = useMemo(
    () =>
      nodes.filter((node) =>
        Boolean((node.data as NodeData | undefined)?.portContractFallback)
      ).length,
    [nodes]
  );

  const filteredNodeIds = useMemo(() => {
    if (!tagFilter.enabled) {
      return new Set(nodes.map((node) => node.id));
    }
    return resolveTagFilterNodeIds(nodes, tagFilter.dimension, tagFilter.query);
  }, [nodes, tagFilter.dimension, tagFilter.enabled, tagFilter.query]);

  const focusSets = useMemo(() => {
    if (!locationFocus.active) {
      return null;
    }
    return resolveLocationFocusNodeIds(
      nodes,
      edges,
      locationFocus.locationPath,
      locationFocus.includeNeighbors
    );
  }, [
    edges,
    locationFocus.active,
    locationFocus.includeNeighbors,
    locationFocus.locationPath,
    nodes,
  ]);

  const tagGroupColorMap = useMemo(
    () => buildTagGroupColorMap(nodes, tagGrouping.dimension),
    [nodes, tagGrouping.dimension]
  );

  const renderNodes = useMemo<Node[]>(
    () =>
      nodes.map((node) => {
        const isVisible = filteredNodeIds.has(node.id);
        const nextNode: Node = {
          ...node,
          hidden: !isVisible,
        };

        if (!isVisible) {
          return nextNode;
        }

        const style: React.CSSProperties = { ...(node.style || {}) };
        const nodeGroup = getPrimaryTagValue(node.data?.tags, tagGrouping.dimension);
        const groupColor =
          tagGrouping.enabled && nodeGroup
            ? tagGroupColorMap.get(nodeGroup)
            : undefined;
        const usesFallbackPorts = Boolean(
          (node.data as NodeData | undefined)?.portContractFallback
        );

        if (groupColor) {
          style.boxShadow = `0 0 0 2px ${groupColor}66`;
        }
        if (usesFallbackPorts) {
          style.outline = '1px dashed #faad14';
          style.outlineOffset = 2;
        }

        if (focusSets) {
          const inFocus = focusSets.focusNodeIds.has(node.id);
          const inRegion = focusSets.regionNodeIds.has(node.id);
          style.opacity = inFocus ? 1 : 0.2;
          if (inRegion) {
            style.boxShadow = `0 0 0 2px #ffd666, 0 0 12px #ffd66688`;
          }
        }

        return {
          ...nextNode,
          style,
        };
      }),
    [filteredNodeIds, focusSets, nodes, tagGroupColorMap, tagGrouping.dimension, tagGrouping.enabled]
  );

  const renderEdges = useMemo<Edge[]>(
    () =>
      edges.map((edge) => {
        const isVisible =
          filteredNodeIds.has(edge.source) && filteredNodeIds.has(edge.target);
        const nextEdge: Edge = {
          ...edge,
          hidden: !isVisible,
        };

        if (!isVisible) {
          return nextEdge;
        }

        const style: React.CSSProperties = { ...(edge.style || {}) };
        const sourceNode = nodesById.get(edge.source);
        const targetNode = nodesById.get(edge.target);
        const sourceData = sourceNode?.data as NodeData | undefined;
        const targetData = targetNode?.data as NodeData | undefined;
        const sourceHandle = normalizeHandleId(edge.sourceHandle);
        const targetHandle = normalizeHandleId(edge.targetHandle);
        const sourcePort = findPortById(sourceData?.ports, sourceHandle);
        const targetPort = findPortById(targetData?.ports, targetHandle);
        const fallbackBinding =
          Boolean(sourceData?.portContractFallback) ||
          Boolean(targetData?.portContractFallback) ||
          !sourcePort ||
          !targetPort;
        const typeMismatch =
          Boolean(sourcePort && targetPort) &&
          !isPortTypeCompatibleForRelation(
            sourcePort,
            targetPort,
            readEdgeRelation(edge.data)
          );

        if (fallbackBinding) {
          style.strokeDasharray = '6 4';
          style.stroke = '#faad14';
        }
        if (typeMismatch) {
          style.strokeDasharray = '6 4';
          style.stroke = '#ff4d4f';
          style.strokeWidth = 2.4;
        }

        if (tagGrouping.enabled) {
          const sourceGroup = sourceNode
            ? getPrimaryTagValue(sourceNode.data?.tags, tagGrouping.dimension)
            : null;
          const groupColor = sourceGroup
            ? tagGroupColorMap.get(sourceGroup)
            : undefined;
          if (groupColor) {
            style.stroke = groupColor;
            style.strokeWidth = 2;
          }
        }

        if (focusSets) {
          const edgeInFocus =
            focusSets.focusNodeIds.has(edge.source) &&
            focusSets.focusNodeIds.has(edge.target);
          style.opacity = edgeInFocus ? 1 : 0.16;
          if (edgeInFocus) {
            style.stroke = '#ffd666';
            style.strokeWidth = 2.2;
          }
        }

        return {
          ...nextEdge,
          label: readEdgeHideLabel(edge.data)
            ? undefined
            : getEdgeSignalLabel(sourceHandle, targetHandle, edge.label),
          style,
        };
      }),
    [
      edges,
      filteredNodeIds,
      focusSets,
      nodesById,
      tagGroupColorMap,
      tagGrouping.dimension,
      tagGrouping.enabled,
    ]
  );

  // Delete key handler
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Delete') {
        if (readOnly) {
          return;
        }

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
  }, [nodes, edges, deleteNode, deleteEdge, readOnly, t]);

  useEffect(() => {
    if (!locationFocus.active || !focusSets || focusSets.focusNodeIds.size === 0) {
      return;
    }

    const instance = reactFlowRef.current;
    if (!instance) {
      return;
    }

    void instance.fitView({
      nodes: Array.from(focusSets.focusNodeIds).map((id) => ({ id })),
      padding: 0.28,
      duration: 350,
    });
  }, [focusSets, locationFocus.active, locationFocus.requestId]);

  const onSelectionChange = useCallback(
    ({ nodes: selectedNodes }: { nodes: Node[] }) => {
      setSelectedNodeId(selectedNodes.length === 1 ? selectedNodes[0].id : null);
    },
    [setSelectedNodeId]
  );

  const onConnect = useCallback(
    (connection: Connection) => {
      if (!connection.source || !connection.target) {
        return;
      }

      const sourceNode = nodesById.get(connection.source);
      const targetNode = nodesById.get(connection.target);
      if (!sourceNode || !targetNode) {
        return;
      }

      const sourcePortResolution = resolvePortForConnection(
        sourceNode,
        normalizeHandleId(connection.sourceHandle),
        'source'
      );
      const targetPortResolution = resolvePortForConnection(
        targetNode,
        normalizeHandleId(connection.targetHandle),
        'target'
      );

      if (!sourcePortResolution.port || !canPortProduce(sourcePortResolution.port)) {
        setConnectionWarning(t('canvas.portRoleMismatch'));
        return;
      }
      if (!targetPortResolution.port || !canPortConsume(targetPortResolution.port)) {
        setConnectionWarning(t('canvas.portRoleMismatch'));
        return;
      }

      if (
        !isPortTypeCompatible(sourcePortResolution.port, targetPortResolution.port)
      ) {
        setConnectionWarning(
          t('canvas.portTypeMismatch', {
            source: sourcePortResolution.port.type,
            target: targetPortResolution.port.type,
          })
        );
        return;
      }

      const sourceHandle = sourcePortResolution.handleId;
      const targetHandle = targetPortResolution.handleId;
      if (!sourceHandle || !targetHandle) {
        setConnectionWarning(t('canvas.portBindingUnavailable'));
        return;
      }

      const useDegradedBinding =
        sourcePortResolution.usedFallbackContract ||
        targetPortResolution.usedFallbackContract ||
        sourcePortResolution.inferredHandle ||
        targetPortResolution.inferredHandle;

      if (useDegradedBinding) {
        setConnectionWarning(t('canvas.portFallbackEdgeWarning'));
      } else {
        setConnectionWarning(null);
      }

      const nextEdge: Edge = {
        id: `e-${Date.now()}`,
        source: connection.source,
        target: connection.target,
        sourceHandle,
        targetHandle,
        label: getEdgeSignalLabel(sourceHandle, targetHandle, connection.sourceHandle),
      };

      if (useDegradedBinding) {
        nextEdge.style = {
          ...(nextEdge.style || {}),
          strokeDasharray: '6 4',
          stroke: '#faad14',
        };
      }

      setEdges(addEdge(nextEdge, edges));
    },
    [edges, nodesById, setEdges, t]
  );

  const onNodeContextMenu = useCallback(
    (event: React.MouseEvent, node: Node) => {
      if (readOnly || !liveSimulationEnabled) {
        return;
      }
      event.preventDefault();
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        node,
      });
    },
    [liveSimulationEnabled, readOnly]
  );

  const getFaultMenuItems = (node: Node): MenuItem[] => {
    const nodeType = node.type || 'generic';
    const items: MenuItem[] = [];

    if (liveSimulationEnabled) {
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
    <div style={{ width: '100%', height: '100%', position: 'relative' }}>
      {fallbackPortNodeCount > 0 && (
        <div
          style={{
            position: 'absolute',
            top: 12,
            left: 12,
            zIndex: 8,
            background: '#3a2a00',
            border: '1px solid #faad14',
            color: '#ffd666',
            borderRadius: 4,
            padding: '4px 8px',
            fontSize: 11,
            maxWidth: 360,
          }}
        >
          {t('canvas.portFallbackNodesNotice', { count: fallbackPortNodeCount })}
        </div>
      )}
      {connectionWarning && (
        <div
          style={{
            position: 'absolute',
            top: fallbackPortNodeCount > 0 ? 44 : 12,
            left: 12,
            zIndex: 8,
            background: '#3a0010',
            border: '1px solid #ff4d4f',
            color: '#ffccc7',
            borderRadius: 4,
            padding: '4px 8px',
            fontSize: 11,
            maxWidth: 360,
            display: 'flex',
            alignItems: 'center',
            gap: 8,
          }}
        >
          <span style={{ flex: 1 }}>{connectionWarning}</span>
          <button
            type="button"
            onClick={() => setConnectionWarning(null)}
            style={{
              background: 'transparent',
              border: 'none',
              color: '#ffccc7',
              cursor: 'pointer',
              fontSize: 12,
              lineHeight: 1,
              padding: 0,
            }}
            aria-label={t('canvas.dismissWarning')}
          >
            ×
          </button>
        </div>
      )}
      {readOnly && showReadOnlyNotice && (
        <div
          style={{
            position: 'absolute',
            top: fallbackPortNodeCount > 0 || connectionWarning ? 76 : 12,
            left: 12,
            zIndex: 8,
            background: '#0f172a',
            border: '1px solid #38bdf8',
            color: '#dbeafe',
            borderRadius: 4,
            padding: '4px 8px',
            fontSize: 11,
            maxWidth: 380,
          }}
        >
          {t('canvas.reviewReadonlyNotice')}
        </div>
      )}
      {!readOnly && !liveSimulationEnabled && (
        <div
          style={{
            position: 'absolute',
            top: fallbackPortNodeCount > 0 || connectionWarning ? 76 : 12,
            left: 12,
            zIndex: 8,
            background: '#172554',
            border: '1px solid #60a5fa',
            color: '#dbeafe',
            borderRadius: 4,
            padding: '4px 8px',
            fontSize: 11,
            maxWidth: 420,
          }}
        >
          {t('canvas.liveControlsUnavailableNotice')}
        </div>
      )}
      <CanvasInteractionContext.Provider value={{ readOnly, liveSimulationEnabled }}>
        <ReactFlow
          nodes={renderNodes}
          edges={renderEdges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={readOnly ? undefined : onConnect}
          onSelectionChange={onSelectionChange}
          onNodeContextMenu={readOnly ? undefined : onNodeContextMenu}
          onInit={(instance) => {
            reactFlowRef.current = instance;
          }}
          nodeTypes={nodeTypes}
          selectionMode={SelectionMode.Partial}
          nodesDraggable={!readOnly && isInteractive}
          nodesConnectable={!readOnly && isInteractive}
          elementsSelectable={!readOnly && isInteractive}
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
            <CanvasControls
              isInteractive={!readOnly && isInteractive}
              onToggleInteractive={() => setIsInteractive((v) => !v)}
            />
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
              const status = (node.data as NodeData | undefined)?.status;
              return status ? statusColors[status] || '#4a4a4a' : '#4a4a4a';
            }}
            maskColor="rgba(0,0,0,0.4)"
          />
        </ReactFlow>
      </CanvasInteractionContext.Provider>
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

function normalizeHandleId(handle: unknown): string | undefined {
  if (typeof handle !== 'string') {
    return undefined;
  }
  const trimmed = handle.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function readEdgeRelation(data: unknown): string | undefined {
  if (!data || typeof data !== 'object') {
    return undefined;
  }
  const relation = (data as Record<string, unknown>).relation;
  if (typeof relation !== 'string') {
    return undefined;
  }
  const trimmed = relation.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function readEdgeHideLabel(data: unknown): boolean {
  if (!data || typeof data !== 'object') {
    return false;
  }
  return (data as Record<string, unknown>).hideLabel === true;
}

function resolvePortForConnection(
  node: Node,
  requestedHandle: string | undefined,
  direction: 'source' | 'target'
): PortResolution {
  const data = (node.data || {}) as NodeData;
  const ports = data.ports || [];
  const usedFallbackContract = Boolean(data.portContractFallback);

  if (requestedHandle) {
    return {
      handleId: requestedHandle,
      port: findPortById(ports, requestedHandle),
      usedFallbackContract,
      inferredHandle: false,
    };
  }

  const candidates = ports.filter((port) =>
    direction === 'source' ? canPortProduce(port) : canPortConsume(port)
  );

  if (candidates.length === 1) {
    return {
      handleId: candidates[0].id,
      port: candidates[0],
      usedFallbackContract,
      inferredHandle: true,
    };
  }

  return {
    usedFallbackContract,
    inferredHandle: true,
  };
}

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
