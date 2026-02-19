import React, { useCallback } from 'react';
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  BackgroundVariant,
  SelectionMode,
  addEdge,
} from '@xyflow/react';
import type { Connection } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { useTopologyStore } from '../../stores/topologyStore';
import CylinderNode from '../nodes/CylinderNode';
import SensorNode from '../nodes/SensorNode';
import SwitchNode from '../nodes/SwitchNode';
import StepperNode from '../nodes/StepperNode';
import GenericNode from '../nodes/GenericNode';

const nodeTypes = {
  cylinder: CylinderNode,
  sensor: SensorNode,
  switch: SwitchNode,
  stepper_pd: StepperNode,
  stepper: StepperNode,
  generic: GenericNode,
};

const TopologyCanvas: React.FC = () => {
  const {
    nodes,
    edges,
    onNodesChange,
    onEdgesChange,
    setEdges,
    setSelectedNodeId,
  } = useTopologyStore();

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

  return (
    <div style={{ width: '100%', height: '100%' }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onSelectionChange={onSelectionChange}
        nodeTypes={nodeTypes}
        selectionMode={SelectionMode.Partial}
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
          style={{
            background: '#2d2d2d',
            border: '1px solid #3a3a3a',
            borderRadius: 6,
          }}
        />
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
    </div>
  );
};

export default TopologyCanvas;
