import type { DeviceTags, TagDimension } from '../types';
import { hasLocationPrefix, hasTag, normalizeDeviceTags } from './deviceTags';

type NodeLike = {
  id: string;
  data: {
    tags?: DeviceTags;
  };
};

type EdgeLike = {
  source: string;
  target: string;
};

export const TAG_GROUP_COLORS = [
  '#00bcd4',
  '#52c41a',
  '#faad14',
  '#f5222d',
  '#722ed1',
  '#13c2c2',
  '#eb2f96',
  '#2f54eb',
] as const;

export function resolveTagFilterNodeIds(
  nodes: NodeLike[],
  dimension: TagDimension,
  rawQuery: string
): Set<string> {
  const query = rawQuery.trim();
  if (!query) {
    return new Set(nodes.map((node) => node.id));
  }

  if (query === '*') {
    return new Set(nodes.map((node) => node.id));
  }

  const matchFn =
    dimension === 'location_group'
      ? (node: NodeLike) => hasLocationPrefix(node.data.tags, query)
      : (node: NodeLike) => hasTag(node.data.tags, dimension, query);

  return new Set(nodes.filter(matchFn).map((node) => node.id));
}

export function buildTagGroupColorMap(
  nodes: NodeLike[],
  dimension: TagDimension
): Map<string, string> {
  const colorMap = new Map<string, string>();
  for (const node of nodes) {
    const groupKey = getPrimaryTagValue(node.data.tags, dimension);
    if (!groupKey || colorMap.has(groupKey)) {
      continue;
    }
    colorMap.set(groupKey, TAG_GROUP_COLORS[colorMap.size % TAG_GROUP_COLORS.length]);
  }
  return colorMap;
}

export function getPrimaryTagValue(
  tags: DeviceTags | undefined,
  dimension: TagDimension
): string | null {
  const values = normalizeDeviceTags(tags)[dimension];
  return values.length > 0 ? values[0].toLowerCase() : null;
}

export function resolveLocationFocusNodeIds(
  nodes: NodeLike[],
  edges: EdgeLike[],
  locationPath: string,
  includeNeighbors: boolean
): {
  regionNodeIds: Set<string>;
  focusNodeIds: Set<string>;
} {
  const normalizedPath = locationPath.trim();
  const regionNodeIds = new Set<string>();

  if (!normalizedPath) {
    return {
      regionNodeIds,
      focusNodeIds: regionNodeIds,
    };
  }

  for (const node of nodes) {
    if (hasLocationPrefix(node.data.tags, normalizedPath)) {
      regionNodeIds.add(node.id);
    }
  }

  if (!includeNeighbors || regionNodeIds.size === 0) {
    return {
      regionNodeIds,
      focusNodeIds: new Set(regionNodeIds),
    };
  }

  const focusNodeIds = new Set(regionNodeIds);
  for (const edge of edges) {
    const sourceInRegion = regionNodeIds.has(edge.source);
    const targetInRegion = regionNodeIds.has(edge.target);
    if (sourceInRegion || targetInRegion) {
      focusNodeIds.add(edge.source);
      focusNodeIds.add(edge.target);
    }
  }

  return {
    regionNodeIds,
    focusNodeIds,
  };
}
