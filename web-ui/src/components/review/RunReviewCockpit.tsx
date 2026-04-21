import React, { useMemo } from 'react';
import { Button, Card, Col, List, Row, Space, Table, Tag, Typography } from 'antd';
import { FileSearchOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type {
  GeometryArtifact,
  GeometryArtifactResponse,
  GeometryNarrativeTask,
  RunStatus,
  TraceData,
  TraceKeypointArtifact,
} from '../../types';
import { formatTimestamp } from '../../utils/time';

const { Paragraph, Text } = Typography;

interface RunReviewCockpitProps {
  run?: RunStatus;
  geometry?: GeometryArtifactResponse;
  keypoints?: TraceKeypointArtifact;
  trace?: TraceData;
  currentTick?: number;
  onSelectTick?: (tick: number) => void;
  title?: string;
  loading?: boolean;
}

interface ComponentStoryLine {
  componentId: string;
  changes: number;
  summary: string;
}

interface NarrativeTaskView {
  label: string;
  entryStepId: string;
  currentStepId: string;
  blockingState: string;
  pendingActions: string[];
  mainPathStepIds: string[];
  blockingPoints: Array<{ stepLabel: string }>;
  faultExits: Array<{ fromStepLabel: string; toStepLabel: string }>;
}

type Translate = (key: string, options?: Record<string, unknown>) => string;

function isGeometryArtifact(
  artifact: GeometryArtifactResponse | undefined
): artifact is GeometryArtifact {
  return Boolean(
    artifact &&
      'summary' in artifact &&
      'nodes' in artifact &&
      'edges' in artifact &&
      'lanes' in artifact
  );
}

function runStatusColor(status: RunStatus['status'] | undefined): string {
  if (status === 'pass') return 'success';
  if (status === 'fail') return 'error';
  return 'processing';
}

function localizeRunStatus(status: RunStatus['status'] | undefined, t: Translate): string {
  if (status === 'pass') return t('run.statusPass');
  if (status === 'fail') return t('run.statusFail');
  if (status === 'running') return t('run.statusRunning');
  return status ?? '';
}

function localizeRunMode(mode: string | undefined, t: Translate): string {
  if (mode === 'component_sim') return t('review.modeComponentSim');
  if (mode === 'no_board_gate') return t('review.modeNoBoardGate');
  return mode ?? '';
}

function taskPriority(task: GeometryNarrativeTask): number {
  const blockingPoints = Array.isArray(task.blocking_points) ? task.blocking_points : [];
  const pendingActions = Array.isArray(task.pending_actions) ? task.pending_actions : [];
  const faultExits = Array.isArray(task.fault_exits) ? task.fault_exits : [];
  return blockingPoints.length * 100 + pendingActions.length * 10 + faultExits.length;
}

function readState(component: Record<string, unknown> | undefined): string {
  if (!component) return 'unknown';
  if (typeof component.state === 'string' && component.state) return component.state;
  if (typeof component.status === 'string' && component.status) return component.status;
  return 'unknown';
}

function localizeStateValue(value: string, t: Translate): string {
  const map: Record<string, string> = {
    disabled: 'review.stateDisabled',
    enabled: 'review.stateEnabled',
    retracted: 'review.stateRetracted',
    extended: 'review.stateExtended',
    moving: 'review.stateMoving',
    on: 'review.stateOn',
    off: 'review.stateOff',
    unknown: 'review.stateUnknown',
    none: 'review.none',
  };
  return map[value] ? t(map[value]) : value;
}

function localizeComponentType(type: string, t: Translate): string {
  const map: Record<string, string> = {
    cylinder: 'review.typeCylinder',
    sensor: 'review.typeSensor',
    switch: 'review.typeSwitch',
    stepper_pd: 'review.typeStepperPd',
    unknown: 'review.typeUnknown',
  };
  return map[type] ? t(map[type]) : type;
}

function localizeEventCategory(value: string, t: Translate): string {
  const map: Record<string, string> = {
    sensor_event: 'review.categorySensorEvent',
    switch_event: 'review.categorySwitchEvent',
  };
  return map[value] ? t(map[value]) : value;
}

function localizeEventSource(value: string, t: Translate): string {
  const map: Record<string, string> = {
    scenario: 'review.sourceScenario',
  };
  return map[value] ? t(map[value]) : value;
}

function readComponentType(component: Record<string, unknown> | undefined): string {
  if (!component) return 'unknown';
  return typeof component.component_type === 'string' && component.component_type
    ? component.component_type
    : 'unknown';
}

function readOutputs(component: Record<string, unknown> | undefined): string {
  const outputs =
    component?.outputs && typeof component.outputs === 'object'
      ? (component.outputs as Record<string, unknown>)
      : undefined;
  if (!outputs) return 'none';
  return Object.entries(outputs)
    .map(([key, value]) => `${key}=${String(value)}`)
    .join(' | ');
}

function readInputs(component: Record<string, unknown> | undefined): string {
  const inputs =
    component?.inputs && typeof component.inputs === 'object'
      ? (component.inputs as Record<string, unknown>)
      : undefined;
  if (!inputs) return 'none';
  return Object.entries(inputs)
    .map(([key, value]) => `${key}=${String(value)}`)
    .join(' | ');
}

function readNumericOutput(
  component: Record<string, unknown> | undefined,
  key: string
): number | undefined {
  const outputs =
    component?.outputs && typeof component.outputs === 'object'
      ? (component.outputs as Record<string, unknown>)
      : undefined;
  const value = outputs?.[key];
  return typeof value === 'number' ? value : undefined;
}

function componentStorySummary(trace: TraceData | undefined, t: Translate): ComponentStoryLine[] {
  const ticks = trace?.ticks ?? [];
  const stats = new Map<
    string,
    {
      componentId: string;
      componentType: string;
      changes: number;
      firstState: string;
      lastState: string;
      firstPosition?: number;
      lastPosition?: number;
    }
  >();

  for (let index = 0; index < ticks.length; index += 1) {
    const current = (ticks[index].component_states ?? {}) as Record<string, Record<string, unknown>>;
    const previous =
      index > 0
        ? ((ticks[index - 1].component_states ?? {}) as Record<string, Record<string, unknown>>)
        : {};

    Object.entries(current).forEach(([componentId, component]) => {
      const currentState = readState(component);
      const previousState = readState(previous[componentId]);
      const currentPosition = readNumericOutput(component, 'position_steps');
      const entry =
        stats.get(componentId) ?? {
          componentId,
          componentType: readComponentType(component),
          changes: 0,
          firstState: currentState,
          lastState: currentState,
          firstPosition: currentPosition,
          lastPosition: currentPosition,
        };

      if (index > 0 && currentState !== previousState) {
        entry.changes += 1;
      }

      entry.lastState = currentState;
      entry.lastPosition = currentPosition;
      stats.set(componentId, entry);
    });
  }

  return [...stats.values()]
    .map((entry) => {
      let summary: string;

      if (
        entry.componentType === 'stepper_pd' &&
        typeof entry.firstPosition === 'number' &&
        typeof entry.lastPosition === 'number' &&
        entry.firstPosition !== entry.lastPosition
      ) {
        summary = t('review.storyLineStepperMoved', {
          componentId: entry.componentId,
          from: entry.firstPosition,
          to: entry.lastPosition,
          state: localizeStateValue(entry.lastState, t),
        });
      } else if (entry.componentType === 'sensor' || entry.componentType === 'switch') {
        summary =
          entry.changes > 0
            ? t('review.storyLineToggled', {
                componentId: entry.componentId,
                count: entry.changes,
                state: localizeStateValue(entry.lastState, t),
              })
            : t('review.storyLineStayed', {
                componentId: entry.componentId,
                state: localizeStateValue(entry.lastState, t),
              });
      } else {
        summary =
          entry.changes > 0
            ? t('review.storyLineChangedState', {
                componentId: entry.componentId,
                count: entry.changes,
                state: localizeStateValue(entry.lastState, t),
              })
            : t('review.storyLineStayed', {
                componentId: entry.componentId,
                state: localizeStateValue(entry.lastState, t),
              });
      }

      return {
        componentId: entry.componentId,
        changes: entry.changes,
        summary,
      };
    })
    .sort((left, right) => right.changes - left.changes || left.componentId.localeCompare(right.componentId));
}

function nearestKeypoints(keypoints: TraceKeypointArtifact | undefined, currentTick: number | undefined) {
  const items = keypoints?.keypoints ?? [];
  if (items.length === 0) return [];

  if (typeof currentTick !== 'number') {
    return [...items]
      .sort((left, right) => right.tick - left.tick)
      .slice(0, 5);
  }

  const pastOrCurrent = items.filter((item) => item.tick <= currentTick);
  if (pastOrCurrent.length > 0) {
    return [...pastOrCurrent]
      .sort((left, right) => right.tick - left.tick)
      .slice(0, 5);
  }

  return [...items]
    .sort((left, right) => left.tick - right.tick)
    .slice(0, 5);
}

function normalizeNarrativeTask(task: GeometryNarrativeTask | undefined): NarrativeTaskView | null {
  if (!task) return null;

  return {
    label: typeof task.label === 'string' && task.label ? task.label : 'Unnamed task',
    entryStepId:
      typeof task.entry_step_id === 'string' && task.entry_step_id ? task.entry_step_id : 'unknown',
    currentStepId:
      typeof task.current_step_id === 'string' && task.current_step_id ? task.current_step_id : 'unknown',
    blockingState:
      typeof task.blocking_state === 'string' && task.blocking_state ? task.blocking_state : 'unknown',
    pendingActions: Array.isArray(task.pending_actions)
      ? task.pending_actions.filter((item): item is string => typeof item === 'string' && item.length > 0)
      : [],
    mainPathStepIds: Array.isArray(task.main_path_step_ids)
      ? task.main_path_step_ids.filter((item): item is string => typeof item === 'string' && item.length > 0)
      : [],
    blockingPoints: Array.isArray(task.blocking_points)
      ? task.blocking_points.map((point) => ({
          stepLabel:
            typeof point?.step_label === 'string' && point.step_label ? point.step_label : 'unknown',
        }))
      : [],
    faultExits: Array.isArray(task.fault_exits)
      ? task.fault_exits.map((exit) => ({
          fromStepLabel:
            typeof exit?.from_step_label === 'string' && exit.from_step_label
              ? exit.from_step_label
              : 'unknown',
          toStepLabel:
            typeof exit?.via?.to_step_label === 'string' && exit.via.to_step_label
              ? exit.via.to_step_label
              : 'unknown',
        }))
      : [],
  };
}

function summarizeNarrative(task: GeometryNarrativeTask | undefined, t: Translate) {
  const normalized = normalizeNarrativeTask(task);
  if (!normalized) return null;

  const mainPath = normalized.mainPathStepIds.join(' -> ');
  const blockingPoint = normalized.blockingPoints[0];
  const faultExit = normalized.faultExits[0];

  return {
    headline: normalized.label,
    body: [
      t('review.entryStep', { value: normalized.entryStepId }),
      t('review.currentStep', { value: normalized.currentStepId }),
      t('review.blockingState', { value: normalized.blockingState }),
      normalized.pendingActions.length > 0
        ? t('review.pendingActions', { value: normalized.pendingActions.join(', ') })
        : undefined,
      mainPath ? t('review.mainPath', { value: mainPath }) : undefined,
      blockingPoint ? t('review.firstBlockingPoint', { value: blockingPoint.stepLabel }) : undefined,
      faultExit
        ? t('review.faultExit', {
            from: faultExit.fromStepLabel,
            to: faultExit.toStepLabel,
          })
        : undefined,
    ].filter(Boolean) as string[],
  };
}

const RunReviewCockpit: React.FC<RunReviewCockpitProps> = ({
  run,
  geometry,
  keypoints,
  trace,
  currentTick,
  onSelectTick,
  title = 'Review',
  loading,
}) => {
  const { t } = useTranslation();
  const artifact = isGeometryArtifact(geometry) ? geometry : undefined;
  const narrativeTask = useMemo(() => {
    const tasks = artifact?.narrative?.tasks ?? [];
    return [...tasks].sort((left, right) => taskPriority(right) - taskPriority(left))[0];
  }, [artifact?.narrative?.tasks]);

  const story = useMemo(() => summarizeNarrative(narrativeTask, t), [narrativeTask, t]);
  const changeSummary = useMemo(() => componentStorySummary(trace, t), [trace, t]);
  const eventList = useMemo(() => nearestKeypoints(keypoints, currentTick), [currentTick, keypoints]);
  const activeSnapshot = useMemo(() => {
    const ticks = trace?.ticks ?? [];
    if (ticks.length === 0) return undefined;
    if (typeof currentTick === 'number') return ticks[Math.min(currentTick, ticks.length - 1)];
    return ticks[ticks.length - 1];
  }, [currentTick, trace?.ticks]);

  const activeComponents = useMemo(() => {
    const components = (activeSnapshot?.component_states ?? {}) as Record<string, Record<string, unknown>>;
    return Object.entries(components).map(([componentId, component]) => ({
      key: componentId,
      componentId,
      type: localizeComponentType(readComponentType(component), t),
      state: localizeStateValue(readState(component), t),
      inputs: readInputs(component),
      outputs: readOutputs(component),
      faults:
        Array.isArray(component.active_faults) && component.active_faults.length > 0
          ? component.active_faults.join(', ')
          : t('review.none'),
    }));
  }, [activeSnapshot?.component_states, t]);

  const evidenceLinks = [
    { key: 'trace', label: t('review.trace'), href: run?.artifacts?.trace },
    { key: 'geometry', label: t('review.geometry'), href: run?.artifacts?.geometry },
    { key: 'diagnosis', label: t('review.diagnosis'), href: run?.artifacts?.diagnosis },
    { key: 'timing', label: t('review.timing'), href: run?.artifacts?.timing },
    { key: 'keypoints', label: t('review.keypoints'), href: run?.artifacts?.keypoints },
    { key: 'fault-audit', label: t('review.faultAudit'), href: run?.artifacts?.fault_audit },
  ].filter((item) => Boolean(item.href));

  return (
    <Card
      title={title}
      loading={loading}
      extra={
        <Space wrap size={[8, 8]}>
          {run?.status && <Tag color={runStatusColor(run.status)}>{localizeRunStatus(run.status, t)}</Tag>}
          {run?.mode && <Tag>{localizeRunMode(run.mode, t)}</Tag>}
          <Tag>{story ? t('review.plcNarrative') : t('review.componentTraceFallback')}</Tag>
        </Space>
      }
    >
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        <Card size="small">
          <Space direction="vertical" size="small" style={{ width: '100%' }}>
            <Space wrap size={[8, 8]}>
              <Text strong>{run?.run_id ? run.run_id.slice(0, 12) : t('review.noRunSelected')}</Text>
              <Text type="secondary">
                {run ? formatTimestamp(run.triggered_at, run.triggered_at_ms) : t('review.noTimestamp')}
              </Text>
              <Text type="secondary">
                {typeof currentTick === 'number'
                  ? t('review.tickLabel', { tick: currentTick })
                  : activeSnapshot
                    ? t('review.tickLabel', { tick: activeSnapshot.tick })
                    : t('review.noTraceSnapshot')}
              </Text>
            </Space>
            <Paragraph style={{ marginBottom: 0 }}>
              {run?.failure_summary
                ? run.failure_summary
                : story
                  ? t('review.summaryWithNarrative', { headline: story.headline })
                  : t('review.summaryComponentOnly')}
            </Paragraph>
          </Space>
        </Card>

        <Row gutter={[16, 16]}>
          <Col xs={24} xl={12}>
            <Card title={t('review.story')}>
              {story ? (
                <List
                  size="small"
                  dataSource={story.body}
                  renderItem={(item) => <List.Item>{item}</List.Item>}
                />
              ) : (
                <Space direction="vertical" size="middle" style={{ width: '100%' }}>
                  <Paragraph style={{ marginBottom: 0 }}>
                    {t('review.noGeometryNarrative')}
                  </Paragraph>
                  <List
                    size="small"
                    dataSource={changeSummary.slice(0, 5)}
                    locale={{ emptyText: t('review.noComponentStateHistory') }}
                    renderItem={(item) => <List.Item>{item.summary}</List.Item>}
                  />
                </Space>
              )}
            </Card>
          </Col>

          <Col xs={24} xl={12}>
            <Card title={t('review.keyEvents')}>
              <List
                size="small"
                dataSource={eventList}
                locale={{ emptyText: t('review.noKeyEvents') }}
                renderItem={(item) => (
                  <List.Item
                    actions={
                      onSelectTick
                        ? [
                            <Button
                              key={`${item.tick}-${item.label}`}
                              size="small"
                              onClick={() => onSelectTick(item.tick)}
                            >
                              {t('review.jump')}
                            </Button>,
                          ]
                        : undefined
                    }
                  >
                    <div>
                      <div style={{ fontWeight: 600 }}>
                        {t('review.tickEventLine', { tick: item.tick, label: item.label })}
                      </div>
                      <div style={{ color: '#64748b' }}>
                        {t('review.eventMetaLine', {
                          category: localizeEventCategory(item.category, t),
                          source: localizeEventSource(item.source, t),
                          atMs: item.at_ms,
                        })}
                      </div>
                    </div>
                  </List.Item>
                )}
              />
            </Card>
          </Col>
        </Row>

        <Card
          title={
            <Space>
              <FileSearchOutlined />
              <span>{t('review.evidence')}</span>
            </Space>
          }
        >
          <Space wrap size={[8, 8]}>
            {evidenceLinks.map((item) => (
              <Button key={item.key} href={item.href} target="_blank">
                {item.label}
              </Button>
            ))}
          </Space>
        </Card>

        <Card title={t('review.componentSnapshotRaw')}>
          <Table
            dataSource={activeComponents}
            pagination={false}
            size="small"
            scroll={{ x: 900 }}
            locale={{ emptyText: t('review.noComponentSnapshot') }}
            columns={[
              { title: t('review.component'), dataIndex: 'componentId', key: 'componentId' },
              { title: t('review.type'), dataIndex: 'type', key: 'type' },
              { title: t('review.state'), dataIndex: 'state', key: 'state' },
              { title: t('review.inputs'), dataIndex: 'inputs', key: 'inputs' },
              { title: t('review.outputs'), dataIndex: 'outputs', key: 'outputs' },
              { title: t('review.faults'), dataIndex: 'faults', key: 'faults' },
            ]}
          />
        </Card>
      </Space>
    </Card>
  );
};

export default RunReviewCockpit;
