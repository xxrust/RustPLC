import React, { useMemo, useState } from 'react';
import { Card, Empty, Segmented, Space, Statistic, Tag, Typography } from 'antd';
import type {
  GeometryArtifact,
  GeometryArtifactResponse,
  GeometryEdge,
  GeometryEvidenceStatus,
  GeometryLane,
  GeometryNode,
  GeometryViewKind,
} from '../../types';

const { Text } = Typography;

const VIEW_OPTIONS: Array<{ label: string; value: GeometryViewKind }> = [
  { label: 'Constellation', value: 'constellation' },
  { label: 'Orbit', value: 'orbit' },
  { label: 'Evidence', value: 'evidence' },
];

const VIEW_LABELS: Record<GeometryViewKind, string> = {
  constellation: 'system structure',
  orbit: 'step transitions',
  evidence: 'constraints and proof',
};

const STATUS_COLORS: Record<GeometryEvidenceStatus, string> = {
  authored: '#94a3b8',
  derived: '#67e8f9',
  verified: '#86efac',
  observed: '#fde68a',
  warning: '#fb923c',
  blocked: '#f87171',
};

const LANE_COLORS: Record<string, string> = {
  topology: '#60a5fa',
  task: '#c084fc',
  evidence: '#34d399',
};

const NODE_SIZES: Record<string, number> = {
  task: 11,
  step: 8,
  device: 7,
  semantic_resource: 7,
  timing_rule: 6,
  causality_chain: 6,
  claim_source: 5,
  workpiece_site: 6,
  workpiece_holder: 6,
  workpiece_carrier: 6,
  external_reference: 5,
};

type NodePoint = { x: number; y: number };

interface GeometryPreviewProps {
  artifact?: GeometryArtifactResponse;
  artifactHref?: string;
  loading?: boolean;
  runMode?: string;
}

function isGeometryArtifact(
  artifact: GeometryArtifactResponse | undefined
): artifact is GeometryArtifact {
  return Boolean(
    artifact &&
      'nodes' in artifact &&
      'edges' in artifact &&
      'lanes' in artifact &&
      'summary' in artifact
  );
}

function buildObservedTransitionSet(artifact: GeometryArtifact): Set<string> {
  const transitions = artifact.overlays.trace?.transitions ?? [];
  return new Set(
    transitions
      .filter((item) => item.from_state && item.to_state)
      .map((item) => `step:${item.from_state}->step:${item.to_state}`)
  );
}

function hashOffset(seed: string): number {
  let hash = 0;
  for (let index = 0; index < seed.length; index += 1) {
    hash = (hash * 31 + seed.charCodeAt(index)) >>> 0;
  }
  return (hash % 360) * (Math.PI / 180);
}

function layoutNodes(
  lanes: GeometryLane[],
  nodes: GeometryNode[],
  width: number,
  height: number
): Map<string, NodePoint> {
  const cx = width / 2;
  const cy = height / 2;
  const sortedLanes = [...lanes].sort((left, right) => left.position - right.position);
  const laneRadius = new Map<string, number>();
  const baseRadius = 118;

  sortedLanes.forEach((lane, index) => {
    laneRadius.set(lane.id, baseRadius + index * 96);
  });

  const positions = new Map<string, NodePoint>();
  for (const lane of sortedLanes) {
    const laneNodes = nodes
      .filter((node) => node.lane_id === lane.id)
      .sort((left, right) => left.label.localeCompare(right.label));
    const count = Math.max(laneNodes.length, 1);
    const radius = laneRadius.get(lane.id) ?? baseRadius;
    const laneOffset = hashOffset(lane.id) / 3;

    laneNodes.forEach((node, index) => {
      const angle =
        -Math.PI / 2 + laneOffset + (Math.PI * 2 * index) / count + hashOffset(node.id) / 12;
      const x = cx + Math.cos(angle) * radius;
      const y = cy + Math.sin(angle) * radius * 0.68;
      positions.set(node.id, { x, y });
    });
  }

  return positions;
}

function edgeKey(edge: GeometryEdge): string {
  return `${edge.from}->${edge.to}`;
}

