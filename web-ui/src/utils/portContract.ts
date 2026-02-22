import type {
  DevicePortMetadata,
  DevicePortRole,
  PortSignalType,
} from '../types';

const TYPE_ALIASES: Record<string, string> = {
  stepper_pd: 'stepper',
};

const DEFAULT_PORT_CONTRACTS: Record<string, DevicePortMetadata[]> = {
  cylinder: [
    { id: 'cmd', type: 'digital', role: 'consumer' },
    { id: 'extended', type: 'logical', role: 'producer' },
    { id: 'retracted', type: 'logical', role: 'producer' },
  ],
  sensor: [
    { id: 'in', type: 'digital', role: 'consumer' },
    { id: 'state', type: 'digital', role: 'producer' },
  ],
  switch: [
    { id: 'in', type: 'digital', role: 'consumer' },
    { id: 'out', type: 'digital', role: 'producer' },
  ],
  stepper: [
    { id: 'cmd', type: 'digital', role: 'consumer' },
    { id: 'state', type: 'logical', role: 'producer' },
  ],
  generic: [
    { id: 'in', type: 'generic', role: 'consumer' },
    { id: 'out', type: 'generic', role: 'producer' },
  ],
  input_terminal: [{ id: 'in', type: 'digital', role: 'consumer' }],
  output_terminal: [{ id: 'out', type: 'digital', role: 'producer' }],
};

const PORT_TYPES: PortSignalType[] = [
  'digital',
  'analog',
  'pneumatic',
  'logical',
  'generic',
];

const PORT_ROLES: DevicePortRole[] = ['producer', 'consumer', 'bidirectional'];

export interface ResolvedNodePorts {
  ports: DevicePortMetadata[];
  hasExplicitMetadata: boolean;
  usedFallbackContract: boolean;
}

export function normalizeDevicePorts(raw: unknown): DevicePortMetadata[] {
  if (!Array.isArray(raw)) {
    return [];
  }

  return raw
    .map((candidate) => {
      if (!candidate || typeof candidate !== 'object') {
        return null;
      }
      const id = normalizePortId((candidate as Record<string, unknown>).id);
      const type = normalizePortType((candidate as Record<string, unknown>).type);
      const role = normalizePortRole((candidate as Record<string, unknown>).role);
      if (!id || !type || !role) {
        return null;
      }
      return { id, type, role };
    })
    .filter((port): port is DevicePortMetadata => port !== null);
}

export function resolveNodePorts(
  nodeType: string | undefined,
  rawPorts: unknown
): ResolvedNodePorts {
  const explicitPorts = normalizeDevicePorts(rawPorts);
  if (explicitPorts.length > 0) {
    return {
      ports: explicitPorts,
      hasExplicitMetadata: true,
      usedFallbackContract: false,
    };
  }

  const fallbackPorts = getDefaultPortsForNodeType(nodeType);
  return {
    ports: fallbackPorts,
    hasExplicitMetadata: false,
    usedFallbackContract: fallbackPorts.length > 0,
  };
}

export function getDefaultPortsForNodeType(
  nodeType: string | undefined
): DevicePortMetadata[] {
  if (!nodeType) {
    return [];
  }
  const normalizedType = normalizeNodeType(nodeType);
  const contract = DEFAULT_PORT_CONTRACTS[normalizedType];
  return contract ? contract.map((port) => ({ ...port })) : [];
}

export function findPortById(
  ports: DevicePortMetadata[] | undefined,
  id: string | null | undefined
): DevicePortMetadata | undefined {
  if (!ports || ports.length === 0 || !id) {
    return undefined;
  }
  return ports.find((port) => port.id === id);
}

export function canPortProduce(port: DevicePortMetadata | undefined): boolean {
  if (!port) {
    return false;
  }
  return port.role === 'producer' || port.role === 'bidirectional';
}

export function canPortConsume(port: DevicePortMetadata | undefined): boolean {
  if (!port) {
    return false;
  }
  return port.role === 'consumer' || port.role === 'bidirectional';
}

export function isPortTypeCompatible(
  sourcePort: DevicePortMetadata | undefined,
  targetPort: DevicePortMetadata | undefined
): boolean {
  if (!sourcePort || !targetPort) {
    return true;
  }
  if (sourcePort.type === 'generic' || targetPort.type === 'generic') {
    return true;
  }
  return sourcePort.type === targetPort.type;
}

export function getEdgeSignalLabel(
  sourceHandle: string | null | undefined,
  targetHandle: string | null | undefined,
  existing: unknown
): string | undefined {
  if (typeof existing === 'string' && existing.trim()) {
    return existing.trim();
  }
  if (typeof sourceHandle === 'string' && sourceHandle.trim()) {
    return sourceHandle.trim();
  }
  if (typeof targetHandle === 'string' && targetHandle.trim()) {
    return targetHandle.trim();
  }
  return undefined;
}

function normalizeNodeType(nodeType: string): string {
  const lowered = nodeType.toLowerCase();
  return TYPE_ALIASES[lowered] || lowered;
}

function normalizePortId(raw: unknown): string | null {
  if (typeof raw !== 'string') {
    return null;
  }
  const id = raw.trim();
  return id.length > 0 ? id : null;
}

function normalizePortType(raw: unknown): PortSignalType | null {
  if (typeof raw !== 'string') {
    return null;
  }
  const normalized = raw.trim().toLowerCase();
  return PORT_TYPES.find((portType) => portType === normalized) || null;
}

function normalizePortRole(raw: unknown): DevicePortRole | null {
  if (typeof raw !== 'string') {
    return null;
  }
  const normalized = raw.trim().toLowerCase();
  return PORT_ROLES.find((role) => role === normalized) || null;
}
