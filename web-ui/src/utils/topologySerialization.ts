import type { Edge, Node } from '@xyflow/react';
import {
  TOPOLOGY_TAGS_SCHEMA_VERSION,
  type ComponentTopology,
} from '../types';
import type { NodeData } from '../stores/topologyStore';
import { normalizeDeviceTags } from './deviceTags';
import { normalizeDevicePorts } from './portContract';

export function toComponentTopology(
  nodes: Array<Node<NodeData>>,
  edges: Edge[]
): ComponentTopology {
  return {
    schema_version: 1,
    tags_schema_version: TOPOLOGY_TAGS_SCHEMA_VERSION,
    component_library: { schema_version: 1, components: [] },
    components: nodes.map((node) => ({
      id: node.id,
      component_id: node.type || 'generic',
      params: sanitizeNodeParams(node.data),
      position: node.position,
    })),
    connections: edges.map((edge) => {
      const connection: ComponentTopology['connections'][number] = {
        from: edge.source,
        to: edge.target,
      };

      if (typeof edge.sourceHandle === 'string' && edge.sourceHandle.trim()) {
        connection.from_port = edge.sourceHandle.trim();
      }
      if (typeof edge.targetHandle === 'string' && edge.targetHandle.trim()) {
        connection.to_port = edge.targetHandle.trim();
      }

      const signal = normalizeEdgeLabel(edge.label);
      if (signal) {
        connection.signal = signal;
      }

      const relation = extractRelationFromEdgeData(edge.data);
      if (relation) {
        connection.relation = relation;
      }

      return connection;
    }),
  };
}

function sanitizeNodeParams(nodeData: NodeData): Record<string, unknown> {
  const sanitized: Record<string, unknown> = { ...nodeData };
  delete sanitized.portContractFallback;

  const normalizedTags = normalizeDeviceTags(nodeData.tags);
  sanitized.tags = normalizedTags;

  if (nodeData.portContractFallback) {
    delete sanitized.ports;
  } else {
    const ports = normalizeDevicePorts(nodeData.ports);
    if (ports.length > 0) {
      sanitized.ports = ports;
    } else {
      delete sanitized.ports;
    }
  }

  return sanitized;
}

export function downloadTopologyAsJson(
  topology: ComponentTopology,
  filename: string
): void {
  const blob = new Blob([JSON.stringify(topology, null, 2)], {
    type: 'application/json',
  });
  const href = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = href;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(href);
}

function normalizeEdgeLabel(label: Edge['label']): string | undefined {
  if (typeof label === 'string') {
    const trimmed = label.trim();
    return trimmed.length > 0 ? trimmed : undefined;
  }
  if (typeof label === 'number') {
    return String(label);
  }
  return undefined;
}

function extractRelationFromEdgeData(data: unknown): string | undefined {
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
