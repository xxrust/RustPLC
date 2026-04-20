import React, { useMemo, useState } from 'react';
import { Card, Empty, Segmented, Space, Statistic, Tag, Typography } from 'antd';
import type {
  GeometryArtifact,
  GeometryArtifactResponse,
  GeometryEdge,
  GeometryEvidenceStatus,
  GeometryLane,
  GeometryNarrative,
  GeometryNarrativeBlockingPoint,
  GeometryNarrativeDeviceChain,
  GeometryNarrativeStep,
  GeometryNarrativeTransition,
  GeometryNarrativeTask,
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
  orbit: 'task and step transitions',
  evidence: 'constraints, trace, and intent evidence',
};

const STATUS_COLORS: Record<GeometryEvidenceStatus, string> = {
  authored: '#94a3b8',
  derived: '#67e8f9',
  verified: '#86efac',
  observed: '#fde68a',
  warning: '#fb923c',
  blocked: '#f87171',
};

const EDGE_KIND_LABELS: Record<GeometryEdge['kind'], string> = {
  contains: 'contains',
  topology_link: 'topology link',
  transition: 'step transition',
  resource_claim: 'resource claim',
  timing_scope: 'timing scope',
  causality: 'causality',
};

const NODE_KIND_ORDER: Record<GeometryNode['kind'], number> = {
  task: 0,
  step: 1,
  semantic_resource: 2,
  timing_rule: 3,
  claim_source: 4,
  causality_chain: 5,
  device: 6,
  workpiece_site: 7,
  workpiece_holder: 8,
  workpiece_carrier: 9,
  external_reference: 10,
};

const NODE_SIZES: Record<GeometryNode['kind'], number> = {
  task: 12,
  step: 9,
  semantic_resource: 9,
  timing_rule: 8,
  claim_source: 7,
  causality_chain: 7,
  device: 7,
  workpiece_site: 7,
  workpiece_holder: 7,
  workpiece_carrier: 7,
  external_reference: 6,
};

type NodePoint = { x: number; y: number };

interface GeometryPreviewProps {
  artifact?: GeometryArtifactResponse;
  artifactHref?: string;
  loading?: boolean;
  runMode?: string;
}