const GeometryPreview: React.FC<GeometryPreviewProps> = ({
  artifact,
  artifactHref,
  loading,
  runMode,
}) => {
  const [view, setView] = useState<GeometryViewKind>('constellation');
  const renderable = isGeometryArtifact(artifact) ? artifact : undefined;

  const scene = useMemo(() => {
    if (!renderable) {
      return undefined;
    }

    const nodes = renderable.nodes.filter((node) => node.views.includes(view));
    const nodeIds = new Set(nodes.map((node) => node.id));
    const edges = renderable.edges.filter(
      (edge) =>
        edge.views.includes(view) && nodeIds.has(edge.from) && nodeIds.has(edge.to)
    );
    const lanes = renderable.lanes.filter((lane) =>
      nodes.some((node) => node.lane_id === lane.id)
    );

    return {
      lanes,
      nodes,
      edges,
      positions: layoutNodes(lanes, nodes, 960, 720),
      observedTransitions: buildObservedTransitionSet(renderable),
    };
  }, [renderable, view]);

  return (
    <Card
      type="inner"
      title="Semantic Twin Geometry"
      loading={loading}
      extra={
        artifactHref ? (
          <a href={artifactHref} target="_blank" rel="noreferrer">
            raw artifact
          </a>
        ) : null
      }
      styles={{
        body: {
          background:
            'radial-gradient(circle at top, rgba(45,55,110,0.32), rgba(8,11,25,0.96) 58%, rgba(4,6,16,1) 100%)',
          borderRadius: 12,
        },
      }}
    >
      {!renderable ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={
            runMode === 'component_sim'
              ? 'This run has no PLC semantic geometry artifact. Geometry preview currently follows PLC-backed runs.'
              : 'No geometry artifact for this run yet.'
          }
        />
      ) : (
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          <Space wrap size={[8, 8]}>
            <Statistic title="Tasks" value={renderable.summary.task_count} />
            <Statistic title="Steps" value={renderable.summary.step_count} />
            <Statistic title="Devices" value={renderable.summary.device_count} />
            <Statistic
              title="Observed transitions"
              value={renderable.summary.observed_transition_count}
            />
          </Space>

          <Space wrap size={[8, 8]}>
            <Tag color="blue">{renderable.source_path}</Tag>
            {renderable.overlays.trace && (
              <Tag color="gold">
                trace {renderable.overlays.trace.observed_transition_count}
              </Tag>
            )}
            {renderable.overlays.intent && (
              <Tag
                color={
                  renderable.overlays.intent.verdict === 'pass'
                    ? 'success'
                    : renderable.overlays.intent.verdict === 'warn'
                      ? 'warning'
                      : 'error'
                }
              >
                intent {renderable.overlays.intent.verdict}
              </Tag>
            )}
            {renderable.overlays.intent &&
              renderable.overlays.intent.mismatch_count > 0 && (
                <Tag color="error">
                  mismatches {renderable.overlays.intent.mismatch_count}
                </Tag>
              )}
            {renderable.overlays.trace && (
              <Tag color="default">{renderable.overlays.trace.resolution}</Tag>
            )}
          </Space>

          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              gap: 12,
              flexWrap: 'wrap',
            }}
          >
            <Text type="secondary">
              Deterministic semantic map of structure, transitions, and evidence.
            </Text>
            <Segmented
              options={VIEW_OPTIONS}
              value={view}
              onChange={(value) => setView(value as GeometryViewKind)}
            />
          </div>

          <div
            style={{
              borderRadius: 18,
              border: '1px solid rgba(148, 163, 184, 0.18)',
              overflow: 'hidden',
              background:
                'radial-gradient(circle at center, rgba(23,37,84,0.24), rgba(3,7,18,0.92) 70%)',
            }}
          >
            <svg
              viewBox="0 0 960 720"
              style={{ width: '100%', height: 'auto', display: 'block' }}
              role="img"
              aria-label={`Semantic twin geometry ${VIEW_LABELS[view]}`}
            >
              <defs>
                <linearGradient id="geometry-edge" x1="0%" y1="0%" x2="100%" y2="100%">
                  <stop offset="0%" stopColor="rgba(103, 232, 249, 0.24)" />
                  <stop offset="100%" stopColor="rgba(196, 132, 252, 0.14)" />
                </linearGradient>
                <radialGradient id="geometry-core" cx="50%" cy="50%" r="50%">
                  <stop offset="0%" stopColor="rgba(255,255,255,0.92)" />
                  <stop offset="40%" stopColor="rgba(125,211,252,0.72)" />
                  <stop offset="100%" stopColor="rgba(15,23,42,0)" />
                </radialGradient>
              </defs>

              {[0, 1, 2, 3, 4, 5, 6, 7].map((index) => (
                <circle
                  key={`star-${index}`}
                  cx={110 + index * 105}
                  cy={90 + (index % 3) * 170}
                  r={index % 2 === 0 ? 1.8 : 1.2}
                  fill="rgba(255,255,255,0.45)"
                />
              ))}

              <circle cx="480" cy="360" r="42" fill="url(#geometry-core)" />
              <circle cx="480" cy="360" r="12" fill="rgba(255,255,255,0.92)" />

              {scene?.lanes
                .slice()
                .sort((left, right) => left.position - right.position)
                .map((lane, index) => {
                  const radius = 118 + index * 96;
                  const ringColor = LANE_COLORS[lane.kind] ?? '#94a3b8';
                  return (
                    <g key={lane.id}>
                      <ellipse
                        cx="480"
                        cy="360"
                        rx={radius}
                        ry={radius * 0.68}
                        fill="none"
                        stroke={`${ringColor}55`}
                        strokeWidth="1.2"
                        strokeDasharray={lane.kind === 'evidence' ? '4 8' : '0'}
                      />
                      <text
                        x="480"
                        y={360 - radius * 0.68 - 12}
                        textAnchor="middle"
                        fill={ringColor}
                        fontSize="14"
                        fontWeight="600"
                        letterSpacing="0.08em"
                      >
                        {lane.label.toUpperCase()}
                      </text>
                    </g>
                  );
                })}

              {scene?.edges.map((edge) => {
                const from = scene.positions.get(edge.from);
                const to = scene.positions.get(edge.to);
                if (!from || !to) {
                  return null;
                }

                const isObserved =
                  edge.kind === 'transition' &&
                  scene.observedTransitions.has(edgeKey(edge));
                const stroke =
                  isObserved
                    ? STATUS_COLORS.observed
                    : STATUS_COLORS[edge.evidence_status] ?? 'url(#geometry-edge)';

                return (
                  <g key={edge.id}>
                    <line
                      x1={from.x}
                      y1={from.y}
                      x2={to.x}
                      y2={to.y}
                      stroke={stroke}
                      strokeOpacity={isObserved ? 0.95 : 0.42}
                      strokeWidth={isObserved ? 3.2 : edge.kind === 'contains' ? 1 : 1.8}
                    />
                    {edge.kind === 'transition' && (
                      <circle
                        cx={(from.x + to.x) / 2}
                        cy={(from.y + to.y) / 2}
                        r={isObserved ? 3 : 2}
                        fill={stroke}
                        fillOpacity={0.9}
                      />
                    )}
                  </g>
                );
              })}

              {scene?.nodes.map((node) => {
                const point = scene.positions.get(node.id);
                if (!point) {
                  return null;
                }

                const radius = NODE_SIZES[node.kind] ?? 6;
                const color = STATUS_COLORS[node.evidence_status] ?? '#cbd5f5';
                const labelY = point.y + radius + 18;
                const detail =
                  node.kind === 'step' && node.attributes.initial === 'true'
                    ? 'initial'
                    : node.kind === 'semantic_resource' && node.attributes.resource_mode
                        ? node.attributes.resource_mode
                      : undefined;

                return (
                  <g key={node.id}>
                    <circle
                      cx={point.x}
                      cy={point.y}
                      r={radius + 6}
                      fill={color}
                      fillOpacity="0.08"
                    />
                    <circle
                      cx={point.x}
                      cy={point.y}
                      r={radius}
                      fill={color}
                      stroke="rgba(255,255,255,0.7)"
                      strokeWidth="1"
                    />
                    <text
                      x={point.x}
                      y={labelY}
                      textAnchor="middle"
                      fill="rgba(226,232,240,0.96)"
                      fontSize="12"
                    >
                      {node.label}
                    </text>
                    {detail && (
                      <text
                        x={point.x}
                        y={labelY + 14}
                        textAnchor="middle"
                        fill="rgba(148,163,184,0.92)"
                        fontSize="10"
                      >
                        {detail}
                      </text>
                    )}
                  </g>
                );
              })}
            </svg>
          </div>

          <Space wrap size={[8, 8]}>
            <Tag color="default">view {VIEW_LABELS[view]}</Tag>
            <Tag color="default">nodes {scene?.nodes.length ?? 0}</Tag>
            <Tag color="default">edges {scene?.edges.length ?? 0}</Tag>
            <Tag color="default">schema v{renderable.schema_version}</Tag>
          </Space>
        </Space>
      )}
    </Card>
  );
};

export default GeometryPreview;
