import React from 'react';
import { useTopologyStore } from '../stores/topologyStore';
import CylinderPropertiesEditor from './properties/CylinderPropertiesEditor';
import SensorPropertiesEditor from './properties/SensorPropertiesEditor';
import SwitchPropertiesEditor from './properties/SwitchPropertiesEditor';
import StepperPropertiesEditor from './properties/StepperPropertiesEditor';
import GenericPropertiesEditor from './properties/GenericPropertiesEditor';
import CanvasPropertiesEditor from './properties/CanvasPropertiesEditor';

const PropertiesPanel: React.FC = () => {
  const { nodes, selectedNodeId, updateNodeData } = useTopologyStore();
  const selectedNode = nodes.find((n: { id: string }) => n.id === selectedNodeId);

  if (!selectedNode) {
    return <CanvasPropertiesEditor />;
  }

  const EditorComponent = {
    cylinder: CylinderPropertiesEditor,
    sensor: SensorPropertiesEditor,
    switch: SwitchPropertiesEditor,
    stepper_pd: StepperPropertiesEditor,
    stepper: StepperPropertiesEditor,
    generic: GenericPropertiesEditor,
  }[selectedNode.type || 'generic'] || GenericPropertiesEditor;

  return (
    <EditorComponent
      nodeId={selectedNode.id}
      data={selectedNode.data}
      onUpdate={updateNodeData}
    />
  );
};

export default PropertiesPanel;
