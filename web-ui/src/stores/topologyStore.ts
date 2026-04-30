import { create } from 'zustand';
import { applyNodeChanges, applyEdgeChanges } from '@xyflow/react';
import { persist } from 'zustand/middleware';
import type { Node, Edge, OnNodesChange, OnEdgesChange } from '@xyflow/react';
import type {
  DevicePortMetadata,
  DeviceTags,
  TagDimension,
} from '../types';
import {
  hasLocationPrefix,
  hasTag,
  normalizeDeviceTags,
} from '../utils/deviceTags';
import { resolveNodePorts } from '../utils/portContract';

export interface NodeData {
  label: string;
  type: string;
  tags?: DeviceTags;
  ports?: DevicePortMetadata[];
  portContractFallback?: boolean;
  status?: string;
  value?: number | boolean;
  [key: string]: any;
}

export interface TagFilterState {
  enabled: boolean;
  dimension: TagDimension;
  query: string;
}

export interface TagGroupingState {
  enabled: boolean;
  dimension: TagDimension;
}

export interface LocationFocusState {
  active: boolean;
  locationPath: string;
  includeNeighbors: boolean;
  requestId: number;
}

interface TopologyState {
  // Nodes and edges
  nodes: Node<NodeData>[];
  edges: Edge[];

  // Selection
  selectedNodeId: string | null;

  // Dirty tracking
  hasUnsavedChanges: boolean;

  // Canvas tag visualization
  tagFilter: TagFilterState;
  tagGrouping: TagGroupingState;
  locationFocus: LocationFocusState;

  // Actions
  setNodes: (nodes: Node<NodeData>[]) => void;
  setEdges: (edges: Edge[]) => void;
  replaceTopology: (nodes: Node<NodeData>[], edges: Edge[], markDirty?: boolean) => void;
  onNodesChange: OnNodesChange;
  onEdgesChange: OnEdgesChange;
  setSelectedNodeId: (id: string | null) => void;
  updateNodeData: (nodeId: string, data: Partial<NodeData>, markDirty?: boolean) => void;
  mergeNodeDataById: (
    updates: Record<string, Partial<NodeData>>,
    markDirty?: boolean
  ) => void;
  addNode: (node: Node<NodeData>) => void;
  deleteNode: (nodeId: string) => void;
  deleteEdge: (edgeId: string) => void;
  setHasUnsavedChanges: (value: boolean) => void;
  setTagFilter: (dimension: TagDimension, query: string) => void;
  clearTagFilter: () => void;
  setTagGrouping: (enabled: boolean, dimension?: TagDimension) => void;
  clearTagGrouping: () => void;
  focusLocationRegion: (locationPath: string, includeNeighbors?: boolean) => void;
  clearLocationFocus: () => void;
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
      tagFilter: {
        enabled: false,
        dimension: 'functional_group',
        query: '',
      },
      tagGrouping: {
        enabled: false,
        dimension: 'functional_group',
      },
      locationFocus: {
        active: false,
        locationPath: '',
        includeNeighbors: true,
        requestId: 0,
      },

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

      updateNodeData: (nodeId, data, markDirty = true) => {
        set({
          nodes: get().nodes.map((node) =>
            node.id === nodeId
              ? normalizeTopologyNode({
                  ...node,
                  data: { ...node.data, ...data },
                })
              : node
          ),
          hasUnsavedChanges: markDirty ? true : get().hasUnsavedChanges,
        });
      },

      mergeNodeDataById: (updates, markDirty = true) => {
        if (Object.keys(updates).length === 0) {
          return;
        }

        set({
          nodes: get().nodes.map((node) => {
            const patch = updates[node.id];
            if (!patch) {
              return node;
            }
            return normalizeTopologyNode({
              ...node,
              data: { ...node.data, ...patch },
            });
          }),
          hasUnsavedChanges: markDirty ? true : get().hasUnsavedChanges,
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

      setTagFilter: (dimension, query) => {
        const normalizedQuery = query.trim();
        set({
          tagFilter: {
            enabled: normalizedQuery.length > 0,
            dimension,
            query: normalizedQuery,
          },
        });
      },

      clearTagFilter: () =>
        set({
          tagFilter: {
            enabled: false,
            dimension: get().tagFilter.dimension,
            query: '',
          },
        }),

      setTagGrouping: (enabled, dimension) =>
        set({
          tagGrouping: {
            enabled,
            dimension: dimension ?? get().tagGrouping.dimension,
          },
        }),

      clearTagGrouping: () =>
        set({
          tagGrouping: {
            enabled: false,
            dimension: get().tagGrouping.dimension,
          },
        }),

      focusLocationRegion: (locationPath, includeNeighbors = true) => {
        const normalizedPath = locationPath.trim();
        set({
          locationFocus: {
            active: normalizedPath.length > 0,
            locationPath: normalizedPath,
            includeNeighbors,
            requestId: get().locationFocus.requestId + 1,
          },
        });
      },

      clearLocationFocus: () =>
        set({
          locationFocus: {
            active: false,
            locationPath: '',
            includeNeighbors: true,
            requestId: get().locationFocus.requestId + 1,
          },
        }),

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
  const resolvedPorts = resolveNodePorts(node.type, node.data.ports);
  return {
    ...node,
    data: {
      ...node.data,
      tags: normalizeDeviceTags(node.data.tags),
      ports: resolvedPorts.ports,
      portContractFallback: resolvedPorts.usedFallbackContract,
    },
  };
}
