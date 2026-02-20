import { create } from 'zustand';
import { applyNodeChanges, applyEdgeChanges } from '@xyflow/react';
import type { Node, Edge, OnNodesChange, OnEdgesChange } from '@xyflow/react';

export interface NodeData {
  label: string;
  type: string;
  status?: string;
  value?: number | boolean;
  [key: string]: any;
}

interface TopologyState {
  // Nodes and edges
  nodes: Node<NodeData>[];
  edges: Edge[];

  // Selection
  selectedNodeId: string | null;

  // Dirty tracking
  hasUnsavedChanges: boolean;

  // Actions
  setNodes: (nodes: Node<NodeData>[]) => void;
  setEdges: (edges: Edge[]) => void;
  onNodesChange: OnNodesChange;
  onEdgesChange: OnEdgesChange;
  setSelectedNodeId: (id: string | null) => void;
  updateNodeData: (nodeId: string, data: Partial<NodeData>) => void;
  addNode: (node: Node<NodeData>) => void;
  deleteNode: (nodeId: string) => void;
  deleteEdge: (edgeId: string) => void;
  setHasUnsavedChanges: (value: boolean) => void;
}

export const useTopologyStore = create<TopologyState>((set, get) => ({
  nodes: [],
  edges: [],
  selectedNodeId: null,
  hasUnsavedChanges: false,

  setNodes: (nodes) => set({ nodes }),

  setEdges: (edges) => set({ edges }),

  onNodesChange: (changes) => {
    set({
      nodes: applyNodeChanges(changes, get().nodes) as Node<NodeData>[],
      hasUnsavedChanges: true,
    });
  },

  onEdgesChange: (changes) => {
    set({
      edges: applyEdgeChanges(changes, get().edges),
      hasUnsavedChanges: true,
    });
  },

  setSelectedNodeId: (id) => set({ selectedNodeId: id }),

  updateNodeData: (nodeId, data) => {
    set({
      nodes: get().nodes.map((node) =>
        node.id === nodeId
          ? { ...node, data: { ...node.data, ...data } }
          : node
      ),
      hasUnsavedChanges: true,
    });
  },

  addNode: (node) => {
    set({ nodes: [...get().nodes, node], hasUnsavedChanges: true });
  },

  deleteNode: (nodeId) => {
    set({
      nodes: get().nodes.filter((n) => n.id !== nodeId),
      edges: get().edges.filter((e) => e.source !== nodeId && e.target !== nodeId),
      selectedNodeId: get().selectedNodeId === nodeId ? null : get().selectedNodeId,
      hasUnsavedChanges: true,
    });
  },

  deleteEdge: (edgeId) => {
    set({
      edges: get().edges.filter((e) => e.id !== edgeId),
      hasUnsavedChanges: true,
    });
  },

  setHasUnsavedChanges: (value) => set({ hasUnsavedChanges: value }),
}));