interface GeometryScene {
  width: number;
  height: number;
  lanes: GeometryLane[];
  nodes: GeometryNode[];
  edges: GeometryEdge[];
  positions: Map<string, NodePoint>;
  contextualNodeIds: Set<string>;
  observedTransitions: Set<string>;
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

function getNodeAttributes(node: GeometryNode): Record<string, string> {
  return node.attributes ?? {};
}

function getEdgeAttributes(edge: GeometryEdge): Record<string, string> {
  return edge.attributes ?? {};
}

function edgeKey(edge: GeometryEdge): string {
  return `${edge.from}->${edge.to}`;
}

function buildObservedTransitionSet(artifact: GeometryArtifact): Set<string> {
  const transitions = artifact.overlays.trace?.transitions ?? [];
  return new Set(
    transitions
      .filter((item) => item.from_state && item.to_state)
      .map((item) => `step:${item.from_state}->step:${item.to_state}`)
  );
}

function sortNodesForLane(nodes: GeometryNode[]): GeometryNode[] {
  return [...nodes].sort((left, right) => {
    const leftOrder = NODE_KIND_ORDER[left.kind] ?? 999;
    const rightOrder = NODE_KIND_ORDER[right.kind] ?? 999;
    if (leftOrder !== rightOrder) {
      return leftOrder - rightOrder;
    }
    return left.label.localeCompare(right.label);
  });
}

function layoutLaneBands(
  lanes: GeometryLane[],
  nodes: GeometryNode[]
): Pick<GeometryScene, 'width' | 'height' | 'positions'> {
  const sortedLanes = [...lanes].sort((left, right) => left.position - right.position);
  const laneNodes = new Map<string, GeometryNode[]>();
  let maxLaneCount = 0;

  for (const lane of sortedLanes) {
    const items = sortNodesForLane(nodes.filter((node) => node.lane_id === lane.id));
    laneNodes.set(lane.id, items);
    maxLaneCount = Math.max(maxLaneCount, items.length);
  }

  const width = Math.max(1080, sortedLanes.length * 220);
  const height = Math.max(620, maxLaneCount * 64 + 180);
  const left = 80;
  const top = 120;
  const bottom = 70;
  const usableWidth = Math.max(width - left * 2, 1);
  const positions = new Map<string, NodePoint>();

  sortedLanes.forEach((lane, laneIndex) => {
    const items = laneNodes.get(lane.id) ?? [];
    const count = Math.max(items.length, 1);
    const x =
      sortedLanes.length === 1
        ? width / 2
        : left + (usableWidth * laneIndex) / (sortedLanes.length - 1);
    const step = count === 1 ? 0 : (height - top - bottom) / Math.max(count - 1, 1);

    items.forEach((node, nodeIndex) => {
      const y = count === 1 ? height / 2 : top + nodeIndex * step;
      positions.set(node.id, { x, y });
    });
  });

  return { width, height, positions };
}

function buildScene(
  artifact: GeometryArtifact,
  view: GeometryViewKind
): GeometryScene {
  const edgeCandidates = artifact.edges.filter((edge) => edge.views.includes(view));
  const contextualEndpoints = new Set(edgeCandidates.flatMap((edge) => [edge.from, edge.to]));
  const nativeNodes = artifact.nodes.filter((node) => node.views.includes(view));
  const nativeNodeIds = new Set(nativeNodes.map((node) => node.id));
  const nodes = artifact.nodes.filter(
    (node) =>
      nativeNodeIds.has(node.id) ||
      (view !== 'constellation' && contextualEndpoints.has(node.id))
  );
  const nodeIds = new Set(nodes.map((node) => node.id));
  const edges = edgeCandidates.filter((edge) => nodeIds.has(edge.from) && nodeIds.has(edge.to));
  const lanes = artifact.lanes.filter((lane) => nodes.some((node) => node.lane_id === lane.id));
  const { width, height, positions } = layoutLaneBands(lanes, nodes);
  const contextualNodeIds = new Set([...nodeIds].filter((nodeId) => !nativeNodeIds.has(nodeId)));

  return {
    width,
    height,
    lanes,
    nodes,
    edges,
    positions,
    contextualNodeIds,
    observedTransitions: buildObservedTransitionSet(artifact),
  };
}

function renderFactCard(
  title: string,
  items: Array<{ key: string; value: string }>,
  subtitle?: string
) {
  return (
    <div
      style={{
        border: '1px solid rgba(148,163,184,0.14)',
        borderRadius: 14,
        padding: 16,
        background: 'rgba(2,6,23,0.28)',
      }}
    >
      <div style={{ marginBottom: 12 }}>
        <div style={{ color: '#e2e8f0', fontWeight: 700, fontSize: 15 }}>{title}</div>
        {subtitle && (
          <div style={{ color: 'rgba(148,163,184,0.92)', fontSize: 12, marginTop: 4 }}>
            {subtitle}
          </div>
        )}
      </div>
      <div style={{ display: 'grid', gap: 10 }}>
        {items.length === 0 ? (
          <Text type="secondary">none</Text>
        ) : (
          items.map((item) => (
            <div key={`${title}-${item.key}`}>
              <div style={{ color: '#94a3b8', fontSize: 12 }}>{item.key}</div>
              <div style={{ color: '#f8fafc', fontSize: 13 }}>{item.value}</div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function withArray<T>(items: T[] | undefined): T[] {
  return items ?? [];
}

function normalizeTransition(
  transition: GeometryNarrativeTransition
): GeometryNarrativeTransition {
  return {
    ...transition,
    timers: withArray(transition.timers),
    actions: withArray(transition.actions),
    effects: withArray(transition.effects),
  };
}

function normalizeDeviceChain(
  chain: GeometryNarrativeDeviceChain
): GeometryNarrativeDeviceChain {
  return {
    ...chain,
    command_devices: withArray(chain.command_devices),
    actuator_devices: withArray(chain.actuator_devices),
    feedback_devices: withArray(chain.feedback_devices),
    io_devices: withArray(chain.io_devices),
    evidence_chain_ids: withArray(chain.evidence_chain_ids),
  };
}

function normalizeBlockingPoint(
  point: GeometryNarrativeBlockingPoint
): GeometryNarrativeBlockingPoint {
  return {
    ...point,
    release_transitions: withArray(point.release_transitions),
    timeout_transitions: withArray(point.timeout_transitions),
  };
}

function normalizeStep(step: GeometryNarrativeStep): GeometryNarrativeStep {
  return {
    ...step,
    incoming_transition_ids: withArray(step.incoming_transition_ids),
    outgoing: withArray(step.outgoing).map(normalizeTransition),
    device_chains: withArray(step.device_chains).map(normalizeDeviceChain),
    evidence_chain_ids: withArray(step.evidence_chain_ids),
    evidence_reasons: withArray(step.evidence_reasons),
  };
}

function normalizeTask(task: GeometryNarrativeTask): GeometryNarrativeTask {
  return {
    ...task,
    pending_actions: withArray(task.pending_actions),
    main_path_step_ids: withArray(task.main_path_step_ids),
    blocking_points: withArray(task.blocking_points).map(normalizeBlockingPoint),
    fault_exits: withArray(task.fault_exits),
    steps: withArray(task.steps).map(normalizeStep),
  };
}

function normalizeNarrative(narrative: GeometryNarrative | undefined): GeometryNarrative | undefined {
  if (!narrative) {
    return undefined;
  }

  return {
    tasks: withArray(narrative.tasks).map(normalizeTask),
  };
}

function taskPriority(task: GeometryNarrativeTask): number {
  return (
    task.blocking_points.length * 100 +
    task.main_path_step_ids.length * 10 +
    task.steps.length
  );
}

function orderNarrativeTasks(tasks: GeometryNarrativeTask[]): GeometryNarrativeTask[] {
  return [...tasks].sort((left, right) => {
    const priorityDelta = taskPriority(right) - taskPriority(left);
    if (priorityDelta !== 0) {
      return priorityDelta;
    }
    return left.label.localeCompare(right.label);
  });
}

function stepLabel(task: GeometryNarrativeTask | undefined, stepId: string): string {
  return task?.steps?.find((step) => step.step_id === stepId)?.label ?? stepId;
}

const GeometryPreview: React.FC<GeometryPreviewProps> = ({
  artifact,
  artifactHref,
  loading,
  runMode,
}) => {
  const [view, setView] = useState<GeometryViewKind>('constellation');
  const [selectedTaskId, setSelectedTaskId] = useState<string>();
  const [selectedStepId, setSelectedStepId] = useState<string>();

  const renderable = isGeometryArtifact(artifact) ? artifact : undefined;
  const narrative = useMemo(() => normalizeNarrative(renderable?.narrative), [renderable?.narrative]);
  const orderedTasks = useMemo(() => orderNarrativeTasks(narrative?.tasks ?? []), [narrative?.tasks]);
  const scene = useMemo(
    () => (renderable ? buildScene(renderable, view) : undefined),
    [renderable, view]
  );

  const selectedTask =
    orderedTasks.find((task) => task.task_id === selectedTaskId) ?? orderedTasks[0];
  const selectedStep =
    selectedTask?.steps.find((step) => step.step_id === selectedStepId) ?? selectedTask?.steps[0];
  const selectedTaskStepIds = new Set(selectedTask?.steps.map((step) => step.step_id) ?? []);

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
            <Statistic title="Transitions" value={renderable.summary.transition_count} />
            <Statistic title="Causality chains" value={renderable.summary.causality_chain_count} />
            <Statistic
              title="Observed transitions"
              value={renderable.summary.observed_transition_count}
            />
          </Space>

          <Space wrap size={[8, 8]}>
            <Tag color="blue">{renderable.source_path}</Tag>
            <Tag color="default">view {VIEW_LABELS[view]}</Tag>
            <Tag color="default">schema v{renderable.schema_version}</Tag>
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
              This page is narrative-first. Read the task headline, then the blocking/fault story,
              then the step-level device closure. The node map is only a reference index.
            </Text>
            <Segmented
              options={VIEW_OPTIONS}
              value={view}
              onChange={(value) => setView(value as GeometryViewKind)}
            />
          </div>

          {!narrative ? (
            <div
              style={{
                border: '1px solid rgba(248,113,113,0.22)',
                borderRadius: 16,
                padding: 16,
                background: 'rgba(127,29,29,0.22)',
                color: '#fecaca',
              }}
            >
              This artifact only contains the old node map. It does not include task headline,
              blocker, fault exit, or step-device closure narrative data.
              {renderable.schema_version < 2
                ? ' Re-run geometry export or trigger a fresh PLC gate run to generate schema v2.'
                : ' Re-run the pipeline so the exporter can regenerate the narrative payload.'}
            </div>
          ) : (
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'minmax(0, 1.85fr) minmax(320px, 0.95fr)',
                gap: 16,
                alignItems: 'start',
              }}
            >
              <div style={{ display: 'grid', gap: 16 }}>
                <div
                  style={{
                    border: '1px solid rgba(148,163,184,0.16)',
                    borderRadius: 16,
                    padding: 16,
                    background: 'rgba(2,6,23,0.30)',
                  }}
                >
                  <div style={{ color: '#e2e8f0', fontWeight: 700, fontSize: 15, marginBottom: 12 }}>
                    Task Narrative
                  </div>
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                    {orderedTasks.map((task) => {
                      const active = task.task_id === selectedTask?.task_id;
                      return (
                        <button
                          key={task.task_id}
                          type="button"
                          onClick={() => {
                            setSelectedTaskId(task.task_id);
                            setSelectedStepId(task.steps[0]?.step_id);
                          }}
                          style={{
                            borderRadius: 999,
                            border: active
                              ? '1px solid rgba(125,211,252,0.75)'
                              : '1px solid rgba(148,163,184,0.22)',
                            background: active ? 'rgba(8,47,73,0.95)' : 'rgba(15,23,42,0.9)',
                            color: '#e2e8f0',
                            padding: '8px 12px',
                            cursor: 'pointer',
                            textAlign: 'left',
                          }}
                        >
                          <div style={{ fontWeight: 700 }}>{task.label}</div>
                          <div style={{ fontSize: 12, color: '#94a3b8' }}>
                            {task.steps.length} step(s) | uncovered {task.coverage.uncovered_step_count}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                </div>

                {selectedTask && (
                  <div
                    style={{
                      border: '1px solid rgba(148,163,184,0.16)',
                      borderRadius: 16,
                      padding: 16,
                      background: 'rgba(2,6,23,0.30)',
                      display: 'grid',
                      gap: 14,
                    }}
                  >
                    <div>
                      <div style={{ color: '#e2e8f0', fontWeight: 700, fontSize: 16 }}>
                        {selectedTask.label}
                      </div>
                      <div style={{ color: '#94a3b8', fontSize: 13, marginTop: 4 }}>
                        entry {stepLabel(selectedTask, selectedTask.entry_step_id)} | current{' '}
                        {stepLabel(selectedTask, selectedTask.current_step_id)} | blocking{' '}
                        {selectedTask.blocking_state}
                        {selectedTask.pending_actions.length > 0
                          ? ` | pending ${selectedTask.pending_actions.join(', ')}`
                          : ''}
                      </div>
                    </div>

                    <div
                      style={{
                        border: '1px solid rgba(148,163,184,0.14)',
                        borderRadius: 14,
                        padding: 14,
                        background: 'rgba(8,47,73,0.34)',
                        display: 'grid',
                        gap: 12,
                      }}
                    >
                      <div style={{ color: '#e2e8f0', fontWeight: 700 }}>Logic Headline</div>
                      <div style={{ color: '#cbd5f5', fontSize: 13 }}>
                        Main path:{' '}
                        {selectedTask.main_path_step_ids
                          .map((stepId) => stepLabel(selectedTask, stepId))
                          .join(' -> ')}
                      </div>
                      <div style={{ display: 'grid', gap: 8 }}>
                        <div style={{ color: '#94a3b8', fontSize: 12 }}>Blocking points</div>
                        {selectedTask.blocking_points.length === 0 ? (
                          <div style={{ color: '#cbd5f5', fontSize: 13 }}>
                            No explicit blocking points exported for this task.
                          </div>
                        ) : (
                          selectedTask.blocking_points.map((point) => (
                            <div key={point.step_id} style={{ color: '#cbd5f5', fontSize: 13 }}>
                              {point.step_label}: release{' '}
                              {point.release_transitions
                                .map((transition) => `${transition.guard_label} -> ${transition.to_step_label}`)
                                .join(' | ') || 'n/a'}
                              ; timeout{' '}
                              {point.timeout_transitions
                                .map((transition) => `${transition.guard_label} -> ${transition.to_step_label}`)
                                .join(' | ') || 'n/a'}
                            </div>
                          ))
                        )}
                      </div>
                      <div style={{ display: 'grid', gap: 8 }}>
                        <div style={{ color: '#94a3b8', fontSize: 12 }}>Fault / recovery exits</div>
                        {selectedTask.fault_exits.length === 0 ? (
                          <div style={{ color: '#cbd5f5', fontSize: 13 }}>
                            No alternate exits exported beyond the main path.
                          </div>
                        ) : (
                          selectedTask.fault_exits.map((exit) => (
                            <div
                              key={`${exit.from_step_id}-${exit.via.transition_id}`}
                              style={{ color: '#cbd5f5', fontSize: 13 }}
                            >
                              {`${exit.from_step_label}: ${exit.via.guard_label} -> ${exit.via.to_step_label}`}
                            </div>
                          ))
                        )}
                      </div>
                      <div style={{ color: '#94a3b8', fontSize: 12 }}>
                        Coverage gap: {selectedTask.coverage.uncovered_step_count} uncovered step(s) |
                        trace {selectedTask.coverage.trace_available ? 'present' : 'absent'} | intent{' '}
                        {selectedTask.coverage.intent_available ? 'present' : 'absent'}
                      </div>
                    </div>

                    <div style={{ display: 'grid', gap: 12 }}>
                      {selectedTask.steps.map((step) => {
                        const active = step.step_id === selectedStep?.step_id;
                        return (
                          <button
                            key={step.step_id}
                            type="button"
                            onClick={() => setSelectedStepId(step.step_id)}
                            style={{
                              borderRadius: 14,
                              border: active
                                ? '1px solid rgba(125,211,252,0.70)'
                                : '1px solid rgba(148,163,184,0.12)',
                              background: active ? 'rgba(8,47,73,0.78)' : 'rgba(15,23,42,0.72)',
                              padding: 16,
                              textAlign: 'left',
                              cursor: 'pointer',
                            }}
                          >
                            <div
                              style={{
                                display: 'flex',
                                justifyContent: 'space-between',
                                alignItems: 'center',
                                gap: 10,
                                flexWrap: 'wrap',
                              }}
                            >
                              <div>
                                <div style={{ color: '#e2e8f0', fontWeight: 700 }}>
                                  {step.index + 1}. {step.label}
                                </div>
                                <div style={{ color: '#94a3b8', fontSize: 12, marginTop: 4 }}>
                                  {step.is_initial ? 'entry step' : 'derived step'}
                                  {step.is_current ? ' | current' : ''}
                                  {step.device_chains.length === 0 ? ' | missing device closure' : ''}
                                </div>
                              </div>
                              <Space wrap size={[6, 6]}>
                                {step.is_initial && <Tag color="blue">entry</Tag>}
                                {step.is_current && <Tag color="gold">current</Tag>}
                                {step.outgoing.some((transition) => transition.observed) && (
                                  <Tag color="cyan">observed</Tag>
                                )}
                                {step.evidence_chain_ids.length > 0 && (
                                  <Tag color="green">{step.evidence_chain_ids.length} evidence chain(s)</Tag>
                                )}
                              </Space>
                            </div>

                            <div style={{ display: 'grid', gap: 8, marginTop: 12 }}>
                              {step.outgoing.length === 0 ? (
                                <div style={{ color: '#94a3b8' }}>No outgoing transitions exported.</div>
                              ) : (
                                step.outgoing.map((transition) => (
                                  <div
                                    key={transition.transition_id}
                                    style={{
                                      borderRadius: 12,
                                      padding: 10,
                                      background: 'rgba(2,6,23,0.55)',
                                      border: '1px solid rgba(148,163,184,0.12)',
                                    }}
                                  >
                                    <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
                                      <div style={{ color: '#f8fafc', fontWeight: 600 }}>
                                        {`${step.label} -> ${transition.to_step_label}`}
                                      </div>
                                      <div style={{ color: '#94a3b8', fontSize: 12 }}>
                                        {transition.observed ? 'observed' : transition.guard_kind}
                                      </div>
                                    </div>
                                    <div style={{ color: '#cbd5f5', fontSize: 13, marginTop: 6 }}>
                                      {transition.guard_label}
                                      {transition.actions.length > 0
                                        ? ` | actions ${transition.actions.map((action) => action.label).join(' | ')}`
                                        : ''}
                                      {transition.effects.length > 0
                                        ? ` | effects ${transition.effects.join(' | ')}`
                                        : ''}
                                      {transition.timers.length > 0
                                        ? ` | timers ${transition.timers.join(' | ')}`
                                        : ''}
                                    </div>
                                  </div>
                                ))
                              )}
                            </div>

                            <div style={{ display: 'grid', gap: 6, marginTop: 12 }}>
                              <div style={{ color: '#94a3b8', fontSize: 12 }}>Device closure</div>
                              {step.device_chains.length === 0 ? (
                                <div style={{ color: '#e2e8f0', fontSize: 13 }}>
                                  No explicit device chain exported for this step.
                                </div>
                              ) : (
                                step.device_chains.map((chain, index) => (
                                  <div key={`${step.step_id}-${chain.source_kind}-${index}`}>
                                    <div style={{ color: '#e2e8f0', fontSize: 13 }}>
                                      {chain.explanation}
                                    </div>
                                    <div style={{ color: '#94a3b8', fontSize: 12, marginTop: 2 }}>
                                      {[
                                        chain.command_devices.map((device) => device.label).join(' -> '),
                                        chain.actuator_devices.map((device) => device.label).join(' -> '),
                                        chain.feedback_devices.map((device) => device.label).join(' -> '),
                                        chain.io_devices.map((device) => device.label).join(' -> '),
                                      ]
                                        .filter(Boolean)
                                        .join(' -> ')}
                                    </div>
                                  </div>
                                ))
                              )}
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>

              <div style={{ display: 'grid', gap: 16 }}>
                <div
                  style={{
                    border: '1px solid rgba(148,163,184,0.16)',
                    borderRadius: 16,
                    padding: 16,
                    background: 'rgba(2,6,23,0.30)',
                  }}
                >
                  <div style={{ color: '#e2e8f0', fontWeight: 700, fontSize: 15, marginBottom: 10 }}>
                    Selected Step Detail
                  </div>
                  {selectedStep ? (
                    <div style={{ display: 'grid', gap: 10 }}>
                      <div style={{ color: '#f8fafc', fontWeight: 700 }}>{selectedStep.label}</div>
                      <div style={{ color: '#94a3b8', fontSize: 13 }}>
                        {selectedStep.incoming_transition_ids.length} incoming transition(s),{' '}
                        {selectedStep.outgoing.length} outgoing transition(s)
                      </div>
                      {renderFactCard(
                        'Transition Summary',
                        selectedStep.outgoing.map((transition) => ({
                          key: `${selectedStep.label} -> ${transition.to_step_label}`,
                          value: `${transition.guard_label}${
                            transition.actions.length > 0
                              ? ` | actions ${transition.actions.map((action) => action.label).join(' | ')}`
                              : ''
                          }${
                            transition.timers.length > 0
                              ? ` | timers ${transition.timers.join(' | ')}`
                              : ''
                          }`,
                        })),
                        'Guard, action, and timer data for this step'
                      )}
                      {renderFactCard(
                        'Device Chains',
                        selectedStep.device_chains.map((chain, index) => ({
                          key: `${chain.source_kind} ${index + 1}`,
                          value: `${chain.explanation} :: ${[
                            chain.command_devices.map((device) => device.label).join(' -> '),
                            chain.actuator_devices.map((device) => device.label).join(' -> '),
                            chain.feedback_devices.map((device) => device.label).join(' -> '),
                            chain.io_devices.map((device) => device.label).join(' -> '),
                          ]
                            .filter(Boolean)
                            .join(' -> ')}`,
                        })),
                        'Explicit step -> device closure exported by the backend'
                      )}
                      {renderFactCard(
                        'Evidence Landing',
                        [
                          ...selectedStep.evidence_chain_ids.map((chainId, index) => ({
                            key: chainId,
                            value: selectedStep.evidence_reasons[index] ?? 'no reason text',
                          })),
                        ],
                        'Which verified causality chains land on this step'
                      )}
                    </div>
                  ) : (
                    <Text type="secondary">Select a task and step to inspect concrete logic.</Text>
                  )}
                </div>

                {renderFactCard(
                  'Trace / Intent Overlay',
                  [
                    ...(renderable.overlays.trace?.transitions.slice(0, 8).map((transition) => ({
                      key: `trace tick ${transition.tick}`,
                      value: `${transition.task_name ?? `task#${transition.task_index}`} : ${
                        transition.from_state ?? transition.from_step
                      } -> ${transition.to_state ?? transition.to_step} (${transition.reason})`,
                    })) ?? []),
                    ...(renderable.overlays.intent?.warnings.slice(0, 6).map((warning, index) => ({
                      key: `intent warning ${index + 1}`,
                      value: warning,
                    })) ?? []),
                  ],
                  'Observed transitions and intent-alignment warnings'
                )}

                <div
                  style={{
                    borderRadius: 18,
                    border: '1px solid rgba(148, 163, 184, 0.18)',
                    overflow: 'hidden',
                    background:
                      'linear-gradient(180deg, rgba(15,23,42,0.95) 0%, rgba(2,6,23,0.96) 100%)',
                  }}
                >
                  <div style={{ padding: '14px 16px 0', color: '#e2e8f0', fontWeight: 700 }}>
                    Reference Map
                  </div>
                  <div style={{ padding: '4px 16px 12px', color: '#94a3b8', fontSize: 12 }}>
                    Secondary index only. Click a task or step to sync the narrative panel.
                  </div>
                  <div style={{ maxHeight: 520, overflow: 'auto' }}>
                    <svg
                      viewBox={`0 0 ${scene?.width ?? 1080} ${scene?.height ?? 620}`}
                      style={{ width: '100%', height: 'auto', display: 'block' }}
                      role="img"
                      aria-label={`Semantic twin geometry ${VIEW_LABELS[view]}`}
                    >
                      <defs>
                        <linearGradient id="geometry-edge-band" x1="0%" y1="0%" x2="100%" y2="0%">
                          <stop offset="0%" stopColor="rgba(96,165,250,0.20)" />
                          <stop offset="50%" stopColor="rgba(103,232,249,0.18)" />
                          <stop offset="100%" stopColor="rgba(196,132,252,0.18)" />
                        </linearGradient>
                      </defs>

                      {scene?.lanes
                        .slice()
                        .sort((left, right) => left.position - right.position)
                        .map((lane, index, lanes) => {
                          const laneWidth = scene.width / Math.max(lanes.length, 1);
                          const x = index * laneWidth + 18;
                          return (
                            <g key={lane.id}>
                              <rect
                                x={x}
                                y="24"
                                width={laneWidth - 36}
                                height={scene.height - 48}
                                rx="22"
                                fill="rgba(15,23,42,0.34)"
                                stroke="rgba(148,163,184,0.10)"
                              />
                              <text
                                x={x + 18}
                                y="54"
                                fill={
                                  lane.kind === 'topology'
                                    ? '#60a5fa'
                                    : lane.kind === 'evidence'
                                      ? '#34d399'
                                      : '#c084fc'
                                }
                                fontSize="16"
                                fontWeight="700"
                                letterSpacing="0.06em"
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
                          edge.kind === 'transition' && scene.observedTransitions.has(edgeKey(edge));
                        const stroke = isObserved
                          ? STATUS_COLORS.observed
                          : STATUS_COLORS[edge.evidence_status] ?? 'url(#geometry-edge-band)';
                        const midX = (from.x + to.x) / 2;
                        const path =
                          from.x === to.x
                            ? `M ${from.x} ${from.y} C ${from.x + 44} ${from.y}, ${to.x + 44} ${to.y}, ${to.x} ${to.y}`
                            : `M ${from.x} ${from.y} C ${midX} ${from.y}, ${midX} ${to.y}, ${to.x} ${to.y}`;
                        const attributes = getEdgeAttributes(edge);
                        const title = [
                          `${EDGE_KIND_LABELS[edge.kind]}: ${edge.label}`,
                          ...Object.entries(attributes).map(([key, value]) => `${key}: ${value}`),
                        ].join('\n');
                        const highlighted =
                          edge.kind === 'transition' &&
                          (edge.from === selectedStep?.step_id || edge.to === selectedStep?.step_id);

                        return (
                          <g
                            key={edge.id}
                            onClick={() => {
                              if (edge.kind !== 'transition') {
                                return;
                              }
                              const targetTask = narrative?.tasks.find((task) =>
                                task.steps.some((step) => step.step_id === edge.from)
                              );
                              if (targetTask) {
                                setSelectedTaskId(targetTask.task_id);
                              }
                              setSelectedStepId(edge.from);
                            }}
                            style={{ cursor: edge.kind === 'transition' ? 'pointer' : 'default' }}
                          >
                            <path
                              d={path}
                              fill="none"
                              stroke={stroke}
                              strokeOpacity={highlighted ? 1 : isObserved ? 0.95 : 0.58}
                              strokeWidth={
                                highlighted ? 4.8 : isObserved ? 4 : edge.kind === 'contains' ? 1.4 : 2.2
                              }
                              strokeDasharray={
                                edge.kind === 'contains' ? '4 8' : edge.kind === 'timing_scope' ? '8 6' : '0'
                              }
                            >
                              <title>{title}</title>
                            </path>
                          </g>
                        );
                      })}

                      {scene?.nodes.map((node) => {
                        const point = scene.positions.get(node.id);
                        if (!point) {
                          return null;
                        }
                        const attributes = getNodeAttributes(node);
                        const radius = NODE_SIZES[node.kind] ?? 7;
                        const color = STATUS_COLORS[node.evidence_status] ?? '#cbd5f5';
                        const labelY = point.y + radius + 18;
                        const detail =
                          node.kind === 'step' && attributes.initial === 'true'
                            ? 'initial'
                            : node.kind === 'semantic_resource'
                              ? attributes.resource_mode
                              : node.kind === 'device'
                                ? attributes.device_kind
                                : attributes.reason ?? '';
                        const contextual = scene.contextualNodeIds.has(node.id);
                        const highlighted =
                          node.id === selectedTask?.task_id ||
                          node.id === selectedStep?.step_id ||
                          selectedTaskStepIds.has(node.id);

                        return (
                          <g
                            key={node.id}
                            opacity={contextual ? 0.52 : 1}
                            onClick={() => {
                              if (node.kind === 'task') {
                                const task = narrative?.tasks.find((item) => item.task_id === node.id);
                                setSelectedTaskId(node.id);
                                setSelectedStepId(task?.steps[0]?.step_id);
                                return;
                              }
                              if (node.kind === 'step') {
                                const task = narrative?.tasks.find((item) =>
                                  item.steps.some((step) => step.step_id === node.id)
                                );
                                if (task) {
                                  setSelectedTaskId(task.task_id);
                                }
                                setSelectedStepId(node.id);
                              }
                            }}
                            style={{
                              cursor:
                                node.kind === 'task' || node.kind === 'step' ? 'pointer' : 'default',
                            }}
                          >
                            <circle
                              cx={point.x}
                              cy={point.y}
                              r={radius + 8}
                              fill={color}
                              fillOpacity={highlighted ? 0.18 : 0.10}
                            />
                            <circle
                              cx={point.x}
                              cy={point.y}
                              r={radius}
                              fill={color}
                              stroke={highlighted ? 'rgba(255,255,255,0.95)' : 'rgba(255,255,255,0.7)'}
                              strokeWidth={highlighted ? '2' : '1.1'}
                            />
                            <text
                              x={point.x}
                              y={labelY}
                              textAnchor="middle"
                              fill="rgba(226,232,240,0.96)"
                              fontSize="12"
                              fontWeight={node.kind === 'task' ? '700' : '500'}
                            >
                              {node.label}
                            </text>
                            {detail ? (
                              <text
                                x={point.x}
                                y={labelY + 14}
                                textAnchor="middle"
                                fill="rgba(148,163,184,0.92)"
                                fontSize="10"
                              >
                                {detail.length > 48 ? `${detail.slice(0, 48)}...` : detail}
                              </text>
                            ) : null}
                          </g>
                        );
                      })}
                    </svg>
                  </div>
                </div>
              </div>
            </div>
          )}
        </Space>
      )}
    </Card>
  );
};

export default GeometryPreview;
