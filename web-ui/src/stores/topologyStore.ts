import { create } from 'zustand';
import { applyNodeChanges, applyEdgeChanges } from '@xyflow/react';
import { persist } from 'zustand/middleware';
import type { Node, Edge, OnNodesChange, OnEdgesChange } from '@xyflow/react';
import type { DeviceTags, TagDimension } from '../types';
import {
  hasLocationPrefix,
  hasTag,
  normalizeDeviceTags,
} from '../utils/deviceTags';

export interface NodeData {
  label: string;
  type: string;
  tags?: DeviceTags;
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
  replaceTopology: (nodes: Node<NodeData>[], edges: Edge[], markDirty?: boolean) => void;
  onNodesChange: OnNodesChange;
  onEdgesChange: OnEdgesChange;
  setSelectedNodeId: (id: string | null) => void;
  updateNodeData: (nodeId: string, data: Partial<NodeData>) => void;
  addNode: (node: Node<NodeData>) => void;
  deleteNode: (nodeId: string) => void;
  deleteEdge: (edgeId: string) => void;
  setHasUnsavedChanges: (value: boolean) => void;
  findNodeIdsByTag: (dimension: TagDimension, tag: string) => string[];
  findNodeIdsByLocationPath: (locationPath: string) => string[];
}

export const useTopologyStore = create<TopologyState>()(
  persist(
    (set, get) => ({
      nodes: [],
      edges: [],
      selectedNodeId: null,
      hasUnsavedChanges: false,

      setNodes: (nodes) => set({ nodes: nodes.map(normalizeTopologyNode) }),

      setEdges: (edges) => set({ edges }),

      replaceTopology: (nodes, edges, markDirty = true) => {
        const normalizedNodes = nodes.map(normalizeTopologyNode);
        const selectedNodeId = get().selectedNodeId;
        const hasSelectedNode = selectedNodeId
          ? normalizedNodes.some((node) => node.id === selectedNodeId)
          : false;

        set({
          nodes: normalizedNodes,
          edges,
          selectedNodeId: hasSelectedNode ? selectedNodeId : null,
          hasUnsavedChanges: markDirty,
        });
      },

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
              ? normalizeTopologyNode({
                  ...node,
                  data: { ...node.data, ...data },
                })
              : node
          ),
          hasUnsavedChanges: true,
        });
      },

      addNode: (node) => {
        set({
          nodes: [...get().nodes, normalizeTopologyNode(node)],
          hasUnsavedChanges: true,
        });
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

      findNodeIdsByTag: (dimension, tag) =>
        get()
          .nodes
          .filter((node) => hasTag(node.data.tags, dimension, tag))
          .map((node) => node.id),

      findNodeIdsByLocationPath: (locationPath) =>
        get()
          .nodes
          .filter((node) => hasLocationPrefix(node.data.tags, locationPath))
          .map((node) => node.id),
    }),
    {
      name: 'rustplc-topology-storage-v1',
      partialize: (state) => ({
        nodes: state.nodes,
        edges: state.edges,
        selectedNodeId: state.selectedNodeId,
      }),
    }
  )
);

function normalizeTopologyNode(node: Node<NodeData>): Node<NodeData> {
  return {
    ...node,
    data: {
      ...node.data,
      tags: normalizeDeviceTags(node.data.tags),
    },
  };
}
