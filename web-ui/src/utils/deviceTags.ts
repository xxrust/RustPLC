import type { DeviceTags, TagDimension } from '../types';

export function emptyDeviceTags(): DeviceTags {
  return {
    functional_group: [],
    danger_level: [],
    location_group: [],
  };
}

export function normalizeDeviceTags(raw: unknown): DeviceTags {
  if (!raw || typeof raw !== 'object') {
    return emptyDeviceTags();
  }

  const source = raw as Record<string, unknown>;
  return {
    functional_group: normalizeTagDimension(source.functional_group),
    danger_level: normalizeTagDimension(source.danger_level),
    location_group: normalizeTagDimension(source.location_group),
  };
}

export function normalizeTagDimension(raw: unknown): string[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  return raw.filter((value): value is string => typeof value === 'string');
}

export function hasTag(
  tags: DeviceTags | undefined,
  dimension: TagDimension,
  query: string
): boolean {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return false;
  }
  const normalizedTags = normalizeDeviceTags(tags);
  return normalizedTags[dimension].some((value) => value.toLowerCase() === normalizedQuery);
}

export function hasLocationPrefix(
  tags: DeviceTags | undefined,
  locationPath: string
): boolean {
  const normalizedPrefix = normalizeLocationPath(locationPath);
  if (!normalizedPrefix) {
    return false;
  }
  const normalizedTags = normalizeDeviceTags(tags);
  return normalizedTags.location_group.some((location) => {
    const normalizedLocation = normalizeLocationPath(location);
    return (
      normalizedLocation === normalizedPrefix ||
      normalizedLocation.startsWith(`${normalizedPrefix}/`)
    );
  });
}

function normalizeLocationPath(raw: string): string {
  return raw
    .trim()
    .split('/')
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0)
    .join('/')
    .toLowerCase();
}
